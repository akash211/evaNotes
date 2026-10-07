mod common;

use axum_test::TestServer;
use common::init_test_db;
use evanotes::auth::session::generate_signed_token;
use evanotes::auth::OtpRateLimiter;
use evanotes::create_app;
use evanotes::db::users::create_or_get_user;
use evanotes::{AppConfig, AppState};
use serde_json::json;

fn setup_test_env() -> (TestServer, String, String, tempfile::TempDir) {
    let (pool, dir) = init_test_db();
    let mut config = AppConfig::from_env();
    config.session_secret = "test-secret-32-chars-long-abcdefghijk".to_string();

    let (user_a_token, user_b_token) = {
        let conn = pool.get().unwrap();
        let user_a = create_or_get_user(&conn, "user_a@test.com", "otp", true).unwrap();
        let user_b = create_or_get_user(&conn, "user_b@test.com", "otp", false).unwrap();

        let token_a = generate_signed_token(&user_a.id, &config.session_secret);
        let token_b = generate_signed_token(&user_b.id, &config.session_secret);

        let expires = (chrono::Utc::now() + chrono::Duration::days(30)).to_rfc3339();
        evanotes::db::users::create_session(&conn, &token_a, &user_a.id, &expires).unwrap();
        evanotes::db::users::create_session(&conn, &token_b, &user_b.id, &expires).unwrap();

        (token_a, token_b)
    };

    let otp_limiter = OtpRateLimiter::new();
    let state = AppState {
        pool,
        config,
        otp_limiter,
    };

    let app = create_app(state);
    let server = TestServer::new(app).expect("Failed to create test server");
    (server, user_a_token, user_b_token, dir)
}

#[tokio::test]
async fn test_notes_crud_via_api() {
    let (server, token_a, _, _dir) = setup_test_env();

    // 1. Create simple note
    let res1 = server
        .post("/api/notes")
        .add_header(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {token_a}"),
        )
        .json(&json!({
            "title": "Grocery List",
            "content": "Milk, Eggs, Bread",
            "note_type": "simple",
            "color": "#10b981"
        }))
        .await;
    assert_eq!(res1.status_code(), 201);
    let note1_json: serde_json::Value = res1.json();
    let note1_id = note1_json["id"].as_str().unwrap().to_string();
    assert_eq!(note1_json["title"], "Grocery List");
    assert_eq!(note1_json["note_type"], "simple");

    // 2. Create detailed markdown note
    let res2 = server
        .post("/api/notes")
        .add_header(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {token_a}"),
        )
        .json(&json!({
            "title": "Architecture Spec",
            "content": "# Heading\n- Point A\n- Point B",
            "note_type": "detailed"
        }))
        .await;
    assert_eq!(res2.status_code(), 201);
    let note2_json: serde_json::Value = res2.json();
    let note2_id = note2_json["id"].as_str().unwrap().to_string();

    // 3. Fetch detailed note by ID and verify markdown rendered HTML
    let get_res = server
        .get(&format!("/api/notes/{note2_id}"))
        .add_header(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {token_a}"),
        )
        .await;
    assert_eq!(get_res.status_code(), 200);
    let get_json: serde_json::Value = get_res.json();
    assert_eq!(get_json["note"]["title"], "Architecture Spec");
    let html = get_json["content_html"].as_str().unwrap();
    assert!(html.contains("<h1>Heading</h1>"));
    assert!(html.contains("<li>Point A</li>"));

    // 4. Update note
    let update_res = server
        .put(&format!("/api/notes/{note1_id}"))
        .add_header(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {token_a}"),
        )
        .json(&json!({
            "title": "Updated Grocery List",
            "content": "Milk, Eggs, Bread, Organic Coffee"
        }))
        .await;
    assert_eq!(update_res.status_code(), 200);
    let update_json: serde_json::Value = update_res.json();
    assert_eq!(update_json["title"], "Updated Grocery List");

    // 5. Toggle Pin
    let pin_res = server
        .put(&format!("/api/notes/{note1_id}/pin"))
        .add_header(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {token_a}"),
        )
        .await;
    assert_eq!(pin_res.status_code(), 200);
    assert_eq!(pin_res.json::<serde_json::Value>()["is_pinned"], true);

    // 6. List notes with search
    let search_res = server
        .get("/api/notes?search=Coffee")
        .add_header(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {token_a}"),
        )
        .await;
    assert_eq!(search_res.status_code(), 200);
    let search_json: serde_json::Value = search_res.json();
    assert_eq!(search_json["pagination"]["total"], 1);
    assert_eq!(search_json["notes"][0]["id"], note1_id);

    // 7. Soft delete note
    let del_res = server
        .delete(&format!("/api/notes/{note1_id}"))
        .add_header(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {token_a}"),
        )
        .await;
    assert_eq!(del_res.status_code(), 200);

    // Verify deleted note is no longer listed
    let list_res = server
        .get("/api/notes")
        .add_header(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {token_a}"),
        )
        .await;
    let list_json: serde_json::Value = list_res.json();
    assert_eq!(list_json["pagination"]["total"], 1);
    assert_eq!(list_json["notes"][0]["id"], note2_id);
}

