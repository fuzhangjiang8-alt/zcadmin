//! CRM 业务数据导出（插件注册到内核导出框架）。
//!
//! 内核只认识 `ExportRegistration`，不认识 CRM；新增导出类型不需要改内核。
//! 数据权限通过 `api::scope_filter` 复用与列表查询相同的口径。

use crate::api::scope_filter;
use kernel::exports::{ExportContext, ExportFile, ExportFuture};
use rust_xlsxwriter::Workbook;
use sqlx::PgConnection;

/// 合同导出。
pub fn contracts<'a>(conn: &'a mut PgConnection, ctx: &'a ExportContext) -> ExportFuture<'a> {
    Box::pin(async move {
        let sql = format!(
            r#"
            SELECT c.contract_no, c.name, c.amount_cents, c.paid_cents, c.status,
                   to_char(c.created_at, 'YYYY-MM-DD HH24:MI:SS')
            FROM crm.contract c
            WHERE c.deleted_at IS NULL AND c.tenant_id = $1
              {}
            ORDER BY c.created_at DESC
            LIMIT 20000
            "#,
            scope_filter("$2", "$3", "$4")
        );
        let rows: Vec<(String, String, i32, i64, String, String)> = sqlx::query_as(&sql)
            .bind(ctx.tenant_id)
            .bind(&ctx.data_scope)
            .bind(ctx.user_id)
            .bind(ctx.dept_id)
            .fetch_all(&mut *conn)
            .await?;

        let row_count = rows.len();
        let bytes = kernel::xlsx::run_anyhow(move || {
            let mut wb = Workbook::new();
            let sheet = wb.add_worksheet();
            for (i, h) in [
                "合同编号",
                "合同名称",
                "金额(元)",
                "已回款(元)",
                "状态",
                "创建时间",
            ]
            .iter()
            .enumerate()
            {
                sheet.write_string(0, i as u16, *h)?;
            }
            for (r, row) in rows.iter().enumerate() {
                let i = (r + 1) as u32;
                sheet.write_string(i, 0, &row.0)?;
                sheet.write_string(i, 1, &row.1)?;
                sheet.write_string(i, 2, format!("{:.2}", row.2 as f64 / 100.0))?;
                sheet.write_string(i, 3, format!("{:.2}", row.3 as f64 / 100.0))?;
                sheet.write_string(i, 4, &row.4)?;
                sheet.write_string(i, 5, &row.5)?;
            }
            Ok(wb.save_to_buffer()?)
        })
        .await?;
        let name = format!(
            "contracts-{}.xlsx",
            chrono::Utc::now().format("%Y%m%d%H%M%S")
        );
        Ok(ExportFile {
            bytes,
            file_name: name,
            rows: row_count,
        })
    })
}

/// 回款导出。
pub fn payments<'a>(conn: &'a mut PgConnection, ctx: &'a ExportContext) -> ExportFuture<'a> {
    Box::pin(async move {
        let sql = format!(
            r#"
            SELECT c.payment_no, ct.contract_no, c.amount_cents,
                   COALESCE(to_char(c.paid_at, 'YYYY-MM-DD HH24:MI:SS'), ''),
                   COALESCE(c.method, '')
            FROM crm.payment c
            JOIN crm.contract ct ON ct.id = c.contract_id
            WHERE c.deleted_at IS NULL AND c.tenant_id = $1
              {}
            ORDER BY c.paid_at DESC NULLS LAST, c.created_at DESC
            LIMIT 20000
            "#,
            scope_filter("$2", "$3", "$4")
        );
        let rows: Vec<(String, String, i32, String, String)> = sqlx::query_as(&sql)
            .bind(ctx.tenant_id)
            .bind(&ctx.data_scope)
            .bind(ctx.user_id)
            .bind(ctx.dept_id)
            .fetch_all(&mut *conn)
            .await?;

        let row_count = rows.len();
        let bytes = kernel::xlsx::run_anyhow(move || {
            let mut wb = Workbook::new();
            let sheet = wb.add_worksheet();
            for (i, h) in ["回款单号", "合同编号", "金额(元)", "回款时间", "方式"]
                .iter()
                .enumerate()
            {
                sheet.write_string(0, i as u16, *h)?;
            }
            for (r, row) in rows.iter().enumerate() {
                let i = (r + 1) as u32;
                sheet.write_string(i, 0, &row.0)?;
                sheet.write_string(i, 1, &row.1)?;
                sheet.write_string(i, 2, format!("{:.2}", row.2 as f64 / 100.0))?;
                sheet.write_string(i, 3, &row.3)?;
                sheet.write_string(i, 4, &row.4)?;
            }
            Ok(wb.save_to_buffer()?)
        })
        .await?;
        let name = format!(
            "payments-{}.xlsx",
            chrono::Utc::now().format("%Y%m%d%H%M%S")
        );
        Ok(ExportFile {
            bytes,
            file_name: name,
            rows: row_count,
        })
    })
}

inventory::submit! {
    kernel::exports::ExportRegistration {
        kind: "crm.contracts",
        permission: "crm:contract:list",
        run: contracts,
    }
}

