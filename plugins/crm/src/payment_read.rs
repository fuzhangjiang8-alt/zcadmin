//! 回款查询：列表与详情。
//!
//! 边界：回款写操作在 `payment`；本模块只读。
//! 关键约束：按数据权限过滤，详情校验可见性。

use axum::extract::{Path, Query, State};
use kernel::prelude::*;
use kernel::response::Page;

use crate::payment_types::*;

/// 列表。
#[utoipa::path(
    get,
    path = "/api/crm/payments",
    tag = "crm",
    params(
        ("page" = Option<u64>, Query, description = "页码"),
        ("size" = Option<u64>, Query, description = "每页条数")
    ),
    responses((status = 200, description = "分页列表", body = ApiResponse<Page<PaymentVo>>))
)]
pub async fn list(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Query(q): Query<PaymentQuery>,
) -> ApiResult<Page<PaymentVo>> {
    crate::perm::require(&user, crate::perm::PAYMENT_LIST)?;
    let keyword = q
        .keyword
        .as_deref()
        .map(|s| format!("%{}%", s.trim()))
        .filter(|s| s != "%%");

    let sql = r#"
        SELECT t.id, t.contract_id, t.payment_no, t.amount_cents, t.paid_at, t.method, t.owner_id, t.dept_id, t.created_at, t.updated_at
        FROM crm.payment t
        WHERE t.deleted_at IS NULL
          AND t.tenant_id = $2
          AND ($1::text IS NULL OR t.payment_no ILIKE $1)
          AND ($8::date IS NULL OR t.paid_at >= $8::date)
          AND ($9::date IS NULL OR t.paid_at < ($9::date + interval '1 day'))

          AND (
                $5 LIKE '%all%'
             OR ($5 LIKE '%self%' AND t.owner_id = $6)
             OR ($5 LIKE '%dept%' AND t.dept_id = $7)
             OR ($5 LIKE '%custom%' AND t.dept_id IN (
                    SELECT rd.dept_id
                    FROM sys.sys_role_dept rd
                    JOIN sys.sys_user_role ur ON ur.role_id = rd.role_id
                    WHERE ur.user_id = $6
                      AND rd.tenant_id = current_setting('app.tenant_id')::uuid
                ))
OR ($5 LIKE '%child%' AND t.dept_id IN (
                    WITH RECURSIVE sub AS (
                        SELECT id FROM sys.sys_dept WHERE id = $7
                        UNION ALL
                        SELECT d.id FROM sys.sys_dept d JOIN sub ON d.parent_id = sub.id
                    )
                    SELECT id FROM sub
                ))
          )

        ORDER BY t.created_at DESC, t.id
        LIMIT $3 OFFSET $4
    "#;

    let rows = sqlx::query_as::<_, PaymentVo>(sql)
        .bind(keyword.as_deref())
        .bind(user.tenant_id)
        .bind(q.page.limit())
        .bind(q.page.offset())
        .bind(&user.data_scope)
        .bind(user.user_id)
        .bind(user.dept_id)
        .bind(q.paid_from)
        .bind(q.paid_to)
        .fetch_all(&mut *tx)
        .await?;

    let total: i64 = sqlx::query_scalar(
        r#"
        SELECT count(*) FROM crm.payment t
        WHERE t.deleted_at IS NULL
          AND t.tenant_id = $2
          AND ($1::text IS NULL OR t.payment_no ILIKE $1)
          AND ($6::date IS NULL OR t.paid_at >= $6::date)
          AND ($7::date IS NULL OR t.paid_at < ($7::date + interval '1 day'))

          AND (
                $3 LIKE '%all%'
             OR ($3 LIKE '%self%' AND t.owner_id = $4)
             OR ($3 LIKE '%dept%' AND t.dept_id = $5)
             OR ($3 LIKE '%custom%' AND t.dept_id IN (
                    SELECT rd.dept_id
                    FROM sys.sys_role_dept rd
                    JOIN sys.sys_user_role ur ON ur.role_id = rd.role_id
                    WHERE ur.user_id = $4
                      AND rd.tenant_id = current_setting('app.tenant_id')::uuid
                ))
OR ($3 LIKE '%child%' AND t.dept_id IN (
                    WITH RECURSIVE sub AS (
                        SELECT id FROM sys.sys_dept WHERE id = $5
                        UNION ALL
                        SELECT d.id FROM sys.sys_dept d JOIN sub ON d.parent_id = sub.id
                    )
                    SELECT id FROM sub
                ))
          )

        "#,
    )
    .bind(keyword.as_deref())
    .bind(user.tenant_id)
    .bind(&user.data_scope)
    .bind(user.user_id)
    .bind(user.dept_id)
    .bind(q.paid_from)
    .bind(q.paid_to)
    .fetch_one(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(ApiResponse::new(Page {
        data: rows,
        page: PageMeta::new(q.page.page(), q.page.size(), total as u64),
    }))
}

/// 详情。
#[utoipa::path(
    get,
    path = "/api/crm/payments/{id}",
    tag = "crm",
    params(("id" = Uuid, Path, description = "主键")),
    responses((status = 200, description = "详情", body = ApiResponse<PaymentVo>))
)]
pub async fn detail(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Path(id): Path<Uuid>,
) -> ApiResult<PaymentVo> {
    crate::perm::require(&user, crate::perm::PAYMENT_LIST)?;
    let sql = r#"
        SELECT t.id, t.contract_id, t.payment_no, t.amount_cents, t.paid_at, t.method, t.owner_id, t.dept_id, t.created_at, t.updated_at
        FROM crm.payment t
        WHERE t.id = $1 AND t.tenant_id = $2 AND t.deleted_at IS NULL
        
          AND (
                $3 LIKE '%all%'
             OR ($3 LIKE '%self%' AND t.owner_id = $4)
             OR ($3 LIKE '%dept%' AND t.dept_id = $5)
             OR ($3 LIKE '%custom%' AND t.dept_id IN (
                    SELECT rd.dept_id
                    FROM sys.sys_role_dept rd
                    JOIN sys.sys_user_role ur ON ur.role_id = rd.role_id
                    WHERE ur.user_id = $4
                      AND rd.tenant_id = current_setting('app.tenant_id')::uuid
                ))
OR ($3 LIKE '%child%' AND t.dept_id IN (
                    WITH RECURSIVE sub AS (
                        SELECT id FROM sys.sys_dept WHERE id = $5
                        UNION ALL
                        SELECT d.id FROM sys.sys_dept d JOIN sub ON d.parent_id = sub.id
                    )
                    SELECT id FROM sub
                ))
          )

    "#;

    let vo = sqlx::query_as::<_, PaymentVo>(sql)
        .bind(id)
        .bind(user.tenant_id)
        .bind(&user.data_scope)
        .bind(user.user_id)
        .bind(user.dept_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(AppError::NotFound)?;

    tx.commit().await?;
    Ok(ApiResponse::new(vo))
}