#[tokio::test]
async fn test_notes_export_api() {
    let (server, token_a, _, _dir) = setup_test_env();

    // Create note
    let res = server
        .post("/api/notes")
        .add_header(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {token_a}"),
        )
        .json(&json!({
            "title": "Meeting Notes - 2026",
            "content": "# Agenda\n1. Review sprint",
            "note_type": "detailed"
        }))
        .await;
    let note_id = res.json::<serde_json::Value>()["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Export note
    let export_res = server
        .get(&format!("/api/notes/{note_id}/export"))
        .add_header(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {token_a}"),
        )
        .await;
    assert_eq!(export_res.status_code(), 200);

    let content_type = export_res.header(axum::http::header::CONTENT_TYPE);
    assert!(content_type.to_str().unwrap().contains("text/markdown"));

    let content_disp = export_res.header(axum::http::header::CONTENT_DISPOSITION);
    assert!(content_disp
        .to_str()
        .unwrap()
        .contains("Meeting-Notes-2026.md"));

    assert_eq!(export_res.text(), "# Agenda\n1. Review sprint");
}

#[tokio::test]
async fn test_notes_security_idor() {
    let (server, token_a, token_b, _dir) = setup_test_env();

    // User A creates a confidential note
    let res = server
        .post("/api/notes")
        .add_header(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {token_a}"),
        )
        .json(&json!({
            "title": "User A Private Secret",
            "content": "Secret content",
            "note_type": "simple"
        }))
        .await;
    let note_id = res.json::<serde_json::Value>()["id"]
        .as_str()
        .unwrap()
        .to_string();

    // User B tries to GET User A's note -> 404
    let b_get = server
        .get(&format!("/api/notes/{note_id}"))
        .add_header(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {token_b}"),
        )
        .await;
    assert_eq!(b_get.status_code(), 404);

    // User B tries to PUT User A's note -> 404
    let b_put = server
        .put(&format!("/api/notes/{note_id}"))
        .add_header(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {token_b}"),
        )
        .json(&json!({ "title": "Tampered Title" }))
        .await;
    assert_eq!(b_put.status_code(), 404);

    // User B tries to DELETE User A's note -> 404
    let b_del = server
        .delete(&format!("/api/notes/{note_id}"))
        .add_header(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {token_b}"),
        )
        .await;
    assert_eq!(b_del.status_code(), 404);

    // User B tries to EXPORT User A's note -> 404
    let b_exp = server
        .get(&format!("/api/notes/{note_id}/export"))
        .add_header(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {token_b}"),
        )
        .await;
    assert_eq!(b_exp.status_code(), 404);
}

#[tokio::test]
async fn test_notes_xss_sanitization_in_api() {
    let (server, token_a, _, _dir) = setup_test_env();

    let res = server
        .post("/api/notes")
        .add_header(axum::http::header::AUTHORIZATION, format!("Bearer {token_a}"))
        .json(&json!({
            "title": "XSS Test Note",
            "content": "<script>alert('xss')</script><img src=\"x\" onerror=\"alert(1)\">\n\n# Safe Title",
            "note_type": "detailed"
        }))
        .await;
    assert_eq!(res.status_code(), 201);
    let note_id = res.json::<serde_json::Value>()["id"]
        .as_str()
        .unwrap()
        .to_string();

    let get_res = server
        .get(&format!("/api/notes/{note_id}"))
        .add_header(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {token_a}"),
        )
        .await;
    assert_eq!(get_res.status_code(), 200);

    let get_json: serde_json::Value = get_res.json();
    let html = get_json["content_html"].as_str().unwrap();

    assert!(!html.contains("<script"));
    assert!(!html.contains("onerror"));
    assert!(html.contains("<h1>Safe Title</h1>"));
}
