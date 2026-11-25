//! CRM 客户写操作：新建、修改、删除。
//!
//! 边界：客户查询在 `api_customers`；本模块只处理写接口。
//! 关键约束：写接口均要求权限点，并校验客户可见性/归属。

use axum::extract::{Path, State};
use kernel::prelude::*;

use crate::api::scope_filter;
use crate::model::{CreateCustomerRequest, CustomerVo, UpdateCustomerRequest, ALLOWED_STATUS};

/// 创建客户。
#[utoipa::path(
    post,
    path = "/api/crm/customers",
    tag = "crm",
    request_body = CreateCustomerRequest,
    responses(
        (status = 200, description = "创建成功", body = ApiResponse<CustomerVo>),
        (status = 409, description = "客户编码已存在")
    )
)]
pub async fn create(
    State(state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Json(req): Json<CreateCustomerRequest>,
) -> ApiResult<CustomerVo> {
    crate::perm::require(&user, crate::perm::CUSTOMER_ADD)?;
    if req.code.trim().is_empty() || req.code.len() > 32 {
        return Err(AppError::Validation(
            "客户编码长度必须为 1~32 个字符".into(),
        ));
    }
    if req.name.trim().is_empty() || req.name.chars().count() > 128 {
        return Err(AppError::Validation(
            "客户名称长度必须为 1~128 个字符".into(),
        ));
    }

    let sql = r#"
        INSERT INTO crm.customer
            (id, tenant_id, code, name, level, status, owner_id, dept_id, phone, email, remark, created_by)
        VALUES ($1, $2, $3, $4, $5, 'potential', $6, $7, $8, $9, $10, $6)
        RETURNING id, code, name, level, status, owner_id, dept_id,
                  phone, email, remark, created_at, updated_at
    "#;

    let result = sqlx::query_as::<_, CustomerVo>(sql)
        .bind(Uuid::now_v7())
        .bind(user.tenant_id)
        .bind(req.code.trim())
        .bind(req.name.trim())
        .bind(req.level.as_deref().unwrap_or("normal"))
        .bind(user.user_id)
        .bind(user.dept_id)
        .bind(&req.phone)
        .bind(&req.email)
        .bind(&req.remark)
        .fetch_one(&mut *tx)
        .await;

    match result {
        Ok(vo) => {
            tx.commit().await?;

            kernel::audit::record_user(
                &state.db,
                &user,
                "crm.customer.create",
                "customer",
                Some(&vo.id.to_string()),
                Some(serde_json::json!({ "code": vo.code, "name": vo.name })),
            )
            .await;

            Ok(ApiResponse::new(vo))
        }
        Err(sqlx::Error::Database(db)) if db.code().as_deref() == Some("23505") => {
            Err(AppError::Conflict("客户编码已存在".into()))
        }
        Err(e) => Err(AppError::Database(e)),
    }
}

/// 更新客户（部分更新，传 null 表示不修改）。
#[utoipa::path(
    patch,
    path = "/api/crm/customers/{id}",
    tag = "crm",
    params(("id" = Uuid, Path, description = "客户 id")),
    request_body = UpdateCustomerRequest,
    responses((status = 200, description = "更新后的客户", body = ApiResponse<CustomerVo>))
)]
pub async fn update(
    State(state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateCustomerRequest>,
) -> ApiResult<CustomerVo> {
    crate::perm::require(&user, crate::perm::CUSTOMER_EDIT)?;
    let before = kernel::audit::snapshot(&mut tx, "crm.customer", id, user.tenant_id).await?;
    if let Some(status) = &req.status {
        if !ALLOWED_STATUS.contains(&status.as_str()) {
            return Err(AppError::Validation("非法的客户状态".into()));
        }
    }

    let sql = format!(
        r#"
        UPDATE crm.customer c SET
            name       = COALESCE($4, c.name),
            level      = COALESCE($5, c.level),
            status     = COALESCE($6, c.status),
            phone      = COALESCE($7, c.phone),
            email      = COALESCE($8, c.email),
            remark     = COALESCE($9, c.remark),
            updated_at = now(),
            updated_by = $3,
            version    = c.version + 1
        WHERE c.id = $1 AND c.tenant_id = $2 AND c.deleted_at IS NULL
          {}
        RETURNING c.id, c.code, c.name, c.level, c.status, c.owner_id, c.dept_id,
                  c.phone, c.email, c.remark, c.created_at, c.updated_at
        "#,
        scope_filter("$10", "$11", "$12")
    );

    let vo = sqlx::query_as::<_, CustomerVo>(&sql)
        .bind(id)
        .bind(user.tenant_id)
        .bind(user.user_id)
        .bind(&req.name)
        .bind(&req.level)
        .bind(&req.status)
        .bind(&req.phone)
        .bind(&req.email)
        .bind(&req.remark)
        .bind(&user.data_scope)
        .bind(user.user_id)
        .bind(user.dept_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(AppError::NotFound)?;

    let after = kernel::audit::snapshot(&mut tx, "crm.customer", id, user.tenant_id).await?;

    tx.commit().await?;

    kernel::audit::record_user(
        &state.db,
        &user,
        "crm.customer.update",
        "customer",
        Some(&vo.id.to_string()),
        Some(serde_json::json!({ "before": before, "after": after })),
    )
    .await;
    Ok(ApiResponse::new(vo))
}

/// 删除客户（软删）。
#[utoipa::path(
    delete,
    path = "/api/crm/customers/{id}",
    tag = "crm",
    params(("id" = Uuid, Path, description = "客户 id")),
    responses((status = 200, description = "删除成功"))
)]
pub async fn delete(
    State(state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Path(id): Path<Uuid>,
) -> ApiResult<()> {
    crate::perm::require(&user, crate::perm::CUSTOMER_DELETE)?;
    let sql = format!(
        r#"
        UPDATE crm.customer c SET deleted_at = now(), updated_by = $3, version = c.version + 1
        WHERE c.id = $1 AND c.tenant_id = $2 AND c.deleted_at IS NULL
          {}
        "#,
        scope_filter("$4", "$5", "$6")
    );

    let affected = sqlx::query(&sql)
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
        "crm.customer.delete",
        "customer",
        Some(&id.to_string()),
        None,
    )
    .await;
    Ok(ApiResponse::new(()))
}
