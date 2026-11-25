//! 异步导出任务。
//!
//! # 为什么异步
//!
//! 同步导出把「查询 + 生成 xlsx + 下载」放在一个 HTTP 请求里，
//! 数据量一大就会触发网关/浏览器超时。异步任务拆成三步：
//!
//! ```text
//! POST /export-tasks  → 创建任务（pending），立即返回 id
//! 后台 tokio 任务     → running → 生成文件 → success/failed
//! GET  /export-tasks  → 前端轮询状态
//! GET  /{id}/download → 下载生成好的文件
//! ```
//!
//! # 当前支持的导出类型
//!
//! | kind | 内容 | 需要的权限 |
//! |---|---|---|
//! | `system.audit` | 审计日志 | `system:audit:list` |
//! | `system.users` | 用户列表 | `system:user:list` |
//!
//! 插件想接入时，在 `run_export` 里加一个分支即可（或后续抽成注册表）。

use std::future::Future;
use std::pin::Pin;

/// 导出上下文（插件导出用于数据权限过滤）。
pub struct ExportContext {
    /// 当前用户 id。
    pub user_id: Uuid,
    /// 当前租户 id。
    pub tenant_id: Uuid,
    /// 数据范围：all / dept_and_child / custom / dept / self。
    pub data_scope: String,
    /// 当前用户部门。
    pub dept_id: Option<Uuid>,
}

/// 导出产物。
pub struct ExportFile {
    /// xlsx 字节。
    pub bytes: Vec<u8>,
    /// 下载文件名。
    pub file_name: String,
    /// 数据行数。
    pub rows: usize,
}

/// 插件导出函数的 future 类型。
pub type ExportFuture<'a> = Pin<Box<dyn Future<Output = anyhow::Result<ExportFile>> + Send + 'a>>;

/// 插件导出注册项。
///
/// 插件在编译期用 `inventory::submit!` 注册，内核无需认识具体插件。
pub struct ExportRegistration {
    /// 导出类型，例如 `crm.contracts`。
    pub kind: &'static str,
    /// 所需权限点。
    pub permission: &'static str,
    /// 执行函数。
    pub run: for<'a> fn(&'a mut sqlx::PgConnection, &'a ExportContext) -> ExportFuture<'a>,
}

inventory::collect!(ExportRegistration);

/// 查找插件注册的导出类型。
pub fn find_export(kind: &str) -> Option<&'static ExportRegistration> {
    inventory::iter::<ExportRegistration>
        .into_iter()
        .find(|r| r.kind == kind)
}
pub use crate::exports_api::routes;
pub use crate::exports_runner::recover_stale;
use chrono::{DateTime, Utc};
use rust_xlsxwriter::Workbook;
use uuid::Uuid;

/// 审计导出行：(时间, 动作, 资源类型, 资源 id, 结果, IP, User-Agent, 变更内容)。
type AuditExportRow = (
    DateTime<Utc>,
    String,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    Option<serde_json::Value>,
);

type UserExportRow = (String, String, Option<String>, Option<String>, String);

/// 导出审计日志。
pub(crate) async fn export_audit(
    conn: &mut sqlx::PgConnection,
    keyword: Option<&str>,
    resource_type: Option<&str>,
    ip: Option<&str>,
    user_agent: Option<&str>,
    from: Option<DateTime<Utc>>,
    to: Option<DateTime<Utc>>,
) -> anyhow::Result<(Vec<u8>, String, usize)> {
    let like = keyword.map(|k| format!("%{}%", k.trim()));
    let ua_like = user_agent.map(|v| format!("%{}%", v.trim()));
    let rows: Vec<AuditExportRow> = sqlx::query_as(
        r#"
        SELECT created_at, action, resource_type, COALESCE(resource_id, ''), result,
               host(ip) AS ip, user_agent, detail
        FROM sys.sys_audit_log
        WHERE ($1::text IS NULL OR action ILIKE $1)
          AND ($2::text IS NULL OR resource_type = $2)
          AND ($3::text IS NULL OR host(ip) = $3)
          AND ($4::text IS NULL OR user_agent ILIKE $4)
          AND ($5::timestamptz IS NULL OR created_at >= $5)
          AND ($6::timestamptz IS NULL OR created_at <= $6)
        ORDER BY created_at DESC
        LIMIT 20000
        "#,
    )
    .bind(like.as_deref())
    .bind(resource_type)
    .bind(ip.map(str::trim).filter(|v| !v.is_empty()))
    .bind(ua_like.as_deref())
    .bind(from)
    .bind(to)
    .fetch_all(&mut *conn)
    .await?;

    let name = format!("audit-{}.xlsx", Utc::now().format("%Y%m%d%H%M%S"));
    let row_count = rows.len();
    // xlsx 生成是同步 CPU/内存密集操作，必须走受控阻塞池（AGENTS 铁律 16）。
    // 行数据先查成拥有的 Vec 再移入闭包，闭包内不得持有 &PgConnection。
    let bytes = crate::xlsx::run_anyhow(move || {
        let mut wb = Workbook::new();
        let sheet = wb.add_worksheet();
        for (i, h) in [
            "时间",
            "动作",
            "资源类型",
            "资源 id",
            "结果",
            "来源 IP",
            "User-Agent",
            "变更内容",
        ]
        .iter()
        .enumerate()
        {
            sheet.write_string(0, i as u16, *h)?;
        }
        for (r, (created, action, resource, rid, result, ip, user_agent, detail)) in
            rows.iter().enumerate()
        {
            let row = (r + 1) as u32;
            sheet.write_string(row, 0, created.to_rfc3339())?;
            sheet.write_string(row, 1, action)?;
            sheet.write_string(row, 2, resource)?;
            sheet.write_string(row, 3, rid)?;
            sheet.write_string(row, 4, result)?;
            sheet.write_string(row, 5, ip.as_deref().unwrap_or(""))?;
            sheet.write_string(row, 6, user_agent.as_deref().unwrap_or(""))?;
            sheet.write_string(
                row,
                7,
                detail
                    .as_ref()
                    .map(serde_json::Value::to_string)
                    .unwrap_or_default(),
            )?;
        }
        Ok(wb.save_to_buffer()?)
    })
    .await?;
    Ok((bytes, name, row_count))
}

