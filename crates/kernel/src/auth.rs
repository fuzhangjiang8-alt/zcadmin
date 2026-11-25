//! 认证与会话。
//!
//! # 当前实现（M1-a）
//!
//! | 项 | 实现 |
//! |---|---|
//! | 用户来源 | `sys.sys_user`（真实表） |
//! | 密码校验 | Argon2id（[`crate::password`]） |
//! | 会话存储 | 进程内 `HashMap`（多实例前需换 Redis / `sys_session`） |
//! | 令牌 | 随机 UUID + HttpOnly Cookie |
//! | 权限 | 登录时计算并写入会话，超级管理员为 `["*"]` |
//!
//! # 安全约束
//!
//! - 登录失败不区分「用户不存在 / 密码错误 / 账号被禁用」，统一 401，
//!   避免账号枚举；具体原因只写日志。
//! - 会话令牌只存内存，不落日志、不回传前端（HttpOnly Cookie）。
//! - 生产环境 Cookie 自动加 `Secure`（`cfg!(debug_assertions)` 为假时）。

use crate::error::AppError;
pub use crate::session::{Session, SessionStore};

use crate::auth_identity::{load_data_scope, load_permissions, scope_matches};

use crate::state::AppState;
use crate::tenant::TenantTx;
use axum::extract::FromRequestParts;
use axum::http::header;
use axum::http::request::Parts;

use axum::Router;
use serde::Serialize;
use uuid::Uuid;

/// 认证相关路由。
pub fn routes() -> Router<AppState> {
    Router::new()
        .merge(crate::auth_login::router())
        .merge(crate::auth_tokens::router())
        .merge(crate::auth_session_api::router())
}

/// 会话 Cookie 的名称。
pub const SESSION_COOKIE: &str = "zcadmin_token";

/// 当前登录用户（axum 提取器）。
///
/// handler 参数里写 `user: CurrentUser` 即可自动完成
/// 「读 Cookie → 查会话 → 401」，无需手写重复代码。
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CurrentUser {
    /// 用户唯一标识。
    pub user_id: Uuid,
    /// 用户所属租户。
    pub tenant_id: Uuid,
    /// 用户所属部门（可空）。
    pub dept_id: Option<Uuid>,
    /// 数据范围：all / dept_and_child / dept / self。
    pub data_scope: String,
    /// 登录名。
    pub username: String,
    /// 权限点集合；`["*"]` 表示超级管理员。
    pub permissions: Vec<String>,
    /// 当前会话 id（PAT 认证为 None）。
    pub session_id: Option<Uuid>,
    /// 是否必须先修改密码。
    pub must_change_password: bool,
    /// 当前会话来源 IP（不序列化）。
    #[serde(skip)]
    pub ip: Option<String>,
    /// 当前会话 User-Agent（不序列化）。
    #[serde(skip)]
    pub user_agent: Option<String>,
}

impl CurrentUser {
    /// 是否拥有某个权限点。
    ///
    /// 目前只支持通配 `*`；后续会换成位图（O(1) 且省内存）。
    pub fn has_permission(&self, code: &str) -> bool {
        self.permissions.iter().any(|p| scope_matches(p, code))
    }

    /// 是否平台超级管理员（`is_super` 登录时会展开为通配 `*`）。
    ///
    /// 用途：平台级能力（租户管理、权限点分域）必须只对平台超管开放。
    /// 注意：带 scopes 的访问令牌会被收窄，因此不算平台超管——
    /// 这正是期望行为，避免一个只读令牌能改租户。
    pub fn is_super_admin(&self) -> bool {
        self.permissions.iter().any(|p| p == "*")
    }
}

impl FromRequestParts<AppState> for CurrentUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let current = if let Some(token) = extract_bearer(parts) {
            resolve_api_token(state, &token).await?
        } else {
            let token = extract_token(parts).ok_or(AppError::Unauthorized)?;
            let session = state
                .sessions
                .get(&token)
                .await
                .map_err(AppError::Database)?
                .ok_or(AppError::Unauthorized)?;
            Self {
                user_id: session.user_id,
                tenant_id: session.tenant_id,
                dept_id: session.dept_id,
                data_scope: session.data_scope,
                username: session.username,
                permissions: session.permissions,
                session_id: Some(session.id),
                must_change_password: session.must_change_password,
                ip: session.ip.clone(),
                user_agent: session.user_agent.clone(),
            }
        };
        enforce_password_change(current, parts.uri.path())
    }
}

