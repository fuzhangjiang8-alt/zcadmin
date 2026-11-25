//! 数据库会话存储（`sys.sys_session`）与进程内短 TTL 缓存。
//!
//! 边界：登录/登出/当前用户在 `auth`；本模块只负责会话读写与缓存。
//! 关键约束：令牌只存 SHA-256；写操作清空本实例缓存，旧查询不得重新回填失效条目。
//! 单端登录由数据库事务锁串行化，多实例缓存撤销仍存在最长 30 秒窗口。

mod cache;
mod create;

use std::sync::Arc;

use chrono::{DateTime, Utc};
use uuid::Uuid;

/// 会话数据。
#[derive(Debug, Clone)]
pub struct Session {
    /// 会话 id（用于在线会话管理，不暴露令牌）。
    pub id: Uuid,
    /// 用户唯一标识。
    pub user_id: Uuid,
    /// 用户所属租户。
    pub tenant_id: Uuid,
    /// 用户所属部门（可空）。
    pub dept_id: Option<Uuid>,
    /// 登录来源 IP。
    pub ip: Option<String>,
    /// 登录 User-Agent。
    pub user_agent: Option<String>,
    /// 数据范围：all / dept_and_child / dept / self。
    pub data_scope: String,
    /// 登录名。
    pub username: String,
    /// 权限点集合；`["*"]` 表示超级管理员。
    pub permissions: Vec<String>,
    /// 过期时间（UTC）。
    pub expires_at: DateTime<Utc>,
    /// 创建时间（UTC）。
    pub created_at: DateTime<Utc>,
    /// 是否必须先修改密码。
    pub must_change_password: bool,
}

/// 数据库会话存储（`sys.sys_session`）。
///
/// 令牌不落库：只存 SHA-256 摘要，数据库泄露也无法直接复用会话。
/// 多实例共享同一张表，天然支持水平扩展；过期行由后台任务清理。
const SESSION_CACHE_TTL: std::time::Duration = std::time::Duration::from_secs(30);

/// 数据库会话存储（带进程内短 TTL 缓存）。
#[derive(Clone)]
pub struct SessionStore {
    pool: sqlx::PgPool,
    /// 本实例缓存由代次保护，防止失效与数据库回填交错时复活旧会话。
    cache: Arc<cache::SessionCache>,
}

impl SessionStore {
    /// 使用连接池构造。
    pub fn new(pool: sqlx::PgPool) -> Self {
        Self {
            pool,
            cache: Arc::new(cache::SessionCache::default()),
        }
    }

    /// 清空本实例缓存；跨实例撤销受 30 秒 TTL 约束。
    fn invalidate_cache(&self) {
        self.cache.invalidate();
    }

    /// 仅返回令牌摘要，缓存键和数据库均不持有客户端原始令牌。
    fn hash_token(token: &str) -> String {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(token.as_bytes());
        hasher
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }

    /// 按令牌取会话；过期视为不存在。命中缓存时跳过数据库。
    pub async fn get(&self, token: &str) -> Result<Option<Session>, sqlx::Error> {
        let key = Self::hash_token(token);
        let (generation, cached) = self.cache.get(&key);
        if let Some(session) = cached {
            metrics::counter!("session_cache_hits_total").increment(1);
            return Ok(Some(session));
        }

        metrics::counter!("session_cache_misses_total").increment(1);
        let row: Option<SessionRow> = sqlx::query_as(
            r#"
            SELECT id, tenant_id, user_id, dept_id, data_scope, username, permissions,
                   created_at, expires_at, host(ip) AS ip, user_agent, must_change_password
            FROM sys.sys_session
            WHERE token_hash = $1 AND expires_at > now()
            "#,
        )
        .bind(&key)
        .fetch_optional(&self.pool)
        .await?;
        let session: Option<Session> = row.map(Into::into);
        if let Some(s) = &session {
            self.cache.insert(key, s.clone(), generation);
        }
        Ok(session)
    }

