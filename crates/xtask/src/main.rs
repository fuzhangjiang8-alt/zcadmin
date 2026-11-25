//! 工程命令（cargo xtask 模式）。
//!
//! 用法：
//! ```bash
//! cargo xtask doctor              # 环境自检
//! cargo xtask verify              # 全量验收（提交前必须全绿）
//! cargo xtask arch-check          # 架构规则检查（依赖方向、跨插件引用）
//! cargo xtask check-perms         # 权限一致性（后端/前端/权限码常量）
//! cargo xtask new-plugin crm      # 生成插件骨架
//! ```
//!
//! 为什么用 xtask 而不是 shell 脚本：
//! - 跨平台（Windows/macOS/Linux 行为一致）；
//! - 可以被 CI 与人/ AI 以同一种方式调用；
//! - 逻辑是 Rust，出错时能给出可读的上下文。

mod gen;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(String::as_str).unwrap_or("help");

    let result = match cmd {
        "doctor" => doctor(),
        "verify" => verify(),
        "arch-check" => arch_check(),
        "check-perms" => check_perms(),
        "gen" => match args.get(1) {
            Some(path) => gen::generate(path, &workspace_root()),
            None => Err("用法: cargo xtask gen <schema.toml>".into()),
        },
        "openapi" => openapi(),
        "new-plugin" => match args.get(1) {
            Some(id) => new_plugin(id),
            None => {
                Err("用法: cargo xtask new-plugin <id>（id 只能是小写字母/数字/下划线）".into())
            }
        },
        "help" | "--help" | "-h" => {
            print_help();
            Ok(())
        }
        other => Err(format!(
            "未知命令: {other}（运行 `cargo xtask help` 查看用法）"
        )),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(msg) => {
            eprintln!("\n❌ {msg}");
            ExitCode::FAILURE
        }
    }
}

fn print_help() {
    println!(
        r#"卓才智能 工程命令

  cargo xtask doctor              检查工具链、配置与数据库连通性
  cargo xtask verify              验收：fmt + clippy + test + arch-check + RLS + 权限与错误码契约
  cargo xtask arch-check          架构规则：内核不得依赖插件，插件之间不得互相引用
  cargo xtask check-perms         权限一致性：后端声明、前端按钮、权限码常量
  cargo xtask new-plugin <id>     生成插件骨架（后端 crate + 迁移 + 注册代码）
  cargo xtask openapi             导出接口契约到 contracts/openapi.json
"#
    );
}

/// 工作区根目录 = 本 crate 的 `../../`。
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("无法定位工作区根目录")
        .to_path_buf()
}

// =============================================================================
// doctor
// =============================================================================

/// 环境自检：把"环境问题"挡在启动之前，而不是让服务跑到一半报错。
fn doctor() -> Result<(), String> {
    println!("== 环境自检 ==");

    // 1. 工具链版本
    for tool in ["rustc", "cargo"] {
        match Command::new(tool).arg("--version").output() {
            Ok(out) => println!(
                "  {tool:<8}: {}",
                String::from_utf8_lossy(&out.stdout).trim()
            ),
            Err(_) => return Err(format!("未找到 {tool}，请先安装 Rust 工具链")),
        }
    }

    // 2. 配置文件
    let cfg = workspace_root().join("config/default.toml");
    if cfg.exists() {
        println!("  配置文件: 存在 ({})", cfg.display());
    } else {
        println!("  配置文件: 缺失，将使用默认值 + 环境变量");
    }

    // 3. 数据库连接串
    match std::env::var("DATABASE_URL") {
        Ok(url) => {
            // 只打印主机部分，避免把密码打出来
            let masked = mask_url(&url);
            println!("  DATABASE_URL: {masked}");
        }
        Err(_) => println!("  DATABASE_URL: 未设置（将使用 config/default.toml 中的默认值）"),
    }

    println!("\n✅ 自检完成");
    Ok(())
}

/// 隐藏连接串里的密码，日志里永远不出现明文密码。
fn mask_url(url: &str) -> String {
    // postgres://user:password@host/db -> postgres://user:***@host/db
    if let Some(at) = url.find('@') {
        if let Some(colon) = url[..at].rfind(':') {
            return format!("{}:***{}", &url[..colon], &url[at..]);
        }
    }
    url.to_string()
}

// =============================================================================
// verify
// =============================================================================

