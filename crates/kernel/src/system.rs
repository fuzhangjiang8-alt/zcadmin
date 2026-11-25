//! 系统管理（内核横切入口）。
//!
//! # 为什么放在内核而不是插件
//!
//! 「用户 / 角色 / 权限 / 菜单」是**所有插件共享的身份基础**，不属于任何业务域。
//! 如果做成插件，就会出现"插件 A 依赖插件 B 的 sys 表"这种反向依赖。
//! 因此由内核提供，路由统一挂在 `/api/system`。
//!
//! # 模块划分
//!
//! 原 `system.rs` 约 7,900 行，已按领域拆分：
//!
//! | 模块 | 职责 |
//! |---|---|
//! | [`crate::system_identity`] | 用户、角色、权限点、用户-角色 |
//! | [`crate::system_org`] | 菜单、字典、部门 |
//! | [`crate::system_security`] | 密码/登录策略、登录风控、账号解锁 |
//! | [`crate::system_seed`] | 初始管理员与默认菜单 |
//! | [`crate::tenant_admin`] | 租户、配额、账单、计费 |
//! | [`crate::audit_admin`] | 审计告警、订阅、投递、配置快照 |
//!
//! 本文件只保留：路由聚合、OpenAPI 聚合、会话管理、审计日志查询、
//! 角色数据范围（dept）、数据范围预览、插件权限同步。
//!
//! # 边界
//!
//! - 权限点由插件声明（`Plugin::permissions`），本模块只负责落库与查询；
//! - 不实现密码找回、短信验证等外部通道能力。

use crate::auth::CurrentUser;
use crate::error::AppError;
use crate::response::ApiResponse;
use crate::state::AppState;
use crate::tenant::TenantTx;
use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use uuid::Uuid;

// =============================================================================
// 种子数据：首次启动时创建默认租户与管理员
// =============================================================================

// =============================================================================
// 路由
// =============================================================================

// 后台任务（app.rs）仍通过这些入口调用租户扫描；实现已拆到 tenant_admin。
pub(crate) use crate::system_identity::{ensure_dept_in_tenant, require_perm, require_platform};
pub use crate::system_menu_sync::sync_plugin_menus;

pub use crate::system_seed::{ensure_default_menus, ensure_seed};

pub use crate::tenant_admin::ensure_default_alert_rules;
pub use crate::tenant_bill_collections::scan_collection_reminders;
pub use crate::tenant_overdue::scan_overdue_bills_internal;

/// 系统管理路由，挂载在 `/api/system`。
pub fn routes() -> Router<AppState> {
    Router::new()
        .merge(crate::system_scope::router())
        .merge(crate::system_audit_api::router())
        .merge(crate::system_sessions::router())
        .merge(crate::tenant_admin::router())
        .merge(crate::audit_admin::router())
        .merge(crate::system_identity::router())
        .merge(crate::system_org::router())
        .merge(crate::system_security::router())
        .merge(crate::system_ops::router())
        .route("/plugins", get(list_enabled_plugins))
        .route("/roles/{id}/depts", get(get_role_depts).put(set_role_depts))
        .route("/imports/{kind}", post(crate::imports::import_file))
}

/// 查询当前实例已启用的可插拔业务插件列表。
#[utoipa::path(
    get,
    path = "/api/system/plugins",
    tag = "system",
    responses((status = 200, description = "已启用插件列表", body = ApiResponse<Vec<crate::plugin::PluginInfoVo>>))
)]
async fn list_enabled_plugins(
    State(state): State<AppState>,
    _user: CurrentUser,
) -> Result<ApiResponse<Vec<crate::plugin::PluginInfoVo>>, AppError> {
    let infos = crate::plugin::enabled_plugin_infos(&state.config.plugins.disabled);
    Ok(ApiResponse::new(infos))
}

// =============================================================================
// 菜单
// =============================================================================

/// 同步插件声明的权限点（幂等 upsert）。
///
/// 权限点的唯一来源是代码（`Plugin::permissions`），数据库只是投影。
/// 这样不会出现「代码删了权限点、库里还留着」或反之的漂移。
pub async fn sync_plugin_permissions(
    pool: &sqlx::PgPool,
    plugin: &str,
    perms: &[crate::plugin::PermissionDef],
) -> anyhow::Result<()> {
    for p in perms {
        // 插件声明的权限点按定义都是**租户级**（插件不提供跨租户能力）。
        // 这里必须显式写 scope：列默认值在 0073 之后是 'platform'，
        // 依赖默认值会把插件权限点变成只有超管看得见/授得出的平台权限。
        sqlx::query(
            r#"
            INSERT INTO sys.sys_permission (id, plugin, code, name, scope)
            VALUES ($1, $2, $3, $4, 'tenant')
            ON CONFLICT (code) DO UPDATE SET name = EXCLUDED.name, plugin = EXCLUDED.plugin
            "#,
        )
        .bind(Uuid::now_v7())
        .bind(plugin)
        .bind(p.code)
        .bind(p.name)
        .execute(pool)
        .await?;
    }
    Ok(())
}

