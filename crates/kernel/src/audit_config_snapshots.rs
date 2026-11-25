//! 审计配置快照：列表、删除与清理。
//!
//! 边界：快照导出/对比/改名在 `audit_config_snapshot_tools`。
//! 关键约束：按租户隔离；清理只删超过保留期的快照。

use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::Router;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::audit_config_bundle::{AuditConfigBundle, AuditConfigDiffVo};
use crate::auth::CurrentUser;
use crate::error::AppError;
use crate::response::{ApiResponse, Page, PageMeta, PageQuery};
use crate::state::AppState;
use crate::system::require_perm;
use crate::tenant::TenantTx;

/// 配置快照路由（挂在 `/api/system` 下）。
pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/audit-config/snapshots",
            get(list_config_snapshots).delete(cleanup_config_snapshots),
        )
        .route(
            "/audit-config/snapshots/{id}",
            axum::routing::delete(delete_config_snapshot),
        )
}

/// 配置快照。
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotVo {
    /// 快照 id。
    pub id: Uuid,
    /// 创建时间。
    pub created_at: DateTime<Utc>,
    /// 创建人。
    pub created_by_name: Option<String>,
    /// 名称。
    pub name: Option<String>,
    /// 备注。
    pub note: Option<String>,
    /// 配置包。
    pub bundle: AuditConfigBundle,
    /// 导入范围。
    pub sections: Option<String>,
    /// 差异摘要。
    pub diff_summary: Option<serde_json::Value>,
    /// 差异详情。
    pub diff_detail: Option<AuditConfigDiffVo>,
}

type SnapshotRow = (
    Uuid,
    DateTime<Utc>,
    Option<String>,
    Option<String>,
    Option<String>,
    sqlx::types::Json<AuditConfigBundle>,
    Option<String>,
    Option<serde_json::Value>,
    Option<sqlx::types::Json<AuditConfigDiffVo>>,
);

/// 配置快照查询。
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotQuery {
    /// 分页参数。
    #[serde(flatten)]
    pub page: PageQuery,
    /// 按导入范围筛选。
    pub sections: Option<String>,
    /// 按创建人筛选。
    pub created_by: Option<String>,
    /// 开始日期（含）。
    pub from: Option<chrono::NaiveDate>,
    /// 结束日期（含）。
    pub to: Option<chrono::NaiveDate>,
}

/// 配置快照列表。
#[utoipa::path(
    get,
    path = "/api/system/audit-config/snapshots",
    tag = "system",
    responses((status = 200, description = "快照分页", body = ApiResponse<Page<SnapshotVo>>))
)]
async fn list_config_snapshots(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Query(q): Query<SnapshotQuery>,
) -> Result<ApiResponse<Page<SnapshotVo>>, AppError> {
    require_perm(&user, crate::perms::AUDIT_ALERT)?;
    let rows: Vec<SnapshotRow> = sqlx::query_as(
        r#"
        SELECT s.id, s.created_at, u.username, s.name, s.note,
               s.bundle, s.sections, s.diff_summary, s.diff_detail
        FROM sys.sys_audit_config_snapshot s
        LEFT JOIN sys.sys_user u ON u.id = s.created_by
        WHERE s.tenant_id = $1
          AND ($2::text IS NULL OR s.sections = $2)
          AND ($3::text IS NULL OR u.username = $3)
          AND ($4::date IS NULL OR s.created_at >= $4::date)
          AND ($5::date IS NULL OR s.created_at < ($5::date + interval '1 day'))
        ORDER BY s.created_at DESC
        LIMIT $6 OFFSET $7
        "#,
    )
    .bind(user.tenant_id)
    .bind(&q.sections)
    .bind(&q.created_by)
    .bind(q.from)
    .bind(q.to)
    .bind(q.page.limit())
    .bind(q.page.offset())
    .fetch_all(&mut *tx)
    .await?;
    let total: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM sys.sys_audit_config_snapshot s
         LEFT JOIN sys.sys_user u ON u.id = s.created_by
         WHERE s.tenant_id = $1
           AND ($2::text IS NULL OR s.sections = $2)
           AND ($3::text IS NULL OR u.username = $3)
           AND ($4::date IS NULL OR s.created_at >= $4::date)
           AND ($5::date IS NULL OR s.created_at < ($5::date + interval '1 day'))",
    )
    .bind(user.tenant_id)
    .bind(&q.sections)
    .bind(&q.created_by)
    .bind(q.from)
    .bind(q.to)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(ApiResponse::new(Page {
        data: rows
            .into_iter()
            .map(
                |(
                    id,
                    created_at,
                    created_by_name,
                    name,
                    note,
                    bundle,
                    sections,
                    diff_summary,
                    diff_detail,
                )| SnapshotVo {
                    id,
                    created_at,
                    created_by_name,
                    name,
                    note,
                    bundle: bundle.0,
                    sections,
                    diff_summary,
                    diff_detail: diff_detail.map(|d| d.0),
                },
            )
            .collect(),
        page: PageMeta::new(q.page.page(), q.page.size(), total as u64),
    }))
}

/// 快照清理参数。
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotCleanupQuery {
    /// 保留最近条数（默认 20）。
    pub keep: Option<i64>,
}

/// 删除单个快照。
#[utoipa::path(
    delete,
    path = "/api/system/audit-config/snapshots/{id}",
    tag = "system",
    params(("id" = Uuid, Path, description = "快照 id")),
    responses((status = 200, description = "已删除"))
)]
async fn delete_config_snapshot(
    State(state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Path(id): Path<Uuid>,
) -> Result<ApiResponse<()>, AppError> {
    require_perm(&user, crate::perms::AUDIT_ALERT)?;
    let result =
        sqlx::query("DELETE FROM sys.sys_audit_config_snapshot WHERE id = $1 AND tenant_id = $2")
            .bind(id)
            .bind(user.tenant_id)
            .execute(&mut *tx)
            .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    tx.commit().await?;
    crate::audit::record_user(
        &state.db,
        &user,
        "system.audit.config.snapshot_delete",
        "audit_config_snapshot",
        Some(&id.to_string()),
        None,
    )
    .await;
    Ok(ApiResponse::new(()))
}

/// 清理旧快照（保留最近 N 条）。
#[utoipa::path(
    delete,
    path = "/api/system/audit-config/snapshots",
    tag = "system",
    params(("keep" = Option<i64>, Query, description = "保留条数，默认 20")),
    responses((status = 200, description = "删除数量", body = ApiResponse<u64>))
)]
async fn cleanup_config_snapshots(
    State(state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Query(q): Query<SnapshotCleanupQuery>,
) -> Result<ApiResponse<u64>, AppError> {
    require_perm(&user, crate::perms::AUDIT_ALERT)?;
    let keep = q.keep.unwrap_or(20).clamp(1, 100);
    let result = sqlx::query(
        "DELETE FROM sys.sys_audit_config_snapshot
         WHERE tenant_id = $1 AND id NOT IN (
             SELECT id FROM sys.sys_audit_config_snapshot
             WHERE tenant_id = $1 ORDER BY created_at DESC LIMIT $2
         )",
    )
    .bind(user.tenant_id)
    .bind(keep)
    .execute(&mut *tx)
    .await?;
    let count = result.rows_affected();
    tx.commit().await?;
    crate::audit::record_user(
        &state.db,
        &user,
        "system.audit.config.snapshot_cleanup",
        "audit_config_snapshot",
        None,
        Some(serde_json::json!({ "count": count, "keep": keep })),
    )
    .await;
    Ok(ApiResponse::new(count))
}
