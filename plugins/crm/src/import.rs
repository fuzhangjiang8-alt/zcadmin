//! CRM 导入公共工具：单元格文本解析。
//!
//! 具体实体的导入在 `import_contracts` / `import_payments` / `import_contacts` / `import_opportunities`。

use calamine::Data;

pub(crate) fn cell_text(cell: &Data) -> String {
    match cell {
        Data::String(s) => s.trim().to_string(),
        Data::Float(f) => {
            if f.fract() == 0.0 {
                format!("{}", *f as i64)
            } else {
                f.to_string()
            }
        }
        Data::Int(i) => i.to_string(),
        _ => String::new(),
    }
}
