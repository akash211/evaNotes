use axum_test::TestServer;
use evanotes::{auth::OtpRateLimiter, create_app, db::init_memory_db, AppConfig, AppState};

#[tokio::test]
async fn test_static_files_and_spa_routing() {
    let pool = init_memory_db().expect("Failed to initialize test DB");
    let config = AppConfig::from_env();
    let otp_limiter = OtpRateLimiter::new();
    let hub = evanotes::NotificationHub::new();

    let state = AppState {
        pool,
        config,
        otp_limiter,
        hub,
    };

    let app = create_app(state);
    let server = TestServer::new(app).expect("Failed to create test server");

    // 1. Root serves index.html
    let res = server.get("/").await;
    assert_eq!(res.status_code(), 200);
    assert!(res.text().contains("evaNotes — Modern Notes & Reminders"));
    assert!(res.text().contains("id=\"app-container\""));

    // 2. /style.css returns CSS
    let res_css = server.get("/style.css").await;
    assert_eq!(res_css.status_code(), 200);
    assert!(res_css.text().contains("--accent-primary: #6366f1"));

    // 3. /app.js returns JavaScript
    let res_js = server.get("/app.js").await;
    assert_eq!(res_js.status_code(), 200);
    assert!(res_js
        .text()
        .contains("evaNotes — Modern Single Page Application Controller"));

    // 4. SPA fallback returns index.html for arbitrary client routes
    let res_spa = server.get("/notes/some-id").await;
    assert_eq!(res_spa.status_code(), 200);
    assert!(res_spa
        .text()
        .contains("evaNotes — Modern Notes & Reminders"));
}
