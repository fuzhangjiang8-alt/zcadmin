//! 数据字典接口：类型 / 字典项的新增、查询与编辑。
//!
//! 边界：只处理 `/dicts` 的列表与新增；编辑在 `system_dicts_edit`，菜单/部门在 `system_org`。
//! 关键约束：编辑只改名称/备注或 label/value/sort，code 不可变。

use axum::extract::{Path, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use crate::auth::CurrentUser;
use crate::error::AppError;
use crate::response::ApiResponse;
use crate::state::AppState;
use crate::system::require_perm;
use crate::tenant::TenantTx;

/// 字典路由（挂在 `/api/system` 下）。
pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route("/dicts", get(list_dict_types).post(create_dict_type))
        .route(
            "/dicts/{code}/items",
            get(list_dict_items).post(create_dict_item),
        )
        .merge(crate::system_dicts_edit::router())
}

/// 字典类型视图对象。
#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DictTypeVo {
    /// 字典类型 id。
    pub id: Uuid,
    /// 类型编码，例如 customer_level。
    pub code: String,
    /// 类型名称。
    pub name: String,
    /// 备注。
    pub remark: Option<String>,
    /// 状态：active / disabled。
    pub status: String,
    /// 该类型下的字典项数量。
    pub item_count: i64,
}

/// 字典项视图对象。
#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DictItemVo {
    /// 字典项 id。
    pub id: Uuid,
    /// 展示文案。
    pub label: String,
    /// 实际值（存库与传输用这个）。
    pub value: String,
    /// 排序号，小的在前。
    pub sort: i32,
    /// 状态：active / disabled。
    pub status: String,
}

