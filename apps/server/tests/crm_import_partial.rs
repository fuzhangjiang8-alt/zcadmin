//! 客户导入「部分成功」集成测试。
//!
//! 背景：导入在一个事务里逐行 INSERT。历史上没有 SAVEPOINT，
//! 第一行唯一键冲突会把事务置为 aborted，后续行全部 25P02，
//! 最终整批回滚——但接口仍返回「成功 N 行」。
//!
//! 本测试构造「1 行重复 + 1 行新增」的 xlsx，断言：
//!   1. 接口返回 200（不是 500）；
//!   2. success = 1、failed = 1；
//!   3. 新客户真的落库。
//!
//! 运行：
//!   $env:DATABASE_URL="postgres://zcadmin:zcadmin@127.0.0.1:5432/zcadmin"
//!   cargo test -p server --test crm_import_partial -- --ignored --nocapture

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
    cookie: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let builder = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::COOKIE, cookie);
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
        .expect("body");
    let json = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, json)
}

async fn login(router: &axum::Router, username: &str, password: &str, tenant: &str) -> String {
    let req = Request::builder()
        .method("POST")
        .uri("/api/auth/login")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({ "username": username, "password": password, "tenant": tenant }).to_string(),
        ))
        .unwrap();
    let resp = router.clone().oneshot(req).await.expect("login");
    assert_eq!(resp.status(), StatusCode::OK, "login failed");
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

/// 构造两行客户导入 xlsx（第一行重复、第二行新增）。
fn build_workbook(duplicate_code: &str, new_code: &str) -> Vec<u8> {
    let mut wb = rust_xlsxwriter::Workbook::new();
    {
        let sheet = wb.add_worksheet();
        for (i, h) in [
            "客户编码",
            "客户名称",
            "客户等级",
            "状态",
            "联系电话",
            "邮箱",
        ]
        .iter()
        .enumerate()
        {
            sheet.write_string(0, i as u16, *h).unwrap();
        }
        sheet.write_string(1, 0, duplicate_code).unwrap();
        sheet.write_string(1, 1, "重复客户").unwrap();
        sheet.write_string(1, 2, "normal").unwrap();
        sheet.write_string(1, 5, "dup@example.com").unwrap();

        sheet.write_string(2, 0, new_code).unwrap();
        sheet.write_string(2, 1, "新客户").unwrap();
        sheet.write_string(2, 2, "vip").unwrap();
        sheet.write_string(2, 5, "new@example.com").unwrap();
    }
    wb.save_to_buffer().unwrap()
}

async fn import_xlsx(router: &axum::Router, cookie: &str, xlsx: Vec<u8>) -> (StatusCode, Value) {
    let boundary = "----zcadmin-customer-import";
    let mut body = Vec::new();
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"customers.xlsx\"\r\nContent-Type: application/vnd.openxmlformats-officedocument.spreadsheetml.sheet\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(&xlsx);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());

    let req = Request::builder()
        .method("POST")
        .uri("/api/crm/customers/import")
        .header(header::COOKIE, cookie)
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body))
        .unwrap();
    let resp = router.clone().oneshot(req).await.expect("import");
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), 4 * 1024 * 1024)
        .await
        .expect("body");
    let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, json)
}

#[tokio::test]
#[ignore = "需要真实 PostgreSQL（DATABASE_URL）"]
async fn customer_import_partial_success() {
    let url = std::env::var("DATABASE_URL").expect("必须设置 DATABASE_URL");
    std::env::set_var("DATABASE_URL", &url);
    let password = std::env::var("AUTH_DEV_PASSWORD").unwrap_or_else(|_| "admin123456".into());

    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_millis();
    let duplicate = format!("DUP{suffix}");
    let fresh = format!("NEW{suffix}");

    let config = kernel::config::Config::load("config/default.toml").expect("config");
    let app = kernel::app::build(config).await.expect("build app");
    let cookie = login(&app.router, "admin", &password, "default").await;

    // 先创建一条，制造唯一键冲突
    let (status, body) = call(
        &app.router,
        "POST",
        "/api/crm/customers",
        &cookie,
        Some(json!({
            "code": duplicate,
            "name": "已存在客户",
            "level": "normal"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "create customer: {body}");

    let xlsx = build_workbook(&duplicate, &fresh);
    let (status, body) = import_xlsx(&app.router, &cookie, xlsx).await;
    assert_eq!(status, StatusCode::OK, "import should not fail整批: {body}");
    assert_eq!(body["data"]["success"], 1, "import result: {body}");
    assert_eq!(body["data"]["failed"], 1, "import result: {body}");

    // 新客户必须真的落库
    let (status, body) = call(
        &app.router,
        "GET",
        &format!("/api/crm/customers?keyword={fresh}&page=1&size=10"),
        &cookie,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "list customers: {body}");
    let total = body["data"]["page"]["total"].as_u64().unwrap_or(0);
    assert_eq!(total, 1, "新客户应已落库: {body}");

    println!("crm_import_partial: passed");
}
