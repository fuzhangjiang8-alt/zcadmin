//! 个人 API Token：列表、创建与吊销。
//!
//! 边界：登录/会话/改密在 `auth`；本模块只处理个人令牌。
//! 关键约束：令牌只存哈希；创建时仅返回一次明文。

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use crate::auth::{hash_api_token, CurrentUser};
use crate::error::AppError;
use crate::response::ApiResponse;
use crate::state::AppState;
use crate::tenant::TenantTx;

/// 个人令牌路由（挂在根路径下）。
pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route("/api/auth/tokens", get(list_tokens).post(create_token))
        .route("/api/auth/tokens/{id}", axum::routing::delete(revoke_token))
}

/// 访问令牌视图。
#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TokenVo {
    /// 令牌 id。
    pub id: Uuid,
    /// 名称。
    pub name: String,
    /// 创建时间。
    pub created_at: DateTime<Utc>,
    /// 过期时间（空表示不过期）。
    pub expires_at: Option<DateTime<Utc>>,
    /// 最近使用时间。
    pub last_used_at: Option<DateTime<Utc>>,
    /// 权限范围（空表示全部）。
    pub scopes: Vec<String>,
}

/// 创建令牌请求。
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateTokenRequest {
    /// 名称。
    pub name: String,
    /// 有效天数（空表示不过期）。
    pub expires_in_days: Option<i64>,
    /// 权限范围（空表示全部）。
    pub scopes: Option<Vec<String>>,
}

/// 创建令牌响应（明文只返回一次）。
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreatedTokenVo {
    /// 明文令牌。
    pub token: String,
    /// 令牌信息。
    pub token_info: TokenVo,
}

/// 我的访问令牌。
#[utoipa::path(
    get,
    path = "/api/auth/tokens",
    tag = "auth",
    responses((status = 200, description = "令牌列表", body = ApiResponse<Vec<TokenVo>>))
)]
async fn list_tokens(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
) -> Result<ApiResponse<Vec<TokenVo>>, AppError> {
    let rows: Vec<TokenVo> = sqlx::query_as(
        "SELECT id, name, created_at, expires_at, last_used_at, scopes
         FROM sys.sys_api_token
         WHERE tenant_id = $1 AND user_id = $2 AND revoked_at IS NULL
         ORDER BY created_at DESC",
    )
    .bind(user.tenant_id)
    .bind(user.user_id)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(ApiResponse::new(rows))
}

/// 创建访问令牌。
#[utoipa::path(
    post,
    path = "/api/auth/tokens",
    tag = "auth",
    request_body = CreateTokenRequest,
    responses((status = 200, description = "创建成功，明文只返回一次", body = ApiResponse<CreatedTokenVo>))
)]
async fn create_token(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Json(req): Json<CreateTokenRequest>,
) -> Result<ApiResponse<CreatedTokenVo>, AppError> {
    let name = req.name.trim();
    if name.is_empty() || name.len() > 100 {
        return Err(AppError::Validation(
            "令牌名称不能为空且不超过 100 字符".into(),
        ));
    }
    let expires_at = match req.expires_in_days {
        Some(days) if !(1..=3650).contains(&days) => {
            return Err(AppError::Validation("有效天数必须在 1-3650 之间".into()));
        }
        Some(days) => Some(Utc::now() + Duration::days(days)),
        None => None,
    };
    let scopes: Vec<String> = req
        .scopes
        .unwrap_or_default()
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if scopes.iter().any(|s| s.len() > 100) {
        return Err(AppError::Validation("权限范围过长".into()));
    }
    let token = format!("zc.{}.{}", user.tenant_id, Uuid::new_v4());
    let hash = hash_api_token(&token);
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO sys.sys_api_token
            (id, tenant_id, user_id, name, token_hash, expires_at, scopes)
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(id)
    .bind(user.tenant_id)
    .bind(user.user_id)
    .bind(name)
    .bind(&hash)
    .bind(expires_at)
    .bind(&scopes)
    .execute(&mut *tx)
    .await?;
    let info: TokenVo = sqlx::query_as(
        "SELECT id, name, created_at, expires_at, last_used_at, scopes
         FROM sys.sys_api_token WHERE id = $1",
    )
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(ApiResponse::new(CreatedTokenVo {
        token,
        token_info: info,
    }))
}

/// 吊销某用户的全部有效访问令牌，返回受影响行数。
///
/// # 为什么必须在会话之外单独吊销
///
/// PAT 是**独立凭据**：不受会话 TTL、单端登录与「强制下线」影响。
/// 管理员重置密码、停用账号、停用租户时如果只清 `sys_session`，
/// 攻击者手里的令牌在事件响应之后依然有效（等于轮换失效）。
pub async fn revoke_user_tokens(
    pool: &sqlx::PgPool,
    tenant_id: Uuid,
    user_id: Uuid,
) -> Result<u64, sqlx::Error> {
    revoke_tokens(pool, tenant_id, Some(user_id)).await
}

/// 吊销某租户的全部有效访问令牌（停用/删除租户时调用）。
pub async fn revoke_tenant_tokens(
    pool: &sqlx::PgPool,
    tenant_id: Uuid,
) -> Result<u64, sqlx::Error> {
    revoke_tokens(pool, tenant_id, None).await
}

/// 统一实现：`sys_api_token` 是 FORCE RLS 表，必须在带租户上下文的短事务里写。
async fn revoke_tokens(
    pool: &sqlx::PgPool,
    tenant_id: Uuid,
    user_id: Option<Uuid>,
) -> Result<u64, sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await?;
    let result = sqlx::query(
        "UPDATE sys.sys_api_token SET revoked_at = now()
         WHERE tenant_id = $1 AND revoked_at IS NULL
           AND ($2::uuid IS NULL OR user_id = $2)",
    )
    .bind(tenant_id)
    .bind(user_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(result.rows_affected())
}

/// 吊销访问令牌。
#[utoipa::path(
    delete,
    path = "/api/auth/tokens/{id}",
    tag = "auth",
    params(("id" = Uuid, Path, description = "令牌 id")),
    responses((status = 200, description = "已吊销"))
)]
async fn revoke_token(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    axum::extract::Path(id): axum::extract::Path<Uuid>,
) -> Result<ApiResponse<()>, AppError> {
    let result = sqlx::query(
        "UPDATE sys.sys_api_token SET revoked_at = now()
         WHERE id = $1 AND tenant_id = $2 AND user_id = $3 AND revoked_at IS NULL",
    )
    .bind(id)
    .bind(user.tenant_id)
    .bind(user.user_id)
    .execute(&mut *tx)
    .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    tx.commit().await?;
    Ok(ApiResponse::new(()))
}
