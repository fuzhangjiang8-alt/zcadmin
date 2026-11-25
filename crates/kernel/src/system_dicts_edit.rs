//! 数据字典的编辑接口（类型名称/备注、字典项 label/value/sort）。
//!
//! 边界：只处理 `PUT /dicts/...`；列表与新增在 `system_dicts`。
//! 关键约束：code 不可变，更新必须带 tenant_id 条件。

use axum::extract::{Path, State};
use axum::routing::put;
use axum::{Json, Router};
use serde::Deserialize;
use uuid::Uuid;

use crate::auth::CurrentUser;
use crate::error::AppError;
use crate::response::ApiResponse;
use crate::state::AppState;
use crate::system::require_perm;
use crate::system_dicts::{DictItemVo, DictTypeVo};
use crate::tenant::TenantTx;

/// 字典编辑路由（挂在 `/api/system` 下）。
pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/dicts/{code}",
            put(update_dict_type).delete(delete_dict_type),
        )
        .route(
            "/dicts/{code}/items/{id}",
            put(update_dict_item).delete(delete_dict_item),
        )
}

/// 更新字典类型请求体。
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateDictTypeRequest {
    /// 类型名称。
    pub name: String,
    /// 备注。
    pub remark: Option<String>,
}

/// 更新字典类型（只改名称/备注，code 不可变）。
#[utoipa::path(
    put,
    path = "/api/system/dicts/{code}",
    tag = "system",
    params(("code" = String, Path, description = "字典类型编码")),
    request_body = UpdateDictTypeRequest,
    responses((status = 200, description = "更新成功", body = ApiResponse<DictTypeVo>))
)]
pub(crate) async fn update_dict_type(
    State(state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Path(code): Path<String>,
    Json(req): Json<UpdateDictTypeRequest>,
) -> Result<ApiResponse<DictTypeVo>, AppError> {
    require_perm(&user, crate::perms::DICT_EDIT)?;

    if req.name.trim().is_empty() {
        return Err(AppError::Validation("字典名称不能为空".into()));
    }

    let vo = sqlx::query_as::<_, DictTypeVo>(
        r#"
        UPDATE sys.sys_dict_type
        SET name = $3, remark = $4, updated_at = now(), updated_by = $5
        WHERE tenant_id = $1 AND code = $2 AND deleted_at IS NULL
        RETURNING id, code, name, remark, status,
                  (SELECT count(*) FROM sys.sys_dict_item i
                    WHERE i.tenant_id = sys_dict_type.tenant_id
                      AND i.type_code = sys_dict_type.code
                      AND i.deleted_at IS NULL) AS item_count
        "#,
    )
    .bind(user.tenant_id)
    .bind(&code)
    .bind(req.name.trim())
    .bind(&req.remark)
    .bind(user.user_id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(AppError::NotFound)?;

    tx.commit().await?;

    crate::audit::record_user(
        &state.db,
        &user,
        "system.dict.update",
        "sys_dict_type",
        Some(&vo.id.to_string()),
        Some(serde_json::json!({ "code": vo.code, "name": vo.name })),
    )
    .await;

    Ok(ApiResponse::new(vo))
}

/// 更新字典项请求体。
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateDictItemRequest {
    /// 展示文案。
    pub label: String,
    /// 实际值。
    pub value: String,
    /// 排序号。
    #[serde(default)]
    pub sort: i32,
}

/// 更新字典项（label/value/sort）。
#[utoipa::path(
    put,
    path = "/api/system/dicts/{code}/items/{id}",
    tag = "system",
    params(
        ("code" = String, Path, description = "字典类型编码"),
        ("id" = Uuid, Path, description = "字典项 id")
    ),
    request_body = UpdateDictItemRequest,
    responses((status = 200, description = "更新成功", body = ApiResponse<DictItemVo>))
)]
pub(crate) async fn update_dict_item(
    State(state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Path((code, id)): Path<(String, Uuid)>,
    Json(req): Json<UpdateDictItemRequest>,
) -> Result<ApiResponse<DictItemVo>, AppError> {
    require_perm(&user, crate::perms::DICT_EDIT)?;

    if req.label.trim().is_empty() || req.value.trim().is_empty() {
        return Err(AppError::Validation(
            "字典项的 label 与 value 不能为空".into(),
        ));
    }

    let result = sqlx::query_as::<_, DictItemVo>(
        r#"
        UPDATE sys.sys_dict_item
        SET label = $4, value = $5, sort = $6
        WHERE tenant_id = $1 AND type_code = $2 AND id = $3 AND deleted_at IS NULL
        RETURNING id, label, value, sort, status
        "#,
    )
    .bind(user.tenant_id)
    .bind(&code)
    .bind(id)
    .bind(req.label.trim())
    .bind(req.value.trim())
    .bind(req.sort)
    .fetch_optional(&mut *tx)
    .await;

    let vo = match result {
        Ok(Some(vo)) => vo,
        Ok(None) => return Err(AppError::NotFound),
        Err(sqlx::Error::Database(db)) if db.code().as_deref() == Some("23505") => {
            return Err(AppError::Conflict("该字典项的值已存在".into()))
        }
        Err(e) => return Err(AppError::Database(e)),
    };

    tx.commit().await?;

    crate::audit::record_user(
        &state.db,
        &user,
        "system.dict.item.update",
        "sys_dict_item",
        Some(&vo.id.to_string()),
        Some(serde_json::json!({ "code": code, "label": vo.label, "value": vo.value })),
    )
    .await;

    Ok(ApiResponse::new(vo))
}

/// 删除字典项（软删）。
#[utoipa::path(
    delete,
    path = "/api/system/dicts/{code}/items/{id}",
    tag = "system",
    params(
        ("code" = String, Path, description = "字典类型编码"),
        ("id" = Uuid, Path, description = "字典项 id")
    ),
    responses((status = 200, description = "删除成功"))
)]
pub(crate) async fn delete_dict_item(
    State(state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Path((code, id)): Path<(String, Uuid)>,
) -> Result<ApiResponse<()>, AppError> {
    require_perm(&user, crate::perms::DICT_EDIT)?;
    let affected = sqlx::query(
        "UPDATE sys.sys_dict_item SET deleted_at = now()
         WHERE tenant_id = $1 AND type_code = $2 AND id = $3 AND deleted_at IS NULL",
    )
    .bind(user.tenant_id)
    .bind(&code)
    .bind(id)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if affected == 0 {
        return Err(AppError::NotFound);
    }
    tx.commit().await?;
    crate::audit::record_user(
        &state.db,
        &user,
        "system.dict.item.delete",
        "sys_dict_item",
        Some(&id.to_string()),
        Some(serde_json::json!({ "code": code })),
    )
    .await;
    Ok(ApiResponse::new(()))
}

/// 删除字典类型（同事务软删字典类型及其下属字典项）。
#[utoipa::path(
    delete,
    path = "/api/system/dicts/{code}",
    tag = "system",
    params(("code" = String, Path, description = "字典类型编码")),
    responses((status = 200, description = "删除成功"))
)]
pub(crate) async fn delete_dict_type(
    State(state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Path(code): Path<String>,
) -> Result<ApiResponse<()>, AppError> {
    require_perm(&user, crate::perms::DICT_EDIT)?;
    let id: Option<Uuid> = sqlx::query_scalar(
        "UPDATE sys.sys_dict_type SET deleted_at = now(), updated_at = now(), updated_by = $3
         WHERE tenant_id = $1 AND code = $2 AND deleted_at IS NULL RETURNING id",
    )
    .bind(user.tenant_id)
    .bind(&code)
    .bind(user.user_id)
    .fetch_optional(&mut *tx)
    .await?;
    let id = id.ok_or(AppError::NotFound)?;
    sqlx::query(
        "UPDATE sys.sys_dict_item SET deleted_at = now()
         WHERE tenant_id = $1 AND type_code = $2 AND deleted_at IS NULL",
    )
    .bind(user.tenant_id)
    .bind(&code)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    crate::audit::record_user(
        &state.db,
        &user,
        "system.dict.delete",
        "sys_dict_type",
        Some(&id.to_string()),
        Some(serde_json::json!({ "code": code })),
    )
    .await;
    Ok(ApiResponse::new(()))
}
