//! 用户导入的解析与单行校验/写入。
//!
//! 边界：导入 HTTP 接口在 `user_import`；本模块只处理行级逻辑。
//! 关键约束：校验与写入都基于同一行数据，避免两处口径不一致。

use crate::auth::CurrentUser;
use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::password;
use uuid::Uuid;

/// 一行导入数据。
pub(crate) struct ImportRow {
    pub(crate) username: String,
    pub(crate) display_name: String,
    pub(crate) password: String,
    pub(crate) email: Option<String>,
    pub(crate) phone: Option<String>,
    pub(crate) dept: Option<String>,
}

pub(crate) fn parse_rows(bytes: &[u8]) -> Result<Vec<ImportRow>, AppError> {
    use calamine::{open_workbook_from_rs, Data, Reader, Xlsx};
    use std::io::Cursor;

    let mut workbook: Xlsx<_> = open_workbook_from_rs(Cursor::new(bytes.to_vec()))
        .map_err(|e| AppError::Validation(format!("无法解析 Excel: {e}")))?;
    let range = workbook
        .worksheet_range_at(0)
        .ok_or_else(|| AppError::Validation("Excel 中没有工作表".into()))?
        .map_err(|e| AppError::Validation(format!("读取工作表失败: {e}")))?;

    let mut header = std::collections::HashMap::new();
    if let Some(first) = range.rows().next() {
        for (idx, cell) in first.iter().enumerate() {
            if let Data::String(name) = cell {
                header.insert(name.trim().to_string(), idx);
            }
        }
    }
    for required in ["用户名", "显示名", "初始密码"] {
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
        if row.iter().all(|c| c.to_string().trim().is_empty()) {
            continue;
        }
        out.push(ImportRow {
            username: cell(row, "用户名").unwrap_or_default(),
            display_name: cell(row, "显示名").unwrap_or_default(),
            password: cell(row, "初始密码").unwrap_or_default(),
            email: cell(row, "邮箱"),
            phone: cell(row, "手机号"),
            dept: cell(row, "部门"),
        });
    }
    Ok(out)
}

pub(crate) async fn validate_row(
    conn: &mut sqlx::PgConnection,
    user: &CurrentUser,
    row: &ImportRow,
) -> Result<(Option<String>, Option<Uuid>), String> {
    if row.username.len() < 3 || row.username.len() > 64 {
        return Err("用户名长度必须为 3~64 个字符".into());
    }
    if row.display_name.is_empty() {
        return Err("显示名不能为空".into());
    }
    password::validate_strength(&row.password).map_err(|e| e.to_string())?;
    let phone = match row.phone.as_deref() {
        Some(value) => Some(
            crate::mask::normalize_phone(value).ok_or_else(|| "手机号格式不正确".to_string())?,
        ),
        None => None,
    };

    let dept_id: Option<Uuid> = match &row.dept {
        None => None,
        Some(name) => {
            let found: Option<Uuid> = sqlx::query_scalar(
                "SELECT id FROM sys.sys_dept WHERE tenant_id = $1 AND name = $2 AND deleted_at IS NULL",
            )
            .bind(user.tenant_id)
            .bind(name)
            .fetch_optional(&mut *conn)
            .await
            .map_err(|e| format!("查询部门失败: {e}"))?;
            match found {
                Some(id) => Some(id),
                None => return Err(format!("部门不存在: {name}")),
            }
        }
    };

    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM sys.sys_user
         WHERE tenant_id = $1 AND username = $2 AND deleted_at IS NULL)",
    )
    .bind(user.tenant_id)
    .bind(&row.username)
    .fetch_one(&mut *conn)
    .await
    .map_err(|e| format!("查询用户名失败: {e}"))?;
    if exists {
        return Err("用户名已存在".into());
    }
    Ok((phone, dept_id))
}

/// 校验并写入单行；调用方提供保存点，数据库内部错误仅记日志，不回显 SQL 细节。
pub(crate) async fn insert_row(
    conn: &mut sqlx::PgConnection,
    user: &CurrentUser,
    row: &ImportRow,
) -> Result<(), String> {
    let (phone, dept_id) = validate_row(conn, user, row).await?;
    // 每行一次 Argon2；必须走受控阻塞池，否则 500 行会把 worker 占满几十秒。
    let hash = password::hash_password_async(row.password.clone())
        .await
        .map_err(|e| format!("密码哈希失败: {e}"))?;

    let result = sqlx::query(
        r#"
        INSERT INTO sys.sys_user
            (id, tenant_id, username, display_name, password_hash, email, phone, dept_id, status,
             created_by)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 'active', $9)
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(user.tenant_id)
    .bind(&row.username)
    .bind(&row.display_name)
    .bind(hash)
    .bind(&row.email)
    .bind(&phone)
    .bind(dept_id)
    .bind(user.user_id)
    .execute(&mut *conn)
    .await;

    match result {
        Ok(_) => Ok(()),
        Err(sqlx::Error::Database(db)) if db.code().as_deref() == Some("23505") => {
            Err("用户名已存在".into())
        }
        Err(error) => {
            tracing::warn!(error = ?error, "用户导入单行写入失败");
            Err("写入失败，请检查数据或联系管理员".into())
        }
    }
}

/// 导入选项。
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ImportOptions {
    /// 仅校验，不写库。
    pub dry_run: Option<bool>,
}

/// 导入错误导出请求。
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ImportErrorsRequest {
    /// 错误明细。
    pub errors: Vec<UserImportError>,
}

/// 导入结果。
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserImportResult {
    /// 总行数。
    pub total: usize,
    /// 成功行数。
    pub success: usize,
    /// 失败行数。
    pub failed: usize,
    /// 失败明细。
    pub errors: Vec<UserImportError>,
}

/// 单行失败。
#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserImportError {
    /// Excel 行号（从 1 开始，含表头）。
    pub row: usize,
    /// 原因。
    pub message: String,
    /// 原始用户名。
    pub username: Option<String>,
    /// 原始显示名。
    pub display_name: Option<String>,
    /// 原始邮箱。
    pub email: Option<String>,
    /// 原始手机号。
    pub phone: Option<String>,
    /// 原始部门。
    pub dept: Option<String>,
}

impl UserImportError {
    pub(crate) fn new(row: usize, source: &ImportRow, message: String) -> Self {
        Self {
            row,
            message,
            username: Some(source.username.clone()),
            display_name: Some(source.display_name.clone()),
            email: source.email.clone(),
            phone: source.phone.clone(),
            dept: source.dept.clone(),
        }
    }
}
