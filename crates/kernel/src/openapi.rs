//! OpenAPI 契约导出。
//!
//! # 为什么每个模块各自定义片段
//!
//! 如果所有接口都写在全局 `ApiDoc` 里，新增一个模块就要改全局文件 ——
//! 多人/多 AI 并行开发必然冲突。这里采用「模块自带片段 + 运行时合并」：
//!
//! ```text
//! auth::AuthApi ─┐
//! system::SystemApi ─┼─▶ ApiDoc::openapi() ─▶ GET /api/openapi.json
//! health::HealthApi ─┘
//! ```
//!
//! # 产物用途
//!
//! 1. 前端用 `openapi-typescript` 生成类型（`pnpm gen:api`）；
//! 2. CI 快照对比，拦截破坏性契约变更；
//! 3. AI 读取该文件即可知道全部接口与字段，不需要翻代码。

use utoipa::OpenApi;

/// 全局 OpenAPI 文档。
#[derive(OpenApi)]
#[openapi(
    info(
        title = "卓才智能 API",
        version = env!("CARGO_PKG_VERSION"),
        description = "企业级管理平台接口。所有接口除登录/健康检查外均需会话 Cookie。"
    ),
    tags(
        (name = "auth", description = "认证与会话"),
        (name = "system", description = "系统管理：用户/角色/权限/菜单/字典"),
        (name = "health", description = "健康检查")
    )
)]
pub struct ApiDoc;

/// 合并内核模块片段后的 OpenAPI 文档（不含业务插件）。
///
/// 用于单元测试与"只要内核契约"的场景；完整契约请用 [`build_with`]。
pub fn build() -> utoipa::openapi::OpenApi {
    build_with(&[])
}

/// 合并内核 + 所有业务插件的 OpenAPI 文档。
///
/// 插件通过 [`crate::plugin::Plugin::openapi`] 提供自己的片段，
/// 内核不需要知道任何具体插件类型（依赖方向依然是插件 → 内核）。
pub fn build_with(plugins: &[Box<dyn crate::plugin::Plugin>]) -> utoipa::openapi::OpenApi {
    let mut doc = ApiDoc::openapi();
    doc.merge(crate::auth::AuthApi::openapi());
    doc.merge(crate::system::SystemApi::openapi());
    doc.merge(crate::files::FileApi::openapi());
    doc.merge(crate::notices::NoticeApi::openapi());
    doc.merge(crate::configs::ConfigApi::openapi());
    doc.merge(crate::exports::ExportApi::openapi());
    doc.merge(crate::user_import::UserImportApi::openapi());
    doc.merge(crate::imports::ImportApi::openapi());
    doc.merge(crate::posts::PostApi::openapi());
    doc.merge(crate::tenant_admin::TenantApi::openapi());
    doc.merge(crate::audit_admin::AuditAdminApi::openapi());
    doc.merge(crate::system_identity::IdentityApi::openapi());
    doc.merge(crate::system_org::OrgApi::openapi());
    doc.merge(crate::system_security::SecurityApi::openapi());

    for plugin in plugins {
        if let Some(fragment) = plugin.openapi() {
            doc.merge(fragment);
        }
    }
    doc
}

/// `GET /api/openapi.json` 的处理器（包含内核 + 已注册插件）。
pub async fn serve() -> axum::Json<utoipa::openapi::OpenApi> {
    let plugins = crate::plugin::load_plugins().unwrap_or_default();
    axum::Json(build_with(&plugins))
}

/// `GET /docs`：服务端渲染的接口文档页。
///
/// 为什么不用 Swagger UI：它会引入几 MB 的前端资源，且 `utoipa-swagger-ui`
/// 在构建时还要联网下载资源，私有化/离线环境不可用。这里直接渲染一份
/// 只读的 HTML 表格，零 JS、零外部依赖，够查接口即可。
pub async fn docs() -> axum::response::Html<String> {
    let plugins = crate::plugin::load_plugins().unwrap_or_default();
    axum::response::Html(render_docs(&build_with(&plugins)))
}

