use crate::error::{AppError, Result};
use crate::models::note::{Note, NoteType};
use crate::models::share::Share;
use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

fn random_slug() -> String {
    // Short, URL-safe slug (22 chars of base62)
    let raw = Uuid::new_v4().to_string();
    let mut out = String::with_capacity(22);
    for b in raw.bytes() {
        match b {
            b'0'..=b'9' | b'a'..=b'z' | b'A'..=b'Z' => out.push(b as char),
            b'-' => out.push('X'),
            _ => continue,
        }
        if out.len() >= 22 {
            break;
        }
    }
    out
}

pub fn create_share(conn: &Connection, user_id: &str, note_id: &str, mode: &str) -> Result<Share> {
    let note = crate::db::notes::get_note_by_id(conn, user_id, note_id)?
        .ok_or_else(|| AppError::NotFound(format!("Note {note_id} not found")))?;

    let mode = match mode {
        "snapshot" | "live" => mode.to_string(),
        _ => {
            return Err(AppError::BadRequest(
                "mode must be 'snapshot' or 'live'".into(),
            ))
        }
    };

    // One share per note: update existing rather than duplicating
    let existing: Option<String> = conn
        .query_row(
            "SELECT id FROM shares WHERE note_id = ?1 AND user_id = ?2",
            params![note_id, user_id],
            |row| row.get(0),
        )
        .optional()?;

    let id = existing.unwrap_or_else(random_slug);

    let (snapshot_title, snapshot_content, snapshot_type) = if mode == "snapshot" {
        (
            Some(note.title),
            Some(note.content),
            Some(note.note_type.as_str().to_string()),
        )
    } else {
        (None, None, None)
    };

    conn.execute(
        "INSERT INTO shares (id, user_id, note_id, mode, snapshot_title, snapshot_content, snapshot_type)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(id) DO UPDATE SET
             mode = excluded.mode,
             snapshot_title = excluded.snapshot_title,
             snapshot_content = excluded.snapshot_content,
             snapshot_type = excluded.snapshot_type",
        params![id, user_id, note_id, mode, snapshot_title, snapshot_content, snapshot_type],
    )?;

    Ok(Share {
        id,
        user_id: user_id.to_string(),
        note_id: note_id.to_string(),
        mode,
        created_at: chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        path: None,
    })
}

pub fn get_share_for_note(
    conn: &Connection,
    user_id: &str,
    note_id: &str,
) -> Result<Option<Share>> {
    let mut stmt = conn.prepare(
        "SELECT id, user_id, note_id, mode, created_at
         FROM shares WHERE note_id = ?1 AND user_id = ?2",
    )?;
    let share = stmt
        .query_row(params![note_id, user_id], |row| {
            Ok(Share {
                id: row.get(0)?,
                user_id: row.get(1)?,
                note_id: row.get(2)?,
                mode: row.get(3)?,
                created_at: row.get(4)?,
                path: None,
            })
        })
        .optional()?;
    Ok(share)
}

pub fn delete_share_for_note(conn: &Connection, user_id: &str, note_id: &str) -> Result<()> {
    let affected = conn.execute(
        "DELETE FROM shares WHERE note_id = ?1 AND user_id = ?2",
        params![note_id, user_id],
    )?;
    if affected == 0 {
        return Err(AppError::NotFound("No share found for this note".into()));
    }
    Ok(())
}

/// Public lookup: returns the note content this share exposes.
/// Snapshot shares return the frozen copy; live shares return current state.
pub fn resolve_share(
    conn: &Connection,
    slug: &str,
) -> Result<Option<(String, String, String, String)>> {
    // (mode, title, content, note_type)
    let row = conn
        .query_row(
            "SELECT s.mode,
                    COALESCE(s.snapshot_title, n.title),
                    COALESCE(s.snapshot_content, n.content),
                    COALESCE(s.snapshot_type, n.note_type)
             FROM shares s
             JOIN notes n ON n.id = s.note_id
             WHERE s.id = ?1 AND n.is_deleted = 0",
            params![slug],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )
        .optional()?;
    Ok(row)
}

/// The raw note for live-share PDF/md exports (already resolved above).
pub fn note_type_from_str(s: &str) -> NoteType {
    NoteType::from_str_opt(s)
}

/// Ensure a `Note`-shaped value for helper reuse in share rendering.
pub fn share_note_view(title: String, content: String, note_type: String) -> Note {
    Note {
        id: String::new(),
        user_id: String::new(),
        title,
        content,
        note_type: note_type_from_str(&note_type),
        is_pinned: false,
        is_archived: false,
        is_deleted: false,
        color: None,
        icon: None,
        folder_id: None,
        folder_name: None,
        tags: Vec::new(),
        created_at: String::new(),
        updated_at: String::new(),
        deleted_at: None,
        reminder_count: None,
    }
}