/// 角色自定义数据范围：已选部门。
#[utoipa::path(
    get,
    path = "/api/system/roles/{id}/depts",
    tag = "system",
    params(("id" = Uuid, Path, description = "角色 id")),
    responses((status = 200, description = "部门 id 列表", body = ApiResponse<Vec<Uuid>>))
)]
async fn get_role_depts(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Path(id): Path<Uuid>,
) -> Result<ApiResponse<Vec<Uuid>>, AppError> {
    require_perm(&user, crate::perms::ROLE_LIST)?;
    let rows: Vec<Uuid> = sqlx::query_scalar(
        "SELECT dept_id FROM sys.sys_role_dept WHERE role_id = $1 AND tenant_id = $2 ORDER BY dept_id",
    )
    .bind(id)
    .bind(user.tenant_id)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(ApiResponse::new(rows))
}

/// 角色部门请求。
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RoleDeptRequest {
    /// 部门 id 列表（全量覆盖）。
    pub dept_ids: Vec<Uuid>,
}

/// 设置角色自定义数据范围部门。
#[utoipa::path(
    put,
    path = "/api/system/roles/{id}/depts",
    tag = "system",
    params(("id" = Uuid, Path, description = "角色 id")),
    request_body = RoleDeptRequest,
    responses((status = 200, description = "保存后的部门 id 列表", body = ApiResponse<Vec<Uuid>>))
)]
async fn set_role_depts(
    State(state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Path(id): Path<Uuid>,
    Json(req): Json<RoleDeptRequest>,
) -> Result<ApiResponse<Vec<Uuid>>, AppError> {
    require_perm(&user, crate::perms::ROLE_EDIT)?;

    let role_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM sys.sys_role WHERE id = $1 AND tenant_id = $2 AND deleted_at IS NULL)",
    )
    .bind(id)
    .bind(user.tenant_id)
    .fetch_one(&mut *tx)
    .await?;
    if !role_exists {
        return Err(AppError::NotFound);
    }

    let before_depts: Vec<Uuid> = sqlx::query_scalar(
        "SELECT dept_id FROM sys.sys_role_dept WHERE role_id = $1 AND tenant_id = $2 ORDER BY dept_id",
    )
    .bind(id)
    .bind(user.tenant_id)
    .fetch_all(&mut *tx)
    .await?;

    sqlx::query("DELETE FROM sys.sys_role_dept WHERE role_id = $1 AND tenant_id = $2")
        .bind(id)
        .bind(user.tenant_id)
        .execute(&mut *tx)
        .await?;

    for dept_id in &req.dept_ids {
        ensure_dept_in_tenant(&mut tx, user.tenant_id, *dept_id).await?;
        sqlx::query(
            "INSERT INTO sys.sys_role_dept (tenant_id, role_id, dept_id) VALUES ($1, $2, $3) ON CONFLICT DO NOTHING",
        )
        .bind(user.tenant_id)
        .bind(id)
        .bind(dept_id)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;

    kernel_audit_role_depts(&state, &user, id, &before_depts, &req.dept_ids).await;
    Ok(ApiResponse::new(req.dept_ids.clone()))
}

async fn kernel_audit_role_depts(
    state: &AppState,
    user: &CurrentUser,
    role_id: Uuid,
    before: &[Uuid],
    after: &[Uuid],
) {
    crate::audit::record_user(
        &state.db,
        user,
        "system.role.depts",
        "role",
        Some(&role_id.to_string()),
        Some(serde_json::json!({ "before": before, "after": after })),
    )
    .await;
}

/// OpenAPI 片段：系统管理接口（各子域见对应模块的 Api 结构）。
#[derive(utoipa::OpenApi)]
#[openapi(
    paths(
        crate::system_sessions::list_sessions,
        crate::system_sessions::revoke_session,
        list_enabled_plugins,
        crate::system_ops::ops_overview,
        crate::system_ops::ops_detail,
        get_role_depts,
        set_role_depts,
        crate::system_audit_api::list_audit_logs,
        crate::system_audit_api::list_audit_actions,
        crate::system_scope::scope_preview
    ),
    components(schemas(
        crate::plugin::PluginInfoVo,
        crate::metrics::TrafficSummaryVo,
        crate::metrics::RouteMetricVo,
        crate::system_ops::ServerInfoVo,
        crate::system_ops::ProbeItemVo,
        crate::system_ops::OpsOverviewQuery,
        crate::system_ops::OpsOverviewVo,
        crate::system_ops::OpsKvItem,
        crate::system_ops::OpsDetailVo,
        crate::system_audit_api::AuditLogVo,
        crate::system_audit_api::AuditQuery,
        crate::system_scope::ScopePreviewVo,
        crate::system_scope::ScopePreviewQuery,
        RoleDeptRequest,
        crate::system_sessions::SessionVo
    )),
    tags((name = "system", description = "系统管理：用户/角色/权限/菜单/字典"))
)]
pub struct SystemApi;
