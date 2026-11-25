//! 角色数据范围预览。
//!
//! 边界：角色授权在 `system_role_permissions`；本模块只做只读预览。
//! 关键约束：预览按当前租户计算，不写库。

use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::Router;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::CurrentUser;
use crate::error::AppError;
use crate::response::ApiResponse;
use crate::state::AppState;
use crate::system::require_perm;
use crate::tenant::TenantTx;

/// 数据范围预览路由（挂在 `/api/system` 下）。
pub(crate) fn router() -> Router<AppState> {
    Router::new().route("/roles/{id}/scope-preview", get(scope_preview))
}

/// 数据范围预览。
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ScopePreviewVo {
    /// 数据范围：all / dept_and_child / custom / dept / self。
    pub data_scope: String,
    /// 中文说明。
    pub description: String,
    /// 可见部门 id（self 时为空，表示按人过滤）。
    pub visible_dept_ids: Vec<Uuid>,
    /// 这些部门下的用户数（帮助判断影响面）。
    pub user_count: i64,
}

/// 数据范围预览参数。
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ScopePreviewQuery {
    /// 示例部门 id（dept / dept_and_child 必填）。
    pub dept_id: Option<Uuid>,
}

/// 角色数据范围预览：给定一个部门，算出该角色能看到哪些部门。
#[utoipa::path(
    get,
    path = "/api/system/roles/{id}/scope-preview",
    tag = "system",
    params(
        ("id" = Uuid, Path, description = "角色 id"),
        ("deptId" = Option<Uuid>, Query, description = "示例部门 id")
    ),
    responses((status = 200, description = "预览结果", body = ApiResponse<ScopePreviewVo>))
)]
async fn scope_preview(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Path(role_id): Path<Uuid>,
    Query(q): Query<ScopePreviewQuery>,
) -> Result<ApiResponse<ScopePreviewVo>, AppError> {
    require_perm(&user, crate::perms::ROLE_LIST)?;

    let scope: Option<String> = sqlx::query_scalar(
        "SELECT data_scope FROM sys.sys_role WHERE id = $1 AND tenant_id = $2 AND deleted_at IS NULL",
    )
    .bind(role_id)
    .bind(user.tenant_id)
    .fetch_optional(&mut *tx)
    .await?;
    let scope = scope.ok_or(AppError::NotFound)?;

    let mut visible: Vec<Uuid> = Vec::new();
    let description = match scope.as_str() {
        "all" => {
            visible = sqlx::query_scalar(
                "SELECT id FROM sys.sys_dept WHERE tenant_id = $1 AND deleted_at IS NULL ORDER BY sort, id",
            )
            .bind(user.tenant_id)
            .fetch_all(&mut *tx)
            .await?;
            "可见全部部门的数据".to_string()
        }
        "dept_and_child" => {
            let dept_id = q
                .dept_id
                .ok_or_else(|| AppError::Validation("请选择示例部门".into()))?;
            visible = sqlx::query_scalar(
                r#"
                WITH RECURSIVE sub AS (
                    SELECT id FROM sys.sys_dept WHERE id = $1 AND tenant_id = $2 AND deleted_at IS NULL
                    UNION ALL
                    SELECT d.id FROM sys.sys_dept d JOIN sub ON d.parent_id = sub.id
                    WHERE d.deleted_at IS NULL
                )
                SELECT id FROM sub
                "#,
            )
            .bind(dept_id)
            .bind(user.tenant_id)
            .fetch_all(&mut *tx)
            .await?;
            "可见示例部门及其所有下级部门".to_string()
        }
        "dept" => {
            let dept_id = q
                .dept_id
                .ok_or_else(|| AppError::Validation("请选择示例部门".into()))?;
            let exists: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM sys.sys_dept WHERE id = $1 AND tenant_id = $2 AND deleted_at IS NULL)",
            )
            .bind(dept_id)
            .bind(user.tenant_id)
            .fetch_one(&mut *tx)
            .await?;
            if !exists {
                return Err(AppError::NotFound);
            }
            visible = vec![dept_id];
            "仅可见示例部门本身".to_string()
        }
        "custom" => {
            visible = sqlx::query_scalar(
                "SELECT dept_id FROM sys.sys_role_dept WHERE role_id = $1 AND tenant_id = $2 ORDER BY dept_id",
            )
            .bind(role_id)
            .bind(user.tenant_id)
            .fetch_all(&mut *tx)
            .await?;
            "仅可见角色指定的部门（自定义范围）".to_string()
        }
        _ => "仅可见本人创建/负责的数据（按人过滤，不按部门）".to_string(),
    };

    let user_count: i64 = if visible.is_empty() {
        0
    } else {
        sqlx::query_scalar(
            "SELECT count(*) FROM sys.sys_user WHERE tenant_id = $1 AND deleted_at IS NULL AND dept_id = ANY($2)",
        )
        .bind(user.tenant_id)
        .bind(&visible)
        .fetch_one(&mut *tx)
        .await?
    };

    tx.commit().await?;
    Ok(ApiResponse::new(ScopePreviewVo {
        data_scope: scope,
        description,
        visible_dept_ids: visible,
        user_count,
    }))
}
