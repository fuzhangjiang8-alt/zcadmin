p = r"D:\fcj\zcadmin\crates\kernel\src\auth_login.rs"
s = open(p, encoding="utf-8").read()
s = s.replace("use axum::extract::State;", "use axum::extract::State;\nuse axum::http::header;", 1)
s = s.replace("use uuid::Uuid;", "use uuid::Uuid;", 1) if "use uuid::Uuid;" in s else s.replace("use crate::tenant::TenantTx;", "use crate::tenant::TenantTx;\nuse uuid::Uuid;", 1)
open(p, "w", encoding="utf-8", newline="").write(s)

p = r"D:\fcj\zcadmin\crates\kernel\src\auth.rs"
s = open(p, encoding="utf-8").read()
for line in (
    "use chrono::{DateTime, Duration, Utc};\n",
    "use axum::response::IntoResponse;\n",
    "use serde::{Deserialize, Serialize};",
):
    s = s.replace(line, "", 1)
s = s.replace("use serde::{Deserialize, Serialize};", "", 1)
s = s.replace("use axum::{Json, Router};", "use axum::Router;", 1)
open(p, "w", encoding="utf-8", newline="").write(s)
print("fixed")
