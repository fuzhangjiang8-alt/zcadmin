//! 自定义数据范围（custom）端到端越权测试。
//!
//! 场景：
//!   - 部门 A / B 各有一个客户；
//!   - 用户 U 属于部门 B，但角色数据范围为 custom = {A}；
//!   - U 登录后只能看到 A 的客户，看不到 B 的客户。
//!
//! 运行：
//!   $env:DATABASE_URL="postgres://zcadmin:zcadmin@127.0.0.1:5432/zcadmin"
//!   cargo test -p server --test custom_scope -- --ignored --nocapture

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use serde_json::{json, Value};
use sqlx::types::Uuid;
use tower::ServiceExt;

use plugin_crm as _;
use plugin_demo as _;

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
    let req = match body {
        Some(v) => builder
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(v.to_string()))
            .unwrap(),
        None => builder.body(Body::empty()).unwrap(),
    };
    let resp = router.clone().oneshot(req).await.expect("request");
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), 4 * 1024 * 1024)
        .await
        .unwrap();
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, value)
}

async fn login_cookie(router: &axum::Router, username: &str, password: &str) -> String {
    let req = Request::builder()
        .method("POST")
        .uri("/api/auth/login")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({ "username": username, "password": password, "tenant": null }).to_string(),
        ))
        .unwrap();
    let resp = router.clone().oneshot(req).await.expect("login");
    assert_eq!(resp.status(), StatusCode::OK, "login failed for {username}");
    resp.headers()
        .get(header::SET_COOKIE)
        .expect("set-cookie")
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string()
}

