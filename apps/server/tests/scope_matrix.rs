//! 数据范围矩阵测试：all / dept_and_child / dept / self / custom。
//!
//! 运行：
//!   $env:DATABASE_URL="postgres://zcadmin:zcadmin@127.0.0.1:5432/zcadmin"
//!   cargo test -p server --test scope_matrix -- --ignored --nocapture

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
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
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

async fn create_dept(
    router: &axum::Router,
    cookie: &str,
    name: &str,
    parent: Option<&str>,
) -> String {
    let (status, body) = call(
        router,
        "POST",
        "/api/system/depts",
        Some(cookie),
        Some(json!({ "name": name, "parentId": parent })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "create dept {name} failed: {body}");
    body["data"]["id"].as_str().unwrap().to_string()
}

async fn create_role(
    router: &axum::Router,
    cookie: &str,
    code: &str,
    data_scope: &str,
    depts: &[String],
) -> String {
    let (status, body) = call(
        router,
        "POST",
        "/api/system/roles",
        Some(cookie),
        Some(json!({ "code": code, "name": code, "dataScope": data_scope })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "create role {code} failed: {body}");
    let role_id = body["data"]["id"].as_str().unwrap().to_string();
    if !depts.is_empty() {
        let (status, body) = call(
            router,
            "PUT",
            &format!("/api/system/roles/{role_id}/depts"),
            Some(cookie),
            Some(json!({ "deptIds": depts })),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "role depts failed: {body}");
    }
    role_id
}

async fn create_user(router: &axum::Router, cookie: &str, username: &str, dept_id: &str) -> String {
    let (status, body) = call(
        router,
        "POST",
        "/api/system/users",
        Some(cookie),
        Some(json!({
            "username": username,
            "displayName": username,
            "password": "Scope#12345",
            "deptId": dept_id
        })),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "create user {username} failed: {body}"
    );
    body["data"]["id"].as_str().unwrap().to_string()
}

#[tokio::test]
#[ignore = "需要真实 PostgreSQL"]
async fn scope_matrix() {
    let url = std::env::var("DATABASE_URL").expect("必须设置 DATABASE_URL");
    std::env::set_var("DATABASE_URL", url);
    let config = kernel::config::Config::load("config/default.toml").expect("config");
    let app = kernel::app::build(config).await.expect("build app");
    let admin = login_cookie(&app.router, "admin", "admin123456").await;
    // 多租户库里 `sys_tenant LIMIT 1` 取到的未必是 admin 所在的租户；
    // 直接以登录身份反查当前租户，测试才能在共享库上重复执行。
    let (status, me) = call(&app.router, "GET", "/api/auth/me", Some(&admin), None).await;
    assert_eq!(status, StatusCode::OK, "me failed: {me}");
    let tenant: Uuid = me["data"]["tenantId"]
        .as_str()
        .expect("tenantId")
        .parse()
        .expect("tenant uuid");

    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();

    let dept_a = create_dept(&app.router, &admin, &format!("Matrix A {suffix}"), None).await;
    let dept_a1 = create_dept(
        &app.router,
        &admin,
        &format!("Matrix A1 {suffix}"),
        Some(&dept_a),
    )
    .await;
    let dept_b = create_dept(&app.router, &admin, &format!("Matrix B {suffix}"), None).await;

    // 权限 id
    let (status, body) = call(
        &app.router,
        "GET",
        "/api/system/permissions",
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "permissions failed: {body}");
    let perm_id = body["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["code"] == "crm:customer:list")
        .and_then(|p| p["id"].as_str())
        .expect("permission")
        .to_string();

    let role_all = create_role(&app.router, &admin, &format!("mx_all_{suffix}"), "all", &[]).await;
    let role_dept = create_role(
        &app.router,
        &admin,
        &format!("mx_dept_{suffix}"),
        "dept",
        &[],
    )
    .await;
    let role_child = create_role(
        &app.router,
        &admin,
        &format!("mx_child_{suffix}"),
        "dept_and_child",
        &[],
    )
    .await;
    let role_self = create_role(
        &app.router,
        &admin,
        &format!("mx_self_{suffix}"),
        "self",
        &[],
    )
    .await;
    let role_custom = create_role(
        &app.router,
        &admin,
        &format!("mx_custom_{suffix}"),
        "custom",
        std::slice::from_ref(&dept_a1),
    )
    .await;

    for role in [&role_all, &role_dept, &role_child, &role_self, &role_custom] {
        let (status, body) = call(
            &app.router,
            "PUT",
            &format!("/api/system/roles/{role}/permissions"),
            Some(&admin),
            Some(json!({ "ids": [perm_id] })),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "role perms failed: {body}");
    }

    let user_all = create_user(&app.router, &admin, &format!("mx_all_{suffix}"), &dept_b).await;
    let user_dept = create_user(&app.router, &admin, &format!("mx_dept_{suffix}"), &dept_b).await;
    let user_child = create_user(&app.router, &admin, &format!("mx_child_{suffix}"), &dept_a).await;
    let user_self = create_user(&app.router, &admin, &format!("mx_self_{suffix}"), &dept_b).await;
    let user_custom =
        create_user(&app.router, &admin, &format!("mx_custom_{suffix}"), &dept_b).await;
    let user_combo = create_user(&app.router, &admin, &format!("mx_combo_{suffix}"), &dept_b).await;

    for (user, role) in [
        (&user_all, &role_all),
        (&user_dept, &role_dept),
        (&user_child, &role_child),
        (&user_self, &role_self),
        (&user_custom, &role_custom),
    ] {
        let (status, body) = call(
            &app.router,
            "PUT",
            &format!("/api/system/users/{user}/roles"),
            Some(&admin),
            Some(json!({ "ids": [role] })),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "assign role failed: {body}");
    }

    // 多角色并集：dept(B) + custom(A1)
    let (status, body) = call(
        &app.router,
        "PUT",
        &format!("/api/system/users/{user_combo}/roles"),
        Some(&admin),
        Some(json!({ "ids": [role_dept, role_custom] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "assign combo roles failed: {body}");

    // 数据：A / A1 / B / SELF / OTHER
    let self_id: Uuid = user_self.parse().unwrap();

    let mut tx = app.state.db.begin().await.unwrap();
    sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
        .bind(tenant.to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    let admin_id: Uuid = sqlx::query_scalar(
        "SELECT id FROM sys.sys_user WHERE username = 'admin' AND tenant_id = $1",
    )
    .bind(tenant)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    for (code, dept, owner) in [
        (format!("MX-A-{suffix}"), &dept_a, admin_id),
        (format!("MX-A1-{suffix}"), &dept_a1, admin_id),
        (format!("MX-B-{suffix}"), &dept_b, admin_id),
        (format!("MX-SELF-{suffix}"), &dept_b, self_id),
        (format!("MX-OTHER-{suffix}"), &dept_b, admin_id),
    ] {
        sqlx::query(
            "INSERT INTO crm.customer (id, tenant_id, code, name, level, status, owner_id, dept_id, created_by)
             VALUES (gen_random_uuid(), $1, $2, $2, 'normal', 'potential', $3, $4::uuid, $3)",
        )
        .bind(tenant)
        .bind(&code)
        .bind(owner)
        .bind(dept)
        .execute(&mut *tx)
        .await
        .unwrap();
    }
    tx.commit().await.unwrap();

    async fn visible_codes(router: &axum::Router, username: &str) -> Vec<String> {
        let cookie = login_cookie(router, username, "Scope#12345").await;
        let (status, body) = call(
            router,
            "GET",
            "/api/crm/customers?page=1&size=200",
            Some(&cookie),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "list customers failed: {body}");
        body["data"]["data"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|r| r["code"].as_str().map(str::to_string))
            .collect()
    }

    let all = visible_codes(&app.router, &format!("mx_all_{suffix}")).await;
    for expected in ["MX-A-", "MX-A1-", "MX-B-", "MX-SELF-", "MX-OTHER-"] {
        assert!(
            all.iter()
                .any(|c| c.starts_with(&format!("{expected}{suffix}"))),
            "all missing {expected}: {all:?}"
        );
    }

    let dept = visible_codes(&app.router, &format!("mx_dept_{suffix}")).await;
    assert!(dept.iter().any(|c| c == &format!("MX-B-{suffix}")));
    assert!(!dept.iter().any(|c| c == &format!("MX-A-{suffix}")));
    assert!(!dept.iter().any(|c| c == &format!("MX-A1-{suffix}")));

    let child = visible_codes(&app.router, &format!("mx_child_{suffix}")).await;
    assert!(child.iter().any(|c| c == &format!("MX-A-{suffix}")));
    assert!(child.iter().any(|c| c == &format!("MX-A1-{suffix}")));
    assert!(!child.iter().any(|c| c == &format!("MX-B-{suffix}")));

    let own = visible_codes(&app.router, &format!("mx_self_{suffix}")).await;
    assert!(own.iter().any(|c| c == &format!("MX-SELF-{suffix}")));
    assert!(!own.iter().any(|c| c == &format!("MX-OTHER-{suffix}")));
    assert!(!own.iter().any(|c| c == &format!("MX-A-{suffix}")));

    let custom = visible_codes(&app.router, &format!("mx_custom_{suffix}")).await;
    assert!(custom.iter().any(|c| c == &format!("MX-A1-{suffix}")));
    assert!(!custom.iter().any(|c| c == &format!("MX-A-{suffix}")));
    assert!(!custom.iter().any(|c| c == &format!("MX-B-{suffix}")));

    let combo = visible_codes(&app.router, &format!("mx_combo_{suffix}")).await;
    assert!(combo.iter().any(|c| c == &format!("MX-A1-{suffix}")));
    assert!(combo.iter().any(|c| c == &format!("MX-B-{suffix}")));
    assert!(!combo.iter().any(|c| c == &format!("MX-A-{suffix}")));

    // 导出必须与列表同口径：数据范围为 self 的用户导出「用户列表」时，
    // 只能拿到自己那一行。旧实现把 data_scope 塞进任务上下文却没在 SQL 里用，
    // 等于给 self 范围用户开了一条全租户用户/邮箱的导出通道。
    let (status, body) = call(
        &app.router,
        "GET",
        "/api/system/permissions",
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "permissions failed: {body}");
    let perm_ids: Vec<String> = ["system:user:list", "system:export:list"]
        .iter()
        .map(|code| {
            body["data"]
                .as_array()
                .unwrap()
                .iter()
                .find(|p| p["code"] == *code)
                .and_then(|p| p["id"].as_str())
                .unwrap_or_else(|| panic!("permission {code} missing"))
                .to_string()
        })
        .collect();

    let role_export = create_role(
        &app.router,
        &admin,
        &format!("mx_export_{suffix}"),
        "self",
        &[],
    )
    .await;
    let (status, body) = call(
        &app.router,
        "PUT",
        &format!("/api/system/roles/{role_export}/permissions"),
        Some(&admin),
        Some(json!({ "ids": perm_ids })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "export role perms failed: {body}");

    let export_username = format!("mx_export_{suffix}");
    let user_export = create_user(&app.router, &admin, &export_username, &dept_b).await;
    let (status, body) = call(
        &app.router,
        "PUT",
        &format!("/api/system/users/{user_export}/roles"),
        Some(&admin),
        Some(json!({ "ids": [role_export] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "export user roles failed: {body}");

    let export_cookie = login_cookie(&app.router, &export_username, "Scope#12345").await;
    let (status, body) = call(
        &app.router,
        "POST",
        "/api/system/export-tasks",
        Some(&export_cookie),
        Some(json!({ "kind": "system.users" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "create export failed: {body}");
    let task_id = body["data"]["id"]
        .as_str()
        .expect("export task id")
        .to_string();

    let mut total_rows = None;
    for _ in 0..50 {
        let (status, body) = call(
            &app.router,
            "GET",
            "/api/system/export-tasks?page=1&size=10",
            Some(&export_cookie),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "list export tasks failed: {body}");
        let tasks = body["data"]["data"].as_array().cloned().unwrap_or_default();
        if let Some(task) = tasks.iter().find(|t| t["id"] == task_id.as_str()) {
            match task["status"].as_str() {
                Some("success") => {
                    total_rows = task["totalRows"].as_i64();
                    break;
                }
                Some("failed") => panic!("export failed: {}", task["error"]),
                _ => {}
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert_eq!(
        total_rows,
        Some(1),
        "self 范围用户导出用户列表应只有本人 1 行（数据范围旁路回归）"
    );

    println!("scope_matrix: all/dept/child/self/custom/union + export scope passed");
}
