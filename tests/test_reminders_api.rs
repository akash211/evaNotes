mod common;

use axum_test::TestServer;
use common::init_test_db;
use evanotes::auth::session::generate_signed_token;
use evanotes::auth::OtpRateLimiter;
use evanotes::create_app;
use evanotes::db::users::create_or_get_user;
use evanotes::services::reminder_engine::compute_next_cron_trigger;
use evanotes::{AppConfig, AppState, NotificationHub};
use serde_json::json;

fn setup_test_env() -> (
    TestServer,
    String,
    String,
    evanotes::DbPool,
    NotificationHub,
    tempfile::TempDir,
) {
    let (pool, dir) = init_test_db();
    let mut config = AppConfig::from_env();
    config.session_secret = "test-secret-32-chars-long-abcdefghijk".to_string();

    let (token_a, token_b) = {
        let conn = pool.get().unwrap();
        let user_a = create_or_get_user(&conn, "rem_a@test.com", "otp", true).unwrap();
        let user_b = create_or_get_user(&conn, "rem_b@test.com", "otp", false).unwrap();

        let t_a = generate_signed_token(&user_a.id, &config.session_secret);
        let t_b = generate_signed_token(&user_b.id, &config.session_secret);

        let expires = (chrono::Utc::now() + chrono::Duration::days(30)).to_rfc3339();
        evanotes::db::users::create_session(&conn, &t_a, &user_a.id, &expires).unwrap();
        evanotes::db::users::create_session(&conn, &t_b, &user_b.id, &expires).unwrap();

        (t_a, t_b)
    };

    let otp_limiter = OtpRateLimiter::new();
    let hub = NotificationHub::new();
    let state = AppState {
        pool: pool.clone(),
        config,
        otp_limiter,
        hub: hub.clone(),
    };

    let app = create_app(state);
    let server = TestServer::new(app).expect("Failed to create test server");
    (server, token_a, token_b, pool, hub, dir)
}

#[tokio::test]
async fn test_reminders_crud_api() {
    let (server, token_a, _, _, _, _dir) = setup_test_env();

    // 1. Create one-time reminder
    let future_time = (chrono::Utc::now() + chrono::Duration::hours(3)).to_rfc3339();
    let create_res = server
        .post("/api/reminders")
        .add_header(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {token_a}"),
        )
        .json(&json!({
            "title": "Pick up dry cleaning",
            "description": "Ticket #4102",
            "remind_at": future_time
        }))
        .await;
    assert_eq!(create_res.status_code(), 201);
    let rem1_json: serde_json::Value = create_res.json();
    let rem1_id = rem1_json["id"].as_str().unwrap().to_string();
    assert_eq!(rem1_json["title"], "Pick up dry cleaning");
    assert_eq!(rem1_json["state"], "pending");

    // 2. Create recurring reminder (daily)
    let rec_res = server
        .post("/api/reminders")
        .add_header(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {token_a}"),
        )
        .json(&json!({
            "title": "Team Daily Standup",
            "remind_at": future_time,
            "is_recurring": true,
            "recur_type": "daily"
        }))
        .await;
    assert_eq!(rec_res.status_code(), 201);
    let rec_json: serde_json::Value = rec_res.json();
    assert_eq!(rec_json["is_recurring"], true);
    assert_eq!(rec_json["cron_expr"], "0 9 * * *");

    // 3. Get reminder by ID
    let get_res = server
        .get(&format!("/api/reminders/{rem1_id}"))
        .add_header(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {token_a}"),
        )
        .await;
    assert_eq!(get_res.status_code(), 200);

    // 4. Update reminder
    let update_res = server
        .put(&format!("/api/reminders/{rem1_id}"))
        .add_header(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {token_a}"),
        )
        .json(&json!({
            "title": "Pick up dry cleaning (Urgent)"
        }))
        .await;
    assert_eq!(update_res.status_code(), 200);
    assert_eq!(
        update_res.json::<serde_json::Value>()["title"],
        "Pick up dry cleaning (Urgent)"
    );

    // 5. Snooze reminder
    let snooze_res = server
        .post(&format!("/api/reminders/{rem1_id}/snooze"))
        .add_header(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {token_a}"),
        )
        .json(&json!({ "duration_minutes": 30 }))
        .await;
    assert_eq!(snooze_res.status_code(), 200);
    let snooze_json: serde_json::Value = snooze_res.json();
    assert_eq!(snooze_json["state"], "snoozed");
    assert!(snooze_json["snooze_until"].is_string());

    // 6. Acknowledge reminder
    let ack_res = server
        .post(&format!("/api/reminders/{rem1_id}/acknowledge"))
        .add_header(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {token_a}"),
        )
        .json(&json!({ "action": "acknowledge" }))
        .await;
    assert_eq!(ack_res.status_code(), 200);
    assert_eq!(ack_res.json::<serde_json::Value>()["state"], "acknowledged");

    // 7. List reminders
    let list_res = server
        .get("/api/reminders")
        .add_header(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {token_a}"),
        )
        .await;
    assert_eq!(list_res.status_code(), 200);
    let list_json: serde_json::Value = list_res.json();
    assert_eq!(list_json["pagination"]["total"], 2);

    // 8. Delete reminder
    let del_res = server
        .delete(&format!("/api/reminders/{rem1_id}"))
        .add_header(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {token_a}"),
        )
        .await;
    assert_eq!(del_res.status_code(), 200);
}

#[tokio::test]
async fn test_reminders_security_idor() {
    let (server, token_a, token_b, _, _, _dir) = setup_test_env();

    // User A creates reminder
    let res = server
        .post("/api/reminders")
        .add_header(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {token_a}"),
        )
        .json(&json!({
            "title": "Private Medical Reminder",
            "remind_at": "2026-10-15T10:00:00Z"
        }))
        .await;
    let rem_id = res.json::<serde_json::Value>()["id"]
        .as_str()
        .unwrap()
        .to_string();

    // User B attempts to access -> 404
    let b_get = server
        .get(&format!("/api/reminders/{rem_id}"))
        .add_header(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {token_b}"),
        )
        .await;
    assert_eq!(b_get.status_code(), 404);

    let b_put = server
        .put(&format!("/api/reminders/{rem_id}"))
        .add_header(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {token_b}"),
        )
        .json(&json!({ "title": "Hacked" }))
        .await;
    assert_eq!(b_put.status_code(), 404);

    let b_snooze = server
        .post(&format!("/api/reminders/{rem_id}/snooze"))
        .add_header(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {token_b}"),
        )
        .json(&json!({ "duration_minutes": 15 }))
        .await;
    assert_eq!(b_snooze.status_code(), 404);

    let b_del = server
        .delete(&format!("/api/reminders/{rem_id}"))
        .add_header(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {token_b}"),
        )
        .await;
    assert_eq!(b_del.status_code(), 404);
}

#[test]
fn test_cron_expressions() {
    // 5-field daily at 9am: "0 9 * * *"
    let next_daily = compute_next_cron_trigger("0 9 * * *");
    assert!(next_daily.is_some());

    // 5-field weekly on Monday at 9am: "0 9 * * 1"
    let next_weekly = compute_next_cron_trigger("0 9 * * 1");
    assert!(next_weekly.is_some());

    // 6-field with seconds: "0 0 9 * * *"
    let next_6 = compute_next_cron_trigger("0 0 9 * * *");
    assert!(next_6.is_some());

    // Invalid expression
    let invalid = compute_next_cron_trigger("invalid-cron-string");
    assert!(invalid.is_none());
}
