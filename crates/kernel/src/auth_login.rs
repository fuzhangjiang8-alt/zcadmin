//! 登录接口。
//!
//! 边界：会话存储在 `session`；登录策略在 `auth_policy`。
//! 关键约束：登录失败统一 401；成功后写会话与审计。

use axum::http::header;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::auth::CurrentUser;
use crate::auth_identity::{load_data_scope, load_dept, load_permissions, resolve_tenant, UserRow};
use crate::auth_policy::{load_login_policy, record_new_device_if_changed};
use crate::auth_session_api::build_session_cookie;
use crate::client_ip::ClientMeta;
use crate::error::AppError;
use crate::password;
use crate::response::ApiResponse;
use crate::session::Session;
use crate::state::AppState;
use crate::tenant::TenantTx;
use uuid::Uuid;

/// 登录路由（挂在根路径下，带 IP 维度限流中间件）。
pub(crate) fn router() -> Router<AppState> {
    Router::new().route(
        "/api/auth/login",
        post(login).layer(axum::middleware::from_fn(crate::ratelimit::login)),
    )
}

/// 登录请求体。
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct LoginRequest {
    /// 用户名。
    pub username: String,
    /// 密码。
    pub password: String,
    /// 租户编码。单租户部署可留空；多租户必须填写。
    pub tenant: Option<String>,
}

/// 登录响应体（令牌仅通过 HttpOnly Cookie 下发，响应体不含明文令牌）。
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LoginResponse {
    /// 用户信息。
    pub user: CurrentUser,
    /// 会话过期时间。
    pub expires_at: DateTime<Utc>,
}

/// 登录：查库 → 校验密码 → 计算权限与数据范围 → 建会话 → 写 HttpOnly Cookie。
#[utoipa::path(
    post,
    path = "/api/auth/login",
    tag = "auth",
    request_body = LoginRequest,
    responses(
        (status = 200, description = "登录成功，会话写入 HttpOnly Cookie", body = LoginResponse),
        (status = 401, description = "用户名或密码错误"),
        (status = 422, description = "参数校验失败")
    )
)]
async fn login(
    axum::extract::State(state): axum::extract::State<AppState>,
    client: ClientMeta,
    Json(req): Json<LoginRequest>,
) -> Result<Response, AppError> {
    if req.username.trim().is_empty() || req.password.is_empty() {
        return Err(AppError::Validation("用户名和密码不能为空".into()));
    }

    let tenant_id = resolve_tenant(&state, req.tenant.as_deref()).await?;
    let mut tx = TenantTx::begin_tenant(&state.db, tenant_id).await?;

    let row = sqlx::query_as::<_, UserRow>(
        "SELECT id, tenant_id, username, password_hash, status, is_super, failed_attempts,
                locked_until, must_change_password, password_changed_at
         FROM sys.sys_user WHERE username = $1 AND tenant_id = $2 AND deleted_at IS NULL LIMIT 1",
    )
    .bind(req.username.trim())
    .bind(tenant_id)
    .fetch_optional(&mut *tx)
    .await?;

    // 时序对齐：用户不存在 / 停用 / 锁定时均付出等价 Argon2 成本，统一返回 401 防枚举。
    let Some(row) = row else {
        password::burn_verification_time().await?;
        return Err(AppError::Unauthorized);
    };
    if row.status != "active" || row.locked_until.is_some_and(|u| u > Utc::now()) {
        password::burn_verification_time().await?;
        return Err(AppError::Unauthorized);
    }

    let client_ip = client.ip.clone();
    let user_agent = client.user_agent.clone();
    if !password::verify_password_async(req.password.clone(), row.password_hash.clone()).await? {
        record_failed_login(
            &state,
            tx,
            &row,
            tenant_id,
            client_ip.as_deref(),
            user_agent.as_deref(),
        )
        .await?;
        return Err(AppError::Unauthorized);
    }

    if row.failed_attempts > 0 || row.locked_until.is_some() {
        sqlx::query(
            "UPDATE sys.sys_user SET failed_attempts = 0, locked_until = NULL WHERE id = $1",
        )
        .bind(row.id)
        .execute(&mut *tx)
        .await?;
    }

    record_new_device_if_changed(
        &mut tx,
        row.tenant_id,
        row.id,
        client_ip.as_deref(),
        user_agent.as_deref(),
    )
    .await?;

    let policy = password::load_policy(&mut tx, row.tenant_id).await?;
    let expired = policy.expires_days > 0
        && (Utc::now() - row.password_changed_at).num_days() >= i64::from(policy.expires_days);
    let must_change_password = row.must_change_password || expired;
    if expired && !row.must_change_password {
        sqlx::query("UPDATE sys.sys_user SET must_change_password = true WHERE id = $1")
            .bind(row.id)
            .execute(&mut *tx)
            .await?;
    }

    let permissions = if row.is_super {
        vec!["*".to_string()]
    } else {
        load_permissions(&mut tx, row.id).await?
    };
    let dept_id = load_dept(&mut tx, row.id).await?;
    let data_scope = if row.is_super {
        "all".to_string()
    } else {
        load_data_scope(&mut tx, row.id).await?
    };
    tx.commit().await?;

    let ttl = state.config.auth.session_ttl_secs;
    let expires_at = Utc::now() + Duration::seconds(ttl);
    let session_id = Uuid::now_v7();
    let token = state
        .sessions
        .create(
            Session {
                id: session_id,
                user_id: row.id,
                tenant_id: row.tenant_id,
                dept_id,
                ip: client_ip.clone(),
                user_agent: user_agent.clone(),
                data_scope: data_scope.clone(),
                username: row.username.clone(),
                permissions: permissions.clone(),
                expires_at,
                created_at: Utc::now(),
                must_change_password,
            },
            state.config.auth.single_session,
        )
        .await
        .map_err(AppError::Database)?;

    let cookie = build_session_cookie(&token, ttl);
    let detail = serde_json::json!({ "ip": client_ip, "userAgent": user_agent });
    let user = CurrentUser {
        user_id: row.id,
        tenant_id: row.tenant_id,
        dept_id,
        data_scope,
        username: row.username,
        permissions,
        session_id: Some(session_id),
        must_change_password,
        ip: client_ip.clone(),
        user_agent: user_agent.clone(),
    };
    crate::audit::record_with_meta(
        &state.db,
        row.tenant_id,
        Some(row.id),
        "auth.login.success",
        "sys_user",
        Some(&row.id.to_string()),
        client_ip.as_deref(),
        user_agent.as_deref(),
        Some(detail),
    )
    .await;
    tracing::info!(user = %req.username, "登录成功");
    Ok((
        [(header::SET_COOKIE, cookie)],
        Json(ApiResponse::new(LoginResponse { user, expires_at })),
    )
        .into_response())
}

