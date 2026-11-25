//! 会话创建事务：单端模式下按租户与用户串行替换，插入失败必须保留旧会话。
//! 事务级咨询锁由 PostgreSQL 自动释放，多实例共用同一锁键，不依赖进程内互斥锁。

use super::{Session, SessionStore};
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

impl SessionStore {
    /// 新建会话并返回原始令牌；数据库只保存摘要。
    ///
    /// `single_session` 为 true 时串行替换同租户同用户的全部会话。
    /// 数据库失败时整体回滚，旧会话不丢失；提交后清空本实例缓存。
    pub async fn create(
        &self,
        session: Session,
        single_session: bool,
    ) -> Result<String, sqlx::Error> {
        let token = Uuid::new_v4().to_string();
        let mut tx = self.pool.begin().await?;
        if single_session {
            // 只给 DELETE 套事务无法阻止并发 INSERT；必须在查询旧会话前获取同一账号的锁。
            // 锁键是服务端 UUID 的确定性组合，哈希碰撞最多多串行一个账号，不影响隔离。
            sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
                .bind(format!("session:{}:{}", session.tenant_id, session.user_id))
                .execute(&mut *tx)
                .await?;
            sqlx::query("DELETE FROM sys.sys_session WHERE tenant_id = $1 AND user_id = $2")
                .bind(session.tenant_id)
                .bind(session.user_id)
                .execute(&mut *tx)
                .await?;
        }
        insert_session(&mut tx, &session, &Self::hash_token(&token)).await?;
        tx.commit().await?;
        self.invalidate_cache();
        Ok(token)
    }
}

/// 所有写入共享替换事务，序列化或约束失败时不允许提交前面的会话删除。
async fn insert_session(
    tx: &mut Transaction<'_, Postgres>,
    session: &Session,
    token_hash: &str,
) -> Result<(), sqlx::Error> {
    let permissions = serde_json::to_value(&session.permissions)
        .map_err(|error| sqlx::Error::Encode(Box::new(error)))?;
    sqlx::query(
        r#"INSERT INTO sys.sys_session
            (id, token_hash, tenant_id, user_id, dept_id, data_scope,
             username, permissions, created_at, expires_at, ip, user_agent,
             must_change_password)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11::inet, $12, $13)"#,
    )
    .bind(session.id)
    .bind(token_hash)
    .bind(session.tenant_id)
    .bind(session.user_id)
    .bind(session.dept_id)
    .bind(&session.data_scope)
    .bind(&session.username)
    .bind(permissions)
    .bind(session.created_at)
    .bind(session.expires_at)
    .bind(&session.ip)
    .bind(&session.user_agent)
    .bind(session.must_change_password)
    .execute(&mut **tx)
    .await?;
    Ok(())
}