/// 强制改密期间只允许访问改密/登出/当前用户接口。
fn enforce_password_change(user: CurrentUser, path: &str) -> Result<CurrentUser, AppError> {
    const ALLOWED: [&str; 3] = [
        "/api/auth/change-password",
        "/api/auth/me",
        "/api/auth/logout",
    ];
    if user.must_change_password && !ALLOWED.contains(&path) {
        return Err(AppError::Forbidden);
    }
    Ok(user)
}

/// 从请求头里提取 Bearer 令牌。
fn extract_bearer(parts: &Parts) -> Option<String> {
    let value = parts.headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    value
        .strip_prefix("Bearer ")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// 解析个人访问令牌为当前用户。
type ApiTokenRow = (Uuid, String, Option<Uuid>, String, bool, Vec<String>, bool);

async fn resolve_api_token(state: &AppState, token: &str) -> Result<CurrentUser, AppError> {
    let mut iter = token.split('.');
    if iter.next() != Some("zc") {
        return Err(AppError::Unauthorized);
    }
    let tenant_id = iter
        .next()
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or(AppError::Unauthorized)?;
    let secret = iter.next().ok_or(AppError::Unauthorized)?;
    if secret.is_empty() || iter.next().is_some() {
        return Err(AppError::Unauthorized);
    }
    let hash = hash_api_token(token);
    let mut tx = TenantTx::begin_tenant(&state.db, tenant_id).await?;
    let row: Option<ApiTokenRow> = sqlx::query_as(
        r#"
        SELECT u.id, u.username, u.dept_id, u.status, u.is_super, t.scopes,
               u.must_change_password
        FROM sys.sys_api_token t
        JOIN sys.sys_user u ON u.id = t.user_id
        JOIN sys.sys_tenant tn ON tn.id = t.tenant_id
        WHERE t.tenant_id = $1 AND t.token_hash = $2
          AND t.revoked_at IS NULL
          AND (t.expires_at IS NULL OR t.expires_at > now())
          AND u.deleted_at IS NULL
          AND tn.status = 'active' AND tn.deleted_at IS NULL
        "#,
    )
    .bind(tenant_id)
    .bind(&hash)
    .fetch_optional(&mut *tx)
    .await?;
    let (user_id, username, dept_id, status, is_super, scopes, must_change_password) =
        row.ok_or(AppError::Unauthorized)?;
    if status != "active" {
        return Err(AppError::Unauthorized);
    }
    let mut permissions = if is_super {
        vec!["*".to_string()]
    } else {
        load_permissions(&mut tx, user_id).await?
    };
    if !scopes.is_empty() {
        permissions = if is_super {
            scopes.clone()
        } else {
            permissions
                .into_iter()
                .filter(|p| scopes.iter().any(|scope| scope_matches(scope, p)))
                .collect()
        };
    }
    let data_scope = if is_super {
        "all".to_string()
    } else {
        load_data_scope(&mut tx, user_id).await?
    };
    sqlx::query(
        "UPDATE sys.sys_api_token SET last_used_at = now()
         WHERE tenant_id = $1 AND token_hash = $2",
    )
    .bind(tenant_id)
    .bind(&hash)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(CurrentUser {
        user_id,
        tenant_id,
        dept_id,
        data_scope,
        username,
        permissions,
        session_id: None,
        must_change_password,
        ip: None,
        user_agent: None,
    })
}

pub(crate) fn hash_api_token(token: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// 从请求头里提取会话令牌（仅 Cookie）。
pub(crate) fn extract_token(parts: &Parts) -> Option<String> {
    let cookie_header = parts.headers.get(header::COOKIE)?.to_str().ok()?;
    cookie_header.split(';').find_map(|kv| {
        let (k, v) = kv.split_once('=')?;
        (k.trim() == SESSION_COOKIE).then(|| v.trim().to_string())
    })
}

/// OpenAPI 片段：认证相关接口。
///
/// 每个模块自带一个片段，最后由 [`crate::openapi::ApiDoc`] 合并。
/// 这样新增模块时不需要改动全局文档定义，符合"插件化"的一致思路。
#[derive(utoipa::OpenApi)]
#[openapi(
    paths(
        crate::auth_login::login,
        crate::auth_session_api::logout,
        crate::auth_session_api::me,
        crate::auth_tokens::list_tokens,
        crate::auth_tokens::create_token,
        crate::auth_tokens::revoke_token,
        crate::auth_session_api::change_password
    ),
    components(schemas(
        crate::auth_login::LoginRequest,
        crate::auth_login::LoginResponse,
        CurrentUser,
        crate::auth_tokens::TokenVo,
        crate::auth_tokens::CreateTokenRequest,
        crate::auth_tokens::CreatedTokenVo,
        crate::auth_session_api::ChangePasswordRequest
    )),
    tags((name = "auth", description = "认证与会话"))
)]
pub struct AuthApi;
