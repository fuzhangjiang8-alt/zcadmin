//! zcadmin 桌面端入口。
//!
//! # 设计
//!
//! 桌面端**不重写 UI**：直接加载 `frontend/dist`（或开发时的 Vite 5173），
//! 与浏览器使用同一套 Vue 代码。这样 CRM/IM/AI 页面只需要维护一份。
//!
//! # 与 Web 的差异
//!
//! | 能力 | Web | 桌面 |
//! |---|---|---|
//! | 文件保存 | 浏览器下载 | 原生对话框（后续接 tauri-plugin-dialog） |
//! | 开机自启 | ✗ | ✓（后续接 tauri-plugin-autostart） |
//! | 深链 | URL | 自定义协议（后续） |
//!
//! 这些能力通过 Tauri 插件逐步加，前端用 `isTauri()` 做能力探测。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("启动 Tauri 应用失败");
}