/// 字典类型列表。
#[utoipa::path(
    get,
    path = "/api/system/dicts",
    tag = "system",
    responses((status = 200, description = "字典类型列表", body = ApiResponse<Vec<DictTypeVo>>))
)]
pub(crate) async fn list_dict_types(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
) -> Result<ApiResponse<Vec<DictTypeVo>>, AppError> {
    require_perm(&user, crate::perms::DICT_LIST)?;

    let rows = sqlx::query_as::<_, DictTypeVo>(
        r#"
        SELECT t.id, t.code, t.name, t.remark, t.status,
               (SELECT count(*) FROM sys.sys_dict_item i
                 WHERE i.tenant_id = t.tenant_id AND i.type_code = t.code AND i.deleted_at IS NULL) AS item_count
        FROM sys.sys_dict_type t
        WHERE t.tenant_id = $1 AND t.deleted_at IS NULL
        ORDER BY t.code
        "#,
    )
    .bind(user.tenant_id)
    .fetch_all(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(ApiResponse::new(rows))
}

/// 创建字典类型请求体。
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateDictTypeRequest {
    /// 类型编码，租户内唯一。
    pub code: String,
    /// 类型名称。
    pub name: String,
    /// 备注。
    pub remark: Option<String>,
}

/// 创建字典类型。
#[utoipa::path(
    post,
    path = "/api/system/dicts",
    tag = "system",
    request_body = CreateDictTypeRequest,
    responses((status = 200, description = "创建成功", body = ApiResponse<DictTypeVo>))
)]
pub(crate) async fn create_dict_type(
    State(state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Json(req): Json<CreateDictTypeRequest>,
) -> Result<ApiResponse<DictTypeVo>, AppError> {
    require_perm(&user, crate::perms::DICT_ADD)?;

    let code = req.code.trim();
    if code.is_empty() || code.len() > 64 {
        return Err(AppError::Validation(
            "字典编码长度必须为 1~64 个字符".into(),
        ));
    }
    if req.name.trim().is_empty() {
        return Err(AppError::Validation("字典名称不能为空".into()));
    }

    let result = sqlx::query_as::<_, DictTypeVo>(
        r#"
        INSERT INTO sys.sys_dict_type (id, tenant_id, code, name, remark, created_by)
        VALUES ($1, $2, $3, $4, $5, $6)
        RETURNING id, code, name, remark, status, 0::bigint AS item_count
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(user.tenant_id)
    .bind(code)
    .bind(req.name.trim())
    .bind(&req.remark)
    .bind(user.user_id)
    .fetch_one(&mut *tx)
    .await;

    let vo = match result {
        Ok(vo) => vo,
        Err(sqlx::Error::Database(db)) if db.code().as_deref() == Some("23505") => {
            return Err(AppError::Conflict("字典编码已存在".into()))
        }
        Err(e) => return Err(AppError::Database(e)),
    };

    tx.commit().await?;

    crate::audit::record_user(
        &state.db,
        &user,
        "system.dict.create",
        "sys_dict_type",
        Some(&vo.id.to_string()),
        Some(serde_json::json!({ "code": vo.code, "name": vo.name })),
    )
    .await;

    Ok(ApiResponse::new(vo))
}

/// 查询某个字典类型下的所有字典项。
///
/// 不做权限校验：任何登录用户都可能需要在表单里渲染下拉框，
/// 具体"能不能改"由写接口控制。
#[utoipa::path(
    get,
    path = "/api/system/dicts/{code}/items",
    tag = "system",
    params(("code" = String, Path, description = "字典类型编码")),
    responses((status = 200, description = "字典项列表", body = ApiResponse<Vec<DictItemVo>>))
)]
pub(crate) async fn list_dict_items(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Path(code): Path<String>,
) -> Result<ApiResponse<Vec<DictItemVo>>, AppError> {
    let rows = sqlx::query_as::<_, DictItemVo>(
        r#"
        SELECT id, label, value, sort, status
        FROM sys.sys_dict_item
        WHERE tenant_id = $1 AND type_code = $2 AND deleted_at IS NULL
        ORDER BY sort, id
        "#,
    )
    .bind(user.tenant_id)
    .bind(&code)
    .fetch_all(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(ApiResponse::new(rows))
}

/// 新增字典项请求体。
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateDictItemRequest {
    /// 展示文案。
    pub label: String,
    /// 实际值。
    pub value: String,
    /// 排序号。
    #[serde(default)]
    pub sort: i32,
}

/// 新增字典项。
#[utoipa::path(
    post,
    path = "/api/system/dicts/{code}/items",
    tag = "system",
    params(("code" = String, Path, description = "字典类型编码")),
    request_body = CreateDictItemRequest,
    responses((status = 200, description = "创建成功", body = ApiResponse<DictItemVo>))
)]
pub(crate) async fn create_dict_item(
    State(state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Path(code): Path<String>,
    Json(req): Json<CreateDictItemRequest>,
) -> Result<ApiResponse<DictItemVo>, AppError> {
    require_perm(&user, crate::perms::DICT_ADD)?;

    if req.label.trim().is_empty() || req.value.trim().is_empty() {
        return Err(AppError::Validation(
            "字典项的 label 与 value 不能为空".into(),
        ));
    }

    // 先确认类型存在且属于当前租户，防止往任意 code 下灌数据
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM sys.sys_dict_type WHERE tenant_id = $1 AND code = $2 AND deleted_at IS NULL)",
    )
    .bind(user.tenant_id)
    .bind(&code)
    .fetch_one(&mut *tx)
    .await?;
    if !exists {
        return Err(AppError::NotFound);
    }

    let result = sqlx::query_as::<_, DictItemVo>(
        r#"
        INSERT INTO sys.sys_dict_item (id, tenant_id, type_code, label, value, sort)
        VALUES ($1, $2, $3, $4, $5, $6)
        RETURNING id, label, value, sort, status
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(user.tenant_id)
    .bind(&code)
    .bind(req.label.trim())
    .bind(req.value.trim())
    .bind(req.sort)
    .fetch_one(&mut *tx)
    .await;

    let vo = match result {
        Ok(vo) => vo,
        Err(sqlx::Error::Database(db)) if db.code().as_deref() == Some("23505") => {
            return Err(AppError::Conflict("该字典项的值已存在".into()))
        }
        Err(e) => return Err(AppError::Database(e)),
    };

    tx.commit().await?;

    crate::audit::record_user(
        &state.db,
        &user,
        "system.dict.item.create",
        "sys_dict_item",
        Some(&vo.id.to_string()),
        Some(serde_json::json!({ "code": code, "label": vo.label, "value": vo.value })),
    )
    .await;

    Ok(ApiResponse::new(vo))
}