/// 把契约渲染成 HTML 表格。
fn render_docs(doc: &utoipa::openapi::OpenApi) -> String {
    let mut rows = String::new();
    for (path, item) in &doc.paths.paths {
        let ops = [
            ("GET", item.get.as_ref()),
            ("POST", item.post.as_ref()),
            ("PUT", item.put.as_ref()),
            ("PATCH", item.patch.as_ref()),
            ("DELETE", item.delete.as_ref()),
        ];
        for (method, op) in ops {
            let Some(op) = op else { continue };
            let summary = op.summary.clone().unwrap_or_default();
            let tags = op.tags.as_ref().map(|t| t.join(", ")).unwrap_or_default();
            rows.push_str(&format!(
                "<tr><td><span class=\"m {}\">{}</span></td><td><code>{}</code></td><td>{}</td><td>{}</td></tr>\n",
                method.to_lowercase(),
                method,
                escape_html(path),
                escape_html(&summary),
                escape_html(&tags)
            ));
        }
    }
    let count = rows.matches("<tr>").count();

    format!(
        r#"<!doctype html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<title>卓才智能 API 文档</title>
<style>
body{{font-family:-apple-system,Segoe UI,Roboto,sans-serif;margin:0;background:#f8fafc;color:#0f172a}}
header{{background:#fff;border-bottom:1px solid #e2e8f0;padding:20px 32px}}
h1{{margin:0;font-size:20px}}
p{{color:#64748b;margin:6px 0 0;font-size:13px}}
main{{padding:24px 32px}}
table{{border-collapse:collapse;width:100%;background:#fff;border:1px solid #e2e8f0;border-radius:8px;overflow:hidden}}
th,td{{text-align:left;padding:10px 12px;border-bottom:1px solid #eef2f7;font-size:13px}}
th{{background:#f1f5f9;font-weight:600}}
code{{font-family:Consolas,monospace;color:#0f172a}}
.m{{display:inline-block;padding:2px 8px;border-radius:4px;color:#fff;font-size:11px;font-weight:600}}
.m.get{{background:#16a34a}} .m.post{{background:#2563eb}} .m.put{{background:#d97706}}
.m.patch{{background:#7c3aed}} .m.delete{{background:#dc2626}}
.note{{margin-top:16px;font-size:12px;color:#94a3b8}}
a{{color:#2563eb}}
</style>
</head>
<body>
<header>
  <h1>卓才智能 API 文档</h1>
  <p>共 {count} 个接口 · 原始契约：<a href="/api/openapi.json">/api/openapi.json</a></p>
</header>
<main>
<table>
<thead><tr><th style="width:90px">方法</th><th style="width:40%">路径</th><th>说明</th><th style="width:120px">分组</th></tr></thead>
<tbody>
{rows}</tbody>
</table>
<p class="note">该页面由服务端根据 OpenAPI 契约实时渲染，无外部依赖，离线环境可用。</p>
</main>
</body>
</html>"#,
        count = count,
        rows = rows
    )
}

/// 最小 HTML 转义。
fn escape_html(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 契约必须包含核心端点：缺失说明注解被误删，CI 会在这里失败。
    #[test]
    fn spec_contains_core_paths() {
        let doc = build();
        let json = serde_json::to_value(&doc).expect("序列化 OpenAPI");
        let paths = json
            .get("paths")
            .and_then(|p| p.as_object())
            .expect("paths 必须存在");

        for path in [
            "/api/auth/login",
            "/api/auth/me",
            "/api/system/users",
            "/api/system/roles",
            "/api/system/menus",
            "/api/system/dicts",
            "/api/system/dicts/{code}/items",
        ] {
            assert!(paths.contains_key(path), "契约缺少路径: {path}");
        }

        assert_eq!(json["info"]["title"], "卓才智能 API");
    }
}
