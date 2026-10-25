mod common;

use axum_test::TestServer;
use common::init_test_db;
use evanotes::auth::session::generate_signed_token;
use evanotes::auth::OtpRateLimiter;
use evanotes::create_app;
use evanotes::db::users::create_or_get_user;
use evanotes::{AppConfig, AppState, NotificationHub, PresenceHub};
use serde_json::{json, Value};

fn setup_test_env() -> (TestServer, String, tempfile::TempDir) {
    let (pool, dir) = init_test_db();
    let mut config = AppConfig::from_env();
    config.session_secret = "test-secret-32-chars-long-abcdefghijk".to_string();

    let token = {
        let conn = pool.get().unwrap();
        let user = create_or_get_user(&conn, "tags_user@test.com", "otp", true).unwrap();
        let t = generate_signed_token(&user.id, &config.session_secret);
        let expires = (chrono::Utc::now() + chrono::Duration::days(30)).to_rfc3339();
        evanotes::db::users::create_session(&conn, &t, &user.id, &expires).unwrap();
        t
    };

    let state = AppState {
        pool,
        config,
        otp_limiter: OtpRateLimiter::new(),
        hub: NotificationHub::new(),
        presence: PresenceHub::new(),
    };
    let app = create_app(state);
    let server = TestServer::new(app).expect("Failed to create test server");
    (server, token, dir)
}

fn auth(token: &str) -> String {
    format!("Bearer {token}")
}

async fn create_note(
    server: &TestServer,
    token: &str,
    title: &str,
    content: &str,
    tags: Value,
) -> Value {
    let res = server
        .post("/api/notes")
        .add_header(axum::http::header::AUTHORIZATION, auth(token))
        .json(&json!({
            "title": title,
            "content": content,
            "note_type": "detailed",
            "tags": tags
        }))
        .await;
    assert_eq!(res.status_code(), 201, "note create failed");
    res.json()
}

