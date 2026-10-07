use crate::error::{AppError, Result};
use crate::models::note::{CreateNoteRequest, Note, NoteFilter, NoteType, UpdateNoteRequest};
use rusqlite::{params, params_from_iter, Connection, OptionalExtension, ToSql};
use uuid::Uuid;

pub fn create_note(conn: &Connection, user_id: &str, req: CreateNoteRequest) -> Result<Note> {
    let id = Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO notes (id, user_id, title, content, note_type, color, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, datetime('now'), datetime('now'))",
        params![
            id,
            user_id,
            req.title.trim(),
            req.content,
            req.note_type.as_str(),
            req.color,
        ],
    )?;

    get_note_by_id(conn, user_id, &id)?
        .ok_or_else(|| AppError::Internal("Note creation failed".into()))
}

pub fn get_note_by_id(conn: &Connection, user_id: &str, note_id: &str) -> Result<Option<Note>> {
    let mut stmt = conn.prepare(
        "SELECT n.id, n.user_id, n.title, n.content, n.note_type,
                n.is_pinned, n.is_archived, n.is_deleted, n.color,
                n.created_at, n.updated_at, n.deleted_at,
                (SELECT COUNT(*) FROM reminders r WHERE r.note_id = n.id AND r.state != 'dismissed') as reminder_count
         FROM notes n
         WHERE n.id = ?1 AND n.user_id = ?2 AND n.is_deleted = 0",
    )?;

    let note = stmt
        .query_row(params![note_id, user_id], |row| {
            let note_type_str: String = row.get(4)?;
            Ok(Note {
                id: row.get(0)?,
                user_id: row.get(1)?,
                title: row.get(2)?,
                content: row.get(3)?,
                note_type: NoteType::from_str_opt(&note_type_str),
                is_pinned: row.get::<_, i32>(5)? == 1,
                is_archived: row.get::<_, i32>(6)? == 1,
                is_deleted: row.get::<_, i32>(7)? == 1,
                color: row.get(8)?,
                created_at: row.get(9)?,
                updated_at: row.get(10)?,
                deleted_at: row.get(11)?,
                reminder_count: Some(row.get(12)?),
            })
        })
        .optional()?;

    Ok(note)
}

pub fn list_notes(
    conn: &Connection,
    user_id: &str,
    filter: &NoteFilter,
) -> Result<(Vec<Note>, i64)> {
    let page = filter.page.unwrap_or(1).max(1);
    let limit = filter.limit.unwrap_or(20).clamp(1, 100);
    let offset = (page - 1) * limit;

    let is_archived_filter = match filter.is_archived {
        Some(true) => 1,
        _ => 0,
    };

    let search_pattern = filter
        .search
        .as_ref()
        .map(|s| format!("%{}%", s.trim()))
        .unwrap_or_default();
    let has_search = filter.search.is_some() && !search_pattern.is_empty();

    let note_type_str = filter.note_type.map(|t| t.as_str().to_string());
    let has_type = note_type_str.is_some();

    // 1. Build and execute count query
    let total: i64 = {
        let mut count_query = String::from(
            "SELECT COUNT(*) FROM notes WHERE user_id = ?1 AND is_deleted = 0 AND is_archived = ?2",
        );
        let mut count_params: Vec<Box<dyn ToSql>> =
            vec![Box::new(user_id.to_string()), Box::new(is_archived_filter)];
        let mut idx = 2;

        if has_search {
            idx += 1;
            count_query.push_str(&format!(" AND (title LIKE ?{idx} OR content LIKE ?{idx})"));
            count_params.push(Box::new(search_pattern.clone()));
        }
        if has_type {
            idx += 1;
            count_query.push_str(&format!(" AND note_type = ?{idx}"));
            count_params.push(Box::new(note_type_str.clone().unwrap()));
        }
        if let Some(pinned) = filter.is_pinned {
            idx += 1;
            count_query.push_str(&format!(" AND is_pinned = ?{idx}"));
            count_params.push(Box::new(if pinned { 1 } else { 0 }));
        }

        let mut stmt = conn.prepare(&count_query)?;
        stmt.query_row(params_from_iter(count_params.iter()), |r| r.get(0))?
    };

    // 2. Build and execute data query
    let mut data_query = String::from(
        "SELECT n.id, n.user_id, n.title, n.content, n.note_type,
                n.is_pinned, n.is_archived, n.is_deleted, n.color,
                n.created_at, n.updated_at, n.deleted_at,
                (SELECT COUNT(*) FROM reminders r WHERE r.note_id = n.id AND r.state != 'dismissed') as reminder_count
         FROM notes n
         WHERE n.user_id = ?1 AND n.is_deleted = 0 AND n.is_archived = ?2",
    );
    let mut data_params: Vec<Box<dyn ToSql>> =
        vec![Box::new(user_id.to_string()), Box::new(is_archived_filter)];
    let mut idx = 2;

    if has_search {
        idx += 1;
        data_query.push_str(&format!(
            " AND (n.title LIKE ?{idx} OR n.content LIKE ?{idx})"
        ));
        data_params.push(Box::new(search_pattern));
    }
    if has_type {
        idx += 1;
        data_query.push_str(&format!(" AND n.note_type = ?{idx}"));
        data_params.push(Box::new(note_type_str.unwrap()));
    }
    if let Some(pinned) = filter.is_pinned {
        idx += 1;
        data_query.push_str(&format!(" AND n.is_pinned = ?{idx}"));
        data_params.push(Box::new(if pinned { 1 } else { 0 }));
    }

    idx += 1;
    data_query.push_str(&format!(
        " ORDER BY n.is_pinned DESC, n.updated_at DESC LIMIT ?{idx}"
    ));
    data_params.push(Box::new(limit));

    idx += 1;
    data_query.push_str(&format!(" OFFSET ?{idx}"));
    data_params.push(Box::new(offset));

    let mut stmt = conn.prepare(&data_query)?;

    let rows: Vec<Note> = stmt
        .query_map(params_from_iter(data_params.iter()), |row| {
            let note_type_raw: String = row.get(4)?;
            Ok(Note {
                id: row.get(0)?,
                user_id: row.get(1)?,
                title: row.get(2)?,
                content: row.get(3)?,
                note_type: NoteType::from_str_opt(&note_type_raw),
                is_pinned: row.get::<_, i32>(5)? == 1,
                is_archived: row.get::<_, i32>(6)? == 1,
                is_deleted: row.get::<_, i32>(7)? == 1,
                color: row.get(8)?,
                created_at: row.get(9)?,
                updated_at: row.get(10)?,
                deleted_at: row.get(11)?,
                reminder_count: Some(row.get(12)?),
            })
        })?
        .filter_map(std::result::Result::ok)
        .collect();

    Ok((rows, total))
}

