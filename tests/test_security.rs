mod common;

use axum_test::TestServer;
use chrono::{Duration as ChronoDuration, Utc};
use common::init_test_db;
use evanotes::auth::session::generate_signed_token;
use evanotes::auth::OtpRateLimiter;
use evanotes::create_app;
use evanotes::db::users::{create_or_get_user, create_session};
use evanotes::{AppConfig, AppState};
use serde_json::json;

fn setup_test_server() -> (TestServer, String, AppState, tempfile::TempDir) {
    let (pool, dir) = init_test_db();
    let mut config = AppConfig::from_env();
    config.session_secret = "test-secret-32-chars-long-abcdefghijk".to_string();

    let token = {
        let conn = pool.get().unwrap();
        let user = create_or_get_user(&conn, "admin@example.com", "otp", true).unwrap();
        let tok = generate_signed_token(&user.id, &config.session_secret);
        let expires = (Utc::now() + ChronoDuration::days(30)).to_rfc3339();
        create_session(&conn, &tok, &user.id, &expires).unwrap();
        tok
    };

    let otp_limiter = OtpRateLimiter::new();
    let hub = evanotes::NotificationHub::new();

    let state = AppState {
        pool,
        config,
        otp_limiter,
        hub,
    };

    let app = create_app(state.clone());
    let server = TestServer::new(app).expect("Failed to create test server");
    (server, token, state, dir)
}

#[tokio::test]
async fn test_security_headers_present() {
    let (server, _, _, _dir) = setup_test_server();

    let res = server.get("/api/auth/status").await;
    assert_eq!(res.status_code(), 200);

    let headers = res.headers();
    assert_eq!(
        headers.get("x-content-type-options").unwrap(),
        "nosniff",
        "X-Content-Type-Options must be nosniff"
    );
    assert_eq!(
        headers.get("x-frame-options").unwrap(),
        "DENY",
        "X-Frame-Options must be DENY"
    );
    assert_eq!(
        headers.get("x-xss-protection").unwrap(),
        "0",
        "X-XSS-Protection must be 0"
    );
    assert_eq!(
        headers.get("referrer-policy").unwrap(),
        "strict-origin-when-cross-origin",
        "Referrer-Policy must be strict-origin-when-cross-origin"
    );
    assert!(
        headers.get("content-security-policy").is_some(),
        "Content-Security-Policy must be set"
    );
}

#[tokio::test]
async fn test_body_size_limit_enforced() {
    let (server, token, _, _dir) = setup_test_server();

    // Create a payload larger than 1MB (1,050,000 bytes)
    let large_text = "A".repeat(1_050_000);
    let payload = json!({
        "title": "Large Note",
        "content": large_text,
        "note_type": "simple"
    });

    let res = server
        .post("/api/notes")
        .add_header("Authorization", format!("Bearer {token}"))
        .json(&payload)
        .await;

    // Axum DefaultBodyLimit returns 413 Payload Too Large
    assert_eq!(
        res.status_code(),
        413,
        "Requests exceeding 1MB must be rejected with 413"
    );
}

#[tokio::test]
async fn test_sql_injection_defense() {
    let (server, token, _, _dir) = setup_test_server();

    // 1. Attempt SQL injection in note title
    let sql_injection_title = "My Note'; DROP TABLE notes; --";
    let payload = json!({
        "title": sql_injection_title,
        "content": "Safe content",
        "note_type": "simple"
    });

    let create_res = server
        .post("/api/notes")
        .add_header("Authorization", format!("Bearer {token}"))
        .json(&payload)
        .await;
    assert_eq!(create_res.status_code(), 201);
    let note: serde_json::Value = create_res.json();
    assert_eq!(note["title"], sql_injection_title);

    // 2. Verify notes table was NOT dropped and note can still be fetched
    let get_res = server
        .get(&format!("/api/notes/{}", note["id"].as_str().unwrap()))
        .add_header("Authorization", format!("Bearer {token}"))
        .await;
    assert_eq!(get_res.status_code(), 200);

    // 3. Attempt SQL injection in search query
    let search_res = server
        .get("/api/notes?search=%27%20OR%201=1%20--")
        .add_header("Authorization", format!("Bearer {token}"))
        .await;
    assert_eq!(search_res.status_code(), 200);
    let list_data: serde_json::Value = search_res.json();
    assert_eq!(
        list_data["pagination"]["total"], 0,
        "SQL injection in search parameter must not leak notes"
    );

    // 4. Attempt SQL injection in reminder title
    let rem_payload = json!({
        "title": "Reminder'; DROP TABLE reminders; --",
        "remind_at": (Utc::now() + ChronoDuration::hours(2)).to_rfc3339()
    });
    let rem_res = server
        .post("/api/reminders")
        .add_header("Authorization", format!("Bearer {token}"))
        .json(&rem_payload)
        .await;
    assert_eq!(rem_res.status_code(), 201);
}

