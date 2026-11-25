p = r"D:\fcj\zcadmin\crates\kernel\src\auth.rs"
s = open(p, encoding="utf-8").read()

start = s.index("/// 会话数据。")
end = s.index("/// 当前登录用户（从会话或 API Token 解析）。")
block = s[start:end]
s = s[:start] + s[end:]

s = s.replace(
    "use crate::state::AppState;",
    "pub use crate::session::{Session, SessionStore};\n\nuse crate::state::AppState;",
    1,
)
open(p, "w", encoding="utf-8", newline="").write(s)

header = """//! 数据库会话存储（`sys.sys_session`）与进程内短 TTL 缓存。
//!
//! 边界：登录/登出/当前用户在 `auth`；本模块只负责会话读写与缓存。
//! 关键约束：令牌不落库，只存 SHA-256 摘要；任何写操作清空缓存。

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use tokio::sync::RwLock;
use uuid::Uuid;

const SESSION_CACHE_TTL: std::time::Duration = std::time::Duration::from_secs(30);

"""

open(
    r"D:\fcj\zcadmin\crates\kernel\src\session.rs",
    "w",
    encoding="utf-8",
    newline="",
).write(header + block)

p = r"D:\fcj\zcadmin\crates\kernel\src\lib.rs"
s = open(p, encoding="utf-8").read()
if "pub mod session;" not in s:
    s = s.replace("pub mod security;", "pub mod security;\npub mod session;", 1)
open(p, "w", encoding="utf-8", newline="").write(s)
print("session extracted")