/// 全量验收。CI 与本地使用同一条命令，避免"我本地是好的"。
fn verify() -> Result<(), String> {
    println!("== 1/6 代码格式 ==");
    run("cargo", &["fmt", "--all", "--check"])?;

    println!("\n== 2/6 Clippy（警告即错误） ==");
    run(
        "cargo",
        &[
            "clippy",
            "--workspace",
            "--all-targets",
            "--",
            "-D",
            "warnings",
        ],
    )?;

    println!("\n== 3/6 单元测试 ==");
    run("cargo", &["test", "--workspace"])?;

    println!("\n== 4/6 架构规则 ==");
    arch_check()?;

    // 租户隔离是安全底线：一旦某张表「建了策略却没 FORCE」，
    // 只有表 owner 能绕过，测试和类型系统都发现不了，必须静态守护。
    println!("\n== 5/6 租户隔离（RLS 策略与 FORCE 一致性） ==");
    run("node", &["scripts/check-rls-coverage.mjs"])?;

    // 本地验收与 CI 使用同一权限/错误码门禁，避免 verify 绿但契约检查遗漏。
    println!("\n== 6/6 权限与错误码契约 ==");
    check_perms()?;

    println!("\n✅ 全部通过");
    Ok(())
}

/// 执行外部命令，失败时带上命令行上下文。
fn run(program: &str, args: &[&str]) -> Result<(), String> {
    println!("  $ {program} {}", args.join(" "));
    let status = Command::new(program)
        .args(args)
        .current_dir(workspace_root())
        .status()
        .map_err(|e| format!("无法执行 {program}: {e}"))?;
    if !status.success() {
        return Err(format!(
            "命令失败（退出码 {:?}）: {program} {}",
            status.code(),
            args.join(" ")
        ));
    }
    Ok(())
}

// =============================================================================
// arch-check
// =============================================================================

/// 架构规则检查。
///
/// R1：内核不得依赖插件（防止地基反向依赖业务）。
/// R2：插件之间不得互相 `use`（跨插件协作只能走事件或公共 contract crate）。
fn arch_check() -> Result<(), String> {
    let root = workspace_root();
    let mut errors: Vec<String> = Vec::new();

    // ---- R1 ----
    let kernel_manifest = root.join("crates/kernel/Cargo.toml");
    let content = fs::read_to_string(&kernel_manifest)
        .map_err(|e| format!("读取 {} 失败: {e}", kernel_manifest.display()))?;
    for line in content.lines() {
        let l = line.trim();
        // 依赖行形如：plugin-crm = { path = ... } 或 plugin-crm = "1.0"
        if l.starts_with("plugin-") || l.starts_with("plugin_") {
            errors.push(format!("R1 违反：crates/kernel 依赖了插件 `{l}`"));
        }
    }

    // ---- R2 ----
    let plugins_dir = root.join("plugins");
    if plugins_dir.exists() {
        let entries = fs::read_dir(&plugins_dir).map_err(|e| e.to_string())?;
        for entry in entries.flatten() {
            if !entry.path().is_dir() {
                continue;
            }
            let me = entry.file_name().to_string_lossy().replace('_', "-");
            // 递归扫描该插件的源码
            for file in collect_rs_files(&entry.path()) {
                let src = fs::read_to_string(&file).unwrap_or_default();
                for other in fs::read_dir(&plugins_dir)
                    .map_err(|e| e.to_string())?
                    .flatten()
                {
                    let other_name = other.file_name().to_string_lossy().replace('_', "-");
                    if other_name == me {
                        continue;
                    }
                    let needle = format!("plugin_{}::", other_name.replace('-', "_"));
                    let needle_alt = format!("use {}::", other_name.replace('-', "_"));
                    if src.contains(&needle) || src.contains(&needle_alt) {
                        errors.push(format!(
                            "R2 违反：{} 直接引用了插件 `{}`（应改用领域事件）",
                            file.display(),
                            other_name
                        ));
                    }
                }
            }
        }
    }

    // ---- R3：业务源码单文件行数上限（AGENTS.md 规定 ≤ 300 行） ----
    let watched_dirs = [
        root.join("crates/kernel/src"),
        root.join("plugins"),
        root.join("apps/server/src"),
        root.join("frontend/src/views"),
        root.join("frontend/src/plugins"),
        root.join("frontend/src/features"),
    ];
    for dir in &watched_dirs {
        for file in collect_source_files(dir) {
            if let Ok(src) = fs::read_to_string(&file) {
                let count = src.lines().count();
                if count > 300 {
                    errors.push(format!(
                        "R3 违反：{} 共 {} 行，超过 300 行上限（请按职责拆分模块）",
                        file.display(),
                        count
                    ));
                }
            }
        }
    }

    if errors.is_empty() {
        println!("  架构规则检查通过（依赖方向 / 跨插件隔离 / 单文件 ≤ 300 行）");
        Ok(())
    } else {
        Err(errors.join("\n"))
    }
}

