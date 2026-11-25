//! 导出文件下载。
//!
//! # 为什么单独成文件
//!
//! 任务创建/列表/删除是「任务管理」（JSON 接口），下载是「字节流 + 头部拼装」，
//! 两者的失败模型与关注点都不同；放在一起会让 `exports_api` 很快超过 300 行。
//!
//! # 安全边界
//!
//! 必须按 `tenant_id + user_id` 双条件取文件 key：导出文件里是完整业务数据，
//! 只校验租户会让同租户的其他用户拿到别人导出的内容。

use axum::extract::{Path, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use uuid::Uuid;

use crate::auth::CurrentUser;
use crate::error::AppError;
use crate::state::AppState;
use crate::tenant::TenantTx;

/// 下载导出文件。
#[utoipa::path(
    get,
    path = "/api/system/export-tasks/{id}/download",
    tag = "system",
    params(("id" = Uuid, Path, description = "任务 id")),
    responses((status = 200, description = "xlsx 文件流"))
)]
pub(crate) async fn download(
    State(state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Path(id): Path<Uuid>,
) -> Result<Response, AppError> {
    let (status, key, name) = sqlx::query_as::<_, (String, Option<String>, Option<String>)>(
        "SELECT status, file_key, file_name FROM sys.sys_export_task WHERE id = $1 AND tenant_id = $2 AND user_id = $3",
    )
    .bind(id)
    .bind(user.tenant_id)
    .bind(user.user_id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(AppError::NotFound)?;
    tx.commit().await?;
    if status != "success" {
        return Err(AppError::Conflict("文件尚未生成完成".into()));
    }
    let key = key.ok_or(AppError::NotFound)?;
    let name = name.unwrap_or_else(|| "export.xlsx".to_string());
    let bytes = state.storage.load(&key).await.map_err(AppError::Internal)?;

    let mut resp = (StatusCode::OK, bytes).into_response();
    resp.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        ),
    );
    if let Ok(v) = HeaderValue::from_str(&crate::files_read::content_disposition(&name)) {
        resp.headers_mut().insert(header::CONTENT_DISPOSITION, v);
    }
    Ok(resp)
}
