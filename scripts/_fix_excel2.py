p = r"D:\fcj\zcadmin\plugins\crm\src\excel.rs"
s = open(p, encoding="utf-8").read()
s = s.replace("pub use crate::excel_import::{import, ImportError, ImportResult};", "pub use crate::excel_import::{__path_import, import, ImportError, ImportResult};", 1)
s = s.replace("use axum::extract::Multipart;\n", "", 1)
s = s.replace("use serde::Serialize;\n", "", 1)
s = s.replace("use std::io::Cursor;\n", "", 1)
open(p, "w", encoding="utf-8", newline="").write(s)

p = r"D:\fcj\zcadmin\plugins\crm\src\excel_import.rs"
s = open(p, encoding="utf-8").read()
s = s.replace("use calamine::{open_workbook_from_rs, Data, Reader, Xlsx};", "use calamine::Xlsx;", 1)
s = s.replace("use serde::{Deserialize, Serialize};", "use serde::Serialize;", 1)
open(p, "w", encoding="utf-8", newline="").write(s)
print("fixed")