/// 递归收集目录下的 `.rs` 文件。
fn collect_rs_files(dir: &Path) -> Vec<PathBuf> {
    collect_files_by_ext(dir, &["rs"])
}

/// 递归收集业务源码文件（`.rs` / `.vue` / `.ts`）。
fn collect_source_files(dir: &Path) -> Vec<PathBuf> {
    collect_files_by_ext(dir, &["rs", "vue", "ts"])
}

/// 按扩展名递归收集文件。
fn collect_files_by_ext(dir: &Path, exts: &[&str]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                out.extend(collect_files_by_ext(&p, exts));
            } else if p
                .extension()
                .and_then(|s| s.to_str())
                .is_some_and(|ext| exts.contains(&ext))
            {
                out.push(p);
            }
        }
    }
    out
}

// =============================================================================
// check-perms
// =============================================================================

/// 权限一致性检查：后端声明、前端按钮权限、权限码常量。
///
/// 为什么放在 xtask 而不是只留 node 脚本：
/// AGENTS.md 承诺的入口是 `cargo xtask check-perms`，人和 CI 应使用同一入口，
/// 否则文档与实际命令会长期漂移（历史上正是如此）。
fn check_perms() -> Result<(), String> {
    println!("== 权限一致性检查 ==");
    let checks: [&[&str]; 5] = [
        &["scripts/check-perms.mjs"],
        &["scripts/check-frontend-perms.mjs"],
        &["scripts/check-perm-codes.mjs"],
        &["scripts/gen-perm-codes.mjs", "--check"],
        // 错误码契约与 AppError::code() 必须同步（scripts/check-error-codes.mjs）。
        // 放在这里而不是单独命令，是因为它和权限码一样属于「跨端唯一来源」。
        &["scripts/check-error-codes.mjs"],
    ];
    for args in checks {
        println!("  $ node {}", args.join(" "));
        let status = Command::new("node")
            .args(args)
            .current_dir(workspace_root())
            .status()
            .map_err(|e| format!("无法执行 node（权限检查需要 Node.js）: {e}"))?;
        if !status.success() {
            return Err(format!("权限检查失败：node {}", args.join(" ")));
        }
    }
    println!("\n✅ 权限检查通过");
    Ok(())
}

// =============================================================================
// new-plugin
// =============================================================================

/// 生成插件骨架。
///
/// 生成内容刻意保持最小，但包含所有"必须有的东西"：
/// Cargo.toml、lib.rs（trait 实现 + 注册）、迁移文件。
///
/// 为什么不生成业务代码：业务必须由人写 Spec、AI 填规则，
/// 自动生成的业务代码是负债（见 docs/07 第 9 节）。
fn new_plugin(id: &str) -> Result<(), String> {
    // 与内核 `validate_identifier` 保持一致的命名约束。
    let valid = !id.is_empty()
        && id.len() <= 63
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    if !valid {
        return Err("插件 id 只能包含小写字母、数字、下划线，且长度 ≤ 63".into());
    }

    let root = workspace_root();
    let dir = root.join("plugins").join(id);
    if dir.exists() {
        return Err(format!("目录已存在: {}", dir.display()));
    }

    fs::create_dir_all(dir.join("src")).map_err(|e| e.to_string())?;
    fs::create_dir_all(dir.join("migrations")).map_err(|e| e.to_string())?;

    let crate_name = format!("plugin-{}", id.replace('_', "-"));

    // ---- Cargo.toml ----
    let cargo_toml = format!(
        r#"[package]
name = "{crate_name}"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
description = "{id} 插件"

[dependencies]
kernel = {{ workspace = true }}
axum = {{ workspace = true }}
sqlx = {{ workspace = true }}
serde = {{ workspace = true }}
uuid = {{ workspace = true }}
chrono = {{ workspace = true }}
inventory = {{ workspace = true }}
tracing = {{ workspace = true }}
anyhow = {{ workspace = true }}
"#
    );
    fs::write(dir.join("Cargo.toml"), cargo_toml).map_err(|e| e.to_string())?;

    // ---- lib.rs ----
    let lib_rs = format!(
        r#"//! {id} 插件。
//!
//! 由 `cargo xtask new-plugin` 生成。请按 `plugins/demo` 的结构补充：
//! `api/`（HTTP）、`service/`（业务规则与事务）、`repo/`（SQL）、`domain/`（实体）。

#![forbid(unsafe_code)]
#![deny(missing_docs)]

use kernel::prelude::*;

/// 插件元信息。
const PLUGIN_ID: &str = "{id}";
const PLUGIN_VERSION: &str = env!("CARGO_PKG_VERSION");

/// 本插件的迁移（编译期内嵌）。
static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

/// 插件实现。
///
/// 结构体刻意命名为 `PluginImpl` 而不是 `Plugin`：
/// 因为 `use kernel::prelude::*` 已经把 `Plugin` trait 引入作用域，
/// 同名会造成 E0255（名称冲突）。生成器必须一次生成可编译的代码。
pub struct PluginImpl;

impl kernel::plugin::Plugin for PluginImpl {{
    fn meta(&self) -> PluginMeta {{
        PluginMeta {{
            id: PLUGIN_ID,
            name: "{id}",
            version: PLUGIN_VERSION,
            depends_on: &[],
            order: 100,
        }}
    }}

    fn migrations(&self) -> Option<&'static sqlx::migrate::Migrator> {{
        Some(&MIGRATOR)
    }}

    fn routes(&self) -> Router<AppState> {{
        router()
    }}
}}

