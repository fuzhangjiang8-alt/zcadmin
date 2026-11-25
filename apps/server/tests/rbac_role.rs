//! 低权限角色真实登录测试：验证「角色 → 权限 → 接口」全链路。
//!
//! 运行：
//!   $env:DATABASE_URL="postgres://zcadmin:zcadmin@127.0.0.1:5432/zcadmin"
//!   cargo test -p server --test rbac_role -- --ignored --nocapture

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use serde_json::{json, Value};
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

async fn call_bearer(router: &axum::Router, uri: &str, token: &str) -> (StatusCode, Value) {
    let req = Request::builder()
        .method("GET")
        .uri(uri)
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
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

async fn login(router: &axum::Router, username: &str, password: &str) -> (StatusCode, String) {
    let req = Request::builder()
        .method("POST")
        .uri("/api/auth/login")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({ "username": username, "password": password, "tenant": null }).to_string(),
        ))
        .unwrap();
    let resp = router.clone().oneshot(req).await.expect("login");
    let status = resp.status();
    let cookie = resp
        .headers()
        .get(header::SET_COOKIE)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.split(';').next().unwrap_or("").to_string())
        .unwrap_or_default();
    (status, cookie)
}

#[tokio::test]
#[ignore = "需要真实 PostgreSQL"]
async fn rbac_role() {
    let url = std::env::var("DATABASE_URL").expect("必须设置 DATABASE_URL");
    std::env::set_var("DATABASE_URL", url);
    let config = kernel::config::Config::load("config/default.toml").expect("config");
    let app = kernel::app::build(config).await.expect("build app");

    let (status, admin_cookie) = login(&app.router, "admin", "admin123456").await;
    assert_eq!(status, StatusCode::OK);
    let suffix = sqlx::types::Uuid::new_v4().simple().to_string();
    let role_code = format!("rbac_{}", &suffix[..8]);
    let username = format!("rbac_{}", &suffix[..8]);

    let (status, role_body) = call(
        &app.router,
        "POST",
        "/api/system/roles",
        Some(&admin_cookie),
        Some(json!({
            "code": role_code,
            "name": "RBAC 测试角色",
            "description": "low privilege",
            "dataScope": "self"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "create role: {role_body}");
    let role_id = role_body["data"]["id"].as_str().unwrap().to_string();

    let (status, perms_body) = call(
        &app.router,
        "GET",
        "/api/system/permissions",
        Some(&admin_cookie),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let perms = perms_body["data"].as_array().unwrap();
    let ids: Vec<String> = ["crm:customer:list", "system:user:list"]
        .iter()
        .map(|code| {
            perms
                .iter()
                .find(|p| p["code"] == *code)
                .and_then(|p| p["id"].as_str())
                .unwrap_or_else(|| panic!("permission missing: {code}"))
                .to_string()
        })
        .collect();
    let (status, body) = call(
        &app.router,
        "PUT",
        &format!("/api/system/roles/{role_id}/permissions"),
        Some(&admin_cookie),
        Some(json!({ "ids": ids })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "set role perms: {body}");

    let (status, user_body) = call(
        &app.router,
        "POST",
        "/api/system/users",
        Some(&admin_cookie),
        Some(json!({
            "username": username,
            "displayName": "RBAC 测试用户",
            "password": "RbacTest123!",
            "email": "rbac-test@example.com",
            "phone": "13812345678"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "create user: {user_body}");
    let user_id = user_body["data"]["id"].as_str().unwrap().to_string();
    let (bad_phone_status, bad_phone_body) = call(
        &app.router,
        "POST",
        "/api/system/users",
        Some(&admin_cookie),
        Some(json!({
            "username": format!("badphone_{}", &suffix[..8]),
            "displayName": "非法手机号",
            "password": "BadPhone123!",
            "phone": "123"
        })),
    )
    .await;
    assert_eq!(
        bad_phone_status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "bad phone: {bad_phone_body}"
    );
    let (status, body) = call(
        &app.router,
        "PUT",
        &format!("/api/system/users/{user_id}/roles"),
        Some(&admin_cookie),
        Some(json!({ "ids": [role_id] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "set user roles: {body}");

    let (status, user_cookie) = login(&app.router, &username, "RbacTest123!").await;
    assert_eq!(status, StatusCode::OK, "low privilege login failed");

    let cases = [
        ("GET", "/api/crm/customers?page=1&size=1", true),
        ("GET", "/api/crm/opportunities?page=1&size=1", false),
        ("GET", "/api/system/users?page=1&size=1", true),
        ("GET", "/api/system/roles", false),
        ("GET", "/api/system/audit-logs?page=1&size=1", false),
    ];
    for (method, uri, allowed) in cases {
        let (status, body) = call(&app.router, method, uri, Some(&user_cookie), None).await;
        if allowed {
            assert_eq!(status, StatusCode::OK, "{uri} 应放行: {body}");
        } else {
            assert_eq!(status, StatusCode::FORBIDDEN, "{uri} 应拒绝");
        }
        println!("{method} {uri} -> {status}");
    }

    // 字段级脱敏：低权限用户只能看到打码邮箱
    let (_, masked_body) = call(
        &app.router,
        "GET",
        &format!("/api/system/users?page=1&size=50&keyword={username}"),
        Some(&user_cookie),
        None,
    )
    .await;
    let masked = masked_body["data"]["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|u| u["username"] == username.as_str())
        .expect("masked user row");
    assert_eq!(masked["email"], "rb***@example.com");
    assert_eq!(masked["phone"], "138****5678");
    let (_, full_body) = call(
        &app.router,
        "GET",
        &format!("/api/system/users?page=1&size=50&keyword={username}"),
        Some(&admin_cookie),
        None,
    )
    .await;
    let full = full_body["data"]["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|u| u["username"] == username.as_str())
        .expect("full user row");
    assert_eq!(full["email"], "rbac-test@example.com");
    assert_eq!(full["phone"], "13812345678");

    let (ut_status, ut_body) = call(
        &app.router,
        "POST",
        "/api/auth/tokens",
        Some(&user_cookie),
        Some(json!({ "name": "user-pat" })),
    )
    .await;
    assert_eq!(ut_status, StatusCode::OK, "user token: {ut_body}");
    let user_token = ut_body["data"]["token"].as_str().unwrap().to_string();
    let (_, other_cookie) = login(&app.router, &username, "RbacTest123!").await;

    // 修改密码
    let (cp_status, cp_body) = call(
        &app.router,
        "POST",
        "/api/auth/change-password",
        Some(&user_cookie),
        Some(json!({
            "oldPassword": "RbacTest123!",
            "newPassword": "NewRbac123!",
            "revokeTokens": true
        })),
    )
    .await;
    assert_eq!(cp_status, StatusCode::OK, "change password: {cp_body}");
    let (other_me, _) = call(
        &app.router,
        "GET",
        "/api/auth/me",
        Some(&other_cookie),
        None,
    )
    .await;
    assert_eq!(other_me, StatusCode::UNAUTHORIZED);
    let (current_me, _) = call(&app.router, "GET", "/api/auth/me", Some(&user_cookie), None).await;
    assert_eq!(current_me, StatusCode::OK);
    let (token_me, _) = call_bearer(&app.router, "/api/auth/me", &user_token).await;
    assert_eq!(token_me, StatusCode::UNAUTHORIZED);
    let (old_login, _) = login(&app.router, &username, "RbacTest123!").await;
    assert_eq!(old_login, StatusCode::UNAUTHORIZED);
    let (new_login, new_cookie) = login(&app.router, &username, "NewRbac123!").await;
    assert_eq!(new_login, StatusCode::OK);
    println!("change password -> ok, new cookie len {}", new_cookie.len());
    let (reuse_status, reuse_body) = call(
        &app.router,
        "POST",
        "/api/auth/change-password",
        Some(&new_cookie),
        Some(json!({
            "oldPassword": "NewRbac123!",
            "newPassword": "RbacTest123!"
        })),
    )
    .await;
    assert_eq!(
        reuse_status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "reuse: {reuse_body}"
    );
    let (third_status, third_body) = call(
        &app.router,
        "POST",
        "/api/auth/change-password",
        Some(&new_cookie),
        Some(json!({
            "oldPassword": "NewRbac123!",
            "newPassword": "ThirdRbac123!"
        })),
    )
    .await;
    assert_eq!(third_status, StatusCode::OK, "third password: {third_body}");

    // 文件可见性：上传者或 system:file:list 才能读取
    let tenant: sqlx::types::Uuid = sqlx::query_scalar("SELECT id FROM sys.sys_tenant LIMIT 1")
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    let file_id: sqlx::types::Uuid = {
        let mut tx = app.state.db.begin().await.unwrap();
        sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
            .bind(tenant.to_string())
            .execute(&mut *tx)
            .await
            .unwrap();
        let id: sqlx::types::Uuid = sqlx::query_scalar(
            "INSERT INTO sys.sys_file
                (id, tenant_id, name, mime, size_bytes, storage_key, uploader_id)
             VALUES (gen_random_uuid(), $1, 'rbac.txt', 'text/plain', 1, $2, $3)
             RETURNING id",
        )
        .bind(tenant)
        .bind(format!("rbac-{}", suffix))
        .bind(sqlx::types::Uuid::parse_str(&user_id).unwrap())
        .fetch_one(&mut *tx)
        .await
        .unwrap();
        tx.commit().await.unwrap();
        id
    };
    let (meta_status, meta_body) = call(
        &app.router,
        "GET",
        &format!("/api/system/files/{file_id}"),
        Some(&new_cookie),
        None,
    )
    .await;
    assert_eq!(meta_status, StatusCode::OK, "uploader meta: {meta_body}");
    let (admin_meta, _) = call(
        &app.router,
        "GET",
        &format!("/api/system/files/{file_id}"),
        Some(&admin_cookie),
        None,
    )
    .await;
    assert_eq!(admin_meta, StatusCode::OK);
    let (tok_status, tok_body) = call(
        &app.router,
        "POST",
        "/api/auth/tokens",
        Some(&admin_cookie),
        Some(json!({ "name": "file-scope", "scopes": ["system:user:list"] })),
    )
    .await;
    assert_eq!(tok_status, StatusCode::OK);
    let file_token = tok_body["data"]["token"].as_str().unwrap().to_string();
    let file_token_id = tok_body["data"]["tokenInfo"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let (denied, _) = call_bearer(
        &app.router,
        &format!("/api/system/files/{file_id}"),
        &file_token,
    )
    .await;
    assert_eq!(denied, StatusCode::NOT_FOUND);
    let _ = call(
        &app.router,
        "DELETE",
        &format!("/api/auth/tokens/{file_token_id}"),
        Some(&admin_cookie),
        None,
    )
    .await;

    // 首次登录强制改密
    let must_username = format!("must_{}", &suffix[..8]);
    let (mu_status, mu_body) = call(
        &app.router,
        "POST",
        "/api/system/users",
        Some(&admin_cookie),
        Some(json!({
            "username": must_username,
            "displayName": "强制改密用户",
            "password": "MustTest123!",
            "mustChangePassword": true
        })),
    )
    .await;
    assert_eq!(mu_status, StatusCode::OK, "create must user: {mu_body}");
    let must_id = mu_body["data"]["id"].as_str().unwrap().to_string();
    let (mu_login, mu_cookie) = login(&app.router, &must_username, "MustTest123!").await;
    assert_eq!(mu_login, StatusCode::OK);
    let (blocked, _) = call(
        &app.router,
        "GET",
        "/api/system/menus",
        Some(&mu_cookie),
        None,
    )
    .await;
    assert_eq!(blocked, StatusCode::FORBIDDEN);
    let (me_ok, _) = call(&app.router, "GET", "/api/auth/me", Some(&mu_cookie), None).await;
    assert_eq!(me_ok, StatusCode::OK);
    let (mp_status, mp_body) = call(
        &app.router,
        "POST",
        "/api/auth/change-password",
        Some(&mu_cookie),
        Some(json!({
            "oldPassword": "MustTest123!",
            "newPassword": "MustNew123!"
        })),
    )
    .await;
    assert_eq!(mp_status, StatusCode::OK, "must change password: {mp_body}");
    let (mu_login2, mu_cookie2) = login(&app.router, &must_username, "MustNew123!").await;
    assert_eq!(mu_login2, StatusCode::OK);
    let (menus_ok, _) = call(
        &app.router,
        "GET",
        "/api/system/menus",
        Some(&mu_cookie2),
        None,
    )
    .await;
    assert_eq!(menus_ok, StatusCode::OK);

    // 密码过期 → 强制改密
    let (ep_status, ep_body) = call(
        &app.router,
        "PUT",
        "/api/system/password-policy",
        Some(&admin_cookie),
        Some(json!({
            "minLength": 10,
            "requireUpper": true,
            "requireLower": true,
            "requireDigit": true,
            "historyCount": 5,
            "expiresDays": 1
        })),
    )
    .await;
    assert_eq!(ep_status, StatusCode::OK, "policy expiry: {ep_body}");
    {
        let mut tx = app.state.db.begin().await.unwrap();
        sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
            .bind(tenant.to_string())
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query(
            "UPDATE sys.sys_user
             SET password_changed_at = now() - interval '2 days', must_change_password = false
             WHERE id = $1",
        )
        .bind(sqlx::types::Uuid::parse_str(&user_id).unwrap())
        .execute(&mut *tx)
        .await
        .unwrap();
        tx.commit().await.unwrap();
    }
    let (exp_login, exp_cookie) = login(&app.router, &username, "ThirdRbac123!").await;
    assert_eq!(exp_login, StatusCode::OK);
    let (me_status, me_body) =
        call(&app.router, "GET", "/api/auth/me", Some(&exp_cookie), None).await;
    assert_eq!(me_status, StatusCode::OK);
    assert_eq!(me_body["data"]["mustChangePassword"], true);
    let (exp_blocked, _) = call(
        &app.router,
        "GET",
        "/api/system/menus",
        Some(&exp_cookie),
        None,
    )
    .await;
    assert_eq!(exp_blocked, StatusCode::FORBIDDEN);
    let (exp_change, exp_change_body) = call(
        &app.router,
        "POST",
        "/api/auth/change-password",
        Some(&exp_cookie),
        Some(json!({
            "oldPassword": "ThirdRbac123!",
            "newPassword": "ExpiredNew123!"
        })),
    )
    .await;
    assert_eq!(
        exp_change,
        StatusCode::OK,
        "expired change: {exp_change_body}"
    );
    let (exp_login2, exp_cookie2) = login(&app.router, &username, "ExpiredNew123!").await;
    assert_eq!(exp_login2, StatusCode::OK);
    let (exp_menus, _) = call(
        &app.router,
        "GET",
        "/api/system/menus",
        Some(&exp_cookie2),
        None,
    )
    .await;
    assert_eq!(exp_menus, StatusCode::OK);

    // 恢复策略与用户状态
    let _ = call(
        &app.router,
        "PUT",
        "/api/system/password-policy",
        Some(&admin_cookie),
        Some(json!({
            "minLength": 10,
            "requireUpper": true,
            "requireLower": true,
            "requireDigit": true,
            "historyCount": 5,
            "expiresDays": 0
        })),
    )
    .await;

    // 新设备登录告警（User-Agent 变化）
    let req = Request::builder()
        .method("POST")
        .uri("/api/auth/login")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::USER_AGENT, "rbac-device-B")
        .body(Body::from(
            json!({
                "username": username,
                "password": "ExpiredNew123!",
                "tenant": null
            })
            .to_string(),
        ))
        .unwrap();
    let resp = app.router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let device_alerts: i64 = {
        let mut tx = app.state.db.begin().await.unwrap();
        sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
            .bind(tenant.to_string())
            .execute(&mut *tx)
            .await
            .unwrap();
        let n: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM sys.sys_audit_alert
             WHERE tenant_id = $1 AND action = 'auth.login.new_device'
               AND created_at >= now() - interval '5 minutes'",
        )
        .bind(tenant)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
        tx.commit().await.unwrap();
        n
    };
    assert!(device_alerts >= 1, "new device alert missing");

    // 连续失败触发账号锁定告警
    for _ in 0..5 {
        let (failed, _) = login(&app.router, &username, "WrongPass123!").await;
        assert_eq!(failed, StatusCode::UNAUTHORIZED);
    }
    let lock_alerts: i64 = {
        let mut tx = app.state.db.begin().await.unwrap();
        sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
            .bind(tenant.to_string())
            .execute(&mut *tx)
            .await
            .unwrap();
        let n: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM sys.sys_audit_alert
             WHERE tenant_id = $1 AND action = 'auth.login.locked'
               AND created_at >= now() - interval '5 minutes'",
        )
        .bind(tenant)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
        tx.commit().await.unwrap();
        n
    };
    assert!(lock_alerts >= 1, "lock alert missing");
    let warn_alerts: i64 = {
        let mut tx = app.state.db.begin().await.unwrap();
        sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
            .bind(tenant.to_string())
            .execute(&mut *tx)
            .await
            .unwrap();
        let n: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM sys.sys_audit_alert
             WHERE tenant_id = $1 AND action = 'auth.login.failed.warning'
               AND created_at >= now() - interval '5 minutes'",
        )
        .bind(tenant)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
        tx.commit().await.unwrap();
        n
    };
    assert!(warn_alerts >= 1, "warning alert missing");
    let (unlock_status, unlock_body) = call(
        &app.router,
        "POST",
        &format!("/api/system/users/{user_id}/unlock"),
        Some(&admin_cookie),
        None,
    )
    .await;
    assert_eq!(unlock_status, StatusCode::OK, "unlock: {unlock_body}");
    let (relogin, fresh_cookie) = login(&app.router, &username, "ExpiredNew123!").await;
    assert_eq!(relogin, StatusCode::OK);

    // 管理员重置密码必须吊销该用户的 PAT：令牌是独立凭据，不随会话失效。
    // 少了这一步，「重置密码」只换掉了密码，攻击者手里的令牌在事件响应后依然有效。
    let (pt_status, pt_body) = call(
        &app.router,
        "POST",
        "/api/auth/tokens",
        Some(&fresh_cookie),
        Some(json!({ "name": "reset-revoke-pat", "expiresInDays": 1 })),
    )
    .await;
    assert_eq!(pt_status, StatusCode::OK, "create pat: {pt_body}");
    let reset_token = pt_body["data"]["token"].as_str().unwrap().to_string();
    let (before_reset, _) = call_bearer(&app.router, "/api/auth/me", &reset_token).await;
    assert_eq!(before_reset, StatusCode::OK, "PAT 在重置前应可用");

    let (rp_status, rp_body) = call(
        &app.router,
        "POST",
        &format!("/api/system/users/{user_id}/reset-password"),
        Some(&admin_cookie),
        Some(json!({ "password": "ResetRbac123!" })),
    )
    .await;
    assert_eq!(rp_status, StatusCode::OK, "reset password: {rp_body}");
    let (after_reset, _) = call_bearer(&app.router, "/api/auth/me", &reset_token).await;
    assert_eq!(
        after_reset,
        StatusCode::UNAUTHORIZED,
        "管理员重置密码后，该用户此前签发的 PAT 必须失效"
    );
    let (session_after_reset, _) = call(
        &app.router,
        "GET",
        "/api/auth/me",
        Some(&fresh_cookie),
        None,
    )
    .await;
    assert_eq!(
        session_after_reset,
        StatusCode::UNAUTHORIZED,
        "管理员重置密码后，该用户的会话必须失效"
    );
    println!("admin reset password -> PAT + sessions revoked");

    // 清理测试数据
    let tenant: sqlx::types::Uuid = sqlx::query_scalar("SELECT id FROM sys.sys_tenant LIMIT 1")
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    let mut tx = app.state.db.begin().await.unwrap();
    sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
        .bind(tenant.to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("DELETE FROM sys.sys_file WHERE id = $1")
        .bind(file_id)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("DELETE FROM sys.sys_user_role WHERE user_id = $1")
        .bind(sqlx::types::Uuid::parse_str(&user_id).unwrap())
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("DELETE FROM sys.sys_user_role WHERE user_id = $1")
        .bind(sqlx::types::Uuid::parse_str(&must_id).unwrap())
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("DELETE FROM sys.sys_user WHERE id = $1")
        .bind(sqlx::types::Uuid::parse_str(&must_id).unwrap())
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("DELETE FROM sys.sys_user WHERE id = $1")
        .bind(sqlx::types::Uuid::parse_str(&user_id).unwrap())
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("DELETE FROM sys.sys_role WHERE id = $1")
        .bind(sqlx::types::Uuid::parse_str(&role_id).unwrap())
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    println!("rbac_role: passed");
}
