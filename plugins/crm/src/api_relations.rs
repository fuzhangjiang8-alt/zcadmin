//! CRM 关联查询：客户联系人/商机/合同、商机合同、Top 客户。
//!
//! 边界：主体 CRUD 在 `api`；本模块只做只读关联查询。
//! 关键约束：所有查询都先校验父对象可见性，再按数据权限过滤。

use axum::extract::{Path, State};
use kernel::prelude::*;

use crate::api::{ensure_customer_visible, scope_filter};
use serde::Serialize;
use sqlx::FromRow;

/// 客户下的联系人。
#[utoipa::path(
    get,
    path = "/api/crm/customers/{id}/contacts",
    tag = "crm",
    params(("id" = Uuid, Path, description = "客户 id")),
    responses((status = 200, description = "联系人列表", body = ApiResponse<Vec<crate::contact::ContactVo>>))
)]
pub async fn customer_contacts(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Path(id): Path<Uuid>,
) -> ApiResult<Vec<crate::contact::ContactVo>> {
    crate::perm::require(&user, crate::perm::CONTACT_LIST)?;
    ensure_customer_visible(&mut tx, &user, id).await?;
    let rows = sqlx::query_as::<_, crate::contact::ContactVo>(
        r#"
        SELECT id, customer_id, name, phone, email, is_primary, owner_id, dept_id, created_at, updated_at
        FROM crm.contact
        WHERE customer_id = $1 AND tenant_id = $2 AND deleted_at IS NULL
        ORDER BY is_primary DESC, created_at DESC
        "#,
    )
    .bind(id)
    .bind(user.tenant_id)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(ApiResponse::new(rows))
}

/// 客户下的商机。
#[utoipa::path(
    get,
    path = "/api/crm/customers/{id}/opportunities",
    tag = "crm",
    params(("id" = Uuid, Path, description = "客户 id")),
    responses((status = 200, description = "商机列表", body = ApiResponse<Vec<crate::opportunity::OpportunityVo>>))
)]
pub async fn customer_opportunities(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Path(id): Path<Uuid>,
) -> ApiResult<Vec<crate::opportunity::OpportunityVo>> {
    crate::perm::require(&user, crate::perm::OPPORTUNITY_LIST)?;
    ensure_customer_visible(&mut tx, &user, id).await?;
    let rows = sqlx::query_as::<_, crate::opportunity::OpportunityVo>(
        r#"
        SELECT id, customer_id, name, amount_cents, stage, expected_close, owner_id, dept_id, created_at, updated_at
        FROM crm.opportunity
        WHERE customer_id = $1 AND tenant_id = $2 AND deleted_at IS NULL
        ORDER BY created_at DESC
        "#,
    )
    .bind(id)
    .bind(user.tenant_id)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(ApiResponse::new(rows))
}

/// 客户下的合同。
#[utoipa::path(
    get,
    path = "/api/crm/customers/{id}/contracts",
    tag = "crm",
    params(("id" = Uuid, Path, description = "客户 id")),
    responses((status = 200, description = "合同列表", body = ApiResponse<Vec<crate::contract::ContractVo>>))
)]
pub async fn customer_contracts(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Path(id): Path<Uuid>,
) -> ApiResult<Vec<crate::contract::ContractVo>> {
    crate::perm::require(&user, crate::perm::CONTRACT_LIST)?;
    ensure_customer_visible(&mut tx, &user, id).await?;
    let rows = sqlx::query_as::<_, crate::contract::ContractVo>(
        r#"
        SELECT id, customer_id, opportunity_id, contract_no, name, amount_cents, paid_cents, status,
               signed_at, owner_id, dept_id, created_at, updated_at
        FROM crm.contract
        WHERE customer_id = $1 AND tenant_id = $2 AND deleted_at IS NULL
        ORDER BY created_at DESC
        "#,
    )
    .bind(id)
    .bind(user.tenant_id)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(ApiResponse::new(rows))
}

