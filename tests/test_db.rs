use evanotes::db::{init_memory_db, notes, reminders, users};
use evanotes::models::note::{CreateNoteRequest, NoteFilter, NoteType, UpdateNoteRequest};
use evanotes::models::reminder::{CreateReminderRequest, RecurType, ReminderState};

#[test]
fn test_database_initialization_and_migrations() {
    let pool = init_memory_db().expect("Failed to initialize in-memory DB");
    let conn = pool.get().expect("Failed to get connection");

    // Verify tables exist
    let mut stmt = conn
        .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
        .unwrap();
    let tables: Vec<String> = stmt
        .query_map([], |row| row.get(0))
        .unwrap()
        .filter_map(Result::ok)
        .collect();

    assert!(tables.contains(&"users".to_string()));
    assert!(tables.contains(&"sessions".to_string()));
    assert!(tables.contains(&"otps".to_string()));
    assert!(tables.contains(&"notes".to_string()));
    assert!(tables.contains(&"reminders".to_string()));
}

#[test]
fn test_user_and_session_lifecycle() {
    let pool = init_memory_db().unwrap();
    let conn = pool.get().unwrap();

    // Create user
    let user = users::create_or_get_user(&conn, "admin@example.com", "otp", true).unwrap();
    assert_eq!(user.email, "admin@example.com");
    assert!(user.is_admin);

    // Retrieve user
    let fetched = users::get_user_by_email(&conn, "Admin@Example.com").unwrap();
    assert!(fetched.is_some());
    assert_eq!(fetched.unwrap().id, user.id);

    // Create session (expires in 1 hour)
    let token = "test-hmac-token-12345";
    let expires = (chrono::Utc::now() + chrono::Duration::hours(1)).to_rfc3339();
    let session = users::create_session(&conn, token, &user.id, &expires).unwrap();
    assert_eq!(session.token, token);

    // Retrieve session with user
    let (s, u) = users::get_session_with_user(&conn, token).unwrap().unwrap();
    assert_eq!(s.token, token);
    assert_eq!(u.id, user.id);

    // Delete session
    users::delete_session(&conn, token).unwrap();
    let after_delete = users::get_session_with_user(&conn, token).unwrap();
    assert!(after_delete.is_none());
}

#[test]
fn test_otp_lifecycle() {
    let pool = init_memory_db().unwrap();
    let conn = pool.get().unwrap();

    let email = "admin@example.com";
    let code = "123456";
    let expires = (chrono::Utc::now() + chrono::Duration::minutes(5)).to_rfc3339();

    let otp_id = users::create_otp(&conn, email, code, &expires).unwrap();

    let record = users::get_latest_active_otp(&conn, email).unwrap().unwrap();
    assert_eq!(record.id, otp_id);
    assert_eq!(record.code, code);
    assert_eq!(record.attempts, 0);

    // Increment attempts
    let attempts = users::increment_otp_attempts(&conn, otp_id).unwrap();
    assert_eq!(attempts, 1);

    // Mark used
    users::mark_otp_used(&conn, otp_id).unwrap();
    let active_after_used = users::get_latest_active_otp(&conn, email).unwrap();
    assert!(active_after_used.is_none());
}

