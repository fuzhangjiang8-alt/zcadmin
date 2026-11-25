//! 租户账单对账 Excel 导出（账单 / 流水 / 汇总三个 sheet）。
//!
//! 边界：只处理 `/tenants/bills/export.xlsx`；其它账单接口在 `tenant_admin`。
//! 关键约束：按数据权限过滤租户；单文件最多 20000 行。

use axum::extract::{Query, State};
use axum::routing::get;
use axum::Router;

use crate::auth::CurrentUser;
use crate::error::AppError;
use crate::state::AppState;
use crate::system::{require_perm, require_platform};
use crate::tenant::TenantTx;
use crate::tenant_bill_export_types::{BillExportRow, PaymentExportRow};
use crate::tenant_bills::{ReconciliationRow, TenantBillQuery};

/// 账单导出路由（挂在 `/api/system` 下）。
pub(crate) fn router() -> Router<AppState> {
    Router::new().route("/tenants/bills/export.xlsx", get(export_tenant_bills_xlsx))
}

/// 导出对账 Excel（账单/流水/汇总三个 sheet）。
#[utoipa::path(
    get,
    path = "/api/system/tenants/bills/export.xlsx",
    tag = "system",
    params(("month" = Option<String>, Query, description = "YYYY-MM，不传返回全部")),
    responses((status = 200, description = "Excel 文件"))
)]
async fn export_tenant_bills_xlsx(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Query(q): Query<TenantBillQuery>,
) -> Result<axum::response::Response, AppError> {
    use axum::response::IntoResponse;
    require_platform(&user)?;
    require_perm(&user, crate::perms::TENANT_LIST)?;
    let month = q
        .month
        .filter(|m| m.len() == 7)
        .and_then(|m| chrono::NaiveDate::parse_from_str(&format!("{m}-01"), "%Y-%m-%d").ok());
    let bills: Vec<BillExportRow> = sqlx::query_as(
        "SELECT t.code, t.name, b.bill_month, b.currency, b.avg_users::float8,
                    b.avg_storage_bytes, b.user_amount_cents, b.storage_amount_cents,
                    b.total_cents, b.status
             FROM sys.sys_tenant_bill b JOIN sys.sys_tenant t ON t.id = b.tenant_id
             WHERE ($1::date IS NULL OR b.bill_month = $1)
             ORDER BY b.bill_month DESC, t.code",
    )
    .bind(month)
    .fetch_all(&mut *tx)
    .await?;
    let payments: Vec<PaymentExportRow> = sqlx::query_as(
        "SELECT t.code, p.created_at, p.kind, p.amount_cents, p.method, p.reference
             FROM sys.sys_tenant_bill_payment p
             JOIN sys.sys_tenant_bill b ON b.id = p.bill_id
             JOIN sys.sys_tenant t ON t.id = b.tenant_id
             WHERE ($1::date IS NULL OR b.bill_month = $1)
             ORDER BY p.created_at DESC",
    )
    .bind(month)
    .fetch_all(&mut *tx)
    .await?;
    let recon: Vec<ReconciliationRow> = sqlx::query_as(
        r#"
        WITH bills AS (
            SELECT * FROM sys.sys_tenant_bill
            WHERE ($1::date IS NULL OR bill_month = $1)
        ),
        pay AS (
            SELECT bill_id,
                   COALESCE(sum(CASE WHEN kind = 'payment' THEN amount_cents ELSE 0 END), 0)::bigint AS paid,
                   COALESCE(sum(CASE WHEN kind = 'refund' THEN amount_cents ELSE 0 END), 0)::bigint AS refunded
            FROM sys.sys_tenant_bill_payment GROUP BY bill_id
        )
        SELECT t.id AS tenant_id, t.code AS tenant_code, t.name AS tenant_name,
               count(b.id)::bigint AS bill_count,
               COALESCE(sum(b.total_cents), 0)::bigint AS total_cents,
               COALESCE(sum(pay.paid), 0)::bigint AS paid_cents,
               COALESCE(sum(pay.refunded), 0)::bigint AS refunded_cents,
               COALESCE(sum(pay.paid - pay.refunded), 0)::bigint AS net_cents,
               COALESCE(sum(b.total_cents - (COALESCE(pay.paid, 0) - COALESCE(pay.refunded, 0))), 0)::bigint AS outstanding_cents
        FROM sys.sys_tenant t
        LEFT JOIN bills b ON b.tenant_id = t.id
        LEFT JOIN pay ON pay.bill_id = b.id
        WHERE t.deleted_at IS NULL
        GROUP BY t.id, t.code, t.name ORDER BY t.code
        "#,
    )
    .bind(month)
    .fetch_all(&mut *tx)
    .await?;

    tx.commit().await?;

    // 先提交只读事务归还连接，再将三 sheet Excel 组装交由受控阻塞池执行（AGENTS 铁律 16）
    let bytes = crate::xlsx::run(move || build_bills_workbook(&bills, &payments, &recon)).await?;
    Ok((
        [
            (
                axum::http::header::CONTENT_TYPE,
                "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet".to_string(),
            ),
            (
                axum::http::header::CONTENT_DISPOSITION,
                "attachment; filename=\"tenant-bills.xlsx\"".to_string(),
            ),
        ],
        bytes,
    )
        .into_response())
}

