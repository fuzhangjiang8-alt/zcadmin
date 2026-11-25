//! 会话 API：登出、当前用户、修改密码。
//!
//! 边界：登录/会话存储在 `auth`/`session`；本模块只暴露会话相关 HTTP 接口。
//! 关键约束：登出删除会话并让 Cookie 立即过期；改密后撤销其它会话。

use axum::extract::State;
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;

use crate::auth::{extract_token, CurrentUser, SESSION_COOKIE};
use crate::error::AppError;
use crate::password;
use crate::response::ApiResponse;
use crate::state::AppState;
use crate::tenant::TenantTx;

/// 会话 API 路由（挂在根路径下）。
pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route("/api/auth/logout", post(logout))
        .route("/api/auth/me", get(me))
        .route("/api/auth/change-password", post(change_password))
}

/// 退出登录：确认数据库撤销成功后清除 Cookie；存储失败返回错误，允许客户端重试。
#[utoipa::path(
    post,
    path = "/api/auth/logout",
    tag = "auth",
    responses(
        (status = 204, description = "已退出登录"),
        (status = 503, description = "会话存储不可用，请重试")
    )
)]
async fn logout(
    axum::extract::State(state): axum::extract::State<AppState>,
    req: axum::extract::Request,
) -> Result<Response, AppError> {
    let (parts, _body) = req.into_parts();
    if let Some(token) = extract_token(&parts) {
        state.sessions.remove(&token).await?;
    }
    let expired = build_session_cookie("", 0);
    let mut resp = StatusCode::NO_CONTENT.into_response();
    if let Ok(v) = HeaderValue::from_str(&expired) {
        resp.headers_mut().insert(header::SET_COOKIE, v);
    }
    Ok(resp)
}

/// 当前登录用户信息。
#[utoipa::path(
    get,
    path = "/api/auth/me",
    tag = "auth",
    responses((status = 200, description = "当前用户与权限点", body = ApiResponse<CurrentUser>))
)]
async fn me(user: CurrentUser) -> Json<ApiResponse<CurrentUser>> {
    Json(ApiResponse::new(user))
}

/// 构造会话 Cookie。
///
/// - `HttpOnly`：JS 读不到，XSS 无法直接窃取；
/// - `SameSite=Lax`：跨站 POST 不带 Cookie（配合 CSRF Token）；
/// - 生产环境加 `Secure`（仅 HTTPS 传输）。
///
/// 注意：`Secure` 由 `cfg!(debug_assertions)` 决定。release 构建**必须**部署在
/// HTTPS 之后（由网关终止 TLS），否则浏览器会丢弃该 Cookie、表现为"登录成功但请求 401"。
pub(crate) fn build_session_cookie(token: &str, max_age_secs: i64) -> String {
    let secure = if cfg!(debug_assertions) {
        ""
    } else {
        "; Secure"
    };
    format!(
        "{name}={token}; HttpOnly; SameSite=Lax; Path=/{secure}; Max-Age={max_age}",
        name = SESSION_COOKIE,
        max_age = max_age_secs
    )
}

/// 修改密码请求。
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChangePasswordRequest {
    /// 原密码。
    pub old_password: String,
    /// 新密码。
    pub new_password: String,
    /// 是否同时撤销所有访问令牌。
    #[serde(default)]
    pub revoke_tokens: bool,
}

/// 修改当前用户密码。
#[utoipa::path(
    post,
    path = "/api/auth/change-password",
    tag = "auth",
    request_body = ChangePasswordRequest,
    responses((status = 200, description = "修改成功"))
)]
async fn change_password(
    State(state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Json(req): Json<ChangePasswordRequest>,
) -> Result<ApiResponse<()>, AppError> {
    if req.old_password == req.new_password {
        return Err(AppError::Validation("新密码不能与原密码相同".into()));
    }
    let current_hash: Option<String> = sqlx::query_scalar(
        "SELECT password_hash FROM sys.sys_user
         WHERE id = $1 AND tenant_id = $2 AND deleted_at IS NULL",
    )
    .bind(user.user_id)
    .bind(user.tenant_id)
    .fetch_optional(&mut *tx)
    .await?;
    let current_hash = current_hash.ok_or(AppError::Unauthorized)?;
    // Argon2 在受控阻塞池里跑；过载时返回 503 而不是把 worker 卡死。
    let password_ok =
        password::verify_password_async(req.old_password.clone(), current_hash.clone()).await?;
    if !password_ok {
        return Err(AppError::Validation("原密码错误".into()));
    }
    let policy = password::load_policy(&mut tx, user.tenant_id).await?;
    policy
        .validate(&req.new_password)
        .map_err(|e| AppError::Validation(e.to_string()))?;
    let history: Vec<String> = sqlx::query_scalar(
        "SELECT password_hash FROM sys.sys_password_history
         WHERE tenant_id = $1 AND user_id = $2
         ORDER BY created_at DESC LIMIT $3",
    )
    .bind(user.tenant_id)
    .bind(user.user_id)
    .bind(policy.history_count.max(1))
    .fetch_all(&mut *tx)
    .await?;
    // 历史密码比对同样需要哈希校验：逐个串行会放大阻塞池占用，
    // 因此先收集需比对的值，再顺序 await（并发上限由阻塞池统一约束）。
    let mut reused = false;
    for hash in &history {
        if password::verify_password_async(req.new_password.clone(), hash.clone()).await? {
            reused = true;
            break;
        }
    }
    if reused {
        return Err(AppError::Validation("不能重复使用最近 5 次密码".into()));
    }
    let new_hash = password::hash_password_async(req.new_password.clone()).await?;
    sqlx::query(
        "UPDATE sys.sys_user
         SET password_hash = $1, failed_attempts = 0, locked_until = NULL,
             must_change_password = false, password_changed_at = now(), updated_at = now()
         WHERE id = $2 AND tenant_id = $3",
    )
    .bind(new_hash)
    .bind(user.user_id)
    .bind(user.tenant_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO sys.sys_password_history (id, tenant_id, user_id, password_hash)
         VALUES (gen_random_uuid(), $1, $2, $3)",
    )
    .bind(user.tenant_id)
    .bind(user.user_id)
    .bind(&current_hash)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "DELETE FROM sys.sys_password_history
         WHERE tenant_id = $1 AND user_id = $2
           AND id NOT IN (
               SELECT id FROM sys.sys_password_history
               WHERE tenant_id = $1 AND user_id = $2
               ORDER BY created_at DESC LIMIT $3
           )",
    )
    .bind(user.tenant_id)
    .bind(user.user_id)
    .bind(policy.history_count.max(1))
    .execute(&mut *tx)
    .await?;
    if req.revoke_tokens {
        sqlx::query(
            "UPDATE sys.sys_api_token SET revoked_at = now()
             WHERE tenant_id = $1 AND user_id = $2 AND revoked_at IS NULL",
        )
        .bind(user.tenant_id)
        .bind(user.user_id)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    let keep = if user.must_change_password {
        None
    } else {
        user.session_id
    };
    let _revoked = state
        .sessions
        .revoke_user_sessions(user.tenant_id, user.user_id, keep)
        .await
        .map_err(AppError::Database)?;

    crate::audit::record_user(
        &state.db,
        &user,
        "auth.password.change",
        "user",
        Some(&user.user_id.to_string()),
        None,
    )
    .await;
    Ok(ApiResponse::new(()))
}
