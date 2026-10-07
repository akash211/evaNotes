mod common;

use axum_test::TestServer;
use common::init_test_db;
use evanotes::auth::session::generate_signed_token;
use evanotes::auth::OtpRateLimiter;
use evanotes::create_app;
use evanotes::db::users::get_latest_active_otp;
use evanotes::{AppConfig, AppState};
use serde_json::json;

fn setup_test_app(
    admin_email: Option<&str>,
) -> (TestServer, evanotes::DbPool, AppConfig, tempfile::TempDir) {
    let (pool, dir) = init_test_db();
    let mut config = AppConfig::from_env();
    config.admin_email = admin_email.map(String::from);
    config.session_secret = "test-secret-32-chars-long-abcdefghijk".to_string();

    let otp_limiter = OtpRateLimiter::new();
    let state = AppState {
        pool: pool.clone(),
        config: config.clone(),
        otp_limiter,
    };

    let app = create_app(state);
    let server = TestServer::new(app).expect("Failed to create test server");
    (server, pool, config, dir)
}

#[tokio::test]
async fn test_unauthenticated_protected_endpoint() {
    let (server, _, _, _dir) = setup_test_app(None);

    let response = server.get("/api/auth/me").await;
    assert_eq!(response.status_code(), 401);
}

#[tokio::test]
async fn test_otp_request_and_verify_flow() {
    let (server, pool, _, _dir) = setup_test_app(Some("admin@example.com"));

    // 1. Request OTP
    let req_res = server
        .post("/api/auth/otp/request")
        .json(&json!({ "email": "admin@example.com" }))
        .await;
    assert_eq!(req_res.status_code(), 200);

    // 2. Query code from database (in its own scope so conn is dropped)
    let otp_code = {
        let conn = pool.get().unwrap();
        let otp_rec = get_latest_active_otp(&conn, "admin@example.com")
            .unwrap()
            .expect("OTP must be in DB");
        otp_rec.code
    };

    // 3. Verify OTP
    let verify_res = server
        .post("/api/auth/otp/verify")
        .json(&json!({
            "email": "admin@example.com",
            "code": otp_code
        }))
        .await;
    assert_eq!(verify_res.status_code(), 200);

    let verify_json: serde_json::Value = verify_res.json();
    assert_eq!(verify_json["status"], "authenticated");
    assert_eq!(verify_json["email"], "admin@example.com");

    let token = verify_json["token"].as_str().unwrap();

    // 4. Access protected endpoint using Bearer token
    let me_res = server
        .get("/api/auth/me")
        .add_header(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {}", token),
        )
        .await;
    assert_eq!(me_res.status_code(), 200);
    let me_json: serde_json::Value = me_res.json();
    assert_eq!(me_json["email"], "admin@example.com");

    // 5. Check auth status endpoint
    let status_res = server
        .get("/api/auth/status")
        .add_header(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {}", token),
        )
        .await;
    assert_eq!(status_res.status_code(), 200);
    let status_json: serde_json::Value = status_res.json();
    assert_eq!(status_json["authenticated"], true);
    assert_eq!(status_json["email"], "admin@example.com");
}

#[tokio::test]
async fn test_otp_incorrect_code_and_lockout() {
    let (server, _, _, _dir) = setup_test_app(None);

    server
        .post("/api/auth/otp/request")
        .json(&json!({ "email": "test@example.com" }))
        .await;

    // Send wrong code 5 times
    for i in 1..=5 {
        let res = server
            .post("/api/auth/otp/verify")
            .json(&json!({
                "email": "test@example.com",
                "code": "000000"
            }))
            .await;
        assert_eq!(res.status_code(), 401, "Attempt {i} should fail with 401");
    }

    // 6th attempt: lockout
    let lockout_res = server
        .post("/api/auth/otp/verify")
        .json(&json!({
            "email": "test@example.com",
            "code": "000000"
        }))
        .await;
    assert_eq!(lockout_res.status_code(), 403);
}

#[tokio::test]
async fn test_otp_rate_limiting() {
    let (server, _, _, _dir) = setup_test_app(None);

    // 1st request
    let r1 = server
        .post("/api/auth/otp/request")
        .json(&json!({ "email": "user1@example.com" }))
        .await;
    assert_eq!(r1.status_code(), 200);

    // 2nd request
    let r2 = server
        .post("/api/auth/otp/request")
        .json(&json!({ "email": "user2@example.com" }))
        .await;
    assert_eq!(r2.status_code(), 200);

    // 3rd request should be blocked with 429
    let r3 = server
        .post("/api/auth/otp/request")
        .json(&json!({ "email": "user3@example.com" }))
        .await;
    assert_eq!(r3.status_code(), 429);
    assert!(r3.headers().contains_key(axum::http::header::RETRY_AFTER));
}

#[tokio::test]
async fn test_tampered_session_token_rejected() {
    let (server, _, _, _dir) = setup_test_app(None);

    // Forged token signed with a bogus key
    let forged_token = generate_signed_token("fake-user", "wrong-secret-key-1234567890");

    let response = server
        .get("/api/auth/me")
        .add_header(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {}", forged_token),
        )
        .await;
    assert_eq!(response.status_code(), 401);
}

#[tokio::test]
async fn test_unauthorized_email_rejected_when_admin_configured() {
    let (server, _, _, _dir) = setup_test_app(Some("owner@example.com"));

    let res = server
        .post("/api/auth/otp/request")
        .json(&json!({ "email": "hacker@example.com" }))
        .await;
    assert_eq!(res.status_code(), 401);
}

#[tokio::test]
async fn test_logout_flow() {
    let (server, pool, _, _dir) = setup_test_app(None);

    // Login via OTP
    server
        .post("/api/auth/otp/request")
        .json(&json!({ "email": "logout_test@example.com" }))
        .await;

    let otp_code = {
        let conn = pool.get().unwrap();
        let otp = get_latest_active_otp(&conn, "logout_test@example.com")
            .unwrap()
            .unwrap();
        otp.code
    };

    let login_res = server
        .post("/api/auth/otp/verify")
        .json(&json!({
            "email": "logout_test@example.com",
            "code": otp_code
        }))
        .await;
    assert_eq!(login_res.status_code(), 200);

    let token = login_res.json::<serde_json::Value>()["token"]
        .as_str()
        .unwrap()
        .to_string();

    // Verify authenticated
    let me_res = server
        .get("/api/auth/me")
        .add_header(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {}", token),
        )
        .await;
    assert_eq!(me_res.status_code(), 200);

    // Logout
    let logout_res = server
        .post("/api/auth/logout")
        .add_header(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {}", token),
        )
        .await;
    assert_eq!(logout_res.status_code(), 200);

    // Verify session is revoked
    let after_logout = server
        .get("/api/auth/me")
        .add_header(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {}", token),
        )
        .await;
    assert_eq!(after_logout.status_code(), 401);
}