/// 路由表：挂载到 `/api/{id}`。
pub fn router() -> Router<AppState> {{
    Router::new().route("/ping", get(ping))
}}

/// 连通性探测。
async fn ping() -> ApiResult<&'static str> {{
    Ok(ApiResponse::new("pong"))
}}

inventory::submit! {{
    kernel::plugin::PluginRegistration {{ create: || Box::new(PluginImpl) }}
}}
"#
    );
    fs::write(dir.join("src").join("lib.rs"), lib_rs).map_err(|e| e.to_string())?;

    // ---- 初始迁移 ----
    let migration = format!(
        r#"-- {id} 插件初始迁移
-- 注意：本文件一旦发布不可修改，只能新增迁移文件。

CREATE TABLE IF NOT EXISTS {id}_example (
    id          uuid        PRIMARY KEY,
    tenant_id   uuid        NOT NULL,
    name        varchar(64) NOT NULL,
    created_at  timestamptz NOT NULL DEFAULT now(),
    created_by  uuid        NOT NULL,
    deleted_at  timestamptz,
    version     integer     NOT NULL DEFAULT 1
);

ALTER TABLE {id}_example ENABLE ROW LEVEL SECURITY;
ALTER TABLE {id}_example FORCE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS tenant_isolation ON {id}_example;
CREATE POLICY tenant_isolation ON {id}_example
    USING (tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid);
"#
    );
    fs::write(dir.join("migrations").join("0001_init.sql"), migration)
        .map_err(|e| e.to_string())?;

    println!("✅ 已生成插件: {}", dir.display());
    println!("\n还需要两步（缺一不可）：");
    println!("  1) 在 apps/server/Cargo.toml 的 [dependencies] 增加：");
    println!("     {crate_name} = {{ path = \"../../plugins/{id}\" }}");
    println!(
        "  2) 在 apps/server/src/main.rs 顶部加一行强制链接：
     use plugin_crm as _;   // 链接器会丢弃未被引用的 crate"
    );
    println!("\n然后在插件目录补 CONTEXT.md（领域术语/表/权限点/事件），再写业务。");
    Ok(())
}

// =============================================================================
// openapi
// =============================================================================

/// 导出 OpenAPI 契约到 `contracts/openapi.json`。
///
/// 为什么用命令而不是启动时下载：
/// - CI 可以在不连数据库、不启服务的情况下生成并比对契约；
/// - 生成的 JSON 入库，PR 里能看到契约变更 diff；
/// - 前端据此生成类型（`pnpm gen:api`）。
fn openapi() -> Result<(), String> {
    // 通过 server 的 --dump-openapi 模式生成：它知道全部插件，
    // 而 xtask 不应该依赖任何插件（否则每加一个插件都要改 xtask）。
    let output = Command::new("cargo")
        .args(["run", "-q", "-p", "server", "--", "--dump-openapi"])
        .current_dir(workspace_root())
        .output()
        .map_err(|e| format!("执行 server --dump-openapi 失败: {e}"))?;

    if !output.status.success() {
        return Err(format!(
            "server --dump-openapi 退出异常: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let json =
        String::from_utf8(output.stdout).map_err(|e| format!("OpenAPI 输出不是合法 UTF-8: {e}"))?;

    // 先校验 JSON 合法性，避免把半截输出写进契约文件
    serde_json::from_str::<serde_json::Value>(&json)
        .map_err(|e| format!("OpenAPI 输出不是合法 JSON: {e}"))?;

    let out_dir = workspace_root().join("contracts");
    fs::create_dir_all(&out_dir).map_err(|e| format!("创建 contracts 目录失败: {e}"))?;
    let out = out_dir.join("openapi.json");
    fs::write(&out, json).map_err(|e| format!("写入 {} 失败: {e}", out.display()))?;

    println!("✅ 已生成接口契约: {}", out.display());
    Ok(())
}
