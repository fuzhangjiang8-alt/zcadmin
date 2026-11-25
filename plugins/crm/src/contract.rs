//! 合同（由 `cargo xtask gen` 生成）
//!
//! 生成后请：
//! 1. 在插件 `lib.rs` 顶部加 `mod contract;`
//! 2. 在 `router()` 里加 `.merge(contract::router())`
//! 3. 把本文件的 5 个 handler 加进插件的 OpenAPI 片段（paths(...)），否则契约里没有它们

#![allow(clippy::too_many_arguments)]
#![allow(unused_imports)]

use chrono::{DateTime, Utc};
use kernel::prelude::*;
use kernel::response::{Page, PageMeta, PageQuery};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use utoipa::ToSchema;

pub use crate::contract_read::{__path_detail, __path_list, detail, list};
pub use crate::contract_types::*;

/// 路由。
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/contracts", get(list).post(create))
        .route("/contracts/{id}", get(detail).patch(update).delete(delete))
}

/// 创建。
#[utoipa::path(
    post,
    path = "/api/crm/contracts",
    tag = "crm",
    request_body = CreateContractRequest,
    responses((status = 200, description = "创建成功", body = ApiResponse<ContractVo>))
)]
pub async fn create(
    State(state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Json(req): Json<CreateContractRequest>,
) -> ApiResult<ContractVo> {
    crate::perm::require(&user, crate::perm::CONTRACT_ADD)?;
    crate::api::ensure_customer_visible(&mut tx, &user, req.customer_id).await?;
    let sql = r#"
        INSERT INTO crm.contract (id, tenant_id, customer_id, contract_no, name, amount_cents, status, signed_at, owner_id, dept_id, created_by, opportunity_id)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
        RETURNING id, customer_id, opportunity_id, contract_no, name, amount_cents, paid_cents, status, signed_at, owner_id, dept_id, created_at, updated_at
    "#;

    let vo = sqlx::query_as::<_, ContractVo>(sql)
        .bind(Uuid::now_v7())
        .bind(user.tenant_id)
        .bind(req.customer_id)
        .bind(req.contract_no)
        .bind(req.name)
        .bind(req.amount_cents)
        .bind(req.status)
        .bind(req.signed_at)
        .bind(user.user_id)
        .bind(user.dept_id)
        .bind(user.user_id)
        .bind(req.opportunity_id)
        .fetch_one(&mut *tx)
        .await?;

    tx.commit().await?;

    kernel::audit::record_user(
        &state.db,
        &user,
        "crm.contract.create",
        "contract",
        Some(&vo.id.to_string()),
        Some(serde_json::json!({ "contractNo": vo.contract_no, "amountCents": vo.amount_cents })),
    )
    .await;
    Ok(ApiResponse::new(vo))
}

/// 更新。
#[utoipa::path(
    patch,
    path = "/api/crm/contracts/{id}",
    tag = "crm",
    params(("id" = Uuid, Path, description = "主键")),
    request_body = UpdateContractRequest,
    responses((status = 200, description = "更新后的记录", body = ApiResponse<ContractVo>))
)]
pub async fn update(
    State(state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateContractRequest>,
) -> ApiResult<ContractVo> {
    crate::perm::require(&user, crate::perm::CONTRACT_EDIT)?;
    if let Some(customer_id) = req.customer_id {
        crate::api::ensure_customer_visible(&mut tx, &user, customer_id).await?;
    }
    let before = kernel::audit::snapshot(&mut tx, "crm.contract", id, user.tenant_id).await?;
    let sql = r#"
        UPDATE crm.contract t SET
            customer_id = COALESCE($4, t.customer_id),
            contract_no = COALESCE($5, t.contract_no),
            name = COALESCE($6, t.name),
            amount_cents = COALESCE($7, t.amount_cents),
            status = COALESCE($8, t.status),
            signed_at = COALESCE($9, t.signed_at),
            opportunity_id = COALESCE($13, t.opportunity_id),
            updated_at = now(),
            updated_by = $3,
            version = t.version + 1
        WHERE t.id = $1 AND t.tenant_id = $2 AND t.deleted_at IS NULL
        
          AND (
                $10 LIKE '%all%'
             OR ($10 LIKE '%self%' AND t.owner_id = $11)
             OR ($10 LIKE '%dept%' AND t.dept_id = $12)
             OR ($10 LIKE '%custom%' AND t.dept_id IN (
                    SELECT rd.dept_id
                    FROM sys.sys_role_dept rd
                    JOIN sys.sys_user_role ur ON ur.role_id = rd.role_id
                    WHERE ur.user_id = $11
                      AND rd.tenant_id = current_setting('app.tenant_id')::uuid
                ))
OR ($10 LIKE '%child%' AND t.dept_id IN (
                    WITH RECURSIVE sub AS (
                        SELECT id FROM sys.sys_dept WHERE id = $12
                        UNION ALL
                        SELECT d.id FROM sys.sys_dept d JOIN sub ON d.parent_id = sub.id
                    )
                    SELECT id FROM sub
                ))
          )

        RETURNING id, customer_id, opportunity_id, contract_no, name, amount_cents, paid_cents, status, signed_at, owner_id, dept_id, created_at, updated_at
    "#;

    let vo = sqlx::query_as::<_, ContractVo>(sql)
        .bind(id)
        .bind(user.tenant_id)
        .bind(user.user_id)
        .bind(req.customer_id)
        .bind(req.contract_no)
        .bind(req.name)
        .bind(req.amount_cents)
        .bind(req.status)
        .bind(req.signed_at)
        .bind(&user.data_scope)
        .bind(user.user_id)
        .bind(user.dept_id)
        .bind(req.opportunity_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(AppError::NotFound)?;

    let after = kernel::audit::snapshot(&mut tx, "crm.contract", id, user.tenant_id).await?;

    tx.commit().await?;

    kernel::audit::record_user(
        &state.db,
        &user,
        "crm.contract.update",
        "contract",
        Some(&vo.id.to_string()),
        Some(serde_json::json!({ "before": before, "after": after })),
    )
    .await;
    Ok(ApiResponse::new(vo))
}

/// 删除（软删）。
#[utoipa::path(
    delete,
    path = "/api/crm/contracts/{id}",
    tag = "crm",
    params(("id" = Uuid, Path, description = "主键")),
    responses((status = 200, description = "删除成功"))
)]
pub async fn delete(
    State(state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Path(id): Path<Uuid>,
) -> ApiResult<()> {
    crate::perm::require(&user, crate::perm::CONTRACT_DELETE)?;
    let sql = r#"
        UPDATE crm.contract t
        SET deleted_at = now(), updated_by = $3, version = t.version + 1
        WHERE t.id = $1 AND t.tenant_id = $2 AND t.deleted_at IS NULL
        
          AND (
                $4 LIKE '%all%'
             OR ($4 LIKE '%self%' AND t.owner_id = $5)
             OR ($4 LIKE '%dept%' AND t.dept_id = $6)
             OR ($4 LIKE '%custom%' AND t.dept_id IN (
                    SELECT rd.dept_id
                    FROM sys.sys_role_dept rd
                    JOIN sys.sys_user_role ur ON ur.role_id = rd.role_id
                    WHERE ur.user_id = $5
                      AND rd.tenant_id = current_setting('app.tenant_id')::uuid
                ))
OR ($4 LIKE '%child%' AND t.dept_id IN (
                    WITH RECURSIVE sub AS (
                        SELECT id FROM sys.sys_dept WHERE id = $6
                        UNION ALL
                        SELECT d.id FROM sys.sys_dept d JOIN sub ON d.parent_id = sub.id
                    )
                    SELECT id FROM sub
                ))
          )

    "#;

    let affected = sqlx::query(sql)
        .bind(id)
        .bind(user.tenant_id)
        .bind(user.user_id)
        .bind(&user.data_scope)
        .bind(user.user_id)
        .bind(user.dept_id)
        .execute(&mut *tx)
        .await?
        .rows_affected();

    if affected == 0 {
        return Err(AppError::NotFound);
    }
    tx.commit().await?;

    kernel::audit::record_user(
        &state.db,
        &user,
        "crm.contract.delete",
        "contract",
        Some(&id.to_string()),
        None,
    )
    .await;
    Ok(ApiResponse::new(()))
}
