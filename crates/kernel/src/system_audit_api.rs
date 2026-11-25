//! 系统审计日志查询接口。
//!
//! 边界：审计写入在 `audit`；本模块只做查询。
//! 关键约束：按租户隔离；动作列表用于前端筛选。

use axum::extract::{Query, State};
use axum::routing::get;
use axum::Router;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use crate::auth::CurrentUser;
use crate::error::AppError;
use crate::response::{ApiResponse, Page, PageMeta, PageQuery};
use crate::state::AppState;
use crate::system::require_perm;
use crate::tenant::TenantTx;

/// 审计查询路由（挂在 `/api/system` 下）。
pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route("/audit-logs", get(list_audit_logs))
        .route("/audit-logs/actions", get(list_audit_actions))
}

/// 审计日志视图对象。
#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AuditLogVo {
    /// 日志 id。
    pub id: Uuid,
    /// 操作人（系统任务可能为空）。
    pub user_id: Option<Uuid>,
    /// 动作，例如 system.user.create。
    pub action: String,
    /// 资源类型。
    pub resource_type: String,
    /// 资源 id。
    pub resource_id: Option<String>,
    /// 结果（目前只记 success）。
    pub result: String,
    /// 来源 IP。
    pub ip: Option<String>,
    /// 客户端 User-Agent。
    pub user_agent: Option<String>,
    /// 时间。
    pub created_at: DateTime<Utc>,
    /// 变更摘要。
    pub detail: Option<serde_json::Value>,
}

/// 审计日志查询参数。
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AuditQuery {
    /// 分页参数。
    #[serde(flatten)]
    pub page: PageQuery,
    /// 动作关键字（模糊匹配）。
    pub action: Option<String>,
    /// 资源类型（精确匹配，如 customer / contract / payment）。
    pub resource_type: Option<String>,
    /// 来源 IP（精确匹配）。
    pub ip: Option<String>,
    /// User-Agent（模糊匹配）。
    pub user_agent: Option<String>,
    /// 起始时间（可选）。
    pub from: Option<DateTime<Utc>>,
    /// 结束时间（可选）。
    pub to: Option<DateTime<Utc>>,
}

/// 审计日志列表（只读，不可修改）。
#[utoipa::path(
    get,
    path = "/api/system/audit-logs",
    tag = "system",
    params(
        ("page" = Option<u64>, Query, description = "页码"),
        ("size" = Option<u64>, Query, description = "每页条数"),
        ("action" = Option<String>, Query, description = "动作关键字"),
        ("resourceType" = Option<String>, Query, description = "资源类型"),
        ("ip" = Option<String>, Query, description = "来源 IP"),
        ("userAgent" = Option<String>, Query, description = "客户端 User-Agent"),
        ("from" = Option<DateTime<Utc>>, Query, description = "起始时间"),
        ("to" = Option<DateTime<Utc>>, Query, description = "结束时间")
    ),
    responses((status = 200, description = "审计日志分页列表", body = ApiResponse<Page<AuditLogVo>>))
)]
async fn list_audit_logs(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Query(q): Query<AuditQuery>,
) -> Result<ApiResponse<Page<AuditLogVo>>, AppError> {
    require_perm(&user, crate::perms::AUDIT_LIST)?;

    let keyword = q
        .action
        .as_deref()
        .map(|v| format!("%{}%", v.trim()))
        .filter(|v| v != "%%");
    let ua_like = q
        .user_agent
        .as_deref()
        .map(|v| format!("%{}%", v.trim()))
        .filter(|v| v != "%%");
    let ip = q.ip.as_deref().map(str::trim).filter(|v| !v.is_empty());
    let resource_type = q
        .resource_type
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty());

    let rows = sqlx::query_as::<_, AuditLogVo>(
        r#"
        SELECT id, user_id, action, resource_type, resource_id, result,
               host(ip) AS ip, user_agent, created_at, detail
        FROM sys.sys_audit_log
        WHERE tenant_id = $1
          AND ($2::text IS NULL OR action ILIKE $2)
          AND ($3::text IS NULL OR resource_type = $3 OR resource_type = 'sys_' || $3)
          AND ($4::text IS NULL OR host(ip) = $4)
          AND ($5::text IS NULL OR user_agent ILIKE $5)
          AND ($6::timestamptz IS NULL OR created_at >= $6)
          AND ($7::timestamptz IS NULL OR created_at <= $7)
        ORDER BY created_at DESC, id
        LIMIT $8 OFFSET $9
        "#,
    )
    .bind(user.tenant_id)
    .bind(keyword.as_deref())
    .bind(resource_type)
    .bind(ip)
    .bind(ua_like.as_deref())
    .bind(q.from)
    .bind(q.to)
    .bind(q.page.limit())
    .bind(q.page.offset())
    .fetch_all(&mut *tx)
    .await?;

    let total: i64 = sqlx::query_scalar(
        r#"
        SELECT count(*) FROM sys.sys_audit_log
        WHERE tenant_id = $1
          AND ($2::text IS NULL OR action ILIKE $2)
          AND ($3::text IS NULL OR resource_type = $3 OR resource_type = 'sys_' || $3)
          AND ($4::text IS NULL OR host(ip) = $4)
          AND ($5::text IS NULL OR user_agent ILIKE $5)
          AND ($6::timestamptz IS NULL OR created_at >= $6)
          AND ($7::timestamptz IS NULL OR created_at <= $7)
        "#,
    )
    .bind(user.tenant_id)
    .bind(keyword.as_deref())
    .bind(resource_type)
    .bind(ip)
    .bind(ua_like.as_deref())
    .bind(q.from)
    .bind(q.to)
    .fetch_one(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(ApiResponse::new(Page {
        data: rows,
        page: PageMeta::new(q.page.page(), q.page.size(), total as u64),
    }))
}

/// 审计动作枚举（供筛选下拉）。
#[utoipa::path(
    get,
    path = "/api/system/audit-logs/actions",
    tag = "system",
    responses((status = 200, description = "动作列表", body = ApiResponse<Vec<String>>))
)]
async fn list_audit_actions(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
) -> Result<ApiResponse<Vec<String>>, AppError> {
    require_perm(&user, crate::perms::AUDIT_LIST)?;
    let rows: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT action FROM sys.sys_audit_log WHERE tenant_id = $1 ORDER BY action LIMIT 200",
    )
    .bind(user.tenant_id)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(ApiResponse::new(rows))
}
