//! 联系人查询：列表与详情。
//!
//! 边界：联系人写操作在 `contact`；本模块只读。
//! 关键约束：按数据权限过滤，详情校验客户可见性。

use axum::extract::{Path, Query, State};
use kernel::prelude::*;
use kernel::response::Page;

use crate::contact_types::*;

/// 列表。
#[utoipa::path(
    get,
    path = "/api/crm/contacts",
    tag = "crm",
    params(
        ("page" = Option<u64>, Query, description = "页码"),
        ("size" = Option<u64>, Query, description = "每页条数")
    ),
    responses((status = 200, description = "联系人分页列表", body = ApiResponse<Page<ContactVo>>))
)]
pub async fn list(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Query(q): Query<ContactQuery>,
) -> ApiResult<Page<ContactVo>> {
    crate::perm::require(&user, crate::perm::CONTACT_LIST)?;
    let keyword = q
        .keyword
        .as_deref()
        .map(|s| format!("%{}%", s.trim()))
        .filter(|s| s != "%%");

    let sql = r#"
        SELECT t.id, t.customer_id, t.name, t.phone, t.email, t.is_primary, t.owner_id, t.dept_id, t.created_at, t.updated_at
        FROM crm.contact t
        WHERE t.deleted_at IS NULL
          AND t.tenant_id = $2
          AND ($1::text IS NULL OR t.name ILIKE $1)
          
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

    let rows = sqlx::query_as::<_, ContactVo>(sql)
        .bind(keyword.as_deref())
        .bind(user.tenant_id)
        .bind(q.page.limit())
        .bind(q.page.offset())
        .bind(&user.data_scope)
        .bind(user.user_id)
        .bind(user.dept_id)
        .fetch_all(&mut *tx)
        .await?;

    let total: i64 = sqlx::query_scalar(
        r#"
        SELECT count(*) FROM crm.contact t
        WHERE t.deleted_at IS NULL
          AND t.tenant_id = $2
          AND ($1::text IS NULL OR t.name ILIKE $1)
          
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
    path = "/api/crm/contacts/{id}",
    tag = "crm",
    params(("id" = Uuid, Path, description = "主键")),
    responses((status = 200, description = "联系人详情", body = ApiResponse<ContactVo>))
)]
pub async fn detail(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Path(id): Path<Uuid>,
) -> ApiResult<ContactVo> {
    crate::perm::require(&user, crate::perm::CONTACT_LIST)?;
    let sql = r#"
        SELECT t.id, t.customer_id, t.name, t.phone, t.email, t.is_primary, t.owner_id, t.dept_id, t.created_at, t.updated_at
        FROM crm.contact t
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

    let vo = sqlx::query_as::<_, ContactVo>(sql)
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
