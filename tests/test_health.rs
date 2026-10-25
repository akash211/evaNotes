use axum_test::TestServer;
use evanotes::{
    auth::OtpRateLimiter, create_app, db::init_memory_db, AppConfig, AppState, PresenceHub,
};

#[tokio::test]
async fn test_health_check_status_endpoint() {
    let pool = init_memory_db().expect("Failed to initialize test DB");
    let config = AppConfig::from_env();
    let otp_limiter = OtpRateLimiter::new();
    let hub = evanotes::NotificationHub::new();

    let state = AppState {
        pool,
        config,
        otp_limiter,
        hub,
        presence: PresenceHub::new(),
    };

    let app = create_app(state);
    let server = TestServer::new(app).expect("Failed to create test server");

    let response = server.get("/api/auth/status").await;
    assert_eq!(response.status_code(), 200);

    let json: serde_json::Value = response.json();
    assert_eq!(json["status"], "ok");
    assert_eq!(json["app"], "evaNotes");
    assert_eq!(json["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(json["authenticated"], false);
}
