//! 认证辅助：租户解析、用户行、权限/部门/数据范围加载。
//!
//! 边界：登录流程在 `auth`；本模块只提供登录期需要的数据加载。
//! 关键约束：所有查询都带 tenant_id 条件，避免跨租户读取。

use crate::error::AppError;
use crate::state::AppState;
use chrono::{DateTime, Utc};
use sqlx::FromRow;
use uuid::Uuid;

/// 数据库中的用户行（只在本模块使用，绝不对外序列化）。
#[derive(Debug, FromRow)]
pub(crate) struct UserRow {
    pub(crate) id: Uuid,
    pub(crate) tenant_id: Uuid,
    pub(crate) username: String,
    pub(crate) password_hash: String,
    pub(crate) status: String,
    pub(crate) is_super: bool,
    pub(crate) failed_attempts: i32,
    pub(crate) locked_until: Option<DateTime<Utc>>,
    pub(crate) must_change_password: bool,
    pub(crate) password_changed_at: DateTime<Utc>,
}

/// 解析租户：显式编码优先；未提供时要求系统只有一个活跃租户。
///
/// 多租户部署必须由前端/调用方传租户编码（或将来从子域解析），
/// 否则同名用户会在多个租户间产生歧义。
pub(crate) async fn resolve_tenant(state: &AppState, code: Option<&str>) -> Result<Uuid, AppError> {
    if let Some(code) = code.map(str::trim).filter(|c| !c.is_empty()) {
        let id = sqlx::query_scalar::<_, Uuid>(
            "SELECT id FROM sys.sys_tenant WHERE code = $1 AND status = 'active' AND deleted_at IS NULL",
        )
        .bind(code)
        .fetch_optional(&state.db)
        .await?;
        // 统一 401：不暴露租户是否存在
        return id.ok_or(AppError::Unauthorized);
    }

    let ids: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM sys.sys_tenant WHERE status = 'active' AND deleted_at IS NULL LIMIT 2",
    )
    .fetch_all(&state.db)
    .await?;

    match ids.len() {
        0 => Err(AppError::Unauthorized),
        1 => Ok(ids[0]),
        // 不透露「系统里有几个租户」：匿名调用方只需要知道必须补租户编码，
        // 具体数量属于部署信息，不该成为枚举素材。
        _ => Err(AppError::Validation("请填写租户编码".into())),
    }
}

pub(crate) async fn load_permissions(
    conn: &mut sqlx::PgConnection,
    user_id: Uuid,
) -> Result<Vec<String>, AppError> {
    let codes = sqlx::query_scalar::<_, String>(
        r#"
        SELECT DISTINCT p.code
        FROM sys.sys_user_role ur
        JOIN sys.sys_role_permission rp ON rp.role_id = ur.role_id
        JOIN sys.sys_permission p ON p.id = rp.permission_id
        WHERE ur.user_id = $1 AND p.deprecated = false
        "#,
    )
    .bind(user_id)
    .fetch_all(&mut *conn)
    .await?;

    Ok(codes)
}

/// 查询用户所属部门。
pub(crate) async fn load_dept(
    conn: &mut sqlx::PgConnection,
    user_id: Uuid,
) -> Result<Option<Uuid>, AppError> {
    let dept_id =
        sqlx::query_scalar::<_, Option<Uuid>>("SELECT dept_id FROM sys.sys_user WHERE id = $1")
            .bind(user_id)
            .fetch_optional(&mut *conn)
            .await?
            .flatten();
    Ok(dept_id)
}

/// 计算用户的数据范围（多角色取最宽）。
///
/// 优先级：all > dept_and_child > dept > self。
/// 没有角色的用户退化为 self（最小权限），避免"忘配角色 = 看全部"。
pub(crate) async fn load_data_scope(
    conn: &mut sqlx::PgConnection,
    user_id: Uuid,
) -> Result<String, AppError> {
    let scopes: Vec<String> = sqlx::query_scalar(
        r#"
        SELECT r.data_scope
        FROM sys.sys_user_role ur
        JOIN sys.sys_role r ON r.id = ur.role_id AND r.deleted_at IS NULL
        WHERE ur.user_id = $1
        "#,
    )
    .bind(user_id)
    .fetch_all(&mut *conn)
    .await?;

    if scopes.iter().any(|s| s == "all") {
        return Ok("all".to_string());
    }

    // 并集语义：多角色同时生效，self 始终包含（本人负责的数据）
    let mut flags: Vec<&str> = vec!["self"];
    for s in &scopes {
        let flag = match s.as_str() {
            "dept_and_child" => "child",
            "dept" => "dept",
            "custom" => "custom",
            _ => continue,
        };
        if !flags.contains(&flag) {
            flags.push(flag);
        }
    }
    flags.sort_unstable();
    Ok(flags.join(","))
}

pub(crate) fn scope_matches(scope: &str, permission: &str) -> bool {
    scope == "*"
        || scope == permission
        || scope
            .strip_suffix('*')
            .is_some_and(|prefix| permission.starts_with(prefix))
}