#[tokio::test]
#[ignore = "需要真实 PostgreSQL"]
async fn custom_scope_filters_rows() {
    let url = std::env::var("DATABASE_URL").expect("必须设置 DATABASE_URL");
    std::env::set_var("DATABASE_URL", url);
    let config = kernel::config::Config::load("config/default.toml").expect("config");
    let app = kernel::app::build(config).await.expect("build app");
    let admin = login_cookie(&app.router, "admin", "admin123456").await;

    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();

    // 两个部门
    let (status, body) = call(
        &app.router,
        "POST",
        "/api/system/depts",
        Some(&admin),
        Some(json!({ "name": format!("Scope A {suffix}"), "parentId": null })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "dept A failed: {body}");
    let dept_a = body["data"]["id"].as_str().unwrap().to_string();

    let (status, body) = call(
        &app.router,
        "POST",
        "/api/system/depts",
        Some(&admin),
        Some(json!({ "name": format!("Scope B {suffix}"), "parentId": null })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "dept B failed: {body}");
    let dept_b = body["data"]["id"].as_str().unwrap().to_string();

    // 两个客户：直接用 SQL 写，归属部门 A / B
    let tenant: Uuid = sqlx::query_scalar("SELECT id FROM sys.sys_tenant LIMIT 1")
        .fetch_one(&app.state.db)
        .await
        .unwrap();

    let mut tx = app.state.db.begin().await.unwrap();
    sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
        .bind(tenant.to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    let owner: Uuid = sqlx::query_scalar("SELECT id FROM sys.sys_user LIMIT 1")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    let mut customer_ids: Vec<Uuid> = Vec::new();
    for (code, name, dept) in [
        (format!("SCOPE-A-{suffix}"), "Scope Customer A", &dept_a),
        (format!("SCOPE-B-{suffix}"), "Scope Customer B", &dept_b),
    ] {
        let id: Uuid = sqlx::query_scalar(
            "INSERT INTO crm.customer (id, tenant_id, code, name, level, status, owner_id, dept_id, created_by)
             VALUES (gen_random_uuid(), $1, $2, $3, 'normal', 'potential', $4, $5::uuid, $4)
             RETURNING id",
        )
        .bind(tenant)
        .bind(&code)
        .bind(name)
        .bind(owner)
        .bind(dept)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
        customer_ids.push(id);
    }

    // 每个部门一条商机（验证生成器模块也支持 custom）
    for (i, (name, dept)) in [
        (format!("Scope Opp A {suffix}"), &dept_a),
        (format!("Scope Opp B {suffix}"), &dept_b),
    ]
    .iter()
    .enumerate()
    {
        sqlx::query(
            "INSERT INTO crm.opportunity (id, tenant_id, customer_id, name, amount_cents, stage, owner_id, dept_id, created_by)
             VALUES (gen_random_uuid(), $1, $2, $3, 100000, 'initial', $4, $5::uuid, $4)",
        )
        .bind(tenant)
        .bind(customer_ids[i])
        .bind(name)
        .bind(owner)
        .bind(dept)
        .execute(&mut *tx)
        .await
        .unwrap();
    }
    tx.commit().await.unwrap();

    // 角色：custom = {A}，只给客户查看权限
    let (status, body) = call(
        &app.router,
        "POST",
        "/api/system/roles",
        Some(&admin),
        Some(json!({
            "code": format!("scope_role_{suffix}"),
            "name": "Scope Role",
            "dataScope": "custom"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "role create failed: {body}");
    let role_id = body["data"]["id"].as_str().unwrap().to_string();

    let (status, body) = call(
        &app.router,
        "PUT",
        &format!("/api/system/roles/{role_id}/depts"),
        Some(&admin),
        Some(json!({ "deptIds": [dept_a] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "role depts failed: {body}");

    let (status, body) = call(
        &app.router,
        "GET",
        &format!("/api/system/roles/{role_id}/scope-preview"),
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "scope preview custom failed: {body}"
    );
    let visible = body["data"]["visibleDeptIds"].as_array().unwrap();
    assert!(
        visible.iter().any(|v| v == &dept_a),
        "custom dept missing from preview: {body}"
    );

    let (status, body) = call(
        &app.router,
        "GET",
        "/api/system/permissions",
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "permissions failed: {body}");
    let perm_ids: Vec<String> = ["crm:customer:list", "crm:opportunity:list"]
        .iter()
        .map(|code| {
            body["data"]
                .as_array()
                .unwrap()
                .iter()
                .find(|p| p["code"] == *code)
                .and_then(|p| p["id"].as_str())
                .unwrap_or_else(|| panic!("missing permission {code}"))
                .to_string()
        })
        .collect();
    let (status, body) = call(
        &app.router,
        "PUT",
        &format!("/api/system/roles/{role_id}/permissions"),
        Some(&admin),
        Some(json!({ "ids": perm_ids })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "role perms failed: {body}");

    // 用户：属于部门 B
    let username = format!("scope_user_{suffix}");
    let (status, body) = call(
        &app.router,
        "POST",
        "/api/system/users",
        Some(&admin),
        Some(json!({
            "username": username,
            "displayName": "Scope User",
            "password": "Scope#12345",
            "deptId": dept_b
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "user create failed: {body}");
    let user_id = body["data"]["id"].as_str().unwrap().to_string();

    let (status, body) = call(
        &app.router,
        "PUT",
        &format!("/api/system/users/{user_id}/roles"),
        Some(&admin),
        Some(json!({ "ids": [role_id] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "assign role failed: {body}");

    // 用 U 登录，看客户列表
    let user_cookie = login_cookie(&app.router, &username, "Scope#12345").await;
    let (status, body) = call(
        &app.router,
        "GET",
        "/api/crm/customers?page=1&size=50",
        Some(&user_cookie),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "customer list failed: {body}");
    let rows = body["data"]["data"].as_array().unwrap();
    let codes: Vec<&str> = rows.iter().filter_map(|r| r["code"].as_str()).collect();
    assert!(
        codes.contains(&format!("SCOPE-A-{suffix}").as_str()),
        "dept A customer missing: {codes:?}"
    );
    assert!(
        !codes.contains(&format!("SCOPE-B-{suffix}").as_str()),
        "dept B customer should be hidden: {codes:?}"
    );

    // 生成器模块（商机）也必须支持 custom
    let (status, body) = call(
        &app.router,
        "GET",
        "/api/crm/opportunities?page=1&size=50",
        Some(&user_cookie),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "opportunity list failed: {body}");
    let opp_rows = body["data"]["data"].as_array().unwrap();
    let names: Vec<&str> = opp_rows.iter().filter_map(|r| r["name"].as_str()).collect();
    assert!(
        names.contains(&format!("Scope Opp A {suffix}").as_str()),
        "dept A opportunity missing: {names:?}"
    );
    assert!(
        !names.contains(&format!("Scope Opp B {suffix}").as_str()),
        "dept B opportunity should be hidden: {names:?}"
    );

    println!("custom_scope_filters_rows: passed (A visible, B hidden)");
}
