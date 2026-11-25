//! 插件数据导入框架。
//!
//! 与导出对称：插件用 `inventory::submit!` 注册 `ImportRegistration`，
//! 内核负责鉴权、文件上传、事务与错误转换，插件只关心 xlsx 解析和写库。

use crate::auth::CurrentUser;
use crate::error::AppError;
use crate::response::ApiResponse;
use crate::state::AppState;
use crate::tenant::TenantTx;
use axum::extract::{Multipart, Path, State};
use serde::Serialize;
use std::future::Future;
use std::pin::Pin;
use uuid::Uuid;

/// 导入上下文（数据权限与归属）。
pub struct ImportContext {
    /// 当前用户 id。
    pub user_id: Uuid,
    /// 当前租户 id。
    pub tenant_id: Uuid,
    /// 数据范围。
    pub data_scope: String,
    /// 当前用户部门。
    pub dept_id: Option<Uuid>,
}

/// 导入结果。
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    /// 成功行数。
    pub success: usize,
    /// 失败行数。
    pub failed: usize,
    /// 错误明细（最多 50 条）。
    pub errors: Vec<String>,
}

/// 插件导入函数的 future 类型。
pub type ImportFuture<'a> = Pin<Box<dyn Future<Output = anyhow::Result<ImportResult>> + Send + 'a>>;

/// 插件导入注册项。
pub struct ImportRegistration {
    /// 导入类型，例如 `crm.contracts`。
    pub kind: &'static str,
    /// 所需权限点。
    pub permission: &'static str,
    /// 执行函数：xlsx 字节 + 事务连接 + 上下文。
    pub run:
        for<'a> fn(&'a [u8], &'a mut sqlx::PgConnection, &'a ImportContext) -> ImportFuture<'a>,
}

inventory::collect!(ImportRegistration);

/// 查找插件注册的导入类型。
pub fn find_import(kind: &str) -> Option<&'static ImportRegistration> {
    inventory::iter::<ImportRegistration>
        .into_iter()
        .find(|r| r.kind == kind)
}

/// 上传并执行导入（同步返回结果）。
#[utoipa::path(
    post,
    path = "/api/system/imports/{kind}",
    tag = "system",
    params(("kind" = String, Path, description = "导入类型，例如 crm.contracts")),
    request_body(content = String, content_type = "multipart/form-data", description = "xlsx 文件，字段名 file"),
    responses((status = 200, description = "导入结果", body = ApiResponse<ImportResult>))
)]
pub async fn import_file(
    State(state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Path(kind): Path<String>,
    mut multipart: Multipart,
) -> Result<ApiResponse<ImportResult>, AppError> {
    let reg = find_import(&kind).ok_or_else(|| AppError::Validation("不支持的导入类型".into()))?;
    if !user.has_permission(reg.permission) {
        return Err(AppError::Forbidden);
    }

    let mut bytes: Option<Vec<u8>> = None;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::Validation(format!("解析上传内容失败: {e}")))?
    {
        if field.name() == Some("file") {
            bytes = Some(
                field
                    .bytes()
                    .await
                    .map_err(|e| AppError::Validation(format!("读取文件失败: {e}")))?
                    .to_vec(),
            );
        }
    }
    let bytes = bytes.ok_or_else(|| AppError::Validation("缺少 file 字段".into()))?;

    let ctx = ImportContext {
        user_id: user.user_id,
        tenant_id: user.tenant_id,
        data_scope: user.data_scope.clone(),
        dept_id: user.dept_id,
    };
    // 插件返回的是「第 N 行: xxx」这类面向用户的消息；完整错误链只进日志，
    // 避免把 SQL / 文件路径带进 problem+json 的 detail。
    let result = (reg.run)(&bytes, &mut tx, &ctx).await.map_err(|e| {
        tracing::warn!(kind, error = %format!("{e:#}"), "数据导入失败");
        AppError::Validation(e.to_string())
    })?;
    tx.commit().await?;

    crate::audit::record_user(
        &state.db,
        &user,
        "system.import",
        "import",
        Some(&kind),
        Some(serde_json::json!({
            "kind": kind,
            "success": result.success,
            "failed": result.failed
        })),
    )
    .await;

    Ok(ApiResponse::new(result))
}

/// 导入接口的 OpenAPI 片段。
#[derive(utoipa::OpenApi)]
#[openapi(
    paths(import_file),
    components(schemas(ImportResult)),
    tags((name = "system", description = "系统管理"))
)]
pub struct ImportApi;