#[tokio::test]
async fn test_xss_protection_in_markdown_notes() {
    let (server, token, _, _dir) = setup_test_server();

    let xss_content = r#"
# Malicious Markdown
<script>alert('xss')</script>
<img src="x" onerror="alert('xss')">
[Malicious Link](javascript:alert('xss'))
"#;

    let payload = json!({
        "title": "XSS Test Note",
        "content": xss_content,
        "note_type": "detailed"
    });

    let create_res = server
        .post("/api/notes")
        .add_header("Authorization", format!("Bearer {token}"))
        .json(&payload)
        .await;
    assert_eq!(create_res.status_code(), 201);
    let note_id = create_res.json::<serde_json::Value>()["id"]
        .as_str()
        .unwrap()
        .to_string();

    let get_res = server
        .get(&format!("/api/notes/{note_id}"))
        .add_header("Authorization", format!("Bearer {token}"))
        .await;
    assert_eq!(get_res.status_code(), 200);
    let note: serde_json::Value = get_res.json();

    let html = note["content_html"].as_str().unwrap();
    assert!(
        !html.contains("<script>"),
        "Script tags must be stripped from rendered HTML"
    );
    assert!(
        !html.contains("onerror"),
        "Event handlers must be stripped from rendered HTML"
    );
    assert!(
        !html.contains("javascript:"),
        "javascript: URI schemes must be blocked from rendered HTML"
    );
}

#[tokio::test]
async fn test_path_traversal_export_sanitization() {
    let (server, token, _, _dir) = setup_test_server();

    let payload = json!({
        "title": "../../../etc/shadow",
        "content": "Secret note content",
        "note_type": "detailed"
    });

    let create_res = server
        .post("/api/notes")
        .add_header("Authorization", format!("Bearer {token}"))
        .json(&payload)
        .await;
    assert_eq!(create_res.status_code(), 201);
    let note_id = create_res.json::<serde_json::Value>()["id"]
        .as_str()
        .unwrap()
        .to_string();

    let export_res = server
        .get(&format!("/api/notes/{note_id}/export"))
        .add_header("Authorization", format!("Bearer {token}"))
        .await;
    assert_eq!(export_res.status_code(), 200);

    let disposition = export_res
        .headers()
        .get("content-disposition")
        .unwrap()
        .to_str()
        .unwrap();

    assert!(
        !disposition.contains(".."),
        "Content-Disposition filename must not contain directory traversal .."
    );
    assert!(
        !disposition.contains('/'),
        "Content-Disposition filename must not contain directory separators /"
    );
}

#[tokio::test]
async fn test_unauthenticated_and_tampered_sessions_rejected() {
    let (server, token, _, _dir) = setup_test_server();

    // 1. Unauthenticated request to /api/notes returns 401
    let res = server.get("/api/notes").await;
    assert_eq!(res.status_code(), 401);

    // 2. Tampered session cookie returns 401
    let tampered_cookie = format!("session_token={token}tampered");
    let res_tampered = server
        .get("/api/notes")
        .add_header("cookie", tampered_cookie)
        .await;
    assert_eq!(res_tampered.status_code(), 401);
}

#[tokio::test]
async fn test_invalid_uuid_rejected_safely() {
    let (server, token, _, _dir) = setup_test_server();

    // Invalid non-UUID string in path parameter
    let res = server
        .get("/api/notes/not-a-valid-uuid")
        .add_header("Authorization", format!("Bearer {token}"))
        .await;

    // Should return 400 Bad Request (or 404), never 500 panic
    assert!(
        res.status_code() == 400 || res.status_code() == 404,
        "Malformed UUID path parameter must be rejected gracefully with 400 or 404"
    );
}