#[tokio::test]
async fn test_tags_crud_and_note_attachment() {
    let (server, token, _dir) = setup_test_env();

    // 1. Create note with new tags -> tags are auto-created and returned
    let note = create_note(
        &server,
        &token,
        "Kubernetes cheatsheet",
        "# kubectl\nkubectl get pods",
        json!(["kubernetes", "devops", "infra"]),
    )
    .await;
    let note_id = note["id"].as_str().unwrap().to_string();
    let note_tags: Vec<String> = note["tags"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t.as_str().unwrap().to_string())
        .collect();
    assert!(note_tags.contains(&"kubernetes".to_string()));
    assert!(note_tags.contains(&"devops".to_string()));

    // 2. Tag list with counts
    let list_res = server
        .get("/api/tags")
        .add_header(axum::http::header::AUTHORIZATION, auth(&token))
        .await;
    assert_eq!(list_res.status_code(), 200);
    let list_json: Value = list_res.json();
    let tags = list_json["tags"].as_array().unwrap();
    assert_eq!(tags.len(), 3);
    let k8s_tag = tags
        .iter()
        .find(|t| t["name"] == "kubernetes")
        .expect("kubernetes tag present");
    assert_eq!(k8s_tag["note_count"], 1);

    // 3. Fast autocomplete suggestions (prefix search)
    let suggest_res = server
        .get("/api/tags/suggest?q=dev")
        .add_header(axum::http::header::AUTHORIZATION, auth(&token))
        .await;
    let suggest_json: Value = suggest_res.json();
    let suggestions: Vec<String> = suggest_json["tags"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(suggestions, vec!["devops"]);

    // 4. Replace tag set via PUT /api/notes/{id}/tags
    let set_res = server
        .put(&format!("/api/notes/{note_id}/tags"))
        .add_header(axum::http::header::AUTHORIZATION, auth(&token))
        .json(&json!({ "tags": ["kubernetes", "cheatsheets"] }))
        .await;
    assert_eq!(set_res.status_code(), 200);
    let set_json: Value = set_res.json();
    assert_eq!(set_json["tags"].as_array().unwrap().len(), 2);

    // 5. Rename tag
    let k8s_id = k8s_tag["id"].as_i64().unwrap();
    let rename_res = server
        .put(&format!("/api/tags/{k8s_id}"))
        .add_header(axum::http::header::AUTHORIZATION, auth(&token))
        .json(&json!({ "name": "k8s" }))
        .await;
    assert_eq!(rename_res.status_code(), 200);
    assert_eq!(rename_res.json::<Value>()["name"], "k8s");

    // 6. Delete tag detaches it from notes but keeps the note
    let del_res = server
        .delete(&format!("/api/tags/{k8s_id}"))
        .add_header(axum::http::header::AUTHORIZATION, auth(&token))
        .await;
    assert_eq!(del_res.status_code(), 200);

    let get_note = server
        .get(&format!("/api/notes/{note_id}"))
        .add_header(axum::http::header::AUTHORIZATION, auth(&token))
        .await;
    let get_json: Value = get_note.json();
    let remaining: Vec<String> = get_json["note"]["tags"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t.as_str().unwrap().to_string())
        .collect();
    assert_eq!(remaining, vec!["cheatsheets"]);
}

#[tokio::test]
async fn test_full_text_search_matches_content() {
    let (server, token, _dir) = setup_test_env();

    create_note(
        &server,
        &token,
        "Grocery list",
        "Milk, Eggs, Organic Coffee Beans",
        json!([]),
    )
    .await;
    create_note(
        &server,
        &token,
        "Project plan",
        "Ship the quantum anvil module by Friday",
        json!([]),
    )
    .await;

    // Search inside the CONTENT (not the title) — FTS5 must find it
    let res = server
        .get("/api/notes?search=quantum")
        .add_header(axum::http::header::AUTHORIZATION, auth(&token))
        .await;
    assert_eq!(res.status_code(), 200);
    let json: Value = res.json();
    assert_eq!(json["pagination"]["total"], 1);
    assert_eq!(json["notes"][0]["title"], "Project plan");

    // Prefix-style partial word search
    let res2 = server
        .get("/api/notes?search=quant")
        .add_header(axum::http::header::AUTHORIZATION, auth(&token))
        .await;
    assert_eq!(res2.json::<Value>()["pagination"]["total"], 1);

    // Multi-word search (implicit AND)
    let res3 = server
        .get("/api/notes?search=quantum%20anvil")
        .add_header(axum::http::header::AUTHORIZATION, auth(&token))
        .await;
    assert_eq!(res3.json::<Value>()["pagination"]["total"], 1);

    // No match
    let res4 = server
        .get("/api/notes?search=unicorn")
        .add_header(axum::http::header::AUTHORIZATION, auth(&token))
        .await;
    assert_eq!(res4.json::<Value>()["pagination"]["total"], 0);
}

#[tokio::test]
async fn test_note_list_tag_filter() {
    let (server, token, _dir) = setup_test_env();

    create_note(
        &server,
        &token,
        "Docker notes",
        "volumes and networks",
        json!(["docker"]),
    )
    .await;
    create_note(
        &server,
        &token,
        "Rust notes",
        "traits and lifetimes",
        json!(["rust"]),
    )
    .await;

    // Filter by tag
    let res = server
        .get("/api/notes?tag=docker")
        .add_header(axum::http::header::AUTHORIZATION, auth(&token))
        .await;
    let json: Value = res.json();
    assert_eq!(json["pagination"]["total"], 1);
    assert_eq!(json["notes"][0]["title"], "Docker notes");

    // Combined: tag + type + search
    let res2 = server
        .get("/api/notes?tag=rust&type=detailed&search=lifetimes")
        .add_header(axum::http::header::AUTHORIZATION, auth(&token))
        .await;
    assert_eq!(res2.json::<Value>()["pagination"]["total"], 1);

    // Unknown tag -> empty
    let res3 = server
        .get("/api/notes?tag=nope")
        .add_header(axum::http::header::AUTHORIZATION, auth(&token))
        .await;
    assert_eq!(res3.json::<Value>()["pagination"]["total"], 0);
}

#[tokio::test]
async fn test_reminder_channels_roundtrip() {
    let (server, token, _dir) = setup_test_env();

    let future_time = (chrono::Utc::now() + chrono::Duration::hours(1)).to_rfc3339();
    let create_res = server
        .post("/api/reminders")
        .add_header(axum::http::header::AUTHORIZATION, auth(&token))
        .json(&json!({
            "title": "Multi-channel reminder",
            "remind_at": future_time,
            "channels": ["web", "email", "telegram"]
        }))
        .await;
    assert_eq!(create_res.status_code(), 201);
    let rem: Value = create_res.json();
    assert_eq!(rem["channels"], json!(["web", "email", "telegram"]));
    // remind_at normalized to canonical UTC
    assert!(rem["remind_at"].as_str().unwrap().ends_with('Z'));

    let rem_id = rem["id"].as_str().unwrap().to_string();

    // Update channels
    let upd_res = server
        .put(&format!("/api/reminders/{rem_id}"))
        .add_header(axum::http::header::AUTHORIZATION, auth(&token))
        .json(&json!({ "channels": ["web"] }))
        .await;
    assert_eq!(upd_res.status_code(), 200);
    assert_eq!(upd_res.json::<Value>()["channels"], json!(["web"]));

    // Default channels when omitted
    let create_res2 = server
        .post("/api/reminders")
        .add_header(axum::http::header::AUTHORIZATION, auth(&token))
        .json(&json!({
            "title": "Default channels reminder",
            "remind_at": future_time
        }))
        .await;
    assert_eq!(create_res2.json::<Value>()["channels"], json!(["web"]));
}