inventory::submit! {
    kernel::exports::ExportRegistration {
        kind: "crm.payments",
        permission: "crm:payment:list",
        run: payments,
    }
}

/// 联系人导出。
pub fn contacts<'a>(conn: &'a mut PgConnection, ctx: &'a ExportContext) -> ExportFuture<'a> {
    Box::pin(async move {
        let sql = format!(
            r#"
            SELECT c.name, cu.code, COALESCE(c.phone, ''), COALESCE(c.email, ''),
                   CASE WHEN c.is_primary THEN '是' ELSE '否' END,
                   to_char(c.created_at, 'YYYY-MM-DD HH24:MI:SS')
            FROM crm.contact c
            JOIN crm.customer cu ON cu.id = c.customer_id
            WHERE c.deleted_at IS NULL AND c.tenant_id = $1
              {}
            ORDER BY c.created_at DESC
            LIMIT 20000
            "#,
            scope_filter("$2", "$3", "$4")
        );
        let rows: Vec<(String, String, String, String, String, String)> = sqlx::query_as(&sql)
            .bind(ctx.tenant_id)
            .bind(&ctx.data_scope)
            .bind(ctx.user_id)
            .bind(ctx.dept_id)
            .fetch_all(&mut *conn)
            .await?;

        let row_count = rows.len();
        let bytes = kernel::xlsx::run_anyhow(move || {
            let mut wb = Workbook::new();
            let sheet = wb.add_worksheet();
            for (i, h) in ["姓名", "客户编码", "电话", "邮箱", "主要联系人", "创建时间"]
                .iter()
                .enumerate()
            {
                sheet.write_string(0, i as u16, *h)?;
            }
            for (r, row) in rows.iter().enumerate() {
                let i = (r + 1) as u32;
                sheet.write_string(i, 0, &row.0)?;
                sheet.write_string(i, 1, &row.1)?;
                sheet.write_string(i, 2, &row.2)?;
                sheet.write_string(i, 3, &row.3)?;
                sheet.write_string(i, 4, &row.4)?;
                sheet.write_string(i, 5, &row.5)?;
            }
            Ok(wb.save_to_buffer()?)
        })
        .await?;
        let name = format!(
            "contacts-{}.xlsx",
            chrono::Utc::now().format("%Y%m%d%H%M%S")
        );
        Ok(ExportFile {
            bytes,
            file_name: name,
            rows: row_count,
        })
    })
}

/// 商机导出。
pub fn opportunities<'a>(conn: &'a mut PgConnection, ctx: &'a ExportContext) -> ExportFuture<'a> {
    Box::pin(async move {
        let sql = format!(
            r#"
            SELECT cu.code, c.name, COALESCE(c.amount_cents, 0), c.stage,
                   COALESCE(to_char(c.expected_close, 'YYYY-MM-DD'), ''),
                   to_char(c.created_at, 'YYYY-MM-DD HH24:MI:SS')
            FROM crm.opportunity c
            JOIN crm.customer cu ON cu.id = c.customer_id
            WHERE c.deleted_at IS NULL AND c.tenant_id = $1
              {}
            ORDER BY c.created_at DESC
            LIMIT 20000
            "#,
            scope_filter("$2", "$3", "$4")
        );
        let rows: Vec<(String, String, i32, String, String, String)> = sqlx::query_as(&sql)
            .bind(ctx.tenant_id)
            .bind(&ctx.data_scope)
            .bind(ctx.user_id)
            .bind(ctx.dept_id)
            .fetch_all(&mut *conn)
            .await?;

        let row_count = rows.len();
        let bytes = kernel::xlsx::run_anyhow(move || {
            let mut wb = Workbook::new();
            let sheet = wb.add_worksheet();
            for (i, h) in [
                "客户编码",
                "商机名称",
                "金额(元)",
                "阶段",
                "预计成交",
                "创建时间",
            ]
            .iter()
            .enumerate()
            {
                sheet.write_string(0, i as u16, *h)?;
            }
            for (r, row) in rows.iter().enumerate() {
                let i = (r + 1) as u32;
                sheet.write_string(i, 0, &row.0)?;
                sheet.write_string(i, 1, &row.1)?;
                sheet.write_string(i, 2, format!("{:.2}", row.2 as f64 / 100.0))?;
                sheet.write_string(i, 3, &row.3)?;
                sheet.write_string(i, 4, &row.4)?;
                sheet.write_string(i, 5, &row.5)?;
            }
            Ok(wb.save_to_buffer()?)
        })
        .await?;
        let name = format!(
            "opportunities-{}.xlsx",
            chrono::Utc::now().format("%Y%m%d%H%M%S")
        );
        Ok(ExportFile {
            bytes,
            file_name: name,
            rows: row_count,
        })
    })
}

inventory::submit! {
    kernel::exports::ExportRegistration {
        kind: "crm.contacts",
        permission: "crm:contact:list",
        run: contacts,
    }
}

inventory::submit! {
    kernel::exports::ExportRegistration {
        kind: "crm.opportunities",
        permission: "crm:opportunity:list",
        run: opportunities,
    }
}
