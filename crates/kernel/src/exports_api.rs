//! 导出任务 HTTP 接口：列表、创建、下载、删除（后台执行位于 `exports_runner`）。

use crate::auth::CurrentUser;
use crate::error::AppError;
use crate::response::ApiResponse;
use crate::response::{Page, PageMeta, PageQuery};
use crate::state::AppState;
use crate::tenant::TenantTx;
use axum::extract::{Path, Query, State};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use crate::exports::find_export;
use crate::exports_runner::{mark_failed, run_export, ExportJob};

/// 导出任务路由（挂在根路径下）。
pub fn routes() -> axum::Router<AppState> {
    use axum::routing::{delete, get};
    axum::Router::new()
        .route(
            "/api/system/export-tasks",
            get(list_tasks).post(create_task),
        )
        .route(
            "/api/system/export-tasks/{id}/download",
            get(crate::exports_download::download),
        )
        .route("/api/system/export-tasks/{id}", delete(delete_task))
}

/// 导出任务视图。
#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExportTaskVo {
    /// 任务 id。
    pub id: Uuid,
    /// 导出类型。
    pub kind: String,
    /// 关键字过滤。
    pub keyword: Option<String>,
    /// 状态：pending / running / success / failed。
    pub status: String,
    /// 生成的文件名。
    pub file_name: Option<String>,
    /// 行数。
    pub total_rows: Option<i64>,
    /// 失败原因。
    pub error: Option<String>,
    /// 创建时间。
    pub created_at: DateTime<Utc>,
    /// 完成时间。
    pub finished_at: Option<DateTime<Utc>>,
    /// 筛选条件快照。
    pub filters: Option<serde_json::Value>,
}

/// 创建导出任务请求。
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateExportRequest {
    /// 导出类型：system.audit / system.users。
    pub kind: String,
    /// 关键字过滤（可选）。
    pub keyword: Option<String>,
    /// 资源类型过滤（可选，仅审计导出使用）。
    pub resource_type: Option<String>,
    /// 来源 IP 过滤（可选，仅审计导出使用）。
    pub ip: Option<String>,
    /// User-Agent 过滤（可选，仅审计导出使用）。
    pub user_agent: Option<String>,
    /// 起始时间（可选）。
    pub from: Option<DateTime<Utc>>,
    /// 结束时间（可选）。
    pub to: Option<DateTime<Utc>>,
}

/// 导出任务列表（只看自己的）。
#[utoipa::path(
    get,
    path = "/api/system/export-tasks",
    tag = "system",
    params(("page" = Option<u64>, Query, description = "页码"), ("size" = Option<u64>, Query, description = "每页条数")),
    responses((status = 200, description = "任务分页列表", body = ApiResponse<Page<ExportTaskVo>>))
)]
async fn list_tasks(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Query(page): Query<PageQuery>,
) -> Result<ApiResponse<Page<ExportTaskVo>>, AppError> {
    if !user.has_permission(crate::perms::EXPORT_LIST) {
        return Err(AppError::Forbidden);
    }
    let rows = sqlx::query_as::<_, ExportTaskVo>(
        "SELECT id, kind, keyword, status, file_name, total_rows, error, created_at, finished_at, filters
         FROM sys.sys_export_task WHERE tenant_id = $1 AND user_id = $2
         ORDER BY created_at DESC, id LIMIT $3 OFFSET $4",
    )
    .bind(user.tenant_id)
    .bind(user.user_id)
    .bind(page.limit())
    .bind(page.offset())
    .fetch_all(&mut *tx)
    .await?;
    let total: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM sys.sys_export_task WHERE tenant_id = $1 AND user_id = $2",
    )
    .bind(user.tenant_id)
    .bind(user.user_id)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(ApiResponse::new(Page {
        data: rows,
        page: PageMeta::new(page.page(), page.size(), total as u64),
    }))
}

/// 同一用户同时进行中的导出任务上限。
///
/// 导出会做 20k 行查询 + xlsx 生成，属于重活；没有准入闸门时，
/// 一个有导出权限的账号可以刷出任意多个并发任务把连接池与内存吃满。
const MAX_INFLIGHT_TASKS: i64 = 5;

