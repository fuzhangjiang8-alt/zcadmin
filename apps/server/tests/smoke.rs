//! 真库冒烟测试（不依赖 HTTP 端口，直接调用 axum Router）。
//!
//! 为什么不用 curl：Windows 防火墙可能拦截本地监听端口，
//! `tower::ServiceExt::oneshot` 直接走内存调用，验证的仍是完整的中间件链。
//!
//! 运行：
//! ```powershell
//! $env:DATABASE_URL="postgres://zcadmin:zcadmin@127.0.0.1:5432/zcadmin"
//! cargo test -p server --test smoke -- --ignored --nocapture
//! ```
//!
//! 未设置 DATABASE_URL 时测试会直接 panic（`--ignored` 才运行，不影响 CI 默认流程）。

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use serde_json::{json, Value};
use std::time::{SystemTime, UNIX_EPOCH};
use tower::ServiceExt;

// 强制链接插件 crate，让 inventory 能发现它们（与 main.rs 同理）
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

/// 在告警列表里按 `action` 精确查找告警 id。
///
/// # 为什么要有界翻页
///
/// 开发库会累积历史告警（反复跑测试、其他用例也会写入）。如果只看第一页，
/// 新告警会随着历史数据增长被挤出窗口，表现为「代码没坏但测试红了」。
/// 这里按 `created_at DESC` 翻页查找，最多 20 页（200 条）——足够覆盖本次运行，
/// 又不会在异常情况下无限循环。
///
/// 同时保留了对**列表接口**的覆盖（而不是直接查库），因为它本身也在被测范围内。
async fn find_alert_id(router: &axum::Router, cookie: &str, action: &str) -> String {
    const PAGE_SIZE: usize = 10;
    const MAX_PAGES: usize = 20;

    for page in 1..=MAX_PAGES {
        let uri = format!("/api/system/audit-alerts?page={page}&size={PAGE_SIZE}");
        let (status, body) = call(router, "GET", &uri, Some(cookie), None).await;
        assert_eq!(status, StatusCode::OK, "audit alerts failed: {body}");

        let Some(items) = body["data"]["data"].as_array() else {
            panic!("告警列表结构异常: {body}");
        };
        if let Some(id) = items
            .iter()
            .find(|a| a["action"].as_str() == Some(action))
            .and_then(|a| a["id"].as_str())
        {
            return id.to_string();
        }
        // 数据不足一页说明已经到底，不必继续翻
        if items.len() < PAGE_SIZE {
            break;
        }
    }
    panic!("告警未生成：在前 {MAX_PAGES} 页中找不到 action={action}");
}

