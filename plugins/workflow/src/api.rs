//! 审批流 HTTP 接口处理器（列表、统计、详情、发起、决议流转）。
//!
//! 边界：只做参数绑定、权限与数据范围校验、事务提交与审计记录；
//! 领域校验规则统一位于 [`crate::model`]。

use crate::model::{
    resolve_target_status, CreateWorkflowRequest, DecideWorkflowRequest, WorkflowQuery,
    WorkflowRequestVo, WorkflowStatsVo,
};
use crate::perm;
use kernel::prelude::*;
use kernel::response::{Page, PageMeta};

/// 数据权限 SQL 片段（复用内核统一实现，按负责人 `w.owner_id` 与部门 `w.dept_id` 过滤）。
fn scope_sql(scope: &str, user_idx: &str, dept_idx: &str) -> String {
    kernel::scope::condition("w", scope, user_idx, dept_idx)
}

/// 分页查询审批单列表（带数据范围过滤）。
#[utoipa::path(
    get,
    path = "/api/workflow/requests",
    tag = "workflow",
    params(
        ("page" = Option<u64>, Query, description = "页码"),
        ("size" = Option<u64>, Query, description = "每页条数"),
        ("status" = Option<String>, Query, description = "状态"),
        ("bizType" = Option<String>, Query, description = "业务类型"),
        ("keyword" = Option<String>, Query, description = "关键字")
    ),
    responses((status = 200, description = "审批单分页列表", body = ApiResponse<Page<WorkflowRequestVo>>))
)]
pub async fn list_requests(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Query(q): Query<WorkflowQuery>,
) -> ApiResult<Page<WorkflowRequestVo>> {
    perm::require(&user, perm::REQUEST_LIST)?;
    let scope = scope_sql("$2", "$3", "$4");
    let kw = q
        .keyword
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| format!("%{s}%"));
    let status = q.status.as_deref().filter(|s| !s.is_empty());
    let biz_type = q.biz_type.as_deref().filter(|s| !s.is_empty());

    let where_clause = format!(
        "WHERE w.tenant_id = $1 AND w.deleted_at IS NULL
           AND ($5::text IS NULL OR w.status = $5)
           AND ($6::text IS NULL OR w.biz_type = $6)
           AND ($7::text IS NULL OR w.code ILIKE $7 OR w.title ILIKE $7)
           {scope}"
    );
    let list_sql = format!(
        "SELECT w.id, w.code, w.title, w.biz_type, w.biz_id, w.status, w.amount_cents,
                w.remark, w.decision_note, w.decided_by, w.decided_at,
                w.created_by, w.created_at, w.version
         FROM workflow.workflow_request w {where_clause}
         ORDER BY w.created_at DESC, w.id LIMIT $8 OFFSET $9"
    );
    let rows = sqlx::query_as::<_, WorkflowRequestVo>(&list_sql)
        .bind(user.tenant_id)
        .bind(&user.data_scope)
        .bind(user.user_id)
        .bind(user.dept_id)
        .bind(status)
        .bind(biz_type)
        .bind(kw.as_deref())
        .bind(q.page.limit())
        .bind(q.page.offset())
        .fetch_all(&mut *tx)
        .await?;

    let count_sql = format!("SELECT count(*) FROM workflow.workflow_request w {where_clause}");
    let total: i64 = sqlx::query_scalar(&count_sql)
        .bind(user.tenant_id)
        .bind(&user.data_scope)
        .bind(user.user_id)
        .bind(user.dept_id)
        .bind(status)
        .bind(biz_type)
        .bind(kw.as_deref())
        .fetch_one(&mut *tx)
        .await?;

    tx.commit().await?;
    Ok(ApiResponse::new(Page {
        data: rows,
        page: PageMeta::new(q.page.page(), q.page.size(), total as u64),
    }))
}