/// 在受控阻塞线程中组装对账 Excel（账单 / 收款流水 / 对账汇总）。
fn build_bills_workbook(
    bills: &[BillExportRow],
    payments: &[PaymentExportRow],
    recon: &[ReconciliationRow],
) -> anyhow::Result<Vec<u8>> {
    use rust_xlsxwriter::{Color, Format, FormatBorder, Workbook};
    let header = Format::new()
        .set_bold()
        .set_background_color(Color::RGB(0xE8EEFF))
        .set_border(FormatBorder::Thin);
    let money = Format::new().set_num_format("#,##0.00");
    let mut workbook = Workbook::new();
    let bill_sheet = workbook.add_worksheet();
    bill_sheet.set_name("账单")?;
    bill_sheet.set_freeze_panes(1, 0)?;
    for (col, width) in [
        (0, 16.0),
        (1, 24.0),
        (2, 12.0),
        (3, 8.0),
        (4, 10.0),
        (5, 14.0),
        (6, 14.0),
        (7, 14.0),
        (8, 14.0),
        (9, 10.0),
    ] {
        bill_sheet.set_column_width(col, width)?;
    }
    for (col, title) in [
        "租户编码",
        "租户名称",
        "月份",
        "币种",
        "均用户",
        "均存储字节",
        "用户费用(分)",
        "存储费用(分)",
        "合计(分)",
        "状态",
    ]
    .iter()
    .enumerate()
    {
        bill_sheet.write_string_with_format(0, col as u16, *title, &header)?;
    }
    for (row, b) in bills.iter().enumerate() {
        let r = (row + 1) as u32;
        bill_sheet.write_string(r, 0, &b.0)?;
        bill_sheet.write_string(r, 1, &b.1)?;
        bill_sheet.write_string(r, 2, b.2.to_string())?;
        bill_sheet.write_string(r, 3, &b.3)?;
        bill_sheet.write_number(r, 4, b.4)?;
        bill_sheet.write_number(r, 5, b.5 as f64)?;
        bill_sheet.write_number(r, 6, b.6 as f64)?;
        bill_sheet.write_number(r, 7, b.7 as f64)?;
        bill_sheet.write_number(r, 8, b.8 as f64)?;
        bill_sheet.write_string(r, 9, &b.9)?;
    }
    let bill_total_row = (bills.len() + 1) as u32;
    bill_sheet.write_string_with_format(bill_total_row, 0, "合计", &header)?;
    for (col, letter) in [(6, "G"), (7, "H"), (8, "I")] {
        let formula = format!("SUM({letter}2:{letter}{})", bills.len() + 1);
        bill_sheet.write_formula_with_format(bill_total_row, col, formula.as_str(), &money)?;
    }
    let pay_sheet = workbook.add_worksheet();
    pay_sheet.set_name("收款流水")?;
    pay_sheet.set_freeze_panes(1, 0)?;
    for (col, width) in [
        (0, 16.0),
        (1, 20.0),
        (2, 10.0),
        (3, 14.0),
        (4, 10.0),
        (5, 20.0),
    ] {
        pay_sheet.set_column_width(col, width)?;
    }
    for (col, title) in ["租户", "时间", "类型", "金额(分)", "方式", "凭证号"]
        .iter()
        .enumerate()
    {
        pay_sheet.write_string_with_format(0, col as u16, *title, &header)?;
    }
    for (row, p) in payments.iter().enumerate() {
        let r = (row + 1) as u32;
        pay_sheet.write_string(r, 0, &p.0)?;
        pay_sheet.write_string(r, 1, p.1.format("%Y-%m-%d %H:%M:%S").to_string())?;
        pay_sheet.write_string(r, 2, &p.2)?;
        pay_sheet.write_number_with_format(r, 3, p.3 as f64, &money)?;
        pay_sheet.write_string(r, 4, &p.4)?;
        pay_sheet.write_string(r, 5, p.5.as_deref().unwrap_or(""))?;
    }
    let pay_total_row = (payments.len() + 1) as u32;
    pay_sheet.write_string_with_format(pay_total_row, 0, "合计", &header)?;
    let pay_formula = format!("SUM(D2:D{})", payments.len() + 1);
    pay_sheet.write_formula_with_format(pay_total_row, 3, pay_formula.as_str(), &money)?;
    let recon_sheet = workbook.add_worksheet();
    recon_sheet.set_name("对账汇总")?;
    recon_sheet.set_freeze_panes(1, 0)?;
    for (col, width) in [
        (0, 16.0),
        (1, 24.0),
        (2, 10.0),
        (3, 14.0),
        (4, 14.0),
        (5, 14.0),
        (6, 14.0),
        (7, 14.0),
    ] {
        recon_sheet.set_column_width(col, width)?;
    }
    for (col, title) in [
        "租户编码",
        "租户名称",
        "账单数",
        "应收",
        "收款",
        "退款",
        "净收款",
        "未收",
    ]
    .iter()
    .enumerate()
    {
        recon_sheet.write_string_with_format(0, col as u16, *title, &header)?;
    }
    for (row, r) in recon.iter().enumerate() {
        let row = (row + 1) as u32;
        recon_sheet.write_string(row, 0, &r.tenant_code)?;
        recon_sheet.write_string(row, 1, &r.tenant_name)?;
        recon_sheet.write_number(row, 2, r.bill_count as f64)?;
        recon_sheet.write_number(row, 3, r.total_cents as f64)?;
        recon_sheet.write_number(row, 4, r.paid_cents as f64)?;
        recon_sheet.write_number(row, 5, r.refunded_cents as f64)?;
        recon_sheet.write_number(row, 6, r.net_cents as f64)?;
        recon_sheet.write_number(row, 7, r.outstanding_cents as f64)?;
    }
    let recon_total_row = (recon.len() + 1) as u32;
    recon_sheet.write_string_with_format(recon_total_row, 0, "合计", &header)?;
    for (col, letter) in [(3, "D"), (4, "E"), (5, "F"), (6, "G"), (7, "H")] {
        let formula = format!("SUM({letter}2:{letter}{})", recon.len() + 1);
        recon_sheet.write_formula_with_format(recon_total_row, col, formula.as_str(), &money)?;
    }
    Ok(workbook.save_to_buffer()?)
}