    /// 列出某租户的全部有效会话（按创建时间倒序）。
    pub async fn list(&self, tenant_id: Uuid) -> Result<Vec<Session>, sqlx::Error> {
        let rows: Vec<SessionRow> = sqlx::query_as(
            r#"
            SELECT id, tenant_id, user_id, dept_id, data_scope, username, permissions,
                   created_at, expires_at, host(ip) AS ip, user_agent, must_change_password
            FROM sys.sys_session
            WHERE tenant_id = $1 AND expires_at > now()
            ORDER BY created_at DESC
            "#,
        )
        .bind(tenant_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    /// 按会话 id 强制下线；返回是否命中。
    /// 撤销某用户的会话，可保留一个当前会话。
    pub async fn revoke_user_sessions(
        &self,
        tenant_id: Uuid,
        user_id: Uuid,
        keep: Option<Uuid>,
    ) -> Result<u64, sqlx::Error> {
        let result = sqlx::query(
            "DELETE FROM sys.sys_session
             WHERE tenant_id = $1 AND user_id = $2
               AND ($3::uuid IS NULL OR id <> $3)",
        )
        .bind(tenant_id)
        .bind(user_id)
        .bind(keep)
        .execute(&self.pool)
        .await?;
        self.invalidate_cache();
        Ok(result.rows_affected())
    }

    /// 撤销拥有某角色的全部用户会话。
    ///
    /// 角色权限变更后必须调用：会话里缓存了登录时的权限快照，
    /// 不吊销的话被收回的权限最长会继续有效一个 TTL（默认 8 小时）。
    pub async fn revoke_role_sessions(
        &self,
        tenant_id: Uuid,
        role_id: Uuid,
    ) -> Result<u64, sqlx::Error> {
        let result = sqlx::query(
            "DELETE FROM sys.sys_session AS s
             USING sys.sys_user_role AS ur
             WHERE s.tenant_id = $1 AND s.user_id = ur.user_id AND ur.role_id = $2",
        )
        .bind(tenant_id)
        .bind(role_id)
        .execute(&self.pool)
        .await?;
        self.invalidate_cache();
        Ok(result.rows_affected())
    }

    /// 撤销某租户的全部会话（停用租户时调用）。
    pub async fn revoke_tenant_sessions(&self, tenant_id: Uuid) -> Result<u64, sqlx::Error> {
        let result = sqlx::query("DELETE FROM sys.sys_session WHERE tenant_id = $1")
            .bind(tenant_id)
            .execute(&self.pool)
            .await?;
        self.invalidate_cache();
        Ok(result.rows_affected())
    }

    /// 撤销指定会话。
    pub async fn revoke(&self, tenant_id: Uuid, session_id: Uuid) -> Result<bool, sqlx::Error> {
        let result = sqlx::query("DELETE FROM sys.sys_session WHERE tenant_id = $1 AND id = $2")
            .bind(tenant_id)
            .bind(session_id)
            .execute(&self.pool)
            .await?;
        // 必须清缓存：否则内存里的旧会话最长还能用 30 秒，「强制下线」不即时。
        self.invalidate_cache();
        Ok(result.rows_affected() > 0)
    }

    /// 删除会话（退出登录）。
    pub async fn remove(&self, token: &str) -> Result<(), sqlx::Error> {
        sqlx::query("DELETE FROM sys.sys_session WHERE token_hash = $1")
            .bind(Self::hash_token(token))
            .execute(&self.pool)
            .await?;
        self.invalidate_cache();
        Ok(())
    }

    /// 清理已过期会话；返回删除行数。
    pub async fn purge_expired(&self) -> Result<u64, sqlx::Error> {
        let result = sqlx::query("DELETE FROM sys.sys_session WHERE expires_at <= now()")
            .execute(&self.pool)
            .await?;
        if result.rows_affected() > 0 {
            self.invalidate_cache();
        }
        Ok(result.rows_affected())
    }
}

#[derive(Debug, sqlx::FromRow)]
struct SessionRow {
    id: Uuid,
    tenant_id: Uuid,
    user_id: Uuid,
    dept_id: Option<Uuid>,
    ip: Option<String>,
    user_agent: Option<String>,
    data_scope: String,
    username: String,
    permissions: serde_json::Value,
    created_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    must_change_password: bool,
}

impl From<SessionRow> for Session {
    fn from(r: SessionRow) -> Self {
        Self {
            id: r.id,
            user_id: r.user_id,
            tenant_id: r.tenant_id,
            dept_id: r.dept_id,
            ip: r.ip,
            user_agent: r.user_agent,
            data_scope: r.data_scope,
            username: r.username,
            permissions: serde_json::from_value(r.permissions).unwrap_or_default(),
            expires_at: r.expires_at,
            created_at: r.created_at,
            must_change_password: r.must_change_password,
        }
    }
}