/// 密码校验失败：累加错误计数、按策略触发预警或锁定，提交事务后写审计日志。
async fn record_failed_login(
    state: &AppState,
    mut tx: TenantTx,
    row: &UserRow,
    tenant_id: Uuid,
    client_ip: Option<&str>,
    user_agent: Option<&str>,
) -> Result<(), AppError> {
    let attempts = row.failed_attempts + 1;
    let login_policy = load_login_policy(&mut tx, tenant_id).await?;
    let max = login_policy.max_failed_attempts.max(1);
    let locked = attempts >= max;
    let locked_until = Utc::now() + Duration::seconds(login_policy.lock_seconds.max(60));

    sqlx::query(
        "UPDATE sys.sys_user SET failed_attempts = $1, \
         locked_until = CASE WHEN $2 THEN $3 ELSE locked_until END WHERE id = $4",
    )
    .bind(attempts)
    .bind(locked)
    .bind(locked_until)
    .bind(row.id)
    .execute(&mut *tx)
    .await?;

    let warn_threshold = (max * login_policy.warning_percent.clamp(1, 99) / 100).clamp(1, max);
    let alert_action = if locked {
        Some("auth.login.locked")
    } else if attempts == warn_threshold {
        Some("auth.login.failed.warning")
    } else {
        None
    };
    if let Some(action) = alert_action {
        sqlx::query(
            "INSERT INTO sys.sys_audit_alert (id, tenant_id, user_id, action, window_start, window_end, count, threshold)
             VALUES (gen_random_uuid(), $1, $2, $3, date_trunc('minute', now()), now(), 1, 1)
             ON CONFLICT (tenant_id, action, window_start)
             DO UPDATE SET count = sys.sys_audit_alert.count + 1, window_end = now()",
        )
        .bind(row.tenant_id)
        .bind(row.id)
        .bind(action)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;

    let action = if locked {
        "auth.login.locked"
    } else {
        "auth.login.failed"
    };
    crate::audit::record_with_meta(
        &state.db,
        row.tenant_id,
        Some(row.id),
        action,
        "sys_user",
        Some(&row.id.to_string()),
        client_ip,
        user_agent,
        Some(serde_json::json!({ "attempts": attempts, "locked": locked, "ip": client_ip, "userAgent": user_agent })),
    )
    .await;
    tracing::warn!(username = %row.username, attempts, locked, "登录失败：密码错误");
    Ok(())
}