/// 统计当前用户可见范围内的审批单各状态数量。
#[utoipa::path(
    get,
    path = "/api/workflow/requests/stats",
    tag = "workflow",
    responses((status = 200, description = "审批单状态统计", body = ApiResponse<WorkflowStatsVo>))
)]
pub async fn request_stats(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
) -> ApiResult<WorkflowStatsVo> {
    perm::require(&user, perm::REQUEST_LIST)?;
    let sql = format!(
        r#"
        SELECT count(*)::bigint AS total,
               count(*) FILTER (WHERE w.status = 'pending')::bigint AS pending,
               count(*) FILTER (WHERE w.status = 'approved')::bigint AS approved,
               count(*) FILTER (WHERE w.status = 'rejected')::bigint AS rejected,
               count(*) FILTER (WHERE w.status = 'cancelled')::bigint AS cancelled
        FROM workflow.workflow_request w
        WHERE w.tenant_id = $1 AND w.deleted_at IS NULL
          {}
        "#,
        scope_sql("$2", "$3", "$4")
    );
    let stats = sqlx::query_as::<_, WorkflowStatsVo>(&sql)
        .bind(user.tenant_id)
        .bind(&user.data_scope)
        .bind(user.user_id)
        .bind(user.dept_id)
        .fetch_one(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(ApiResponse::new(stats))
}

/// 查询审批单详情（越权或不存在统一返回 404）。
#[utoipa::path(
    get,
    path = "/api/workflow/requests/{id}",
    tag = "workflow",
    params(("id" = Uuid, Path, description = "审批单 ID")),
    responses((status = 200, description = "审批单详情", body = ApiResponse<WorkflowRequestVo>))
)]
pub async fn get_request(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Path(id): Path<Uuid>,
) -> ApiResult<WorkflowRequestVo> {
    perm::require(&user, perm::REQUEST_GET)?;
    let sql = format!(
        r#"
        SELECT w.id, w.code, w.title, w.biz_type, w.biz_id, w.status, w.amount_cents,
               w.remark, w.decision_note, w.decided_by, w.decided_at,
               w.created_by, w.created_at, w.version
        FROM workflow.workflow_request w
        WHERE w.id = $1 AND w.tenant_id = $2 AND w.deleted_at IS NULL
          {}
        "#,
        scope_sql("$3", "$4", "$5")
    );
    let item = sqlx::query_as::<_, WorkflowRequestVo>(&sql)
        .bind(id)
        .bind(user.tenant_id)
        .bind(&user.data_scope)
        .bind(user.user_id)
        .bind(user.dept_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(AppError::NotFound)?;

    tx.commit().await?;
    Ok(ApiResponse::new(item))
}

/// 发起审批单。
#[utoipa::path(
    post,
    path = "/api/workflow/requests",
    tag = "workflow",
    request_body = CreateWorkflowRequest,
    responses((status = 200, description = "创建成功", body = ApiResponse<WorkflowRequestVo>))
)]
pub async fn create_request(
    State(state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Json(req): Json<CreateWorkflowRequest>,
) -> ApiResult<WorkflowRequestVo> {
    perm::require(&user, perm::REQUEST_ADD)?;
    req.validate()?;

    let item = sqlx::query_as::<_, WorkflowRequestVo>(
        r#"
        INSERT INTO workflow.workflow_request
            (id, tenant_id, code, title, biz_type, biz_id, status, amount_cents, remark, owner_id, dept_id, created_by)
        VALUES ($1, $2, $3, $4, $5, $6, 'pending', $7, $8, $9, $10, $9)
        RETURNING id, code, title, biz_type, biz_id, status, amount_cents,
                  remark, decision_note, decided_by, decided_at, created_by, created_at, version
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(user.tenant_id)
    .bind(req.code.trim())
    .bind(req.title.trim())
    .bind(req.biz_type.trim())
    .bind(req.biz_id.as_deref().map(str::trim).filter(|s| !s.is_empty()))
    .bind(req.amount_cents)
    .bind(req.remark.as_deref().map(str::trim).filter(|s| !s.is_empty()))
    .bind(user.user_id)
    .bind(user.dept_id)
    .fetch_one(&mut *tx)
    .await?;

    tx.commit().await?;
    kernel::audit::record_user(
        &state.db,
        &user,
        "workflow.request.create",
        "workflow_request",
        Some(&item.id.to_string()),
        Some(serde_json::json!({ "code": item.code, "bizType": item.biz_type, "amountCents": item.amount_cents })),
    )
    .await;
    Ok(ApiResponse::new(item))
}

/// 审批决议（通过 / 驳回 / 撤回）。
#[utoipa::path(
    post,
    path = "/api/workflow/requests/{id}/decide",
    tag = "workflow",
    params(("id" = Uuid, Path, description = "审批单 ID")),
    request_body = DecideWorkflowRequest,
    responses((status = 200, description = "状态流转后的审批单", body = ApiResponse<WorkflowRequestVo>))
)]
pub async fn decide_request(
    State(state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Path(id): Path<Uuid>,
    Json(req): Json<DecideWorkflowRequest>,
) -> ApiResult<WorkflowRequestVo> {
    perm::require(&user, perm::REQUEST_APPROVE)?;
    let target_status = resolve_target_status(&req.action)?;
    if let Some(note) = &req.note {
        if note.chars().count() > 500 {
            return Err(AppError::Validation("审批意见最长 500 字符".into()));
        }
    }

    let sql = format!(
        r#"
        UPDATE workflow.workflow_request w
        SET status = $3,
            decision_note = $4,
            decided_by = $5,
            decided_at = now(),
            version = w.version + 1
        WHERE w.id = $1 AND w.tenant_id = $2 AND w.deleted_at IS NULL AND w.status = 'pending'
          {}
        RETURNING w.id, w.code, w.title, w.biz_type, w.biz_id, w.status, w.amount_cents,
                  w.remark, w.decision_note, w.decided_by, w.decided_at,
                  w.created_by, w.created_at, w.version
        "#,
        scope_sql("$6", "$7", "$8")
    );
    let updated = sqlx::query_as::<_, WorkflowRequestVo>(&sql)
        .bind(id)
        .bind(user.tenant_id)
        .bind(target_status)
        .bind(req.note.as_deref().map(str::trim).filter(|s| !s.is_empty()))
        .bind(user.user_id)
        .bind(&user.data_scope)
        .bind(user.user_id)
        .bind(user.dept_id)
        .fetch_optional(&mut *tx)
        .await?;

    let Some(item) = updated else {
        return Err(AppError::Validation(
            "审批单不存在、无权访问或已处于终态（仅待审批状态可操作）".into(),
        ));
    };

    tx.commit().await?;
    kernel::audit::record_user(
        &state.db,
        &user,
        "workflow.request.decide",
        "workflow_request",
        Some(&item.id.to_string()),
        Some(serde_json::json!({ "action": req.action, "status": item.status, "version": item.version })),
    )
    .await;
    Ok(ApiResponse::new(item))
}
