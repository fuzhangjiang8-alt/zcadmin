//! CRM 合同导入：Excel 行解析与逐行写入。
//!
//! 边界：通用单元格解析在 `import`；本模块只处理单一实体。
//! 关键约束：单行失败不阻断整批，错误按行号收集。

use calamine::{open_workbook_from_rs, Reader, Xlsx};
use kernel::imports::{ImportContext, ImportFuture, ImportResult};
use sqlx::PgConnection;
use std::io::Cursor;

use crate::import::cell_text;

/// 导入实现。
pub fn contracts<'a>(
    bytes: &'a [u8],
    conn: &'a mut PgConnection,
    ctx: &'a ImportContext,
) -> ImportFuture<'a> {
    Box::pin(async move {
        let cursor = Cursor::new(bytes.to_vec());
        let mut workbook: Xlsx<_> =
            open_workbook_from_rs(cursor).map_err(|e| anyhow::anyhow!("无法读取 xlsx: {e}"))?;
        let range = workbook
            .worksheet_range_at(0)
            .ok_or_else(|| anyhow::anyhow!("xlsx 没有工作表"))?
            .map_err(|e| anyhow::anyhow!("读取工作表失败: {e}"))?;

        let mut success = 0usize;
        let mut failed = 0usize;
        let mut errors: Vec<String> = Vec::new();

        for (idx, row) in range.rows().enumerate().skip(1).take(2000) {
            let line = idx + 1;
            let contract_no = row.first().map(cell_text).unwrap_or_default();
            let name = row.get(1).map(cell_text).unwrap_or_default();
            let customer_code = row.get(2).map(cell_text).unwrap_or_default();
            let amount_text = row.get(3).map(cell_text).unwrap_or_default();
            let status = {
                let s = row.get(4).map(cell_text).unwrap_or_default();
                if s.is_empty() {
                    "draft".to_string()
                } else {
                    s
                }
            };

            if contract_no.is_empty() || name.is_empty() || customer_code.is_empty() {
                failed += 1;
                if errors.len() < 50 {
                    errors.push(format!("第 {line} 行: 合同编号/名称/客户编码不能为空"));
                }
                continue;
            }
            let amount: f64 = match amount_text.parse() {
                Ok(v) if v >= 0.0 => v,
                _ => {
                    failed += 1;
                    if errors.len() < 50 {
                        errors.push(format!("第 {line} 行: 金额格式错误（{amount_text}）"));
                    }
                    continue;
                }
            };
            if !["draft", "active", "completed", "void"].contains(&status.as_str()) {
                failed += 1;
                if errors.len() < 50 {
                    errors.push(format!("第 {line} 行: 状态非法（{status}）"));
                }
                continue;
            }

            let customer_id: Option<uuid::Uuid> = sqlx::query_scalar(
                "SELECT id FROM crm.customer WHERE tenant_id = $1 AND code = $2 AND deleted_at IS NULL",
            )
            .bind(ctx.tenant_id)
            .bind(&customer_code)
            .fetch_optional(&mut *conn)
            .await?;
            let Some(customer_id) = customer_id else {
                failed += 1;
                if errors.len() < 50 {
                    errors.push(format!("第 {line} 行: 客户编码不存在（{customer_code}）"));
                }
                continue;
            };

            let exists: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM crm.contract WHERE tenant_id = $1 AND contract_no = $2 AND deleted_at IS NULL)",
            )
            .bind(ctx.tenant_id)
            .bind(&contract_no)
            .fetch_one(&mut *conn)
            .await?;
            if exists {
                failed += 1;
                if errors.len() < 50 {
                    errors.push(format!("第 {line} 行: 合同编号已存在（{contract_no}）"));
                }
                continue;
            }

            sqlx::query("SAVEPOINT import_row")
                .execute(&mut *conn)
                .await?;

            let result = sqlx::query(
                r#"
                INSERT INTO crm.contract
                    (id, tenant_id, customer_id, contract_no, name, amount_cents, status,
                     owner_id, dept_id, created_by)
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $8)
                "#,
            )
            .bind(uuid::Uuid::now_v7())
            .bind(ctx.tenant_id)
            .bind(customer_id)
            .bind(&contract_no)
            .bind(&name)
            .bind((amount * 100.0).round() as i32)
            .bind(&status)
            .bind(ctx.user_id)
            .bind(ctx.dept_id)
            .execute(&mut *conn)
            .await;

            match result {
                Ok(_) => {
                    sqlx::query("RELEASE SAVEPOINT import_row")
                        .execute(&mut *conn)
                        .await?;
                    success += 1
                }
                Err(e) => {
                    sqlx::query("ROLLBACK TO SAVEPOINT import_row")
                        .execute(&mut *conn)
                        .await?;
                    failed += 1;
                    if errors.len() < 50 {
                        // 原始 sqlx 错误会带表名/约束/参数：只进日志，报告里给稳定文案。
                        tracing::warn!(line, error = %e, "crm 合同导入写入失败");
                        errors.push(format!("第 {line} 行: 写入失败（内部错误）"));
                    }
                }
            }
        }

        Ok(ImportResult {
            success,
            failed,
            errors,
        })
    })
}

inventory::submit! {
    kernel::imports::ImportRegistration {
        kind: "crm.contracts",
        permission: "crm:contract:add",
        run: contracts,
    }
}
