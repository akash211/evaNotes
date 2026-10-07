use axum_test::TestServer;
use evanotes::{create_app, AppConfig};

#[tokio::test]
async fn test_health_check_status_endpoint() {
    let config = AppConfig::from_env();
    let app = create_app(config);
    let server = TestServer::new(app).expect("Failed to create test server");

    let response = server.get("/api/auth/status").await;
    assert_eq!(response.status_code(), 200);

    let json: serde_json::Value = response.json();
    assert_eq!(json["status"], "ok");
    assert_eq!(json["app"], "evaNotes");
    assert_eq!(json["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(json["authenticated"], false);
}
