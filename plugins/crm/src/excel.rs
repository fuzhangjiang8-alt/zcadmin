//! 客户导入 / 导出（Excel）。
//!
//! # 为什么放在插件里而不是内核
//!
//! 导入导出的列结构强绑定业务实体（客户的编码/名称/等级…），
//! 属于 CRM 域，不应该污染内核。内核只提供通用的文件存储。
//!
//! # 限制（M3 简化版）
//!
//! - 同步处理，单次最多 5000 行（超过请走异步任务，见 docs/01 第 15 节）；
//! - 导入只做新增，不做更新（避免误覆盖）；
//! - 全部行处理完再返回汇总，失败行不影响成功行（部分成功语义明确）。

use crate::api::scope_filter;
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use chrono::Utc;
use kernel::prelude::*;
use rust_xlsxwriter::Workbook;

pub use crate::excel_import::{__path_import, import, ImportError, ImportResult};

/// Excel 表头（顺序即列顺序，导入时按表头名匹配）。
///
/// 列顺序必须与 [`export`] 的写入顺序严格一致：
/// 编码 / 名称 / 等级 / 状态 / 电话 / 邮箱。
/// 历史上这里少了「状态」而写入多了状态列，导致电话被写进邮箱列——
/// 修改本常量时必须同步 `export` 的 `write_string(r, i, ..)` 与单测样例。
const HEADERS: [&str; 6] = [
    "客户编码",
    "客户名称",
    "客户等级",
    "状态",
    "联系电话",
    "邮箱",
];

/// 单次导入上限。
pub(crate) const MAX_IMPORT_ROWS: usize = 5000;

/// 导出行：(编码, 名称, 等级, 状态, 电话, 邮箱)。
type ExportRow = (
    String,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
);

/// 导出客户（xlsx，最多 5000 行）。
#[utoipa::path(
    get,
    path = "/api/crm/customers/export",
    tag = "crm",
    responses((status = 200, description = "xlsx 文件流"))
)]
pub async fn export(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
) -> Result<Response, AppError> {
    crate::perm::require(&user, crate::perm::CUSTOMER_EXPORT)?;

    let sql = format!(
        r#"
        SELECT c.code, c.name, c.level, c.status, c.phone, c.email
        FROM crm.customer c
        WHERE c.deleted_at IS NULL AND c.tenant_id = $1
          {}
        ORDER BY c.created_at DESC
        LIMIT 5000
        "#,
        scope_filter("$2", "$3", "$4")
    );

    let rows: Vec<ExportRow> = sqlx::query_as(&sql)
        .bind(user.tenant_id)
        .bind(&user.data_scope)
        .bind(user.user_id)
        .bind(user.dept_id)
        .fetch_all(&mut *tx)
        .await?;

    tx.commit().await?;

    // 生成 xlsx 属同步 CPU/内存密集操作，先释放数据库事务再走受控阻塞池（AGENTS 铁律 16）
    let bytes = kernel::xlsx::run(move || {
        let mut workbook = Workbook::new();
        let sheet = workbook.add_worksheet();
        sheet.set_name("客户")?;

        for (col, title) in HEADERS.iter().enumerate() {
            sheet.write_string(0, col as u16, *title)?;
        }

        for (i, (code, name, level, status, phone, email)) in rows.iter().enumerate() {
            let r = (i + 1) as u32;
            // 与 HEADERS 同一顺序构造，避免「表头 5 列、数据 6 列」再次错位。
            let cells: [Option<&str>; HEADERS.len()] = [
                Some(code.as_str()),
                Some(name.as_str()),
                Some(level.as_str()),
                Some(status.as_str()),
                phone.as_deref(),
                email.as_deref(),
            ];
            for (col, value) in cells.iter().enumerate() {
                if let Some(v) = value {
                    sheet.write_string(r, col as u16, *v)?;
                }
            }
        }

        Ok(workbook.save_to_buffer()?)
    })
    .await?;
    let filename = format!("customers-{}.xlsx", Utc::now().format("%Y%m%d%H%M%S"));

    Ok(file_response(bytes, &filename))
}

/// 构造 xlsx 下载响应。
fn file_response(bytes: Vec<u8>, filename: &str) -> Response {
    let mut resp = (StatusCode::OK, bytes).into_response();
    resp.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        ),
    );
    let encoded: String = filename
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_') {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect();
    if let Ok(v) = HeaderValue::from_str(&format!("attachment; filename*=UTF-8''{encoded}")) {
        resp.headers_mut().insert(header::CONTENT_DISPOSITION, v);
    }
    resp
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::excel_import::parse_rows;

    /// 用 rust_xlsxwriter 造一个样例文件，验证解析链路（读写库互为校验）。
    fn sample_workbook(with_header: bool) -> Vec<u8> {
        let mut wb = Workbook::new();
        let sheet = wb.add_worksheet();
        if with_header {
            for (i, h) in HEADERS.iter().enumerate() {
                sheet.write_string(0, i as u16, *h).unwrap();
            }
        }
        sheet.write_string(1, 0, "C001").unwrap();
        sheet.write_string(1, 1, "测试客户").unwrap();
        sheet.write_string(1, 2, "vip").unwrap();
        sheet.write_string(1, 3, "potential").unwrap();
        sheet.write_string(1, 4, "13800000000").unwrap();
        sheet.write_string(1, 5, "a@b.com").unwrap();
        wb.save_to_buffer().unwrap()
    }

    #[test]
    fn parses_sample_rows() {
        let rows = parse_rows(&sample_workbook(true)).expect("parse");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].code, "C001");
        assert_eq!(rows[0].name, "测试客户");
        assert_eq!(rows[0].level, "vip");
        assert_eq!(rows[0].phone.as_deref(), Some("13800000000"));
        assert_eq!(rows[0].email.as_deref(), Some("a@b.com"));
    }

    /// 表头必须覆盖导出写入的每一列；漏列会让「电话」落进「邮箱」列（历史缺陷）。
    #[test]
    fn headers_match_export_columns() {
        assert_eq!(HEADERS.len(), 6, "导出写入 6 列，表头必须同为 6 列");
        assert!(HEADERS.contains(&"状态"), "缺少状态列会导致后续列整体左移");
        let unique: std::collections::HashSet<_> = HEADERS.iter().collect();
        assert_eq!(unique.len(), HEADERS.len(), "表头不允许重复");
    }

    #[test]
    fn rejects_missing_required_header() {
        let err = parse_rows(&sample_workbook(false));
        assert!(err.is_err());
    }
}
