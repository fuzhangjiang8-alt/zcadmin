//! 用户导入的同步工作簿构造；只能由 user_import 通过 xlsx::run 调度。
//! 不访问数据库、不记录原始口令；模板列名必须与 user_import_parse 的读取约定一致。

use crate::user_import_parse::UserImportError;
use rust_xlsxwriter::Workbook;

/// 构造一行示例的导入模板；写入或压缩失败返回错误，不产生持久化副作用。
pub(crate) fn build_template() -> anyhow::Result<Vec<u8>> {
    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();
    let headers = ["用户名", "显示名", "初始密码", "手机号", "邮箱", "部门"];
    let example = [
        "zhangsan",
        "张三",
        "Smoke#12345",
        "13800000000",
        "zhangsan@example.com",
        "研发部",
    ];
    for (column, header) in headers.iter().enumerate() {
        sheet.write_string(0, column as u16, *header)?;
        sheet.write_string(1, column as u16, example[column])?;
    }
    Ok(workbook.save_to_buffer()?)
}

/// 构造不含口令的错误报告；调用方负责按导入行数上限拒绝超大请求。
pub(crate) fn build_errors(errors: Vec<UserImportError>) -> anyhow::Result<Vec<u8>> {
    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();
    for (column, header) in [
        "行号",
        "用户名",
        "显示名",
        "邮箱",
        "手机号",
        "部门",
        "错误原因",
    ]
    .iter()
    .enumerate()
    {
        sheet.write_string(0, column as u16, *header)?;
    }
    for (index, error) in errors.into_iter().enumerate() {
        let values = [
            error.row.to_string(),
            error.username.unwrap_or_default(),
            error.display_name.unwrap_or_default(),
            error.email.unwrap_or_default(),
            error.phone.unwrap_or_default(),
            error.dept.unwrap_or_default(),
            error.message,
        ];
        for (column, value) in values.into_iter().enumerate() {
            sheet.write_string((index + 1) as u32, column as u16, value)?;
        }
    }
    Ok(workbook.save_to_buffer()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use calamine::{open_workbook_from_rs, Reader, Xlsx};
    use std::io::Cursor;

    #[test]
    fn generated_template_roundtrips_through_import_parser() {
        let bytes = build_template().expect("template");
        let rows = crate::user_import_parse::parse_rows(&bytes).expect("parse template");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].username, "zhangsan");
        assert_eq!(rows[0].phone.as_deref(), Some("13800000000"));
    }

    #[test]
    fn error_report_preserves_columns_and_excludes_password() {
        let error = UserImportError {
            row: 2,
            message: "invalid row".into(),
            username: Some("test".into()),
            display_name: None,
            email: None,
            phone: None,
            dept: None,
        };
        let bytes = build_errors(vec![error]).expect("error workbook");
        let mut workbook: Xlsx<_> = open_workbook_from_rs(Cursor::new(bytes)).expect("xlsx");
        let sheet = workbook
            .worksheet_range_at(0)
            .expect("sheet")
            .expect("range");
        assert_eq!(sheet.width(), 7);
        assert_eq!(sheet.height(), 2);
        assert_eq!(sheet.get((0, 6)).expect("header").to_string(), "错误原因");
        assert_eq!(
            sheet.get((1, 6)).expect("reason").to_string(),
            "invalid row"
        );
        assert!(sheet
            .rows()
            .next()
            .expect("headers")
            .iter()
            .all(|cell| cell != "初始密码"));
    }
}