/// 商机转化出的合同。
#[utoipa::path(
    get,
    path = "/api/crm/opportunities/{id}/contracts",
    tag = "crm",
    params(("id" = Uuid, Path, description = "商机 id")),
    responses((status = 200, description = "合同列表", body = ApiResponse<Vec<crate::contract::ContractVo>>))
)]
pub async fn opportunity_contracts(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Path(id): Path<Uuid>,
) -> ApiResult<Vec<crate::contract::ContractVo>> {
    crate::perm::require(&user, crate::perm::CONTRACT_LIST)?;
    let exists_sql = format!(
        r#"
        SELECT EXISTS(
            SELECT 1 FROM crm.opportunity c
            WHERE c.id = $1 AND c.deleted_at IS NULL {}
        )
        "#,
        scope_filter("$2", "$3", "$4")
    );
    let visible: bool = sqlx::query_scalar(&exists_sql)
        .bind(id)
        .bind(&user.data_scope)
        .bind(user.user_id)
        .bind(user.dept_id)
        .fetch_one(&mut *tx)
        .await?;
    if !visible {
        return Err(AppError::NotFound);
    }

    let rows = sqlx::query_as::<_, crate::contract::ContractVo>(
        r#"
        SELECT id, customer_id, opportunity_id, contract_no, name, amount_cents, paid_cents,
               status, signed_at, owner_id, dept_id, created_at, updated_at
        FROM crm.contract
        WHERE opportunity_id = $1 AND tenant_id = $2 AND deleted_at IS NULL
        ORDER BY created_at DESC
        "#,
    )
    .bind(id)
    .bind(user.tenant_id)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(ApiResponse::new(rows))
}

/// 客户合同排行。
#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TopCustomer {
    /// 客户 id。
    pub customer_id: Uuid,
    /// 客户名称。
    pub name: String,
    /// 合同数量。
    pub contract_count: i64,
    /// 合同金额（分）。
    pub contract_cents: i64,
    /// 已回款（分）。
    pub paid_cents: i64,
}

/// TOP 客户：按合同金额排序。
#[utoipa::path(
    get,
    path = "/api/crm/dashboard/top-customers",
    tag = "crm",
    params(("limit" = Option<i64>, Query, description = "返回条数，默认 5，最大 20")),
    responses((status = 200, description = "TOP 客户", body = ApiResponse<Vec<TopCustomer>>))
)]
pub async fn top_customers(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Query(q): Query<TopQuery>,
) -> ApiResult<Vec<TopCustomer>> {
    crate::perm::require(&user, crate::perm::CUSTOMER_LIST)?;
    let limit = q.limit.unwrap_or(5).clamp(1, 20);
    let sql = format!(
        r#"
        SELECT c.id AS customer_id, c.name,
               count(ct.id)::bigint AS contract_count,
               COALESCE(sum(ct.amount_cents), 0)::bigint AS contract_cents,
               COALESCE(sum(ct.paid_cents), 0)::bigint AS paid_cents
        FROM crm.customer c
        JOIN crm.contract ct
          ON ct.customer_id = c.id
         AND ct.tenant_id = c.tenant_id
         AND ct.deleted_at IS NULL
         AND ct.status <> 'void'
        WHERE c.deleted_at IS NULL AND c.tenant_id = $1
          {}
        GROUP BY c.id, c.name
        ORDER BY contract_cents DESC, c.id
        LIMIT $5
        "#,
        scope_filter("$2", "$3", "$4")
    );
    let rows = sqlx::query_as::<_, TopCustomer>(&sql)
        .bind(user.tenant_id)
        .bind(&user.data_scope)
        .bind(user.user_id)
        .bind(user.dept_id)
        .bind(limit)
        .fetch_all(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(ApiResponse::new(rows))
}

/// TOP 客户查询参数。
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TopQuery {
    /// 返回条数。
    #[serde(default)]
    pub limit: Option<i64>,
}
