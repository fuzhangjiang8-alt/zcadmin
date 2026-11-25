//! # 内核（kernel）
//!
//! 本 crate 是整个系统的「地基」，只做四件事：
//!
//! 1. **插件契约**：定义 [`plugin::Plugin`] trait 与注册机制，业务代码通过实现它接入系统。
//! 2. **装配器**：启动时收集所有插件，按依赖顺序执行迁移、挂载路由（见 [`app::build`]）。
//! 3. **横切能力**：配置、错误、统一响应、认证会话、健康检查、数据库连接池。
//! 4. **公共类型**：所有插件共享的 [`AppState`]、[`AppError`]、[`ApiResult`]。
//!
//! ## 边界（很重要）
//!
//! - 内核**不得**依赖任何插件（`cargo xtask arch-check` 会检查）。
//! - 内核**不包含业务逻辑**，业务一律放在 `plugins/*`。
//! - 内核的公开 API 一旦发布就要保持兼容，破坏性变更必须升主版本。
//!
//! ## 目录导航
//!
//! | 模块 | 职责 |
//! |---|---|
//! | [`config`] | 配置加载（TOML + 环境变量覆盖） |
//! | [`db`] | 连接池、schema 创建、迁移执行 |
//! | [`error`] | 统一错误类型与 HTTP 映射（RFC 9457） |
//! | [`response`] | 统一成功响应与分页结构 |
//! | [`state`] | 应用状态（数据库池 + 配置 + 会话） |
//! | [`plugin`] | 插件契约、注册表、拓扑排序 |
//! | [`auth`] | 会话、登录与当前用户 |
//! | [`auth_identity`] | 租户解析与权限/部门加载 |
//! | [`auth_login`] | 登录接口 |
//! | [`auth_policy`] | 登录策略加载 |
//! | [`auth_session_api`] | 登出/当前用户/改密 |
//! | [`auth_tokens`] | 个人 API Token |
//! | [`session`] | 数据库会话存储与缓存 |
//! | [`system`] | 系统路由聚合、插件权限同步 |
//! | [`system_audit_api`] | 审计日志查询接口 |
//! | [`system_sessions`] | 在线会话管理 |
//! | [`system_scope`] | 数据范围预览 |
//! | [`system_identity`] | 租户归属校验与身份域路由聚合 |
//! | [`system_users`] | 用户列表/状态 |
//! | [`system_users_create`] | 新建用户 |
//! | [`system_user_roles`] | 用户-角色绑定 |
//! | [`system_roles`] | 角色与权限点查询/创建 |
//! | [`system_role_permissions`] | 角色授权 |
//! | [`system_org`] | 菜单/部门/字典路由聚合 |
//! | [`system_menus`] | 当前用户菜单树 |
//! | [`system_menus_admin`] | 菜单管理 CRUD |
//! | [`system_menu_sync`] | 插件菜单同步 |
//! | [`system_depts`] | 部门（组织树） |
//! | [`system_dicts`] | 数据字典查询/新增 |
//! | [`system_dicts_edit`] | 字典编辑 |
//! | [`system_security`] | 密码/登录策略、登录风控、账号解锁 |
//! | [`system_seed`] | 初始管理员与默认菜单 |
//! | [`tenant_admin`] | 租户列表与默认告警规则 |
//! | [`tenant_create`] | 新建租户（含管理员初始化） |
//! | [`tenant_lifecycle`] | 租户状态/配额/删除/恢复 |
//! | [`tenant_billing`] | 计费配置/账单预估/导出 |
//! | [`tenant_bills`] | 账单生成/列表/状态/对账 |
//! | [`tenant_bill_backfill`] | 账单回填 |
//! | [`tenant_bill_export`] | 对账 Excel 导出 |
//! | [`tenant_bill_export_types`] | 导出类型与错误转换 |
//! | [`tenant_bill_payments`] | 账单收款与退款 |
//! | [`tenant_bill_collections`] | 催收记录与提醒扫描 |
//! | [`tenant_overdue`] | 逾期账单扫描与告警 |
//! | [`tenant_usage_api`] | 租户用量采集/趋势接口 |
//! | [`tenant_summary`] | 租户详情汇总 |
//! | [`tenant_purge_api`] | 租户物理清理接口 |
//! | [`audit_admin`] | 审计告警规则与配置包 |
//! | [`audit_alert_settings`] | 告警配置与 Webhook |
//! | [`audit_rules`] | 告警规则列表/替换/预览 |
//! | [`audit_alerts`] | 审计告警列表与确认 |
//! | [`audit_stats`] | 审计统计聚合 |
//! | [`audit_config_bundle`] | 审计配置包模型/迁移/差异 |
//! | [`audit_config_export`] | 审计配置导出 |
//! | [`audit_config_import`] | 审计配置导入 |
//! | [`audit_config_preview`] | 审计配置导入预览 |
//! | [`audit_config_snapshots`] | 审计配置快照列表/删除/清理 |
//! | [`audit_config_snapshot_tools`] | 快照导出/对比/改名 |
//! | [`audit_subscriptions`] | 告警订阅与订阅配置 |
//! | [`audit_deliveries`] | 审计投递记录与重试 |
//! | [`health`] | 存活/就绪探针 |
//! | [`security`] | 安全响应头（CSP / HSTS / 点击劫持防护） |
//! | [`ratelimit`] | 登录接口 IP 维度限流 |
//! | [`client_ip`] | 可信代理与客户端 IP 解析 |
//! | [`blocking`] | 受控阻塞池（Argon2 / xlsx 等重 CPU 任务） |
//! | [`app`] | 装配器：把上面这些组装成一个可运行的 Router |

