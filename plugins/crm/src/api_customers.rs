//! CRM 客户查询：列表与详情。
//!
//! 边界：客户写操作在 `api_customer_write`；关联查询在 `api_relations`。
//! 关键约束：列表与详情都按数据权限过滤。

use axum::extract::{Path, Query, State};
use kernel::prelude::*;
use kernel::response::{Page, PageMeta};

use crate::api::scope_filter;
use crate::model::{CustomerQuery, CustomerVo};

/// 客户列表。
#[utoipa::path(
    get,
    path = "/api/crm/customers",
    tag = "crm",
    params(
        ("page" = Option<u64>, Query, description = "页码，从 1 开始"),
        ("size" = Option<u64>, Query, description = "每页条数，最大 200"),
        ("keyword" = Option<String>, Query, description = "编码或名称模糊匹配"),
        ("level" = Option<String>, Query, description = "客户等级"),
        ("status" = Option<String>, Query, description = "生命周期状态")
    ),
    responses((status = 200, description = "客户分页列表", body = ApiResponse<Page<CustomerVo>>))
)]
pub async fn list(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Query(q): Query<CustomerQuery>,
) -> ApiResult<Page<CustomerVo>> {
    crate::perm::require(&user, crate::perm::CUSTOMER_LIST)?;
    let keyword = q
        .keyword
        .as_deref()
        .map(|s| format!("%{}%", s.trim()))
        .filter(|s| s != "%%");

    let sql = format!(
        r#"
        SELECT c.id, c.code, c.name, c.level, c.status, c.owner_id, c.dept_id,
               c.phone, c.email, c.remark, c.created_at, c.updated_at
        FROM crm.customer c
        WHERE c.deleted_at IS NULL
          AND c.tenant_id = $4
          AND ($1::text IS NULL OR c.code ILIKE $1 OR c.name ILIKE $1)
          AND ($2::text IS NULL OR c.level = $2)
          AND ($3::text IS NULL OR c.status = $3)
          {}
        ORDER BY c.created_at DESC, c.id
        LIMIT $5 OFFSET $6
        "#,
        scope_filter("$7", "$8", "$9")
    );

    let rows = sqlx::query_as::<_, CustomerVo>(&sql)
        .bind(keyword.as_deref())
        .bind(q.level.as_deref())
        .bind(q.status.as_deref())
        .bind(user.tenant_id)
        .bind(q.page.limit())
        .bind(q.page.offset())
        .bind(&user.data_scope)
        .bind(user.user_id)
        .bind(user.dept_id)
        .fetch_all(&mut *tx)
        .await?;

    let count_sql = format!(
        r#"
        SELECT count(*)
        FROM crm.customer c
        WHERE c.deleted_at IS NULL
          AND c.tenant_id = $4
          AND ($1::text IS NULL OR c.code ILIKE $1 OR c.name ILIKE $1)
          AND ($2::text IS NULL OR c.level = $2)
          AND ($3::text IS NULL OR c.status = $3)
          {}
        "#,
        scope_filter("$5", "$6", "$7")
    );

    let total: i64 = sqlx::query_scalar(&count_sql)
        .bind(keyword.as_deref())
        .bind(q.level.as_deref())
        .bind(q.status.as_deref())
        .bind(user.tenant_id)
        .bind(&user.data_scope)
        .bind(user.user_id)
        .bind(user.dept_id)
        .fetch_one(&mut *tx)
        .await?;

    tx.commit().await?;
    Ok(ApiResponse::new(Page {
        data: rows,
        page: PageMeta::new(q.page.page(), q.page.size(), total as u64),
    }))
}

/// 客户详情。
#[utoipa::path(
    get,
    path = "/api/crm/customers/{id}",
    tag = "crm",
    params(("id" = Uuid, Path, description = "客户 id")),
    responses((status = 200, description = "客户详情", body = ApiResponse<CustomerVo>))
)]
pub async fn get(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Path(id): Path<Uuid>,
) -> ApiResult<CustomerVo> {
    crate::perm::require(&user, crate::perm::CUSTOMER_GET)?;
    let sql = format!(
        r#"
        SELECT c.id, c.code, c.name, c.level, c.status, c.owner_id, c.dept_id,
               c.phone, c.email, c.remark, c.created_at, c.updated_at
        FROM crm.customer c
        WHERE c.id = $1 AND c.tenant_id = $2 AND c.deleted_at IS NULL
          {}
        "#,
        scope_filter("$3", "$4", "$5")
    );

    let vo = sqlx::query_as::<_, CustomerVo>(&sql)
        .bind(id)
        .bind(user.tenant_id)
        .bind(&user.data_scope)
        .bind(user.user_id)
        .bind(user.dept_id)
        .fetch_optional(&mut *tx)
        .await?
        // 越权与不存在都返回 404，避免探测资源是否存在
        .ok_or(AppError::NotFound)?;

    tx.commit().await?;
    Ok(ApiResponse::new(vo))
}
