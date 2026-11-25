//! 租户隔离集成测试：验证「租户管理员 ≠ 平台超管」。
//!
//! 背景：曾经 `create_tenant` 把租户管理员写成 `is_super = true`，
//! 登录后展开为通配权限，可以直接调用 `/api/system/tenants`
//! 管理所有租户。本测试锁死修复后的行为：
//!
//! 1. 租户管理员登录后权限集不含 `*`；
//! 2. `/api/system/tenants` 返回 403；
//! 3. `/api/system/permissions` 不返回平台级权限点；
//! 4. 给角色授予 `system:tenant:*` 会被拒绝。
//!
//! 运行：
//! ```powershell
//! $env:DATABASE_URL="postgres://zcadmin:zcadmin@127.0.0.1:5432/zcadmin"
//! cargo test -p server --test tenant_isolation -- --ignored --nocapture
//! ```

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use serde_json::{json, Value};
use tower::ServiceExt;

use plugin_crm as _;
use plugin_demo as _;

/// 发起一次请求并返回 (状态码, JSON 响应体)。
async fn call(
    router: &axum::Router,
    method: &str,
    uri: &str,
    cookie: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(c) = cookie {
        builder = builder.header(header::COOKIE, c);
    }
    let request = match body {
        Some(v) => builder
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(v.to_string()))
            .unwrap(),
        None => builder.body(Body::empty()).unwrap(),
    };
    let response = router.clone().oneshot(request).await.expect("request");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 4 * 1024 * 1024)
        .await
        .expect("body");
    let json = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, json)
}

/// 登录并返回会话 Cookie。
async fn login(
    router: &axum::Router,
    username: &str,
    password: &str,
    tenant: Option<&str>,
) -> (StatusCode, String) {
    let request = Request::builder()
        .method("POST")
        .uri("/api/auth/login")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({ "username": username, "password": password, "tenant": tenant }).to_string(),
        ))
        .unwrap();
    let response = router.clone().oneshot(request).await.expect("login");
    let status = response.status();
    let cookie = response
        .headers()
        .get(header::SET_COOKIE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(';').next())
        .unwrap_or_default()
        .to_string();
    (status, cookie)
}