// 安全基线：整个内核禁止 unsafe。任何需要 unsafe 的场景都应该先重新设计。
#![forbid(unsafe_code)]
// 文档缺失视为错误：公开 API 必须有注释（配合 `cargo doc` 可直接产出接口手册）。
#![deny(missing_docs)]

pub mod app;
pub mod audit;
pub mod audit_admin;
pub mod audit_alert_settings;
pub mod audit_alerts;
pub mod audit_config_bundle;
pub mod audit_config_export;
pub mod audit_config_import;
pub mod audit_config_preview;
pub mod audit_config_snapshot_tools;
pub mod audit_config_snapshots;
pub mod audit_deliveries;
pub mod audit_maintenance;
pub mod audit_notify;
pub mod audit_rules;
pub mod audit_stats;
pub mod audit_subscription_dispatch;
pub mod audit_subscriptions;
pub mod auth;
pub mod auth_identity;
pub mod auth_login;
pub mod auth_policy;
pub mod auth_session_api;
pub mod auth_tokens;
pub mod blocking;
pub mod client_ip;
pub mod config;
pub mod configs;
pub mod csrf;
pub mod db;
pub mod error;
pub mod exports;
pub mod exports_api;
mod exports_download;
pub mod exports_runner;
pub mod extract;
pub mod files;
pub mod files_read;
pub mod health;
pub mod imports;
pub mod jobs;
pub mod mask;
pub mod metrics;
mod metrics_export;
pub mod notices;
pub mod openapi;
pub mod password;
pub mod perms;
pub mod plugin;
pub mod posts;
pub mod posts_types;
pub mod posts_user;
pub mod ratelimit;
pub mod response;
pub mod scope;
pub mod security;
pub mod session;
pub mod ssrf;
pub mod state;
pub mod storage;
pub mod system;
pub mod system_audit_api;
pub mod system_depts;
pub mod system_dicts;
pub mod system_dicts_edit;
pub mod system_identity;
pub mod system_login_security;
pub mod system_menu_sync;
pub mod system_menus;
pub mod system_menus_admin;
pub mod system_ops;
pub mod system_org;
mod system_password_policy;
pub mod system_role_permissions;
pub mod system_roles;
pub mod system_scope;
pub mod system_security;
pub mod system_seed;
pub mod system_sessions;
pub mod system_user_roles;
pub mod system_users;
pub mod system_users_create;
pub mod telemetry;
pub mod tenant;
pub mod tenant_admin;
pub mod tenant_bill_backfill;
pub mod tenant_bill_collections;
pub mod tenant_bill_export;
pub mod tenant_bill_export_types;
pub mod tenant_bill_payments;
pub mod tenant_billing;
pub mod tenant_bills;
pub mod tenant_create;
pub mod tenant_lifecycle;
pub mod tenant_overdue;
pub mod tenant_purge;
pub mod tenant_purge_api;
pub mod tenant_summary;
pub mod tenant_usage;
pub mod tenant_usage_api;
pub mod user_import;
pub mod user_import_parse;
mod user_import_xlsx;
pub mod xlsx;

pub use error::{AppError, AppResult};
pub use response::{ApiResponse, ApiResult, Page, PageMeta};
pub use state::AppState;

/// 常用类型的统一入口。
///
/// 插件作者只需要：
/// ```ignore
/// use kernel::prelude::*;
/// ```
/// 即可拿到实现插件所需的全部类型，避免在每个文件里写一长串 `use`。
pub mod prelude {
    pub use crate::auth::CurrentUser;
    pub use crate::config::Config;
    pub use crate::error::{AppError, AppResult};
    pub use crate::extract::Query;
    pub use crate::plugin::{PermissionDef, Plugin, PluginMeta, PluginRegistration};
    pub use crate::response::{ApiResponse, ApiResult, Page, PageMeta};
    pub use crate::state::AppState;
    pub use crate::tenant::TenantTx;
    pub use axum::{
        extract::{Path, State},
        routing::{delete, get, patch, post, put},
        Json, Router,
    };
    pub use serde::{Deserialize, Serialize};
    pub use sqlx::PgPool;
    pub use uuid::Uuid;
}