async fn call_bearer(router: &axum::Router, uri: &str, token: &str) -> (StatusCode, Value) {
    let request = Request::builder()
        .method("GET")
        .uri(uri)
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
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

#[tokio::test]
#[ignore = "需要真实 PostgreSQL（DATABASE_URL）"]
async fn smoke_end_to_end() {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .try_init();
    let url = std::env::var("DATABASE_URL").expect("必须设置 DATABASE_URL");
    std::env::set_var("DATABASE_URL", &url);

    let password = std::env::var("AUTH_DEV_PASSWORD").unwrap_or_else(|_| "admin123456".into());
    let config = kernel::config::Config::load("config/default.toml").expect("config");
    let app = kernel::app::build(config).await.expect("build app");

    // 1. 登录（一次性拿到 Cookie 与响应体）
    let login = Request::builder()
        .method("POST")
        .uri("/api/auth/login")
        .header(header::CONTENT_TYPE, "application/json")
        .header("x-forwarded-for", "203.0.113.9, 10.0.0.1")
        .header(header::USER_AGENT, "smoke-test-agent")
        .body(Body::from(
            json!({ "username": "admin", "password": password, "tenant": "default" }).to_string(),
        ))
        .unwrap();
    let resp = app.router.clone().oneshot(login).await.expect("login");
    assert_eq!(resp.status(), StatusCode::OK, "login failed");
    let cookie = resp
        .headers()
        .get(header::SET_COOKIE)
        .expect("set-cookie")
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();

    let (status, body) = call(
        &app.router,
        "GET",
        "/api/system/audit-logs?action=auth.login.success&page=1&size=5",
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "login audit failed: {body}");
    let login_row = &body["data"]["data"][0];
    assert_eq!(login_row["detail"]["ip"], "203.0.113.9");
    assert_eq!(login_row["detail"]["userAgent"], "smoke-test-agent");

    // 第二个会话：用于验证在线会话列表与强制下线
    let login2 = Request::builder()
        .method("POST")
        .uri("/api/auth/login")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({ "username": "admin", "password": password, "tenant": "default" }).to_string(),
        ))
        .unwrap();
    let resp2 = app.router.clone().oneshot(login2).await.expect("login2");
    assert_eq!(resp2.status(), StatusCode::OK, "second login failed");
    let cookie2 = resp2
        .headers()
        .get(header::SET_COOKIE)
        .expect("set-cookie2")
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();

    let (status, body) = call(
        &app.router,
        "GET",
        "/api/system/sessions",
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "sessions failed: {body}");
    let sessions = body["data"].as_array().unwrap();
    assert!(sessions.len() >= 2, "expected 2 sessions: {body}");
    assert!(
        sessions
            .iter()
            .any(|s| s["ip"] == "203.0.113.9" && s["userAgent"] == "smoke-test-agent"),
        "session ip/ua missing: {body}"
    );
    let session_id = sessions[0]["id"].as_str().expect("session id").to_string();
    let (status, body) = call(
        &app.router,
        "DELETE",
        &format!("/api/system/sessions/{session_id}"),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "revoke session failed: {body}");
    let (status, _) = call(&app.router, "GET", "/api/auth/me", Some(&cookie2), None).await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "revoked session still valid"
    );

    // 2. 身份
    let (status, body) = call(&app.router, "GET", "/api/auth/me", Some(&cookie), None).await;
    assert_eq!(status, StatusCode::OK, "me failed: {body}");
    assert!(body["data"]["permissions"].is_array());

    // 3. 系统读接口
    for path in [
        "/api/system/users?page=1&size=5",
        "/api/system/roles",
        "/api/system/depts",
        "/api/system/dicts",
        "/api/system/configs",
        "/api/system/notices",
        "/api/system/files",
        "/api/system/audit-logs?page=1&size=5",
        "/api/system/audit-logs?page=1&size=5&from=2020-01-01T00:00:00Z&to=2030-01-01T00:00:00Z",
    ] {
        let (status, body) = call(&app.router, "GET", path, Some(&cookie), None).await;
        assert_eq!(status, StatusCode::OK, "GET {path} failed: {body}");
    }

    // demo 插件：权限校验 + CRUD + OpenAPI 挂载
    let (demo_list, demo_list_body) = call(
        &app.router,
        "GET",
        "/api/demo/items?page=1&size=5",
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(demo_list, StatusCode::OK, "demo list: {demo_list_body}");
    let (demo_create, demo_create_body) = call(
        &app.router,
        "POST",
        "/api/demo/items",
        Some(&cookie),
        Some(json!({ "name": "smoke-demo", "note": "ci" })),
    )
    .await;
    assert_eq!(
        demo_create,
        StatusCode::OK,
        "demo create: {demo_create_body}"
    );
    let demo_id = demo_create_body["data"]["id"].as_str().unwrap().to_string();
    let (demo_get, _) = call(
        &app.router,
        "GET",
        &format!("/api/demo/items/{demo_id}"),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(demo_get, StatusCode::OK);
    let (demo_del, _) = call(
        &app.router,
        "DELETE",
        &format!("/api/demo/items/{demo_id}"),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(demo_del, StatusCode::OK);

    // 审计导出（带时间过滤）
    let (ex_status, ex_body) = call(
        &app.router,
        "POST",
        "/api/system/export-tasks",
        Some(&cookie),
        Some(json!({
            "kind": "system.audit",
            "keyword": null,
            "resourceType": null,
            "ip": null,
            "userAgent": null,
            "from": "2020-01-01T00:00:00Z",
            "to": "2030-01-01T00:00:00Z"
        })),
    )
    .await;
    assert_eq!(ex_status, StatusCode::OK, "create export: {ex_body}");
    let export_id = ex_body["data"]["id"].as_str().unwrap().to_string();
    let mut export_ok = false;
    let mut export_filters_ok = false;
    for _ in 0..50 {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let (_, page) = call(
            &app.router,
            "GET",
            "/api/system/export-tasks?page=1&size=20",
            Some(&cookie),
            None,
        )
        .await;
        let tasks = page["data"]["data"].as_array().unwrap();
        if let Some(task) = tasks.iter().find(|t| t["id"].as_str() == Some(&export_id)) {
            match task["status"].as_str() {
                Some("success") => {
                    export_ok = true;
                    export_filters_ok = task["filters"]["from"].is_string();
                    break;
                }
                Some("failed") => panic!("export failed: {task}"),
                _ => {}
            }
        }
    }
    assert!(export_ok, "export task did not finish");
    assert!(export_filters_ok, "export filters not persisted");

    // 4. CRM 写读删闭环
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis();
    let code = format!("SMOKE-{suffix}");
    let (status, body) = call(
        &app.router,
        "POST",
        "/api/crm/customers",
        Some(&cookie),
        Some(json!({ "code": code, "name": "Smoke Test", "level": "normal" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "create customer failed: {body}");
    let id = body["data"]["id"]
        .as_str()
        .expect("customer id")
        .to_string();

    let (status, body) = call(
        &app.router,
        "PATCH",
        &format!("/api/crm/customers/{id}"),
        Some(&cookie),
        Some(json!({ "name": "Smoke Test Updated" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "update customer failed: {body}");

    let (audit_status, audit_body) = call(
        &app.router,
        "GET",
        "/api/system/audit-logs?action=crm.customer.update&page=1&size=20",
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(
        audit_status,
        StatusCode::OK,
        "customer audit query failed: {audit_body}"
    );
    let row = audit_body["data"]["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["resourceId"] == id.as_str())
        .expect("customer update audit");
    assert_eq!(row["detail"]["before"]["name"], "Smoke Test");
    assert_eq!(row["detail"]["after"]["name"], "Smoke Test Updated");

    let (status, body) = call(
        &app.router,
        "DELETE",
        &format!("/api/crm/customers/{id}"),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "delete customer failed: {body}");

    // 经营看板
    let (status, body) = call(
        &app.router,
        "GET",
        "/api/crm/dashboard/summary",
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "dashboard summary failed: {body}");
    assert!(body["data"]["months"].as_array().unwrap().len() == 6);

    let (status, body) = call(
        &app.router,
        "GET",
        "/api/crm/dashboard/top-customers?limit=5",
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "top customers failed: {body}");
    assert!(body["data"].as_array().is_some());

    // CRM 回款：建客户 → 合同 → 回款 → 汇总 → 清理
    let (status, body) = call(
        &app.router,
        "POST",
        "/api/crm/customers",
        Some(&cookie),
        Some(json!({ "code": format!("SMOKE-P-{suffix}"), "name": "Payment Host", "level": "normal" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "create payment host failed: {body}");
    let pay_host = body["data"]["id"].as_str().expect("pay host").to_string();

    let (status, body) = call(
        &app.router,
        "POST",
        "/api/crm/contracts",
        Some(&cookie),
        Some(json!({
            "customerId": pay_host,
            "contractNo": format!("HT-P-{suffix}"),
            "name": "Payment Contract",
            "amountCents": 1000000,
            "status": "active"
        })),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "create payment contract failed: {body}"
    );
    let pay_contract = body["data"]["id"]
        .as_str()
        .expect("pay contract")
        .to_string();

    let (status, body) = call(
        &app.router,
        "POST",
        "/api/crm/payments",
        Some(&cookie),
        Some(json!({
            "contractId": pay_contract,
            "paymentNo": format!("HK-{suffix}"),
            "amountCents": 400000,
            "paidAt": "2026-10-30T00:00:00Z",
            "method": "transfer"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "create payment failed: {body}");
    let payment_id = body["data"]["id"].as_str().expect("payment id").to_string();

    let (status, body) = call(
        &app.router,
        "GET",
        "/api/crm/payments?page=1&size=10&paidFrom=2000-01-01&paidTo=2100-01-01",
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "payment list failed: {body}");
    assert!(
        body["data"]["data"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["id"] == payment_id.as_str()),
        "payment date filter missing payment: {body}"
    );

    let (status, body) = call(
        &app.router,
        "GET",
        &format!("/api/crm/contracts/{pay_contract}/payments"),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "contract payments failed: {body}");
    assert_eq!(body["data"]["paidCents"], 400000);
    assert_eq!(body["data"]["remainingCents"], 600000);
    assert_eq!(body["data"]["total"], 1);

    // 回款导入：xlsx → /api/system/imports/crm.payments
    let mut wb = rust_xlsxwriter::Workbook::new();
    {
        let sheet = wb.add_worksheet();
        for (i, h) in ["回款单号", "合同编号", "金额(元)", "回款时间", "方式"]
            .iter()
            .enumerate()
        {
            sheet.write_string(0, i as u16, *h).unwrap();
        }
        sheet
            .write_string(1, 0, format!("HK-IMP-{suffix}"))
            .unwrap();
        sheet.write_string(1, 1, format!("HT-P-{suffix}")).unwrap();
        sheet.write_string(1, 2, "100.50").unwrap();
        sheet.write_string(1, 3, "2026-10-30").unwrap();
        sheet.write_string(1, 4, "transfer").unwrap();
    }
    let xlsx = wb.save_to_buffer().unwrap();
    let boundary = "----zcadmin-payment-import";
    let mut mp = Vec::new();
    mp.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"payments.xlsx\"\r\nContent-Type: application/vnd.openxmlformats-officedocument.spreadsheetml.sheet\r\n\r\n"
        )
        .as_bytes(),
    );
    mp.extend_from_slice(&xlsx);
    mp.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    let req = Request::builder()
        .method("POST")
        .uri("/api/system/imports/crm.payments")
        .header(header::COOKIE, &cookie)
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(mp))
        .unwrap();
    let resp = app
        .router
        .clone()
        .oneshot(req)
        .await
        .expect("payment import");
    assert_eq!(resp.status(), StatusCode::OK, "crm.payments import failed");
    let body: Value = serde_json::from_slice(
        &axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(body["data"]["success"], 1, "payment import result: {body}");
    assert_eq!(
        body["data"]["failed"], 0,
        "payment import failed rows: {body}"
    );

    let (status, body) = call(
        &app.router,
        "DELETE",
        &format!("/api/crm/payments/{payment_id}"),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "delete payment failed: {body}");

    let (status, body) = call(
        &app.router,
        "DELETE",
        &format!("/api/crm/contracts/{pay_contract}"),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "delete payment contract failed: {body}"
    );

    let (status, body) = call(
        &app.router,
        "DELETE",
        &format!("/api/crm/customers/{pay_host}"),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "delete payment host failed: {body}");

    // CRM 合同：建客户 → 建合同 → 改状态 → 列表 → 删 → 删客户
    let (status, body) = call(
        &app.router,
        "POST",
        "/api/crm/customers",
        Some(&cookie),
        Some(json!({ "code": format!("SMOKE-CT-{suffix}"), "name": "Contract Host", "level": "normal" })),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "create contract host failed: {body}"
    );
    let ct_host = body["data"]["id"].as_str().expect("ct host").to_string();

    let (status, body) = call(
        &app.router,
        "POST",
        "/api/crm/contracts",
        Some(&cookie),
        Some(json!({
            "customerId": ct_host,
            "contractNo": format!("HT-{suffix}"),
            "name": "Smoke Contract",
            "amountCents": 5000000,
            "status": "draft"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "create contract failed: {body}");

    // 业务写操作应进入审计日志
    let (audit_status, audit_body) = call(
        &app.router,
        "GET",
        "/api/system/audit-logs?page=1&size=50",
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(
        audit_status,
        StatusCode::OK,
        "audit logs failed: {audit_body}"
    );
    let has_audit = audit_body["data"]["data"]
        .as_array()
        .unwrap_or_else(|| panic!("audit body: {audit_body}"))
        .iter()
        .any(|x| x["action"] == "crm.contract.create");
    assert!(has_audit, "missing crm.contract.create audit: {audit_body}");

    let (audit_status, audit_body) = call(
        &app.router,
        "GET",
        "/api/system/audit-logs?resourceType=contract&page=1&size=20",
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(
        audit_status,
        StatusCode::OK,
        "audit filter failed: {audit_body}"
    );
    let all_contracts = audit_body["data"]["data"]
        .as_array()
        .unwrap()
        .iter()
        .all(|x| x["resourceType"] == "contract");
    assert!(
        all_contracts,
        "audit filter returned other types: {audit_body}"
    );

    let (actions_status, actions_body) = call(
        &app.router,
        "GET",
        "/api/system/audit-logs/actions",
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(
        actions_status,
        StatusCode::OK,
        "audit actions failed: {actions_body}"
    );
    assert!(actions_body["data"].as_array().is_some());

    let (stats_status, stats_body) = call(
        &app.router,
        "GET",
        "/api/system/audit-logs/stats?days=30",
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(
        stats_status,
        StatusCode::OK,
        "audit stats failed: {stats_body}"
    );
    assert!(stats_body["data"]["total"].as_i64().unwrap_or(0) >= 1);
    assert!(stats_body["data"]["byAction"].as_array().is_some());

    let (cfg_status, cfg_body) = call(
        &app.router,
        "GET",
        "/api/system/audit-alert-config",
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(
        cfg_status,
        StatusCode::OK,
        "alert config failed: {cfg_body}"
    );
    assert_eq!(cfg_body["data"]["windowMinutes"], 5);
    let (cfg_status, cfg_body) = call(
        &app.router,
        "PUT",
        "/api/system/audit-alert-config",
        Some(&cookie),
        Some(json!({ "windowMinutes": 5, "threshold": 50 })),
    )
    .await;
    assert_eq!(
        cfg_status,
        StatusCode::OK,
        "update alert config failed: {cfg_body}"
    );
    assert_eq!(cfg_body["data"]["threshold"], 50);

    let (rules_status, rules_body) = call(
        &app.router,
        "GET",
        "/api/system/audit-alert-rules",
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(
        rules_status,
        StatusCode::OK,
        "alert rules failed: {rules_body}"
    );
    let rules = rules_body["data"].as_array().unwrap();
    assert!(rules.iter().any(|r| r["actionPattern"] == "*"));
    assert!(rules
        .iter()
        .any(|r| r["actionPattern"] == "auth.login.failed"));

    let (save_status, save_body) = call(
        &app.router,
        "PUT",
        "/api/system/audit-alert-rules",
        Some(&cookie),
        Some(json!({ "rules": rules_body["data"].clone() })),
    )
    .await;
    assert_eq!(
        save_status,
        StatusCode::OK,
        "save alert rules failed: {save_body}"
    );
    assert!(save_body["data"].as_array().is_some());

    let preview_action = format!("preview.{suffix}");
    let (pv_status, pv_body) = call(
        &app.router,
        "POST",
        "/api/system/audit-alert-rules/preview",
        Some(&cookie),
        Some(json!({ "action": preview_action, "count": 60 })),
    )
    .await;
    assert_eq!(pv_status, StatusCode::OK, "rule preview failed: {pv_body}");
    assert_eq!(pv_body["data"]["wouldAlert"], true);
    assert_eq!(pv_body["data"]["matchedPattern"], "*");

    let (pv_status, pv_body) = call(
        &app.router,
        "POST",
        "/api/system/audit-alert-rules/preview",
        Some(&cookie),
        Some(json!({ "action": preview_action, "count": 1 })),
    )
    .await;
    assert_eq!(pv_status, StatusCode::OK, "rule preview failed: {pv_body}");
    assert_eq!(pv_body["data"]["wouldAlert"], false);

    let (hook_status, hook_body) = call(
        &app.router,
        "GET",
        "/api/system/audit-webhook",
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(
        hook_status,
        StatusCode::OK,
        "get webhook failed: {hook_body}"
    );
    let (hook_status, hook_body) = call(
        &app.router,
        "PUT",
        "/api/system/audit-webhook",
        Some(&cookie),
        Some(json!({
            "url": "https://example.com/hook",
            "enabled": false,
            "secret": "smoke-secret",
            "payloadTemplate": "{\"text\":\"{{action}} x{{count}}\"}"
        })),
    )
    .await;
    assert_eq!(
        hook_status,
        StatusCode::OK,
        "update webhook failed: {hook_body}"
    );
    assert_eq!(hook_body["data"]["hasSecret"], true);
    assert_eq!(
        hook_body["data"]["payloadTemplate"],
        "{\"text\":\"{{action}} x{{count}}\"}"
    );

    // 会话缓存指标
    let metrics_req = Request::builder()
        .method("GET")
        .uri("/metrics")
        .body(Body::empty())
        .unwrap();
    let metrics_resp = app.router.clone().oneshot(metrics_req).await.unwrap();
    let metrics_bytes = axum::body::to_bytes(metrics_resp.into_body(), 2 * 1024 * 1024)
        .await
        .unwrap();
    let metrics_text = String::from_utf8_lossy(&metrics_bytes);
    assert!(
        metrics_text.contains("session_cache_hits_total"),
        "metrics missing cache hits: {metrics_text}"
    );
    assert!(
        metrics_text.contains("session_cache_misses_total"),
        "metrics missing cache misses"
    );

    // 审计告警：制造 50 条同动作日志 → 扫描 → 应生成告警
    //
    // 必须与登录会话属于**同一个租户**：`LIMIT 1` 没有 ORDER BY 时，
    // 多租户库里取到哪一行是不确定的（本测试曾经因此写进隔离测试租户，
    // 而查询用的是 default 租户的 cookie，表现为"告警没生成"）。
    // 这里显式取 default 租户——与上面的登录一致。
    let tenant_id: sqlx::types::Uuid =
        sqlx::query_scalar("SELECT id FROM sys.sys_tenant WHERE code = 'default'")
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    let mut alert_tx = app.state.db.begin().await.unwrap();
    let burst_action = format!("smoke.alert.{suffix}");
    sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *alert_tx)
        .await
        .unwrap();
    for _ in 0..50 {
        sqlx::query(
            "INSERT INTO sys.sys_audit_log
                (id, tenant_id, user_id, action, resource_type, resource_id, result, detail)
             VALUES (gen_random_uuid(), $1, NULL, $2, 'test', NULL, 'success', NULL)",
        )
        .bind(tenant_id)
        .bind(&burst_action)
        .execute(&mut *alert_tx)
        .await
        .unwrap();
    }
    alert_tx.commit().await.unwrap();
    kernel::audit::detect_alerts(&app.state.db).await.unwrap();

    // 按 action 精确查找，而不是只看第一页。
    //
    // 为什么：开发库会累积历史告警（反复跑测试、其他用例也会产生），
    // 「第一页 size=10」会随着历史数据增长把新告警挤出窗口，
    // 表现为「代码没坏但测试红了」。
    // 这里沿用列表接口（保留接口覆盖），但有界翻页，与库中数据量解耦。
    let alert_id = find_alert_id(&app.router, &cookie, &burst_action).await;
    let (ack_status, ack_body) = call(
        &app.router,
        "POST",
        &format!("/api/system/audit-alerts/{alert_id}/ack"),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(ack_status, StatusCode::OK, "ack alert failed: {ack_body}");

    // 死信重投
    let dead_id: sqlx::types::Uuid = {
        let mut tx = app.state.db.begin().await.unwrap();
        sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
            .bind(tenant_id.to_string())
            .execute(&mut *tx)
            .await
            .unwrap();
        let id: sqlx::types::Uuid = sqlx::query_scalar(
            "INSERT INTO sys.sys_audit_alert
                (id, tenant_id, action, window_start, window_end, count, threshold,
                 notify_attempts, notify_failed_at)
             VALUES (gen_random_uuid(), $1, 'dead.letter.test', now() - interval '5 minutes',
                     now(), 10, 5, 8, now())
             RETURNING id",
        )
        .bind(tenant_id)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
        tx.commit().await.unwrap();
        id
    };
    let (retry_status, retry_body) = call(
        &app.router,
        "POST",
        &format!("/api/system/audit-alerts/{dead_id}/retry"),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(
        retry_status,
        StatusCode::OK,
        "retry dead letter failed: {retry_body}"
    );
    let mut tx = app.state.db.begin().await.unwrap();
    sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    let (reset, attempts): (bool, i32) = sqlx::query_as(
        "SELECT notify_failed_at IS NULL, notify_attempts FROM sys.sys_audit_alert WHERE id = $1",
    )
    .bind(dead_id)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    assert!(reset);
    assert_eq!(attempts, 0);

    let (sub_status, sub_body) = call(
        &app.router,
        "GET",
        "/api/system/audit-alert-subscriptions",
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(
        sub_status,
        StatusCode::OK,
        "subscriptions failed: {sub_body}"
    );
    let (sub_status, sub_body) = call(
        &app.router,
        "PUT",
        "/api/system/audit-alert-subscriptions",
        Some(&cookie),
        Some(json!({ "patterns": ["auth.login.failed", "*"] })),
    )
    .await;
    assert_eq!(
        sub_status,
        StatusCode::OK,
        "save subscriptions failed: {sub_body}"
    );
    assert_eq!(sub_body["data"].as_array().unwrap().len(), 2);
    let (subs_status, subs_body) = call(
        &app.router,
        "GET",
        "/api/system/audit-alerts/subscribed?page=1&size=10",
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(
        subs_status,
        StatusCode::OK,
        "subscribed alerts failed: {subs_body}"
    );
    assert!(subs_body["data"]["data"].as_array().is_some());

    // 只订阅本轮唯一动作，避免为历史告警创建大量投递
    let _ = call(
        &app.router,
        "PUT",
        "/api/system/audit-alert-subscriptions",
        Some(&cookie),
        Some(json!({ "patterns": [burst_action] })),
    )
    .await;

    let (cfg_status, cfg_body) = call(
        &app.router,
        "PUT",
        "/api/system/audit-alert-subscription-config",
        Some(&cookie),
        Some(json!({
            "url": "http://127.0.0.1:1/hook",
            "enabled": true,
            "secret": "sub-secret",
            "payloadTemplate": "{\"text\":\"{{action}} x{{count}}\"}"
        })),
    )
    .await;
    assert_eq!(
        cfg_status,
        StatusCode::OK,
        "subscription webhook config failed: {cfg_body}"
    );
    assert_eq!(
        cfg_body["data"]["payloadTemplate"],
        "{\"text\":\"{{action}} x{{count}}\"}"
    );
    let _ = kernel::audit::dispatch_subscriptions(&app.state.db)
        .await
        .unwrap();
    let delivery_count: i64 = {
        let mut tx = app.state.db.begin().await.unwrap();
        sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
            .bind(tenant_id.to_string())
            .execute(&mut *tx)
            .await
            .unwrap();
        let n: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM sys.sys_audit_alert_delivery WHERE tenant_id = $1",
        )
        .bind(tenant_id)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
        tx.commit().await.unwrap();
        n
    };
    assert!(delivery_count > 0, "subscription deliveries not created");

    let error_count: i64 = {
        let mut tx = app.state.db.begin().await.unwrap();
        sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
            .bind(tenant_id.to_string())
            .execute(&mut *tx)
            .await
            .unwrap();
        let n: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM sys.sys_audit_alert_delivery
             WHERE tenant_id = $1 AND last_error IS NOT NULL",
        )
        .bind(tenant_id)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
        tx.commit().await.unwrap();
        n
    };
    assert!(error_count > 0, "delivery last_error not recorded");

    let (ds_status, ds_body) = call(
        &app.router,
        "GET",
        "/api/system/audit-delivery-stats?days=30",
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(
        ds_status,
        StatusCode::OK,
        "delivery stats failed: {ds_body}"
    );
    assert!(ds_body["data"]["total"].as_i64().unwrap_or(0) >= delivery_count);
    assert!(ds_body["data"]["dead"].is_number());
    assert!(ds_body["data"]["byUser"].as_array().is_some());

    let dead_delivery: sqlx::types::Uuid = {
        let mut tx = app.state.db.begin().await.unwrap();
        sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
            .bind(tenant_id.to_string())
            .execute(&mut *tx)
            .await
            .unwrap();
        let id: sqlx::types::Uuid = sqlx::query_scalar(
            "UPDATE sys.sys_audit_alert_delivery
             SET status = 'dead', attempts = 8
             WHERE id = (SELECT id FROM sys.sys_audit_alert_delivery
                         WHERE tenant_id = $1 ORDER BY created_at LIMIT 1)
             RETURNING id",
        )
        .bind(tenant_id)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
        tx.commit().await.unwrap();
        id
    };
    let (rd_status, rd_body) = call(
        &app.router,
        "POST",
        &format!("/api/system/audit-deliveries/{dead_delivery}/retry"),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(
        rd_status,
        StatusCode::OK,
        "retry delivery failed: {rd_body}"
    );
    let mut tx = app.state.db.begin().await.unwrap();
    sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    let (status, attempts): (String, i32) =
        sqlx::query_as("SELECT status, attempts FROM sys.sys_audit_alert_delivery WHERE id = $1")
            .bind(dead_delivery)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(status, "pending");
    assert_eq!(attempts, 0);

    // 第 8 次失败 → 投递死信 + 生成审计告警
    let mut tx = app.state.db.begin().await.unwrap();
    sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query(
        "DELETE FROM sys.sys_audit_alert
         WHERE tenant_id = $1 AND action = 'system.delivery.dead'",
    )
    .bind(tenant_id)
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE sys.sys_audit_alert_delivery
         SET status = 'pending', attempts = 7, next_attempt_at = now() - interval '1 second'
         WHERE id = $1",
    )
    .bind(dead_delivery)
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let _ = kernel::audit::dispatch_subscriptions(&app.state.db)
        .await
        .unwrap();
    let dead_alert: bool = {
        let mut tx = app.state.db.begin().await.unwrap();
        sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
            .bind(tenant_id.to_string())
            .execute(&mut *tx)
            .await
            .unwrap();
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM sys.sys_audit_alert
                           WHERE tenant_id = $1 AND action = 'system.delivery.dead')",
        )
        .bind(tenant_id)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
        tx.commit().await.unwrap();
        exists
    };
    assert!(dead_alert, "delivery dead should raise audit alert");

    let (batch_status, batch_body) = call(
        &app.router,
        "POST",
        "/api/system/audit-deliveries/retry-dead",
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(
        batch_status,
        StatusCode::OK,
        "batch retry failed: {batch_body}"
    );
    assert!(batch_body["data"].as_u64().unwrap_or(0) >= 1);
    let mut tx = app.state.db.begin().await.unwrap();
    sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    let dead_status: String =
        sqlx::query_scalar("SELECT status FROM sys.sys_audit_alert_delivery WHERE id = $1")
            .bind(dead_delivery)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(dead_status, "pending");

    let _ = call(
        &app.router,
        "PUT",
        "/api/system/audit-alert-subscription-config",
        Some(&cookie),
        Some(json!({ "url": "", "enabled": false, "secret": null })),
    )
    .await;

    let silent = kernel::audit::detect_alerts(&app.state.db).await.unwrap();
    assert_eq!(silent, 0, "silence period should prevent duplicate alert");

    let (as_status, as_body) = call(
        &app.router,
        "GET",
        "/api/system/audit-alerts/stats?days=30",
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(as_status, StatusCode::OK, "alert stats failed: {as_body}");
    assert!(as_body["data"]["acknowledged"].as_i64().unwrap_or(0) >= 1);
    assert!(as_body["data"]["avgAckMinutes"].is_number());
    assert!(as_body["data"]["byDay"].as_array().is_some());

    let (lr_status, lr_body) = call(
        &app.router,
        "GET",
        "/api/system/login-risk?days=30",
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(lr_status, StatusCode::OK, "login risk: {lr_body}");
    assert!(lr_body["data"]["success"].as_i64().unwrap_or(0) >= 1);
    assert!(lr_body["data"]["byDay"].as_array().is_some());

    let contract_id = body["data"]["id"]
        .as_str()
        .expect("contract id")
        .to_string();

    let (status, body) = call(
        &app.router,
        "PATCH",
        &format!("/api/crm/contracts/{contract_id}"),
        Some(&cookie),
        Some(json!({ "status": "active" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "update contract failed: {body}");
    assert_eq!(body["data"]["status"], "active");

    let (status, body) = call(
        &app.router,
        "GET",
        "/api/crm/contracts?page=1&size=10",
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "list contracts failed: {body}");

    let (status, body) = call(
        &app.router,
        "GET",
        &format!("/api/crm/customers/{ct_host}/contracts"),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "customer contracts failed: {body}");
    assert_eq!(body["data"][0]["id"].as_str(), Some(contract_id.as_str()));
    assert_eq!(body["data"][0]["paidCents"], 0);

    let (status, body) = call(
        &app.router,
        "DELETE",
        &format!("/api/crm/contracts/{contract_id}"),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "delete contract failed: {body}");

    let (status, body) = call(
        &app.router,
        "DELETE",
        &format!("/api/crm/customers/{ct_host}"),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "delete contract host failed: {body}"
    );

    // CRM 商机：建客户 → 建商机 → 改 → 列表 → 删 → 删客户
    let (status, body) = call(
        &app.router,
        "POST",
        "/api/crm/customers",
        Some(&cookie),
        Some(json!({ "code": format!("SMOKE-O-{suffix}"), "name": "Opp Host", "level": "normal" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "create opp host failed: {body}");
    let opp_host = body["data"]["id"].as_str().expect("opp host").to_string();

    let (status, body) = call(
        &app.router,
        "POST",
        "/api/crm/opportunities",
        Some(&cookie),
        Some(json!({
            "customerId": opp_host,
            "name": "Smoke Opportunity",
            "amountCents": 1234500,
            "stage": "initial"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "create opportunity failed: {body}");

    // 商机导入
    let mut wb = rust_xlsxwriter::Workbook::new();
    {
        let sheet = wb.add_worksheet();
        for (i, h) in ["客户编码", "商机名称", "金额(元)", "阶段", "预计成交"]
            .iter()
            .enumerate()
        {
            sheet.write_string(0, i as u16, *h).unwrap();
        }
        sheet
            .write_string(1, 0, format!("SMOKE-O-{suffix}"))
            .unwrap();
        sheet.write_string(1, 1, "Imported Opportunity").unwrap();
        sheet.write_string(1, 2, "8888.88").unwrap();
        sheet.write_string(1, 3, "proposal").unwrap();
        sheet.write_string(1, 4, "2026-12-31").unwrap();
    }
    let xlsx = wb.save_to_buffer().unwrap();
    let boundary = "----zcadmin-opportunity-import";
    let mut mp = Vec::new();
    mp.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"opportunities.xlsx\"\r\nContent-Type: application/vnd.openxmlformats-officedocument.spreadsheetml.sheet\r\n\r\n"
        )
        .as_bytes(),
    );
    mp.extend_from_slice(&xlsx);
    mp.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    let req = Request::builder()
        .method("POST")
        .uri("/api/system/imports/crm.opportunities")
        .header(header::COOKIE, &cookie)
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(mp))
        .unwrap();
    let resp = app
        .router
        .clone()
        .oneshot(req)
        .await
        .expect("opportunity import");
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "crm.opportunities import failed"
    );
    let import_body: Value = serde_json::from_slice(
        &axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        import_body["data"]["success"], 1,
        "opportunity import result: {import_body}"
    );
    let opp_id = body["data"]["id"].as_str().expect("opp id").to_string();

    // 商机 → 合同转化
    let (status, body) = call(
        &app.router,
        "POST",
        "/api/crm/contracts",
        Some(&cookie),
        Some(json!({
            "customerId": opp_host,
            "opportunityId": opp_id,
            "contractNo": format!("HT-O-{suffix}"),
            "name": "Converted Contract",
            "amountCents": 1234500,
            "status": "active"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "convert opportunity failed: {body}");
    let conv_id = body["data"]["id"].as_str().expect("conv id").to_string();
    assert_eq!(
        body["data"]["opportunityId"].as_str(),
        Some(opp_id.as_str())
    );

    let (status, body) = call(
        &app.router,
        "GET",
        &format!("/api/crm/opportunities/{opp_id}/contracts"),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "opportunity contracts failed: {body}"
    );
    assert_eq!(body["data"][0]["id"].as_str(), Some(conv_id.as_str()));

    let (status, body) = call(
        &app.router,
        "DELETE",
        &format!("/api/crm/contracts/{conv_id}"),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "delete converted contract failed: {body}"
    );

    let (status, body) = call(
        &app.router,
        "PATCH",
        &format!("/api/crm/opportunities/{opp_id}"),
        Some(&cookie),
        Some(json!({ "stage": "won" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "update opportunity failed: {body}");
    assert_eq!(body["data"]["stage"], "won");

    let (status, body) = call(
        &app.router,
        "GET",
        &format!("/api/crm/opportunities/{opp_id}/stage-history"),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "stage history failed: {body}");
    assert!(
        body["data"].as_array().unwrap().len() >= 2,
        "expected at least 2 stage records: {body}"
    );

    let (status, body) = call(
        &app.router,
        "GET",
        "/api/crm/opportunities/stage-duration",
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "stage duration failed: {body}");
    assert!(body["data"].as_array().is_some());

    let (status, body) = call(
        &app.router,
        "GET",
        "/api/crm/opportunities/stage-conversion",
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "stage conversion failed: {body}");
    assert!(body["data"].as_array().is_some());

    let (status, body) = call(
        &app.router,
        "GET",
        "/api/crm/opportunities/stuck?days=1",
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "stuck opportunities failed: {body}");
    assert!(body["data"].as_array().is_some());

    let (status, body) = call(
        &app.router,
        "GET",
        "/api/crm/opportunities/stats",
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "opportunity stats failed: {body}");
    let won = body["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["stage"] == "won")
        .expect("won stage");
    assert!(won["count"].as_i64().unwrap_or(0) >= 1, "won count: {won}");

    let (status, body) = call(
        &app.router,
        "GET",
        "/api/crm/opportunities?page=1&size=10",
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "list opportunities failed: {body}");

    let (status, body) = call(
        &app.router,
        "GET",
        &format!("/api/crm/customers/{opp_host}/opportunities"),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "customer opportunities failed: {body}"
    );
    let opp_ids: Vec<&str> = body["data"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|x| x["id"].as_str())
        .collect();
    assert!(
        opp_ids.contains(&opp_id.as_str()),
        "created opportunity missing from customer list: {body}"
    );

    let (status, body) = call(
        &app.router,
        "DELETE",
        &format!("/api/crm/opportunities/{opp_id}"),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "delete opportunity failed: {body}");

    let (status, body) = call(
        &app.router,
        "DELETE",
        &format!("/api/crm/customers/{opp_host}"),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "delete opp host failed: {body}");

    // CRM 联系人：建客户 → 建联系人 → 列表 → 删联系人 → 删客户
    let (status, body) = call(
        &app.router,
        "POST",
        "/api/crm/customers",
        Some(&cookie),
        Some(json!({ "code": format!("SMOKE-C-{suffix}"), "name": "Contact Host", "level": "normal" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "create contact host failed: {body}");
    let host_id = body["data"]["id"].as_str().expect("host id").to_string();

    let (status, body) = call(
        &app.router,
        "POST",
        "/api/crm/contacts",
        Some(&cookie),
        Some(json!({ "customerId": host_id, "name": "Smoke Contact", "isPrimary": true })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "create contact failed: {body}");

    // 联系人导入
    let mut wb = rust_xlsxwriter::Workbook::new();
    {
        let sheet = wb.add_worksheet();
        for (i, h) in ["客户编码", "姓名", "电话", "邮箱", "主要联系人"]
            .iter()
            .enumerate()
        {
            sheet.write_string(0, i as u16, *h).unwrap();
        }
        sheet
            .write_string(1, 0, format!("SMOKE-C-{suffix}"))
            .unwrap();
        sheet.write_string(1, 1, "Imported Contact").unwrap();
        sheet.write_string(1, 2, "13800000000").unwrap();
        sheet.write_string(1, 3, "imported@example.com").unwrap();
        sheet.write_string(1, 4, "是").unwrap();
    }
    let xlsx = wb.save_to_buffer().unwrap();
    let boundary = "----zcadmin-contact-import";
    let mut mp = Vec::new();
    mp.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"contacts.xlsx\"\r\nContent-Type: application/vnd.openxmlformats-officedocument.spreadsheetml.sheet\r\n\r\n"
        )
        .as_bytes(),
    );
    mp.extend_from_slice(&xlsx);
    mp.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    let req = Request::builder()
        .method("POST")
        .uri("/api/system/imports/crm.contacts")
        .header(header::COOKIE, &cookie)
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(mp))
        .unwrap();
    let resp = app
        .router
        .clone()
        .oneshot(req)
        .await
        .expect("contact import");
    assert_eq!(resp.status(), StatusCode::OK, "crm.contacts import failed");
    let import_body: Value = serde_json::from_slice(
        &axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        import_body["data"]["success"], 1,
        "contact import result: {import_body}"
    );
    assert_eq!(
        import_body["data"]["failed"], 0,
        "contact import failed rows: {import_body}"
    );
    let contact_id = body["data"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("contact body: {body}"))
        .to_string();

    let (status, body) = call(
        &app.router,
        "GET",
        "/api/crm/contacts?page=1&size=10",
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "list contacts failed: {body}");

    let (status, body) = call(
        &app.router,
        "GET",
        &format!("/api/crm/customers/{host_id}/contacts"),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "customer contacts failed: {body}");
    let contact_ids: Vec<&str> = body["data"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|x| x["id"].as_str())
        .collect();
    assert!(
        contact_ids.contains(&contact_id.as_str()),
        "created contact missing from customer contacts: {body}"
    );

    let (status, body) = call(
        &app.router,
        "DELETE",
        &format!("/api/crm/contacts/{contact_id}"),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "delete contact failed: {body}");

    let (status, body) = call(
        &app.router,
        "DELETE",
        &format!("/api/crm/customers/{host_id}"),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "delete contact host failed: {body}");

    // 5. 系统写接口（覆盖 RLS 的 INSERT / UPDATE WITH CHECK）
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis();

    let dict_code = format!("smoke_{suffix}");
    let (status, body) = call(
        &app.router,
        "POST",
        "/api/system/dicts",
        Some(&cookie),
        Some(json!({ "code": dict_code, "name": "Smoke Dict" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "create dict failed: {body}");

    let (status, body) = call(
        &app.router,
        "POST",
        &format!("/api/system/dicts/{dict_code}/items"),
        Some(&cookie),
        Some(json!({ "value": "v1", "label": "Value 1", "sort": 1 })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "create dict item failed: {body}");

    let (status, body) = call(
        &app.router,
        "POST",
        "/api/system/roles",
        Some(&cookie),
        Some(json!({ "code": format!("smoke_role_{suffix}"), "name": "Smoke Role", "dataScope": "self" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "create role failed: {body}");
    let role_id = body["data"]["id"].as_str().expect("role id").to_string();

    let (status, body) = call(
        &app.router,
        "GET",
        &format!("/api/system/roles/{role_id}/scope-preview"),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "scope preview failed: {body}");
    assert_eq!(body["data"]["dataScope"], "self");

    let (status, body) = call(
        &app.router,
        "PUT",
        &format!("/api/system/roles/{role_id}/permissions"),
        Some(&cookie),
        Some(json!({ "ids": [] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "set role perms failed: {body}");

    // 角色自定义数据范围：指定部门
    let (status, body) = call(
        &app.router,
        "POST",
        "/api/system/depts",
        Some(&cookie),
        Some(json!({ "name": format!("Smoke Dept {suffix}"), "parentId": null })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "create dept failed: {body}");
    let dept_id = body["data"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("dept body: {body}"))
        .to_string();

    let (status, body) = call(
        &app.router,
        "PUT",
        &format!("/api/system/roles/{role_id}/depts"),
        Some(&cookie),
        Some(json!({ "deptIds": [dept_id] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "set role depts failed: {body}");

    let (status, body) = call(
        &app.router,
        "GET",
        &format!("/api/system/roles/{role_id}/depts"),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "get role depts failed: {body}");
    assert!(
        body["data"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == &dept_id),
        "saved dept missing: {body}"
    );

    let (audit_status, audit_body) = call(
        &app.router,
        "GET",
        "/api/system/audit-logs?action=system.role.depts&page=1&size=20",
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(
        audit_status,
        StatusCode::OK,
        "role depts audit failed: {audit_body}"
    );
    let dept_row = audit_body["data"]["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["resourceId"] == role_id.as_str())
        .expect("role depts audit");
    assert_eq!(dept_row["detail"]["before"].as_array().unwrap().len(), 0);
    assert!(dept_row["detail"]["after"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v == &dept_id));

    let smoke_username = format!("smoke_{suffix}");
    let (status, body) = call(
        &app.router,
        "POST",
        "/api/system/users",
        Some(&cookie),
        Some(json!({
            "username": smoke_username,
            "displayName": "Smoke User",
            "password": "Smoke#12345",
            "email": "smoke@example.com"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "create user failed: {body}");
    let user_id = body["data"]["id"].as_str().expect("user id").to_string();

    let (status, body) = call(
        &app.router,
        "PATCH",
        &format!("/api/system/users/{user_id}/status"),
        Some(&cookie),
        Some(json!({ "status": "active" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "update user status failed: {body}");

    let (audit_status, audit_body) = call(
        &app.router,
        "GET",
        "/api/system/audit-logs?action=system.user.status&page=1&size=20",
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(
        audit_status,
        StatusCode::OK,
        "user status audit failed: {audit_body}"
    );
    let status_row = audit_body["data"]["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["resourceId"] == user_id.as_str())
        .expect("user status audit");
    assert_eq!(status_row["detail"]["before"]["status"], "active");
    assert_eq!(status_row["detail"]["after"]["status"], "active");

    // 登录失败锁定：5 次错误后，即使密码正确也应被拒绝
    for attempt in 1..=5 {
        let (status, body) = call(
            &app.router,
            "POST",
            "/api/auth/login",
            None,
            Some(json!({ "username": smoke_username, "password": "wrong", "tenant": "default" })),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::UNAUTHORIZED,
            "attempt {attempt}: {body}"
        );
    }
    let (status, body) = call(
        &app.router,
        "POST",
        "/api/auth/login",
        None,
        Some(json!({ "username": smoke_username, "password": "Smoke#12345", "tenant": "default" })),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "locked account must reject correct password: {body}"
    );

    // 管理员解锁后应能正常登录（直连数据库也要注入租户上下文，否则被 RLS 拦截）
    let pool = sqlx::PgPool::connect(&url).await.expect("db pool");
    let mut tx = pool.begin().await.expect("db tx");
    sqlx::query(
        // 必须按 code 精确定位：多租户库里 `LIMIT 1` 无 ORDER BY 时取到哪行不确定，
        // 会解锁到别的租户，表现为"解锁了但仍 401"。
        "SELECT set_config('app.tenant_id', (SELECT id::text FROM sys.sys_tenant WHERE code = 'default'), true)",
    )
    .execute(&mut *tx)
    .await
    .expect("set tenant");
    sqlx::query(
        "UPDATE sys.sys_user SET locked_until = NULL, failed_attempts = 0 WHERE username = $1",
    )
    .bind(&smoke_username)
    .execute(&mut *tx)
    .await
    .expect("unlock user");
    tx.commit().await.expect("commit unlock");
    let (status, body) = call(
        &app.router,
        "POST",
        "/api/auth/login",
        None,
        Some(json!({ "username": smoke_username, "password": "Smoke#12345", "tenant": "default" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "unlock failed: {body}");

    let menu_path = format!("/smoke-{suffix}");
    let (status, body) = call(
        &app.router,
        "POST",
        "/api/system/menus",
        Some(&cookie),
        Some(json!({
            "parentId": null,
            "title": "Smoke Menu",
            "path": menu_path,
            "sort": 999,
            "visible": false
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "create menu failed: {body}");
    let menu_id = body["data"]["id"].as_str().expect("menu id").to_string();

    let (status, body) = call(
        &app.router,
        "PUT",
        &format!("/api/system/menus/{menu_id}"),
        Some(&cookie),
        Some(json!({
            "parentId": null,
            "title": "Smoke Menu Updated",
            "path": menu_path,
            "sort": 999,
            "visible": false
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "update menu failed: {body}");

    let (status, body) = call(
        &app.router,
        "DELETE",
        &format!("/api/system/menus/{menu_id}"),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "delete menu failed: {body}");

    let (status, body) = call(
        &app.router,
        "POST",
        "/api/system/notices",
        Some(&cookie),
        Some(json!({ "title": "Smoke Notice", "content": "hello", "level": "info" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "publish notice failed: {body}");

    let (status, body) = call(
        &app.router,
        "POST",
        "/api/system/configs",
        Some(&cookie),
        Some(json!({ "key": format!("smoke.{suffix}"), "value": "1", "name": "Smoke", "isPublic": false })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "create config failed: {body}");
    let config_id = body["data"]["id"].as_str().expect("config id").to_string();

    let (status, body) = call(
        &app.router,
        "PUT",
        &format!("/api/system/configs/{config_id}"),
        Some(&cookie),
        Some(json!({ "value": "2" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "update config failed: {body}");

    // 文件上传 + 删除（multipart 手工构造，验证存储与 RLS 写入）
    let boundary = "----zcadmin-smoke-boundary";
    let multipart_body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"smoke.txt\"\r\nContent-Type: text/plain\r\n\r\nhello smoke\r\n--{boundary}--\r\n"
    );
    let upload = Request::builder()
        .method("POST")
        .uri("/api/system/files/upload")
        .header(header::COOKIE, &cookie)
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(multipart_body))
        .unwrap();
    let resp = app.router.clone().oneshot(upload).await.expect("upload");
    assert_eq!(resp.status(), StatusCode::OK, "upload failed");
    let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let upload_body: Value = serde_json::from_slice(&bytes).unwrap();
    let file_id = upload_body["data"]["id"]
        .as_str()
        .expect("file id")
        .to_string();

    let (status, body) = call(
        &app.router,
        "DELETE",
        &format!("/api/system/files/{file_id}"),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "delete file failed: {body}");

    // 存储配额：设为 1 字节后上传必须被拒绝
    let (_, tenant_list) = call(
        &app.router,
        "GET",
        "/api/system/tenants",
        Some(&cookie),
        None,
    )
    .await;
    let default_tenant = tenant_list["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["code"] == "default")
        .and_then(|t| t["id"].as_str())
        .expect("default tenant")
        .to_string();
    let (quota_set, quota_body) = call(
        &app.router,
        "PUT",
        &format!("/api/system/tenants/{default_tenant}/quota"),
        Some(&cookie),
        Some(json!({ "maxUsers": 0, "maxStorageBytes": 1 })),
    )
    .await;
    assert_eq!(quota_set, StatusCode::OK, "set storage quota: {quota_body}");
    let quota_upload = Request::builder()
        .method("POST")
        .uri("/api/system/files/upload")
        .header(header::COOKIE, &cookie)
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"quota.txt\"\r\nContent-Type: text/plain\r\n\r\nquota-test\r\n--{boundary}--\r\n"
        )))
        .unwrap();
    let resp = app
        .router
        .clone()
        .oneshot(quota_upload)
        .await
        .expect("quota upload");
    assert_eq!(
        resp.status(),
        StatusCode::UNPROCESSABLE_ENTITY,
        "storage quota must reject"
    );
    let (quota_reset, quota_reset_body) = call(
        &app.router,
        "PUT",
        &format!("/api/system/tenants/{default_tenant}/quota"),
        Some(&cookie),
        Some(json!({ "maxUsers": 0, "maxStorageBytes": 0 })),
    )
    .await;
    assert_eq!(
        quota_reset,
        StatusCode::OK,
        "reset quota: {quota_reset_body}"
    );

    // 岗位：创建 → 修改 → 删除
    let (status, body) = call(
        &app.router,
        "POST",
        "/api/system/posts",
        Some(&cookie),
        Some(json!({ "code": format!("smoke_post_{suffix}"), "name": "Smoke Post" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "create post failed: {body}");
    let post_id = body["data"]["id"].as_str().expect("post id").to_string();

    let (status, body) = call(
        &app.router,
        "PUT",
        &format!("/api/system/posts/{post_id}"),
        Some(&cookie),
        Some(json!({ "code": format!("smoke_post_{suffix}"), "name": "Smoke Post Updated", "sort": 5 })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "update post failed: {body}");

    let (status, body) = call(
        &app.router,
        "GET",
        "/api/system/posts?page=1&size=20",
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "list posts failed: {body}");

    // 绑定岗位到冒烟用户，再解绑（否则岗位不允许删除）
    let (status, body) = call(
        &app.router,
        "PUT",
        &format!("/api/system/users/{user_id}/posts"),
        Some(&cookie),
        Some(json!({ "postIds": [post_id] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "assign post failed: {body}");

    let (status, body) = call(
        &app.router,
        "GET",
        &format!("/api/system/users/{user_id}/posts"),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "get user posts failed: {body}");
    assert_eq!(body["data"][0].as_str(), Some(post_id.as_str()));

    let (status, body) = call(
        &app.router,
        "PUT",
        &format!("/api/system/users/{user_id}/posts"),
        Some(&cookie),
        Some(json!({ "postIds": [] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "unassign post failed: {body}");

    let (status, body) = call(
        &app.router,
        "DELETE",
        &format!("/api/system/posts/{post_id}"),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "delete post failed: {body}");

    // 部门：创建 → 改名 → 删除
    let (status, body) = call(
        &app.router,
        "POST",
        "/api/system/depts",
        Some(&cookie),
        Some(json!({ "name": format!("Smoke Dept {suffix}"), "parentId": null })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "create dept failed: {body}");
    let dept_id = body["data"]["id"].as_str().expect("dept id").to_string();

    let (status, body) = call(
        &app.router,
        "PUT",
        &format!("/api/system/depts/{dept_id}"),
        Some(&cookie),
        Some(json!({ "name": "Smoke Dept Renamed", "sort": null })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "update dept failed: {body}");

    let (status, body) = call(
        &app.router,
        "DELETE",
        &format!("/api/system/depts/{dept_id}"),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "delete dept failed: {body}");

    // 用户导入模板下载
    let req = Request::builder()
        .method("GET")
        .uri("/api/system/users/import-template")
        .header(header::COOKIE, &cookie)
        .body(Body::empty())
        .unwrap();
    let resp = app.router.clone().oneshot(req).await.expect("template");
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok()),
        Some("application/vnd.openxmlformats-officedocument.spreadsheetml.sheet")
    );
    let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
        .await
        .unwrap();
    assert_eq!(&bytes[..2], b"PK", "template should be xlsx zip");

    // 导入错误报告导出
    let req = Request::builder()
        .method("POST")
        .uri("/api/system/users/import-errors")
        .header(header::COOKIE, &cookie)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({ "errors": [{ "row": 2, "message": "用户名已存在" }] }).to_string(),
        ))
        .unwrap();
    let resp = app
        .router
        .clone()
        .oneshot(req)
        .await
        .expect("import errors export");
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
        .await
        .unwrap();
    assert_eq!(&bytes[..2], b"PK", "error report should be xlsx zip");

    // 用户批量导入（构造 xlsx → multipart 上传）
    let mut wb = rust_xlsxwriter::Workbook::new();
    {
        let sheet = wb.add_worksheet();
        for (i, h) in ["用户名", "显示名", "初始密码", "手机号"]
            .iter()
            .enumerate()
        {
            sheet.write_string(0, i as u16, *h).unwrap();
        }
        sheet.write_string(1, 0, format!("imp_{suffix}")).unwrap();
        sheet.write_string(1, 1, "Imported User").unwrap();
        sheet.write_string(1, 2, "Smoke#12345").unwrap();
        sheet.write_string(1, 3, "13900001111").unwrap();
        sheet.write_string(2, 0, format!("imp_{suffix}")).unwrap();
        sheet.write_string(2, 1, "Duplicate User").unwrap();
        sheet.write_string(2, 2, "Smoke#12345").unwrap();
    }
    let xlsx = wb.save_to_buffer().expect("build xlsx");

    let boundary = "----zcadmin-import-boundary";
    let mut body: Vec<u8> = Vec::new();
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"users.xlsx\"\r\nContent-Type: application/vnd.openxmlformats-officedocument.spreadsheetml.sheet\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(&xlsx);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());

    // 仅校验：不写库
    let dry_req = Request::builder()
        .method("POST")
        .uri("/api/system/users/import?dryRun=true")
        .header(header::COOKIE, &cookie)
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body.clone()))
        .unwrap();
    let dry_resp = app
        .router
        .clone()
        .oneshot(dry_req)
        .await
        .expect("dry-run import");
    assert_eq!(dry_resp.status(), StatusCode::OK, "dry-run import failed");
    let dry_bytes = axum::body::to_bytes(dry_resp.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let dry_body: Value = serde_json::from_slice(&dry_bytes).unwrap();
    assert_eq!(dry_body["data"]["success"], 1, "dry-run result: {dry_body}");
    assert_eq!(
        dry_body["data"]["failed"], 1,
        "dry-run duplicate: {dry_body}"
    );
    let (_, pre_import) = call(
        &app.router,
        "GET",
        &format!("/api/system/users?page=1&size=20&keyword=imp_{suffix}"),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(
        pre_import["data"]["data"].as_array().unwrap().len(),
        0,
        "dry-run must not insert"
    );

    let req = Request::builder()
        .method("POST")
        .uri("/api/system/users/import")
        .header(header::COOKIE, &cookie)
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body))
        .unwrap();
    let resp = app.router.clone().oneshot(req).await.expect("import users");
    assert_eq!(resp.status(), StatusCode::OK, "import users failed");
    let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["data"]["success"], 1, "import result: {body}");
    assert_eq!(body["data"]["failed"], 1, "import duplicate: {body}");
    let (_, imported) = call(
        &app.router,
        "GET",
        &format!("/api/system/users?page=1&size=20&keyword=imp_{suffix}"),
        Some(&cookie),
        None,
    )
    .await;
    let imported_phone = imported["data"]["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|u| u["username"] == format!("imp_{suffix}"))
        .and_then(|u| u["phone"].as_str());
    assert_eq!(imported_phone, Some("13900001111"));

    // 插件导入：合同 xlsx → /api/system/imports/crm.contracts
    let (status, body) = call(
        &app.router,
        "POST",
        "/api/crm/customers",
        Some(&cookie),
        Some(json!({ "code": format!("IMP-{suffix}"), "name": "Import Host", "level": "normal" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "create import host failed: {body}");

    let mut wb = rust_xlsxwriter::Workbook::new();
    {
        let sheet = wb.add_worksheet();
        for (i, h) in ["合同编号", "合同名称", "客户编码", "金额(元)", "状态"]
            .iter()
            .enumerate()
        {
            sheet.write_string(0, i as u16, *h).unwrap();
        }
        sheet
            .write_string(1, 0, format!("HT-IMP-{suffix}"))
            .unwrap();
        sheet.write_string(1, 1, "Imported Contract").unwrap();
        sheet.write_string(1, 2, format!("IMP-{suffix}")).unwrap();
        sheet.write_string(1, 3, "1000.50").unwrap();
        sheet.write_string(1, 4, "active").unwrap();
    }
    let xlsx = wb.save_to_buffer().unwrap();
    let boundary = "----zcadmin-import-boundary";
    let mut mp = Vec::new();
    mp.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"contracts.xlsx\"\r\nContent-Type: application/vnd.openxmlformats-officedocument.spreadsheetml.sheet\r\n\r\n"
        )
        .as_bytes(),
    );
    mp.extend_from_slice(&xlsx);
    mp.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    let req = Request::builder()
        .method("POST")
        .uri("/api/system/imports/crm.contracts")
        .header(header::COOKIE, &cookie)
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(mp))
        .unwrap();
    let resp = app.router.clone().oneshot(req).await.expect("import");
    assert_eq!(resp.status(), StatusCode::OK, "crm.contracts import failed");
    let body: Value = serde_json::from_slice(
        &axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(body["data"]["success"], 1, "crm import result: {body}");
    assert_eq!(body["data"]["failed"], 0, "crm import failed rows: {body}");

    // 异步导出：创建任务 → 轮询 → 下载
    let (status, body) = call(
        &app.router,
        "POST",
        "/api/system/export-tasks",
        Some(&cookie),
        Some(json!({ "kind": "system.audit", "resourceType": "contract" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "create export task failed: {body}");
    let export_id = body["data"]["id"].as_str().expect("export id").to_string();

    let (audit_status, audit_body) = call(
        &app.router,
        "GET",
        "/api/system/audit-logs?action=system.export.create&page=1&size=20",
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(
        audit_status,
        StatusCode::OK,
        "export audit query failed: {audit_body}"
    );
    assert!(
        audit_body["data"]["data"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x["resourceId"] == export_id.as_str()),
        "export audit missing: {audit_body}"
    );

    let mut exported = false;
    for _ in 0..40 {
        let (status, body) = call(
            &app.router,
            "GET",
            "/api/system/export-tasks?page=1&size=10",
            Some(&cookie),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "list export tasks failed: {body}");
        let tasks = body["data"]["data"].as_array().cloned().unwrap_or_default();
        if let Some(task) = tasks.iter().find(|t| t["id"] == export_id.as_str()) {
            match task["status"].as_str() {
                Some("success") => {
                    exported = true;
                    break;
                }
                Some("failed") => panic!("export failed: {}", task["error"]),
                _ => {}
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert!(exported, "export task did not finish in time");

    let download = Request::builder()
        .method("GET")
        .uri(format!("/api/system/export-tasks/{export_id}/download"))
        .header(header::COOKIE, &cookie)
        .body(Body::empty())
        .unwrap();
    let resp = app
        .router
        .clone()
        .oneshot(download)
        .await
        .expect("download export");
    assert_eq!(resp.status(), StatusCode::OK, "download failed");
    let bytes = axum::body::to_bytes(resp.into_body(), 10 * 1024 * 1024)
        .await
        .unwrap();
    assert_eq!(&bytes[..2], b"PK", "download is not an xlsx file");

    // 插件注册的业务导出：合同
    let (status, body) = call(
        &app.router,
        "POST",
        "/api/system/export-tasks",
        Some(&cookie),
        Some(json!({ "kind": "crm.contracts" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "create crm export failed: {body}");
    let crm_export = body["data"]["id"]
        .as_str()
        .expect("crm export id")
        .to_string();

    let mut crm_exported = false;
    for _ in 0..40 {
        let (status, body) = call(
            &app.router,
            "GET",
            "/api/system/export-tasks?page=1&size=10",
            Some(&cookie),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "list crm export failed: {body}");
        let tasks = body["data"]["data"].as_array().cloned().unwrap_or_default();
        if let Some(task) = tasks.iter().find(|t| t["id"] == crm_export.as_str()) {
            match task["status"].as_str() {
                Some("success") => {
                    crm_exported = true;
                    break;
                }
                Some("failed") => panic!("crm export failed: {}", task["error"]),
                _ => {}
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert!(crm_exported, "crm export did not finish in time");

    // 联系人导出（插件注册）
    let (status, body) = call(
        &app.router,
        "POST",
        "/api/system/export-tasks",
        Some(&cookie),
        Some(json!({ "kind": "crm.contacts" })),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "create contacts export failed: {body}"
    );
    let contacts_export = body["data"]["id"]
        .as_str()
        .expect("contacts export id")
        .to_string();

    let mut contacts_exported = false;
    for _ in 0..40 {
        let (status, body) = call(
            &app.router,
            "GET",
            "/api/system/export-tasks?page=1&size=10",
            Some(&cookie),
            None,
        )
        .await;
        assert_eq!(
            status,
            StatusCode::OK,
            "list contacts export failed: {body}"
        );
        let tasks = body["data"]["data"].as_array().cloned().unwrap_or_default();
        if let Some(task) = tasks.iter().find(|t| t["id"] == contacts_export.as_str()) {
            match task["status"].as_str() {
                Some("success") => {
                    contacts_exported = true;
                    break;
                }
                Some("failed") => panic!("contacts export failed: {}", task["error"]),
                _ => {}
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert!(contacts_exported, "contacts export did not finish in time");

    let (status, body) = call(
        &app.router,
        "DELETE",
        &format!("/api/system/export-tasks/{export_id}"),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "delete export task failed: {body}");

    // 审计配置导出 / 导入
    let (cfg_status, cfg_body) = call(
        &app.router,
        "GET",
        "/api/system/audit-config/export",
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(
        cfg_status,
        StatusCode::OK,
        "export config failed: {cfg_body}"
    );
    assert!(cfg_body["data"]["alertRules"].as_array().is_some());
    assert_eq!(cfg_body["data"]["version"], 2);
    let mut v1_bundle = cfg_body["data"].clone();
    v1_bundle["version"] = json!(1);
    v1_bundle.as_object_mut().unwrap().remove("metadata");
    let (v1_status, v1_body) = call(
        &app.router,
        "PUT",
        "/api/system/audit-config/import",
        Some(&cookie),
        Some(v1_bundle),
    )
    .await;
    assert_eq!(v1_status, StatusCode::OK, "v1 import failed: {v1_body}");
    let cfg_alert_count: i64 = {
        let mut tx = app.state.db.begin().await.unwrap();
        sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
            .bind(tenant_id.to_string())
            .execute(&mut *tx)
            .await
            .unwrap();
        let n: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM sys.sys_audit_alert
             WHERE tenant_id = $1 AND action = 'system.config.import'",
        )
        .bind(tenant_id)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
        tx.commit().await.unwrap();
        n
    };
    assert!(cfg_alert_count >= 1, "config import alert missing");

    // 导入失败必须整体回滚：重复规则触发唯一约束
    let (rules_before_status, rules_before) = call(
        &app.router,
        "GET",
        "/api/system/audit-alert-rules",
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(rules_before_status, StatusCode::OK);
    let mut bad_bundle = cfg_body["data"].clone();
    let duplicate = bad_bundle["alertRules"][0].clone();
    bad_bundle["alertRules"]
        .as_array_mut()
        .unwrap()
        .push(duplicate);
    let (bad_status, bad_body) = call(
        &app.router,
        "PUT",
        "/api/system/audit-config/import?sections=rules",
        Some(&cookie),
        Some(bad_bundle),
    )
    .await;
    assert!(
        !bad_status.is_success(),
        "duplicate import should fail: {bad_body}"
    );
    let (rules_after_status, rules_after) = call(
        &app.router,
        "GET",
        "/api/system/audit-alert-rules",
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(rules_after_status, StatusCode::OK);
    assert_eq!(rules_after["data"], rules_before["data"]);

    let (diff_status, diff_body) = call(
        &app.router,
        "POST",
        "/api/system/audit-config/preview",
        Some(&cookie),
        Some(cfg_body["data"].clone()),
    )
    .await;
    assert_eq!(
        diff_status,
        StatusCode::OK,
        "preview config failed: {diff_body}"
    );
    assert!(diff_body["data"]["rulesAdded"].as_array().is_some());
    assert_eq!(diff_body["data"]["rulesAdded"].as_array().unwrap().len(), 0);
    let (filtered_status, filtered_body) = call(
        &app.router,
        "POST",
        "/api/system/audit-config/preview?sections=rules",
        Some(&cookie),
        Some(cfg_body["data"].clone()),
    )
    .await;
    assert_eq!(
        filtered_status,
        StatusCode::OK,
        "filtered preview failed: {filtered_body}"
    );
    assert!(filtered_body["data"]["subscriptionsAdded"]
        .as_array()
        .unwrap()
        .is_empty());
    let (imp_status, imp_body) = call(
        &app.router,
        "PUT",
        "/api/system/audit-config/import",
        Some(&cookie),
        Some(cfg_body["data"].clone()),
    )
    .await;
    assert_eq!(
        imp_status,
        StatusCode::OK,
        "import config failed: {imp_body}"
    );

    // 选择性导入：只导入规则，订阅保持不变
    let (old_sub_status, old_sub_body) = call(
        &app.router,
        "GET",
        "/api/system/audit-alert-subscriptions",
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(old_sub_status, StatusCode::OK);
    let old_patterns = old_sub_body["data"].clone();
    let mut selective = cfg_body["data"].clone();
    selective["subscriptionPatterns"] = json!([]);
    let (sel_status, sel_body) = call(
        &app.router,
        "PUT",
        "/api/system/audit-config/import?sections=rules",
        Some(&cookie),
        Some(selective),
    )
    .await;
    assert_eq!(
        sel_status,
        StatusCode::OK,
        "selective import failed: {sel_body}"
    );
    let (after_sub_status, after_sub_body) = call(
        &app.router,
        "GET",
        "/api/system/audit-alert-subscriptions",
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(after_sub_status, StatusCode::OK);
    assert_eq!(after_sub_body["data"], old_patterns);

    let (snap_status, snap_body) = call(
        &app.router,
        "GET",
        "/api/system/audit-config/snapshots",
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(
        snap_status,
        StatusCode::OK,
        "list snapshots failed: {snap_body}"
    );
    let snapshots = snap_body["data"]["data"].as_array().unwrap();
    assert!(!snapshots.is_empty(), "snapshot missing");
    assert_eq!(snapshots[0]["sections"], "rules");
    assert!(snapshots[0]["diffDetail"]["rulesAdded"].is_array());
    let (filter_status, filter_body) = call(
        &app.router,
        "GET",
        "/api/system/audit-config/snapshots?createdBy=admin&sections=rules",
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(filter_status, StatusCode::OK);
    assert!(!filter_body["data"]["data"].as_array().unwrap().is_empty());
    let (empty_status, empty_body) = call(
        &app.router,
        "GET",
        "/api/system/audit-config/snapshots?from=2099-01-01",
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(empty_status, StatusCode::OK);
    assert!(empty_body["data"]["data"].as_array().unwrap().is_empty());
    let (rb_status, rb_body) = call(
        &app.router,
        "PUT",
        "/api/system/audit-config/import?mode=rollback",
        Some(&cookie),
        Some(snapshots[0]["bundle"].clone()),
    )
    .await;
    assert_eq!(rb_status, StatusCode::OK, "rollback failed: {rb_body}");
    let rollback_alert: i64 = {
        let mut tx = app.state.db.begin().await.unwrap();
        sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
            .bind(tenant_id.to_string())
            .execute(&mut *tx)
            .await
            .unwrap();
        let n: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM sys.sys_audit_alert
             WHERE tenant_id = $1 AND action = 'system.config.rollback'",
        )
        .bind(tenant_id)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
        tx.commit().await.unwrap();
        n
    };
    assert!(rollback_alert >= 1, "rollback alert missing");

    let del_id = snapshots[0]["id"].as_str().unwrap();
    let (exp_status, exp_body) = call(
        &app.router,
        "GET",
        &format!("/api/system/audit-config/snapshots/{del_id}/export"),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(
        exp_status,
        StatusCode::OK,
        "export snapshot failed: {exp_body}"
    );
    assert!(exp_body["data"]["alertRules"].as_array().is_some());
    assert!(snapshots.len() >= 2, "need two snapshots to compare");
    let left_id = snapshots[0]["id"].as_str().unwrap();
    let right_id = snapshots[1]["id"].as_str().unwrap();
    let (cmp_status, cmp_body) = call(
        &app.router,
        "GET",
        &format!("/api/system/audit-config/snapshots/compare?left={left_id}&right={right_id}"),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(cmp_status, StatusCode::OK, "compare failed: {cmp_body}");
    assert!(cmp_body["data"]["rulesAdded"].is_array());
    let (label_status, label_body) = call(
        &app.router,
        "PUT",
        &format!("/api/system/audit-config/snapshots/{del_id}"),
        Some(&cookie),
        Some(json!({ "name": "baseline", "note": "smoke" })),
    )
    .await;
    assert_eq!(
        label_status,
        StatusCode::OK,
        "label snapshot failed: {label_body}"
    );
    let (kw_status, kw_body) = call(
        &app.router,
        "GET",
        "/api/system/audit-config/snapshots?keyword=baseline",
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(kw_status, StatusCode::OK);
    assert!(!kw_body["data"]["data"].as_array().unwrap().is_empty());
    let (del_status, del_body) = call(
        &app.router,
        "DELETE",
        &format!("/api/system/audit-config/snapshots/{del_id}"),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(
        del_status,
        StatusCode::OK,
        "delete snapshot failed: {del_body}"
    );
    let (clean_status, clean_body) = call(
        &app.router,
        "DELETE",
        "/api/system/audit-config/snapshots?keep=1",
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(
        clean_status,
        StatusCode::OK,
        "cleanup snapshots failed: {clean_body}"
    );

    // 密码策略
    let (policy_status, policy_body) = call(
        &app.router,
        "GET",
        "/api/system/password-policy",
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(policy_status, StatusCode::OK, "policy: {policy_body}");
    assert_eq!(policy_body["data"]["minLength"], 10);
    let default_policy = json!({
        "minLength": 10,
        "requireUpper": true,
        "requireLower": true,
        "requireDigit": true,
        "historyCount": 5,
        "expiresDays": 0
    });
    let (put_status, put_body) = call(
        &app.router,
        "PUT",
        "/api/system/password-policy",
        Some(&cookie),
        Some(json!({
            "minLength": 12,
            "requireUpper": true,
            "requireLower": true,
            "requireDigit": true,
            "historyCount": 3,
            "expiresDays": 0
        })),
    )
    .await;
    assert_eq!(put_status, StatusCode::OK, "put policy: {put_body}");
    let (reset_status, reset_body) = call(
        &app.router,
        "PUT",
        "/api/system/password-policy",
        Some(&cookie),
        Some(default_policy),
    )
    .await;
    assert_eq!(reset_status, StatusCode::OK, "reset policy: {reset_body}");

    let (lp_status, lp_body) = call(
        &app.router,
        "GET",
        "/api/system/login-policy",
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(lp_status, StatusCode::OK, "login policy: {lp_body}");
    assert_eq!(lp_body["data"]["maxFailedAttempts"], 5);
    let (lpu_status, lpu_body) = call(
        &app.router,
        "PUT",
        "/api/system/login-policy",
        Some(&cookie),
        Some(json!({
            "maxFailedAttempts": 5,
            "lockSeconds": 900,
            "warningPercent": 60
        })),
    )
    .await;
    assert_eq!(lpu_status, StatusCode::OK, "put login policy: {lpu_body}");
    let policy_audits: i64 = {
        // 与登录会话保持同一租户，避免多租户库下取错行
        let tenant: sqlx::types::Uuid =
            sqlx::query_scalar("SELECT id FROM sys.sys_tenant WHERE code = 'default'")
                .fetch_one(&app.state.db)
                .await
                .unwrap();
        let mut tx = app.state.db.begin().await.unwrap();
        sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
            .bind(tenant.to_string())
            .execute(&mut *tx)
            .await
            .unwrap();
        let n: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM sys.sys_audit_log
             WHERE action IN ('system.password_policy.update', 'system.login_policy.update')",
        )
        .fetch_one(&mut *tx)
        .await
        .unwrap();
        let detail: serde_json::Value = sqlx::query_scalar(
            "SELECT detail FROM sys.sys_audit_log
             WHERE action = 'system.login_policy.update'
             ORDER BY created_at DESC LIMIT 1",
        )
        .fetch_one(&mut *tx)
        .await
        .unwrap();
        assert!(detail.get("old").is_some(), "audit old missing: {detail}");
        assert!(detail.get("new").is_some(), "audit new missing: {detail}");
        tx.commit().await.unwrap();
        n
    };
    assert!(policy_audits >= 2, "policy audits: {policy_audits}");

    // 个人访问令牌
    let (tok_status, tok_body) = call(
        &app.router,
        "POST",
        "/api/auth/tokens",
        Some(&cookie),
        Some(json!({ "name": "smoke-token", "expiresInDays": 1 })),
    )
    .await;
    assert_eq!(
        tok_status,
        StatusCode::OK,
        "create token failed: {tok_body}"
    );
    let plain_token = tok_body["data"]["token"].as_str().unwrap().to_string();
    let token_id = tok_body["data"]["tokenInfo"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let (me_status, me_body) = call_bearer(&app.router, "/api/auth/me", &plain_token).await;
    assert_eq!(me_status, StatusCode::OK, "bearer auth failed");
    assert_eq!(me_body["data"]["username"], "admin");
    let (rev_status, rev_body) = call(
        &app.router,
        "DELETE",
        &format!("/api/auth/tokens/{token_id}"),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(
        rev_status,
        StatusCode::OK,
        "revoke token failed: {rev_body}"
    );

    let (sc_status, sc_body) = call(
        &app.router,
        "POST",
        "/api/auth/tokens",
        Some(&cookie),
        Some(json!({
            "name": "scoped-token",
            "scopes": ["system:user:list"]
        })),
    )
    .await;
    assert_eq!(sc_status, StatusCode::OK, "create scoped token: {sc_body}");
    let scoped_token = sc_body["data"]["token"].as_str().unwrap().to_string();
    let scoped_id = sc_body["data"]["tokenInfo"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let (users_status, users_body) = call_bearer(
        &app.router,
        "/api/system/users?page=1&size=1",
        &scoped_token,
    )
    .await;
    assert_eq!(users_status, StatusCode::OK, "scoped users: {users_body}");
    let (audit_status, _) = call_bearer(
        &app.router,
        "/api/system/audit-logs?page=1&size=1",
        &scoped_token,
    )
    .await;
    assert_eq!(audit_status, StatusCode::FORBIDDEN);
    let (roles_status, _) = call_bearer(&app.router, "/api/system/roles", &scoped_token).await;
    assert_eq!(roles_status, StatusCode::FORBIDDEN);
    let (crm_status, _) = call_bearer(
        &app.router,
        "/api/crm/customers?page=1&size=1",
        &scoped_token,
    )
    .await;
    assert_eq!(crm_status, StatusCode::FORBIDDEN);
    let _ = call(
        &app.router,
        "DELETE",
        &format!("/api/auth/tokens/{scoped_id}"),
        Some(&cookie),
        None,
    )
    .await;

    // API 文档：运行时契约必须包含插件路径，/docs 可匿名访问
    let (status, spec) = call(&app.router, "GET", "/api/openapi.json", None, None).await;
    assert_eq!(status, StatusCode::OK, "openapi.json failed");
    assert!(
        spec["paths"]["/api/crm/customers"].is_object(),
        "runtime openapi missing plugin paths"
    );
    let (status, _) = call(&app.router, "GET", "/docs", None, None).await;
    assert_eq!(status, StatusCode::OK, "docs page failed");

    // 6. 指标端点
    let (status, _) = call(&app.router, "GET", "/metrics", None, None).await;
    assert_eq!(status, StatusCode::OK, "metrics failed");

    // 审计按月分区：至少存在一个月份分区
    let partition_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM pg_class WHERE relname LIKE 'sys_audit_log_2%' AND relkind = 'r'",
    )
    .fetch_one(&app.state.db)
    .await
    .expect("query partitions");
    assert!(partition_count >= 1, "audit partitions missing");

    // 租户管理：创建 → 列表 → 停用 → 清理
    let tenant_code = format!("t{}", suffix % 1_000_000);
    let (tenant_status, tenant_body) = call(
        &app.router,
        "POST",
        "/api/system/tenants",
        Some(&cookie),
        Some(json!({
            "code": tenant_code,
            "name": "Smoke Tenant",
            "adminUsername": format!("tadmin_{suffix}"),
            "adminDisplayName": "Tenant Admin",
            "adminPassword": "Tenant#12345",
            "maxUsers": 2,
            "maxStorageBytes": 1048576
        })),
    )
    .await;
    assert_eq!(
        tenant_status,
        StatusCode::OK,
        "create tenant: {tenant_body}"
    );
    let new_tenant = tenant_body["data"]["id"].as_str().unwrap().to_string();
    let (summary_status, summary_body) = call(
        &app.router,
        "GET",
        &format!("/api/system/tenants/{new_tenant}/summary"),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(
        summary_status,
        StatusCode::OK,
        "tenant summary: {summary_body}"
    );
    assert_eq!(summary_body["data"]["userCount"], 1);
    assert_eq!(summary_body["data"]["customerCount"], 0);
    assert_eq!(summary_body["data"]["opportunityCount"], 0);
    let (billing_status, billing_body) = call(
        &app.router,
        "PUT",
        &format!("/api/system/tenants/{new_tenant}/billing"),
        Some(&cookie),
        Some(json!({
            "pricePerUserMonthCents": 100,
            "pricePerGbMonthCents": 0,
            "currency": "CNY"
        })),
    )
    .await;
    assert_eq!(
        billing_status,
        StatusCode::OK,
        "billing config: {billing_body}"
    );
    let (bill_status, bill_body) = call(
        &app.router,
        "GET",
        &format!("/api/system/tenants/{new_tenant}/bill"),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(bill_status, StatusCode::OK, "tenant bill: {bill_body}");
    assert_eq!(bill_body["data"]["totalCents"], 100);
    let (generate_status, generate_body) = call(
        &app.router,
        "POST",
        "/api/system/tenants/bills/generate",
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(
        generate_status,
        StatusCode::OK,
        "generate bills: {generate_body}"
    );
    assert!(generate_body["data"]["generated"].as_u64().unwrap_or(0) >= 1);
    let (backfill_status, backfill_body) = call(
        &app.router,
        "POST",
        "/api/system/tenants/bills/backfill?from=2020-01&to=2020-01",
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(
        backfill_status,
        StatusCode::OK,
        "backfill bills: {backfill_body}"
    );
    assert_eq!(backfill_body["data"]["months"], 1);
    assert!(backfill_body["data"]["bills"].as_u64().unwrap_or(0) >= 1);
    let (_, all_bills) = call(
        &app.router,
        "GET",
        "/api/system/tenants/bills",
        Some(&cookie),
        None,
    )
    .await;
    let old_bill_id = all_bills["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| {
            b["tenantCode"] == tenant_code
                && b["billMonth"]
                    .as_str()
                    .unwrap_or_default()
                    .starts_with("2020-01")
        })
        .and_then(|b| b["id"].as_str())
        .expect("old bill")
        .to_string();
    let (issue_old_status, issue_old_body) = call(
        &app.router,
        "PUT",
        &format!("/api/system/tenants/bills/{old_bill_id}/status"),
        Some(&cookie),
        Some(json!({ "status": "issued" })),
    )
    .await;
    assert_eq!(
        issue_old_status,
        StatusCode::OK,
        "issue old bill: {issue_old_body}"
    );
    let (overdue_status, overdue_body) = call(
        &app.router,
        "POST",
        "/api/system/tenants/bills/overdue/scan?days=0",
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(
        overdue_status,
        StatusCode::OK,
        "overdue scan: {overdue_body}"
    );
    assert!(
        overdue_body["data"]["scanned"].as_u64().unwrap_or(0) >= 1,
        "overdue scan should hit old bill"
    );
    let (second_status, second_body) = call(
        &app.router,
        "POST",
        "/api/system/tenants/bills/overdue/scan?days=0",
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(second_status, StatusCode::OK, "second scan: {second_body}");
    assert_eq!(second_body["data"]["scanned"], 0, "overdue scan dedupe");
    let (bills_status, bills_body) = call(
        &app.router,
        "GET",
        "/api/system/tenants/bills",
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(bills_status, StatusCode::OK, "list bills: {bills_body}");
    let bill_id = bills_body["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["tenantCode"] == tenant_code)
        .and_then(|b| b["id"].as_str())
        .expect("bill id")
        .to_string();
    let (issued_status, issued_body) = call(
        &app.router,
        "PUT",
        &format!("/api/system/tenants/bills/{bill_id}/status"),
        Some(&cookie),
        Some(json!({ "status": "issued" })),
    )
    .await;
    assert_eq!(issued_status, StatusCode::OK, "issue bill: {issued_body}");
    let (pay1_status, pay1_body) = call(
        &app.router,
        "POST",
        &format!("/api/system/tenants/bills/{bill_id}/payments"),
        Some(&cookie),
        Some(json!({
            "amountCents": 40,
            "method": "bank",
            "reference": "SMOKE-1"
        })),
    )
    .await;
    assert_eq!(pay1_status, StatusCode::OK, "partial payment: {pay1_body}");
    let (pay2_status, pay2_body) = call(
        &app.router,
        "POST",
        &format!("/api/system/tenants/bills/{bill_id}/payments"),
        Some(&cookie),
        Some(json!({
            "amountCents": 60,
            "method": "bank",
            "reference": "SMOKE-2"
        })),
    )
    .await;
    assert_eq!(pay2_status, StatusCode::OK, "final payment: {pay2_body}");
    let (payments_status, payments_body) = call(
        &app.router,
        "GET",
        &format!("/api/system/tenants/bills/{bill_id}/payments"),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(payments_status, StatusCode::OK);
    assert_eq!(payments_body["data"].as_array().unwrap().len(), 2);
    let (_, paid_list) = call(
        &app.router,
        "GET",
        "/api/system/tenants/bills",
        Some(&cookie),
        None,
    )
    .await;
    assert!(paid_list["data"]
        .as_array()
        .unwrap()
        .iter()
        .any(|b| b["id"] == bill_id && b["status"] == "paid"));
    let (refund_status, refund_body) = call(
        &app.router,
        "POST",
        &format!("/api/system/tenants/bills/{bill_id}/refunds"),
        Some(&cookie),
        Some(json!({ "amountCents": 40, "reason": "smoke refund" })),
    )
    .await;
    assert_eq!(refund_status, StatusCode::OK, "refund bill: {refund_body}");
    let (_, refunded_list) = call(
        &app.router,
        "GET",
        "/api/system/tenants/bills",
        Some(&cookie),
        None,
    )
    .await;
    let refunded_row = refunded_list["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["id"] == bill_id)
        .unwrap();
    assert_eq!(refunded_row["status"], "issued");
    assert_eq!(refunded_row["paidCents"], 60);
    let (recon_status, recon_body) = call(
        &app.router,
        "GET",
        "/api/system/tenants/bills/reconciliation",
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(recon_status, StatusCode::OK, "reconciliation: {recon_body}");
    let recon_row = recon_body["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["tenantCode"] == tenant_code)
        .expect("reconciliation row");
    assert!(recon_row["totalCents"].as_i64().unwrap_or(0) >= 100);
    assert_eq!(recon_row["netCents"], 60);
    assert_eq!(
        recon_row["outstandingCents"].as_i64().unwrap_or(0),
        recon_row["totalCents"].as_i64().unwrap_or(0) - 60
    );
    let export_req = Request::builder()
        .method("GET")
        .uri("/api/system/tenants/bills/export")
        .header(header::COOKIE, &cookie)
        .body(Body::empty())
        .unwrap();
    let export_resp = app
        .router
        .clone()
        .oneshot(export_req)
        .await
        .expect("bill export");
    assert_eq!(export_resp.status(), StatusCode::OK);
    assert_eq!(
        export_resp
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default(),
        "text/csv; charset=utf-8"
    );
    let export_bytes = axum::body::to_bytes(export_resp.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let export_text = String::from_utf8_lossy(&export_bytes);
    assert!(export_text.contains(&tenant_code), "csv missing tenant");
    let xlsx_req = Request::builder()
        .method("GET")
        .uri("/api/system/tenants/bills/export.xlsx")
        .header(header::COOKIE, &cookie)
        .body(Body::empty())
        .unwrap();
    let xlsx_resp = app
        .router
        .clone()
        .oneshot(xlsx_req)
        .await
        .expect("xlsx export");
    assert_eq!(xlsx_resp.status(), StatusCode::OK);
    assert_eq!(
        xlsx_resp
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default(),
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
    );
    let xlsx_bytes = axum::body::to_bytes(xlsx_resp.into_body(), 4 * 1024 * 1024)
        .await
        .unwrap();
    assert!(xlsx_bytes.starts_with(b"PK"), "xlsx should be zip");
    let (capture_status, capture_body) = call(
        &app.router,
        "POST",
        "/api/system/tenants/usage/capture",
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(
        capture_status,
        StatusCode::OK,
        "capture usage: {capture_body}"
    );
    assert!(capture_body["data"]["captured"].as_u64().unwrap_or(0) >= 1);
    let (usage_status, usage_body) = call(
        &app.router,
        "GET",
        &format!("/api/system/tenants/{new_tenant}/usage?days=30"),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(usage_status, StatusCode::OK, "usage history: {usage_body}");
    assert!(!usage_body["data"].as_array().unwrap().is_empty());
    let tenant_login_req = Request::builder()
        .method("POST")
        .uri("/api/auth/login")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({
                "username": format!("tadmin_{suffix}"),
                "password": "Tenant#12345",
                "tenant": tenant_code
            })
            .to_string(),
        ))
        .unwrap();
    let tenant_login = app
        .router
        .clone()
        .oneshot(tenant_login_req)
        .await
        .expect("tenant login");
    assert_eq!(tenant_login.status(), StatusCode::OK, "tenant admin login");
    let tenant_cookie = tenant_login
        .headers()
        .get(header::SET_COOKIE)
        .and_then(|v| v.to_str().ok())
        .expect("tenant cookie")
        .split(';')
        .next()
        .unwrap()
        .to_string();
    let (tenant_pwd_status, tenant_pwd_body) = call(
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
        tenant_pwd_status,
        StatusCode::OK,
        "tenant change password: {tenant_pwd_body}"
    );
    let tenant_login2_req = Request::builder()
        .method("POST")
        .uri("/api/auth/login")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({
                "username": format!("tadmin_{suffix}"),
                "password": "TenantNew#123",
                "tenant": tenant_code
            })
            .to_string(),
        ))
        .unwrap();
    let tenant_login2 = app
        .router
        .clone()
        .oneshot(tenant_login2_req)
        .await
        .expect("tenant login 2");
    assert_eq!(tenant_login2.status(), StatusCode::OK, "tenant relogin");
    let tenant_cookie2 = tenant_login2
        .headers()
        .get(header::SET_COOKIE)
        .and_then(|v| v.to_str().ok())
        .expect("tenant cookie 2")
        .split(';')
        .next()
        .unwrap()
        .to_string();
    let (quota_user_status, quota_user_body) = call(
        &app.router,
        "POST",
        "/api/system/users",
        Some(&tenant_cookie2),
        Some(json!({
            "username": format!("quota_user_{suffix}"),
            "displayName": "Quota User",
            "password": "OverQuota#123"
        })),
    )
    .await;
    assert_eq!(
        quota_user_status,
        StatusCode::OK,
        "second user should fit quota: {quota_user_body}"
    );
    let (over_status, over_body) = call(
        &app.router,
        "POST",
        "/api/system/users",
        Some(&tenant_cookie2),
        Some(json!({
            "username": format!("over_quota_{suffix}"),
            "displayName": "Over Quota",
            "password": "OverQuota#123"
        })),
    )
    .await;
    assert_eq!(
        over_status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "user quota should reject: {over_body}"
    );
    let quota_alerts: i64 = {
        let mut tx = app.state.db.begin().await.unwrap();
        sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
            .bind(&new_tenant)
            .execute(&mut *tx)
            .await
            .unwrap();
        let n: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM sys.sys_audit_alert
             WHERE action = 'system.tenant.quota.user'",
        )
        .fetch_one(&mut *tx)
        .await
        .unwrap();
        tx.commit().await.unwrap();
        n
    };
    assert!(quota_alerts >= 1, "quota warning missing");
    let (tenants_status, tenants_body) = call(
        &app.router,
        "GET",
        "/api/system/tenants",
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(tenants_status, StatusCode::OK);
    assert!(tenants_body["data"]
        .as_array()
        .unwrap()
        .iter()
        .any(|t| t["code"] == tenant_code));
    let created_tenant = tenants_body["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["code"] == tenant_code)
        .unwrap();
    assert_eq!(created_tenant["maxUsers"], 2);
    assert_eq!(created_tenant["maxStorageBytes"], 1048576);
    let (quota_status, quota_body) = call(
        &app.router,
        "PUT",
        &format!("/api/system/tenants/{new_tenant}/quota"),
        Some(&cookie),
        Some(json!({ "maxUsers": 10 })),
    )
    .await;
    assert_eq!(quota_status, StatusCode::OK, "quota: {quota_body}");
    let (suspend_status, suspend_body) = call(
        &app.router,
        "PUT",
        &format!("/api/system/tenants/{new_tenant}/status"),
        Some(&cookie),
        Some(json!({ "status": "suspended" })),
    )
    .await;
    assert_eq!(suspend_status, StatusCode::OK, "suspend: {suspend_body}");
    let (delete_status, delete_body) = call(
        &app.router,
        "DELETE",
        &format!("/api/system/tenants/{new_tenant}"),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(
        delete_status,
        StatusCode::OK,
        "delete tenant: {delete_body}"
    );
    let (_, after_delete) = call(
        &app.router,
        "GET",
        "/api/system/tenants",
        Some(&cookie),
        None,
    )
    .await;
    assert!(
        !after_delete["data"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["code"] == tenant_code),
        "deleted tenant should disappear"
    );
    let (restore_status, restore_body) = call(
        &app.router,
        "POST",
        &format!("/api/system/tenants/{new_tenant}/restore"),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(
        restore_status,
        StatusCode::OK,
        "restore tenant: {restore_body}"
    );
    let (_, restored) = call(
        &app.router,
        "GET",
        "/api/system/tenants",
        Some(&cookie),
        None,
    )
    .await;
    let restored_row = restored["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["code"] == tenant_code)
        .expect("restored tenant");
    assert_eq!(restored_row["status"], "active");
    let (delete2_status, _) = call(
        &app.router,
        "DELETE",
        &format!("/api/system/tenants/{new_tenant}"),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(delete2_status, StatusCode::OK);
    let (purge_status, purge_body) = call(
        &app.router,
        "POST",
        "/api/system/tenants/purge?retentionDays=0",
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(purge_status, StatusCode::OK, "purge tenant: {purge_body}");
    assert!(
        purge_body["data"]["purged"].as_u64().unwrap_or(0) >= 1,
        "tenant should be purged: {purge_body}"
    );
    assert!(
        purge_body["data"]["details"][0]["users"]
            .as_i64()
            .unwrap_or(0)
            >= 1,
        "purge details missing: {purge_body}"
    );

    // 清理新租户
    {
        let mut tx = app.state.db.begin().await.unwrap();
        sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
            .bind(&new_tenant)
            .execute(&mut *tx)
            .await
            .unwrap();
        for sql in [
            "DELETE FROM sys.sys_session WHERE tenant_id = $1",
            "DELETE FROM sys.sys_menu WHERE tenant_id = $1",
            "DELETE FROM sys.sys_user WHERE tenant_id = $1",
        ] {
            sqlx::query(sql)
                .bind(sqlx::types::Uuid::parse_str(&new_tenant).unwrap())
                .execute(&mut *tx)
                .await
                .unwrap();
        }
        tx.commit().await.unwrap();
    }
    sqlx::query("DELETE FROM sys.sys_tenant WHERE id = $1")
        .bind(sqlx::types::Uuid::parse_str(&new_tenant).unwrap())
        .execute(&app.state.db)
        .await
        .unwrap();

    // 审计保留：0 天 = 清理全部，验证逐租户清理逻辑
    let purged = kernel::audit::purge_older_than(&app.state.db, 0)
        .await
        .expect("purge audit");
    assert!(purged > 0, "expected audit rows to be purged");

    println!("smoke_end_to_end: all checks passed");
}