#[tokio::test]
#[ignore = "需要真实 PostgreSQL（DATABASE_URL）"]
async fn tenant_admin_cannot_manage_platform() {
    let url = std::env::var("DATABASE_URL").expect("必须设置 DATABASE_URL");
    std::env::set_var("DATABASE_URL", &url);

    let password = std::env::var("AUTH_DEV_PASSWORD").unwrap_or_else(|_| "admin123456".into());
    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_millis();
    let tenant_code = format!("iso{suffix}");

    let config = kernel::config::Config::load("config/default.toml").expect("config");
    let app = kernel::app::build(config).await.expect("build app");

    // ---- 1. 平台管理员登录 ----
    // 显式指定默认租户：测试库可能已有多个租户，留空会触发「请选择租户」校验。
    let (status, admin_cookie) = login(&app.router, "admin", &password, Some("default")).await;
    assert_eq!(status, StatusCode::OK, "platform admin login failed");

    // ---- 2. 创建新租户 ----
    let admin_username = format!("tadmin{suffix}");
    let (status, body) = call(
        &app.router,
        "POST",
        "/api/system/tenants",
        Some(&admin_cookie),
        Some(json!({
            "code": tenant_code,
            "name": "隔离测试租户",
            "adminUsername": admin_username,
            "adminDisplayName": "租户管理员",
            "adminPassword": "Tenant#12345",
            "maxUsers": 10
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "create tenant failed: {body}");
    let tenant_id = body["data"]["id"]
        .as_str()
        .expect("tenant id missing")
        .to_string();

    // ---- 3. 租户管理员登录（多租户必须带租户编码） ----
    let (status, tenant_cookie) = login(
        &app.router,
        &admin_username,
        "Tenant#12345",
        Some(&tenant_code),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "tenant admin login failed");

    // 新建租户的管理员被要求首次改密；改密后才能访问其他接口。
    let (status, body) = call(
        &app.router,
        "POST",
        "/api/auth/change-password",
        Some(&tenant_cookie),
        Some(json!({
            "oldPassword": "Tenant#12345",
            "newPassword": "TenantNew#123"
        })),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "tenant admin change password: {body}"
    );

    // 首登改密会吊销全部会话（强制重新认证），用新密码重新登录。
    let (status, tenant_cookie) = login(
        &app.router,
        &admin_username,
        "TenantNew#123",
        Some(&tenant_code),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "tenant admin re-login failed");

    // ---- 4. 身份：不是超管（权限集不含 `*`） ----
    let (status, me) = call(
        &app.router,
        "GET",
        "/api/auth/me",
        Some(&tenant_cookie),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "me failed: {me}");
    let permissions = me["data"]["permissions"]
        .as_array()
        .expect("permissions missing");
    assert!(
        !permissions.iter().any(|p| p.as_str() == Some("*")),
        "租户管理员不应拥有通配权限: {permissions:?}"
    );
    assert!(
        permissions
            .iter()
            .any(|p| p.as_str() == Some("system:user:list")),
        "租户管理员应保留租户内权限: {permissions:?}"
    );
    let tenant_user_id = me["data"]["userId"]
        .as_str()
        .expect("userId missing")
        .to_string();

    // ---- 5. 平台租户管理接口必须 403 ----
    let (status, body) = call(
        &app.router,
        "GET",
        "/api/system/tenants",
        Some(&tenant_cookie),
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "租户管理员不应能列出所有租户: {body}"
    );

    // ---- 6. 权限点列表不得包含平台级权限 ----
    let (status, body) = call(
        &app.router,
        "GET",
        "/api/system/permissions",
        Some(&tenant_cookie),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "list permissions: {body}");
    let leaked: Vec<String> = body["data"]
        .as_array()
        .expect("permissions list")
        .iter()
        .filter_map(|p| p["code"].as_str())
        .filter(|code| code.starts_with("system:tenant:"))
        .map(str::to_string)
        .collect();
    assert!(
        leaked.is_empty(),
        "租户管理员看到了平台级权限点: {leaked:?}"
    );

    // ---- 7. 授平台权限必须被拒绝 ----
    let (status, roles) = call(
        &app.router,
        "GET",
        &format!("/api/system/users/{tenant_user_id}/roles"),
        Some(&tenant_cookie),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "get user roles: {roles}");
    let role_id = roles["data"]
        .as_array()
        .and_then(|v| v.first())
        .and_then(|v| v.as_str())
        .expect("tenant admin role missing")
        .to_string();

    let (_, all_perms) = call(
        &app.router,
        "GET",
        "/api/system/permissions",
        Some(&admin_cookie),
        None,
    )
    .await;
    let platform_perm_id = all_perms["data"]
        .as_array()
        .expect("platform permissions")
        .iter()
        .find(|p| p["code"].as_str() == Some("system:tenant:list"))
        .and_then(|p| p["id"].as_str())
        .expect("system:tenant:list not found")
        .to_string();

    let (status, body) = call(
        &app.router,
        "PUT",
        &format!("/api/system/roles/{role_id}/permissions"),
        Some(&tenant_cookie),
        Some(json!({ "ids": [platform_perm_id] })),
    )
    .await;
    assert!(
        status == StatusCode::UNPROCESSABLE_ENTITY || status == StatusCode::FORBIDDEN,
        "授予平台权限应被拒绝，实际 {status}: {body}"
    );

    // ---- 8. 清理：平台管理员删除租户 ----
    let (status, body) = call(
        &app.router,
        "DELETE",
        &format!("/api/system/tenants/{tenant_id}"),
        Some(&admin_cookie),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "cleanup tenant failed: {body}");

    println!("tenant_isolation: passed");
}
