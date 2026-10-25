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
    config.version_min_interval_secs = 0; // force a version snapshot on every update
    config.version_max_keep = 10;

    let token = {
        let conn = pool.get().unwrap();
        let user = create_or_get_user(&conn, "feat_user@test.com", "otp", true).unwrap();
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

async fn make_note(server: &TestServer, token: &str, payload: Value) -> Value {
    let res = server
        .post("/api/notes")
        .add_header(axum::http::header::AUTHORIZATION, auth(token))
        .json(&payload)
        .await;
    assert_eq!(res.status_code(), 201, "create failed: {:?}", res.text());
    res.json()
}

#[tokio::test]
async fn test_folders_crud_nesting_and_note_filter() {
    let (server, token, _dir) = setup_test_env();

    // Create top folder + subfolder
    let f1 = server
        .post("/api/folders")
        .add_header(axum::http::header::AUTHORIZATION, auth(&token))
        .json(&json!({ "name": "Work", "icon": "💼" }))
        .await;
    assert_eq!(f1.status_code(), 201);
    let f1_id = f1.json::<Value>()["id"].as_str().unwrap().to_string();

    let f2 = server
        .post("/api/folders")
        .add_header(axum::http::header::AUTHORIZATION, auth(&token))
        .json(&json!({ "name": "Projects", "parent_id": f1_id, "icon": "📁" }))
        .await;
    assert_eq!(f2.status_code(), 201);
    let f2_id = f2.json::<Value>()["id"].as_str().unwrap().to_string();

    // Notes: one in subfolder, one at root
    make_note(
        &server,
        &token,
        json!({ "title": "Alpha project", "content": "plan", "folder_id": f2_id, "icon": "🚀" }),
    )
    .await;
    make_note(
        &server,
        &token,
        json!({ "title": "Loose note", "content": "no folder" }),
    )
    .await;

    // Tree endpoint: Work has child Projects
    let tree = server
        .get("/api/folders")
        .add_header(axum::http::header::AUTHORIZATION, auth(&token))
        .await;
    let tree_json: Value = tree.json();
    let work = tree_json["folders"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["name"] == "Work")
        .expect("Work folder");
    assert_eq!(work["icon"], "💼");
    assert_eq!(work["children"][0]["name"], "Projects");

    // Filtering by parent folder includes subfolder notes
    let res = server
        .get(&format!("/api/notes?folder={f1_id}"))
        .add_header(axum::http::header::AUTHORIZATION, auth(&token))
        .await;
    let json: Value = res.json();
    assert_eq!(json["pagination"]["total"], 1);
    assert_eq!(json["notes"][0]["title"], "Alpha project");

    // Root filter shows only the folderless note
    let res_root = server
        .get("/api/notes?folder=root")
        .add_header(axum::http::header::AUTHORIZATION, auth(&token))
        .await;
    let root_json: Value = res_root.json();
    assert_eq!(root_json["pagination"]["total"], 1);
    assert_eq!(root_json["notes"][0]["title"], "Loose note");

    // Moving a note to root via update
    let note_id = json["notes"][0]["id"].as_str().unwrap().to_string();
    let upd = server
        .put(&format!("/api/notes/{note_id}"))
        .add_header(axum::http::header::AUTHORIZATION, auth(&token))
        .json(&json!({ "folder_id": null }))
        .await;
    assert_eq!(upd.status_code(), 200);
    assert_eq!(upd.json::<Value>()["folder_id"], Value::Null);

    // Cycle protection: moving Work into its own child must fail
    let cycle = server
        .put(&format!("/api/folders/{f1_id}"))
        .add_header(axum::http::header::AUTHORIZATION, auth(&token))
        .json(&json!({ "parent_id": f2_id }))
        .await;
    assert_eq!(cycle.status_code(), 400);

    // Delete Work: Projects is lifted to root, note moved out
    let del = server
        .delete(&format!("/api/folders/{f1_id}"))
        .add_header(axum::http::header::AUTHORIZATION, auth(&token))
        .await;
    assert_eq!(del.status_code(), 200);

    let tree_after = server
        .get("/api/folders")
        .add_header(axum::http::header::AUTHORIZATION, auth(&token))
        .await;
    let tree_after_json: Value = tree_after.json();
    let names: Vec<&str> = tree_after_json["folders"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, vec!["Projects"]);
}

#[tokio::test]
async fn test_version_history_and_restore() {
    let (server, token, _dir) = setup_test_env();

    let note = make_note(
        &server,
        &token,
        json!({ "title": "V1", "content": "original" }),
    )
    .await;
    let note_id = note["id"].as_str().unwrap().to_string();

    // Two updates -> two version snapshots of previous states
    let upd1 = server
        .put(&format!("/api/notes/{note_id}"))
        .add_header(axum::http::header::AUTHORIZATION, auth(&token))
        .json(&json!({ "content": "second state" }))
        .await;
    assert_eq!(upd1.status_code(), 200);

    let upd2 = server
        .put(&format!("/api/notes/{note_id}"))
        .add_header(axum::http::header::AUTHORIZATION, auth(&token))
        .json(&json!({ "content": "third state" }))
        .await;
    assert_eq!(upd2.status_code(), 200);

    let versions = server
        .get(&format!("/api/notes/{note_id}/versions"))
        .add_header(axum::http::header::AUTHORIZATION, auth(&token))
        .await;
    let versions_json: Value = versions.json();
    let list = versions_json["versions"].as_array().unwrap();
    assert_eq!(list.len(), 2);
    // Newest first: the state before the last update
    assert_eq!(list[0]["content"], "second state");
    assert_eq!(list[1]["content"], "original");

    // Restore the oldest version
    let oldest_id = list[1]["id"].as_i64().unwrap();
    let restore = server
        .post(&format!("/api/notes/{note_id}/restore"))
        .add_header(axum::http::header::AUTHORIZATION, auth(&token))
        .json(&json!({ "version_id": oldest_id }))
        .await;
    assert_eq!(restore.status_code(), 200);
    assert_eq!(restore.json::<Value>()["note"]["content"], "original");

    // Clear all versions
    let clear = server
        .post(&format!("/api/notes/{note_id}/versions/clear"))
        .add_header(axum::http::header::AUTHORIZATION, auth(&token))
        .await;
    assert_eq!(clear.status_code(), 200);

    let versions_after = server
        .get(&format!("/api/notes/{note_id}/versions"))
        .add_header(axum::http::header::AUTHORIZATION, auth(&token))
        .await;
    assert_eq!(
        versions_after.json::<Value>()["versions"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
}

#[tokio::test]
async fn test_public_share_snapshot_and_live() {
    let (server, token, _dir) = setup_test_env();

    let note = make_note(
        &server,
        &token,
        json!({ "title": "Shared doc", "content": "# Hello\nworld", "note_type": "detailed" }),
    )
    .await;
    let note_id = note["id"].as_str().unwrap().to_string();

    // Create snapshot share
    let share_res = server
        .post(&format!("/api/notes/{note_id}/share"))
        .add_header(axum::http::header::AUTHORIZATION, auth(&token))
        .json(&json!({ "mode": "snapshot" }))
        .await;
    assert_eq!(share_res.status_code(), 200);
    let slug = share_res.json::<Value>()["share"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Public page is accessible WITHOUT auth
    let page = server.get(&format!("/share/{slug}")).await;
    assert_eq!(page.status_code(), 200);
    let html = page.text();
    assert!(html.contains("Shared doc"));
    assert!(html.contains("Snapshot"));

    // Public markdown download
    let md = server.get(&format!("/share/{slug}/md")).await;
    assert_eq!(md.status_code(), 200);
    assert!(md.text().contains("# Hello"));

    // Public PDF download
    let pdf = server.get(&format!("/share/{slug}/pdf")).await;
    assert_eq!(pdf.status_code(), 200);
    assert!(pdf
        .header(axum::http::header::CONTENT_TYPE)
        .to_str()
        .unwrap()
        .contains("application/pdf"));

    // Edit the note: snapshot share must NOT change
    let upd = server
        .put(&format!("/api/notes/{note_id}"))
        .add_header(axum::http::header::AUTHORIZATION, auth(&token))
        .json(&json!({ "content": "# Changed\ncompletely" }))
        .await;
    assert_eq!(upd.status_code(), 200);

    let page_after = server.get(&format!("/share/{slug}")).await;
    assert!(page_after.text().contains("Hello")); // still the frozen copy
    assert!(!page_after.text().contains("completely"));

    // Revoke -> 404
    let revoke = server
        .delete(&format!("/api/notes/{note_id}/share"))
        .add_header(axum::http::header::AUTHORIZATION, auth(&token))
        .await;
    assert_eq!(revoke.status_code(), 200);
    assert_eq!(
        server.get(&format!("/share/{slug}")).await.status_code(),
        404
    );

    // Live share follows edits
    let live = server
        .post(&format!("/api/notes/{note_id}/share"))
        .add_header(axum::http::header::AUTHORIZATION, auth(&token))
        .json(&json!({ "mode": "live" }))
        .await;
    let live_slug = live.json::<Value>()["share"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let live_page = server.get(&format!("/share/{live_slug}")).await;
    assert!(live_page.text().contains("completely"));
    assert!(live_page.text().contains("Live"));
}

#[tokio::test]
async fn test_search_scopes_tags_and_folders() {
    let (server, token, _dir) = setup_test_env();

    // Folder named "Recipes"; note inside with tag "cooking"
    let f = server
        .post("/api/folders")
        .add_header(axum::http::header::AUTHORIZATION, auth(&token))
        .json(&json!({ "name": "Recipes" }))
        .await;
    let f_id = f.json::<Value>()["id"].as_str().unwrap().to_string();

    make_note(
        &server,
        &token,
        json!({ "title": "Pancakes", "content": "flour and eggs", "folder_id": f_id, "tags": ["cooking", "breakfast"] }),
    )
    .await;
    make_note(
        &server,
        &token,
        json!({ "title": "Budget", "content": "monthly numbers" }),
    )
    .await;

    // Search only in folders -> matches the Pancakes note via folder name
    let res = server
        .get("/api/notes?search=recipe&search_in=folders")
        .add_header(axum::http::header::AUTHORIZATION, auth(&token))
        .await;
    let json: Value = res.json();
    assert_eq!(json["pagination"]["total"], 1);
    assert_eq!(json["notes"][0]["title"], "Pancakes");

    // Search only in tags -> matches via tag name
    let res2 = server
        .get("/api/notes?search=cooking&search_in=tags")
        .add_header(axum::http::header::AUTHORIZATION, auth(&token))
        .await;
    assert_eq!(res2.json::<Value>()["pagination"]["total"], 1);

    // Search only in content -> "monthly" hits Budget but not Pancakes
    let res3 = server
        .get("/api/notes?search=monthly&search_in=content")
        .add_header(axum::http::header::AUTHORIZATION, auth(&token))
        .await;
    let json3: Value = res3.json();
    assert_eq!(json3["pagination"]["total"], 1);
    assert_eq!(json3["notes"][0]["title"], "Budget");

    // Default (all scopes): "pancakes" found via title
    let res4 = server
        .get("/api/notes?search=pancakes")
        .add_header(axum::http::header::AUTHORIZATION, auth(&token))
        .await;
    assert_eq!(res4.json::<Value>()["pagination"]["total"], 1);

    // Multi-tag AND filter
    let res5 = server
        .get("/api/notes?tag=cooking,breakfast")
        .add_header(axum::http::header::AUTHORIZATION, auth(&token))
        .await;
    assert_eq!(res5.json::<Value>()["pagination"]["total"], 1);

    let res6 = server
        .get("/api/notes?tag=cooking,missing")
        .add_header(axum::http::header::AUTHORIZATION, auth(&token))
        .await;
    assert_eq!(res6.json::<Value>()["pagination"]["total"], 0);
}

#[tokio::test]
async fn test_code_and_sheet_note_types() {
    let (server, token, _dir) = setup_test_env();

    // Code note roundtrip
    let code_note = make_note(
        &server,
        &token,
        json!({ "title": "main.rs", "content": "fn main() {}", "note_type": "code" }),
    )
    .await;
    assert_eq!(code_note["note_type"], "code");
    let code_id = code_note["id"].as_str().unwrap().to_string();

    // Highlight endpoint returns HTML
    let hl = server
        .post("/api/highlight")
        .add_header(axum::http::header::AUTHORIZATION, auth(&token))
        .json(&json!({ "language": "rust", "code": "fn main() {}" }))
        .await;
    assert_eq!(hl.status_code(), 200);
    let html = hl.json::<Value>()["html"].as_str().unwrap().to_string();
    assert!(html.contains("<pre"));

    // Sheet note roundtrip
    let sheet = make_note(
        &server,
        &token,
        json!({ "title": "Budget sheet", "content": "[[\"Item\",\"Cost\"],[\"Milk\",\"2\"]]", "note_type": "sheet" }),
    )
    .await;
    assert_eq!(sheet["note_type"], "sheet");

    // PDF export for both
    let ids = [code_id, sheet["id"].as_str().unwrap().to_string()];
    for id in ids.iter() {
        let pdf = server
            .get(&format!("/api/notes/{id}/export?format=pdf"))
            .add_header(axum::http::header::AUTHORIZATION, auth(&token))
            .await;
        assert_eq!(pdf.status_code(), 200);
        assert!(pdf
            .header(axum::http::header::CONTENT_TYPE)
            .to_str()
            .unwrap()
            .contains("application/pdf"));
        assert!(pdf.text().starts_with("%PDF"));
    }

    // Markdown export still works
    let md = server
        .get(&format!("/api/notes/{}/export", ids[0]))
        .add_header(axum::http::header::AUTHORIZATION, auth(&token))
        .await;
    assert_eq!(md.status_code(), 200);
    assert!(md
        .header(axum::http::header::CONTENT_TYPE)
        .to_str()
        .unwrap()
        .contains("text/markdown"));
}

#[tokio::test]
async fn test_presence_reports_other_editors() {
    let (server, token, _dir) = setup_test_env();

    let note = make_note(
        &server,
        &token,
        json!({ "title": "Shared note", "content": "x" }),
    )
    .await;
    let note_id = note["id"].as_str().unwrap().to_string();

    // Device A opens the note
    let open_a = server
        .post(&format!("/api/notes/{note_id}/presence"))
        .add_header(axum::http::header::AUTHORIZATION, auth(&token))
        .json(&json!({ "device": "phone-a" }))
        .await;
    assert_eq!(open_a.status_code(), 200);

    // Device B sees A in active_editors
    let res = server
        .get(&format!("/api/notes/{note_id}?device=phone-b"))
        .add_header(axum::http::header::AUTHORIZATION, auth(&token))
        .await;
    let json: Value = res.json();
    let editors = json["active_editors"].as_array().unwrap();
    assert_eq!(editors.len(), 1);
    assert_eq!(editors[0]["device"], "phone-a");

    // Device A closes -> nobody else listed
    let close = server
        .delete(&format!("/api/notes/{note_id}/presence"))
        .add_header(axum::http::header::AUTHORIZATION, auth(&token))
        .json(&json!({ "device": "phone-a" }))
        .await;
    assert_eq!(close.status_code(), 200);

    let res2 = server
        .get(&format!("/api/notes/{note_id}?device=phone-b"))
        .add_header(axum::http::header::AUTHORIZATION, auth(&token))
        .await;
    assert_eq!(
        res2.json::<Value>()["active_editors"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
}

#[tokio::test]
async fn test_ai_endpoint_validation() {
    let (server, token, _dir) = setup_test_env();

    // AI_API_KEY not configured -> clean 400
    let res = server
        .post("/api/ai/rewrite")
        .add_header(axum::http::header::AUTHORIZATION, auth(&token))
        .json(&json!({ "text": "helo wrld", "model": "x", "style": "minimal" }))
        .await;
    assert_eq!(res.status_code(), 400);

    // Unknown style rejected
    let res2 = server
        .post("/api/ai/rewrite")
        .add_header(axum::http::header::AUTHORIZATION, auth(&token))
        .json(&json!({ "text": "helo", "model": "x", "style": "bogus" }))
        .await;
    assert_eq!(res2.status_code(), 400);
}