pub fn update_note(
    conn: &Connection,
    user_id: &str,
    note_id: &str,
    req: UpdateNoteRequest,
) -> Result<Note> {
    let existing = get_note_by_id(conn, user_id, note_id)?
        .ok_or_else(|| AppError::NotFound(format!("Note {note_id} not found")))?;

    let title = req.title.unwrap_or(existing.title);
    let content = req.content.unwrap_or(existing.content);
    let note_type = req.note_type.unwrap_or(existing.note_type);
    let color = req.color.or(existing.color);
    let is_pinned = req.is_pinned.unwrap_or(existing.is_pinned);
    let is_archived = req.is_archived.unwrap_or(existing.is_archived);

    conn.execute(
        "UPDATE notes
         SET title = ?1, content = ?2, note_type = ?3, color = ?4,
             is_pinned = ?5, is_archived = ?6, updated_at = datetime('now')
         WHERE id = ?7 AND user_id = ?8 AND is_deleted = 0",
        params![
            title.trim(),
            content,
            note_type.as_str(),
            color,
            if is_pinned { 1 } else { 0 },
            if is_archived { 1 } else { 0 },
            note_id,
            user_id
        ],
    )?;

    get_note_by_id(conn, user_id, note_id)?
        .ok_or_else(|| AppError::Internal("Note update failed".into()))
}

pub fn toggle_pin_note(conn: &Connection, user_id: &str, note_id: &str) -> Result<bool> {
    let existing = get_note_by_id(conn, user_id, note_id)?
        .ok_or_else(|| AppError::NotFound(format!("Note {note_id} not found")))?;

    let new_pinned = !existing.is_pinned;
    conn.execute(
        "UPDATE notes SET is_pinned = ?1, updated_at = datetime('now') WHERE id = ?2 AND user_id = ?3",
        params![if new_pinned { 1 } else { 0 }, note_id, user_id],
    )?;

    Ok(new_pinned)
}

pub fn toggle_archive_note(conn: &Connection, user_id: &str, note_id: &str) -> Result<bool> {
    let existing = get_note_by_id(conn, user_id, note_id)?
        .ok_or_else(|| AppError::NotFound(format!("Note {note_id} not found")))?;

    let new_archived = !existing.is_archived;
    conn.execute(
        "UPDATE notes SET is_archived = ?1, updated_at = datetime('now') WHERE id = ?2 AND user_id = ?3",
        params![if new_archived { 1 } else { 0 }, note_id, user_id],
    )?;

    Ok(new_archived)
}

pub fn soft_delete_note(conn: &Connection, user_id: &str, note_id: &str) -> Result<()> {
    let rows_affected = conn.execute(
        "UPDATE notes SET is_deleted = 1, deleted_at = datetime('now') WHERE id = ?1 AND user_id = ?2",
        params![note_id, user_id],
    )?;

    if rows_affected == 0 {
        return Err(AppError::NotFound(format!("Note {note_id} not found")));
    }

    Ok(())
}

pub fn hard_delete_note(conn: &Connection, user_id: &str, note_id: &str) -> Result<()> {
    let rows_affected = conn.execute(
        "DELETE FROM notes WHERE id = ?1 AND user_id = ?2",
        params![note_id, user_id],
    )?;

    if rows_affected == 0 {
        return Err(AppError::NotFound(format!("Note {note_id} not found")));
    }

    Ok(())
}
