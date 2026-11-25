//! 账单导出的行类型（供 `tenant_bill_export` 使用）。
//!
//! 边界：只放类型定义，不含 SQL 与路由。

/// 账单 sheet 行。
pub(crate) type BillExportRow = (
    String,
    String,
    chrono::NaiveDate,
    String,
    f64,
    i64,
    i64,
    i64,
    i64,
    String,
);

/// 流水 sheet 行。
pub(crate) type PaymentExportRow = (
    String,
    chrono::DateTime<chrono::Utc>,
    String,
    i64,
    String,
    Option<String>,
);