#[test]
fn test_notes_crud_and_features() {
    let pool = init_memory_db().unwrap();
    let conn = pool.get().unwrap();

    let user = users::create_or_get_user(&conn, "notes_user@test.com", "otp", false).unwrap();

    // 1. Create simple note
    let note1 = notes::create_note(
        &conn,
        &user.id,
        CreateNoteRequest {
            title: "Quick Grocery List".into(),
            content: "Milk, Bread, Butter".into(),
            note_type: NoteType::Simple,
            color: Some("#10b981".into()),
        },
    )
    .unwrap();

    assert_eq!(note1.title, "Quick Grocery List");
    assert_eq!(note1.note_type, NoteType::Simple);
    assert!(!note1.is_pinned);

    // 2. Create detailed markdown note
    let note2 = notes::create_note(
        &conn,
        &user.id,
        CreateNoteRequest {
            title: "Project Architecture".into(),
            content: "# High Level Design\n- Rust Axum\n- SQLite".into(),
            note_type: NoteType::Detailed,
            color: None,
        },
    )
    .unwrap();

    assert_eq!(note2.note_type, NoteType::Detailed);

    // 3. Pin note
    let pinned = notes::toggle_pin_note(&conn, &user.id, &note1.id).unwrap();
    assert!(pinned);

    // 4. Update note
    let updated = notes::update_note(
        &conn,
        &user.id,
        &note1.id,
        UpdateNoteRequest {
            title: Some("Updated Grocery List".into()),
            content: Some("Milk, Bread, Butter, Coffee".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(updated.title, "Updated Grocery List");
    assert_eq!(updated.content, "Milk, Bread, Butter, Coffee");

    // 5. Search notes
    let filter = NoteFilter {
        search: Some("Coffee".into()),
        ..Default::default()
    };
    let (search_results, total) = notes::list_notes(&conn, &user.id, &filter).unwrap();
    assert_eq!(total, 1);
    assert_eq!(search_results[0].id, note1.id);

    // 6. Soft delete
    notes::soft_delete_note(&conn, &user.id, &note2.id).unwrap();
    let fetched_deleted = notes::get_note_by_id(&conn, &user.id, &note2.id).unwrap();
    assert!(fetched_deleted.is_none());

    // Listing active notes should only return note1
    let (all_notes, total_count) =
        notes::list_notes(&conn, &user.id, &NoteFilter::default()).unwrap();
    assert_eq!(total_count, 1);
    assert_eq!(all_notes[0].id, note1.id);
}

#[test]
fn test_reminders_crud_and_lifecycle() {
    let pool = init_memory_db().unwrap();
    let conn = pool.get().unwrap();

    let user = users::create_or_get_user(&conn, "remind_user@test.com", "otp", false).unwrap();
    let note = notes::create_note(
        &conn,
        &user.id,
        CreateNoteRequest {
            title: "Doctor Checkup".into(),
            content: "Check blood pressure".into(),
            note_type: NoteType::Simple,
            color: None,
        },
    )
    .unwrap();

    // 1. Create reminder linked to note
    let remind_time = (chrono::Utc::now() + chrono::Duration::hours(2)).to_rfc3339();
    let rem1 = reminders::create_reminder(
        &conn,
        &user.id,
        CreateReminderRequest {
            title: "Leave for clinic".into(),
            description: Some("Take documents".into()),
            remind_at: remind_time,
            is_recurring: false,
            cron_expr: None,
            recur_type: None,
            note_id: Some(note.id.clone()),
        },
    )
    .unwrap();

    assert_eq!(rem1.title, "Leave for clinic");
    assert_eq!(rem1.state, ReminderState::Pending);
    assert_eq!(rem1.note_id, Some(note.id.clone()));

    // Verify note's reminder_count is now 1
    let note_with_rem = notes::get_note_by_id(&conn, &user.id, &note.id)
        .unwrap()
        .unwrap();
    assert_eq!(note_with_rem.reminder_count, Some(1));

    // 2. Create recurring reminder
    let rem2 = reminders::create_reminder(
        &conn,
        &user.id,
        CreateReminderRequest {
            title: "Daily Standup".into(),
            description: None,
            remind_at: "2026-10-09T09:00:00Z".into(),
            is_recurring: true,
            cron_expr: Some("0 9 * * 1-5".into()),
            recur_type: Some(RecurType::Daily),
            note_id: None,
        },
    )
    .unwrap();

    assert!(rem2.is_recurring);
    assert_eq!(rem2.recur_type, Some(RecurType::Daily));

    // 3. Snooze reminder
    let snoozed = reminders::snooze_reminder(&conn, &user.id, &rem1.id, 15).unwrap();
    assert_eq!(snoozed.state, ReminderState::Snoozed);
    assert!(snoozed.snooze_until.is_some());

    // 4. Acknowledge reminder
    let acked = reminders::acknowledge_reminder(&conn, &user.id, &rem1.id, "acknowledge").unwrap();
    assert_eq!(acked.state, ReminderState::Acknowledged);

    // 5. Delete reminder
    reminders::delete_reminder(&conn, &user.id, &rem1.id).unwrap();
    let after_del = reminders::get_reminder_by_id(&conn, &user.id, &rem1.id).unwrap();
    assert!(after_del.is_none());
}

#[test]
fn test_security_idor_and_sql_injection() {
    let pool = init_memory_db().unwrap();
    let conn = pool.get().unwrap();

    let user_a = users::create_or_get_user(&conn, "user_a@test.com", "otp", false).unwrap();
    let user_b = users::create_or_get_user(&conn, "user_b@test.com", "otp", false).unwrap();

    // User A creates a private note
    let note_a = notes::create_note(
        &conn,
        &user_a.id,
        CreateNoteRequest {
            title: "User A Confidential".into(),
            content: "Secret data".into(),
            note_type: NoteType::Simple,
            color: None,
        },
    )
    .unwrap();

    // IDOR Check 1: User B cannot access User A's note by ID
    let b_view = notes::get_note_by_id(&conn, &user_b.id, &note_a.id).unwrap();
    assert!(b_view.is_none());

    // IDOR Check 2: User B cannot update User A's note
    let update_res = notes::update_note(
        &conn,
        &user_b.id,
        &note_a.id,
        UpdateNoteRequest {
            title: Some("Hacked Title".into()),
            ..Default::default()
        },
    );
    assert!(update_res.is_err());

    // IDOR Check 3: User B's list does not contain User A's note
    let (b_list, b_total) = notes::list_notes(&conn, &user_b.id, &NoteFilter::default()).unwrap();
    assert_eq!(b_total, 0);
    assert!(b_list.is_empty());

    // SQL Injection Check: Injection payload in title and content
    let injection_payload = "'; DROP TABLE notes; --";
    let injection_note = notes::create_note(
        &conn,
        &user_a.id,
        CreateNoteRequest {
            title: injection_payload.into(),
            content: injection_payload.into(),
            note_type: NoteType::Simple,
            color: None,
        },
    )
    .unwrap();

    // Verify payload is stored safely as literal text
    assert_eq!(injection_note.title, injection_payload);
    assert_eq!(injection_note.content, injection_payload);

    // Verify notes table still exists and is untouched!
    let (a_list, a_total) = notes::list_notes(&conn, &user_a.id, &NoteFilter::default()).unwrap();
    assert_eq!(a_total, 2);
    assert_eq!(a_list.len(), 2);
}