/// 创建导出任务（立即返回，后台生成）。
#[utoipa::path(
    post,
    path = "/api/system/export-tasks",
    tag = "system",
    request_body = CreateExportRequest,
    responses((status = 200, description = "已创建的任务", body = ApiResponse<ExportTaskVo>))
)]
async fn create_task(
    State(state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    axum::Json(req): axum::Json<CreateExportRequest>,
) -> Result<ApiResponse<ExportTaskVo>, AppError> {
    let kind = req.kind.trim();
    let required = match kind {
        "system.audit" => "system:audit:list",
        "system.users" => "system:user:list",
        other => find_export(other)
            .map(|r| r.permission)
            .ok_or_else(|| AppError::Validation("不支持的导出类型".into()))?,
    };
    if !user.has_permission(required) {
        return Err(AppError::Forbidden);
    }

    let inflight: i64 = sqlx::query_scalar(
        "SELECT count(*)::bigint FROM sys.sys_export_task
         WHERE tenant_id = $1 AND user_id = $2 AND status IN ('pending', 'running')",
    )
    .bind(user.tenant_id)
    .bind(user.user_id)
    .fetch_one(&mut *tx)
    .await?;
    if inflight >= MAX_INFLIGHT_TASKS {
        tracing::warn!(user = %user.username, inflight, "导出任务在途数已达上限");
        return Err(AppError::RateLimited);
    }

    let task_id = Uuid::now_v7();
    let filters = serde_json::json!({
        "resourceType": req.resource_type, "ip": req.ip, "userAgent": req.user_agent, "from": req.from, "to": req.to
    });
    let vo = sqlx::query_as::<_, ExportTaskVo>(
        "INSERT INTO sys.sys_export_task (id, tenant_id, user_id, kind, keyword, filters)
         VALUES ($1, $2, $3, $4, $5, $6)
         RETURNING id, kind, keyword, status, file_name, total_rows, error, created_at, finished_at, filters",
    )
    .bind(task_id)
    .bind(user.tenant_id)
    .bind(user.user_id)
    .bind(kind)
    .bind(&req.keyword)
    .bind(&filters)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;

    crate::audit::record_user(
        &state.db,
        &user,
        "system.export.create",
        "export",
        Some(&task_id.to_string()),
        Some(serde_json::json!({ "kind": kind })),
    )
    .await;

    let state_clone = state.clone();
    let job = ExportJob {
        task_id,
        tenant_id: user.tenant_id,
        user_id: user.user_id,
        kind: kind.to_string(),
        keyword: req.keyword,
        resource_type: req.resource_type,
        ip: req.ip,
        user_agent: req.user_agent,
        from: req.from,
        to: req.to,
        data_scope: user.data_scope,
        dept_id: user.dept_id,
    };
    let tenant_id = user.tenant_id;
    let user_id = user.user_id;
    tokio::spawn(async move {
        if let Err(e) = run_export(&state_clone, job).await {
            // 错误链可能含 SQL、文件路径、内部结构：只写日志（连 cause 一起），
            // 落库与回传客户端的一律是稳定文案。
            tracing::error!(task = %task_id, error = %format!("{e:#}"), "导出任务失败");
            mark_failed(
                &state_clone,
                task_id,
                tenant_id,
                user_id,
                "导出失败（内部错误），请稍后重试或联系管理员",
            )
            .await;
        }
    });

    Ok(ApiResponse::new(vo))
}

/// 删除导出任务（同时清理已生成的文件）。
#[utoipa::path(
    delete,
    path = "/api/system/export-tasks/{id}",
    tag = "system",
    params(("id" = Uuid, Path, description = "任务 id")),
    responses((status = 200, description = "删除成功"))
)]
async fn delete_task(
    State(state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Path(id): Path<Uuid>,
) -> Result<ApiResponse<()>, AppError> {
    if !user.has_permission(crate::perms::EXPORT_LIST) {
        return Err(AppError::Forbidden);
    }
    let key: Option<Option<String>> = sqlx::query_scalar(
        "DELETE FROM sys.sys_export_task WHERE id = $1 AND tenant_id = $2 AND user_id = $3 RETURNING file_key",
    )
    .bind(id)
    .bind(user.tenant_id)
    .bind(user.user_id)
    .fetch_optional(&mut *tx)
    .await?;
    let key = key.ok_or(AppError::NotFound)?;
    tx.commit().await?;
    crate::audit::record_user(
        &state.db,
        &user,
        "system.export.delete",
        "sys_export_task",
        Some(&id.to_string()),
        None,
    )
    .await;
    if let Some(key) = key {
        if let Err(e) = state.storage.remove(&key).await {
            tracing::warn!(error = %e, key = %key, "删除导出文件失败");
        }
    }
    Ok(ApiResponse::new(()))
}
