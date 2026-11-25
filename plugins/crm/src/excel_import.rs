//! CRM Excel 导入：解析、逐行校验与写入。
//!
//! 边界：导出在 `excel`；本模块只处理导入。
//! 关键约束：单行失败不阻断整批，错误按行号返回。

use axum::extract::Multipart;
use kernel::prelude::*;
use serde::Serialize;
use std::io::Cursor;

use crate::excel::MAX_IMPORT_ROWS;

/// 导入结果。
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    /// 总行数（不含表头）。
    pub total: usize,
    /// 成功行数。
    pub success: usize,
    /// 失败行数。
    pub failed: usize,
    /// 失败明细。
    pub errors: Vec<ImportError>,
}

/// 单行失败原因。
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ImportError {
    /// Excel 行号（从 1 开始，含表头）。
    pub row: usize,
    /// 失败原因。
    pub message: String,
}

/// 导入客户（xlsx）。
#[utoipa::path(
    post,
    path = "/api/crm/customers/import",
    tag = "crm",
    responses((status = 200, description = "导入结果", body = ApiResponse<ImportResult>))
)]
pub async fn import(
    State(state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    mut multipart: Multipart,
) -> Result<ApiResponse<ImportResult>, AppError> {
    crate::perm::require(&user, crate::perm::CUSTOMER_IMPORT)?;
    if !user.has_permission("crm:customer:import") {
        return Err(AppError::Forbidden);
    }

    let mut bytes: Option<Vec<u8>> = None;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::Validation(format!("解析上传内容失败: {e}")))?
    {
        if field.file_name().is_none() {
            continue;
        }
        bytes = Some(
            field
                .bytes()
                .await
                .map_err(|e| AppError::Validation(format!("读取文件失败: {e}")))?
                .to_vec(),
        );
        break;
    }
    let bytes = bytes.ok_or_else(|| AppError::Validation("未找到上传文件".into()))?;

    let parsed = parse_rows(&bytes)?;
    if parsed.len() > MAX_IMPORT_ROWS {
        return Err(AppError::Validation(format!(
            "单次最多导入 {MAX_IMPORT_ROWS} 行，当前 {} 行",
            parsed.len()
        )));
    }

    let mut result = ImportResult {
        total: parsed.len(),
        success: 0,
        failed: 0,
        errors: Vec::new(),
    };

    for (idx, row) in parsed.iter().enumerate() {
        let excel_row = idx + 2; // 表头占第 1 行
                                 // 每行一个 SAVEPOINT：单行失败（唯一键/约束）只回滚该行，
                                 // 否则 PostgreSQL 会把整个事务置为 aborted，后续行全部 25P02。
        if let Err(e) = sqlx::query("SAVEPOINT import_row").execute(&mut *tx).await {
            return Err(AppError::Database(e));
        }
        if let Err(msg) = insert_row(&mut tx, &user, row).await {
            if let Err(e) = sqlx::query("ROLLBACK TO SAVEPOINT import_row")
                .execute(&mut *tx)
                .await
            {
                return Err(AppError::Database(e));
            }
            result.failed += 1;
            result.errors.push(ImportError {
                row: excel_row,
                message: msg,
            });
        } else {
            if let Err(e) = sqlx::query("RELEASE SAVEPOINT import_row")
                .execute(&mut *tx)
                .await
            {
                return Err(AppError::Database(e));
            }
            result.success += 1;
        }
    }
    tx.commit().await?;

    kernel::audit::record_user(
        &state.db,
        &user,
        "crm.customer.import",
        "customer",
        None,
        Some(serde_json::json!({
            "total": result.total, "success": result.success, "failed": result.failed
        })),
    )
    .await;

    Ok(ApiResponse::new(result))
}

/// 一行导入数据。
pub(crate) struct ImportRow {
    pub(crate) code: String,
    pub(crate) name: String,
    pub(crate) level: String,
    pub(crate) phone: Option<String>,
    pub(crate) email: Option<String>,
}

/// 解析 xlsx 为业务行。
pub(crate) fn parse_rows(bytes: &[u8]) -> Result<Vec<ImportRow>, AppError> {
    use calamine::{open_workbook_from_rs, Data, Reader, Xlsx};

    let cursor = Cursor::new(bytes.to_vec());
    let mut workbook: Xlsx<_> = open_workbook_from_rs(cursor)
        .map_err(|e| AppError::Validation(format!("无法解析 Excel: {e}")))?;

    let range = workbook
        .worksheet_range_at(0)
        .ok_or_else(|| AppError::Validation("Excel 中没有工作表".into()))?
        .map_err(|e| AppError::Validation(format!("读取工作表失败: {e}")))?;

    // 表头映射：名称 -> 列索引
    let mut header = std::collections::HashMap::new();
    for (idx, cell) in range.rows().next().unwrap_or(&[]).iter().enumerate() {
        if let Data::String(name) = cell {
            header.insert(name.trim().to_string(), idx);
        }
    }
    for required in ["客户编码", "客户名称"] {
        if !header.contains_key(required) {
            return Err(AppError::Validation(format!("缺少必填列: {required}")));
        }
    }

    let cell = |row: &[Data], name: &str| -> Option<String> {
        header.get(name).and_then(|i| row.get(*i)).and_then(|c| {
            let s = c.to_string();
            if s.trim().is_empty() {
                None
            } else {
                Some(s.trim().to_string())
            }
        })
    };

    let mut out = Vec::new();
    for row in range.rows().skip(1) {
        // 整行为空则跳过（Excel 常见的尾部空行）
        if row.iter().all(|c| c.to_string().trim().is_empty()) {
            continue;
        }
        out.push(ImportRow {
            code: cell(row, "客户编码").unwrap_or_default(),
            name: cell(row, "客户名称").unwrap_or_default(),
            level: cell(row, "客户等级").unwrap_or_else(|| "normal".to_string()),
            phone: cell(row, "联系电话"),
            email: cell(row, "邮箱"),
        });
    }
    Ok(out)
}

/// 插入一行；返回可读错误信息（用于汇总给用户）。
async fn insert_row(
    tx: &mut sqlx::PgConnection,
    user: &CurrentUser,
    row: &ImportRow,
) -> Result<(), String> {
    if row.code.is_empty() || row.code.len() > 32 {
        return Err("客户编码为空或超过 32 字符".into());
    }
    if row.name.is_empty() || row.name.chars().count() > 128 {
        return Err("客户名称为空或超过 128 字符".into());
    }

    let result = sqlx::query(
        r#"
        INSERT INTO crm.customer
            (id, tenant_id, code, name, level, status, owner_id, dept_id, phone, email, created_by)
        VALUES ($1, $2, $3, $4, $5, 'potential', $6, $7, $8, $9, $6)
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(user.tenant_id)
    .bind(&row.code)
    .bind(&row.name)
    .bind(&row.level)
    .bind(user.user_id)
    .bind(user.dept_id)
    .bind(&row.phone)
    .bind(&row.email)
    .execute(&mut *tx)
    .await;

    match result {
        Ok(_) => Ok(()),
        Err(sqlx::Error::Database(db)) if db.code().as_deref() == Some("23505") => {
            Err("客户编码已存在".into())
        }
        Err(e) => Err(format!("写入失败: {e}")),
    }
}