/// 导出用户列表。
///
/// 字段级权限与列表接口保持一致：没有 `system:user:view_sensitive`
/// （且不是平台超管）时，邮箱必须打码——否则异步导出会成为脱敏旁路。
///
/// 行级权限同样与 [`crate::system_users::list_users`] 保持一致：
/// 必须复用 [`crate::scope::condition_with_owner`]。导出少写这一段，
/// 数据范围为 `self` 的用户就能导出全租户用户清单（横向越权）。
pub(crate) async fn export_users(
    conn: &mut sqlx::PgConnection,
    ctx: &ExportContext,
) -> anyhow::Result<(Vec<u8>, String, usize)> {
    let can_view_sensitive: bool = sqlx::query_scalar(
        r#"
        SELECT COALESCE(u.is_super, false) OR EXISTS (
            SELECT 1
            FROM sys.sys_user_role ur
            JOIN sys.sys_role_permission rp ON rp.role_id = ur.role_id
            JOIN sys.sys_permission p ON p.id = rp.permission_id
            WHERE ur.user_id = u.id
              AND p.code = 'system:user:view_sensitive'
              AND p.deprecated = false
        )
        FROM sys.sys_user u
        WHERE u.id = $1
        "#,
    )
    .bind(ctx.user_id)
    .fetch_optional(&mut *conn)
    .await?
    .unwrap_or(false);

    let sql = format!(
        r#"
        SELECT u.username, u.display_name, u.email, d.name, u.status
        FROM sys.sys_user u
        LEFT JOIN sys.sys_dept d ON d.id = u.dept_id
        WHERE u.deleted_at IS NULL
          {}
        ORDER BY u.created_at DESC
        LIMIT 20000
        "#,
        crate::scope::condition_with_owner("u.id", "u.dept_id", "$1", "$2", "$3")
    );
    let rows: Vec<UserExportRow> = sqlx::query_as(&sql)
        .bind(&ctx.data_scope)
        .bind(ctx.user_id)
        .bind(ctx.dept_id)
        .fetch_all(&mut *conn)
        .await?;

    let name = format!("users-{}.xlsx", Utc::now().format("%Y%m%d%H%M%S"));
    let row_count = rows.len();
    // 与 export_audit 同理：脱敏后的行数据移入闭包，在受控阻塞池里生成 xlsx。
    let bytes = crate::xlsx::run_anyhow(move || {
        let mut wb = Workbook::new();
        let sheet = wb.add_worksheet();
        for (i, h) in ["用户名", "显示名", "邮箱", "部门", "状态"]
            .iter()
            .enumerate()
        {
            sheet.write_string(0, i as u16, *h)?;
        }
        for (r, (username, display, email, dept, status)) in rows.iter().enumerate() {
            let row = (r + 1) as u32;
            sheet.write_string(row, 0, username)?;
            sheet.write_string(row, 1, display)?;
            if let Some(v) = email {
                let value = if can_view_sensitive {
                    v.clone()
                } else {
                    crate::mask::email(v)
                };
                sheet.write_string(row, 2, &value)?;
            }
            if let Some(v) = dept {
                sheet.write_string(row, 3, v)?;
            }
            sheet.write_string(row, 4, status)?;
        }
        Ok(wb.save_to_buffer()?)
    })
    .await?;
    Ok((bytes, name, row_count))
}

/// OpenAPI 片段。
#[derive(utoipa::OpenApi)]
#[openapi(
    paths(
        crate::exports_api::list_tasks,
        crate::exports_api::create_task,
        crate::exports_download::download,
        crate::exports_api::delete_task
    ),
    components(schemas(
        crate::exports_api::ExportTaskVo,
        crate::exports_api::CreateExportRequest
    )),
    tags((name = "system", description = "系统管理：用户/角色/权限/菜单/字典"))
)]
pub struct ExportApi;
