use crate::db::tags::{get_tags_for_notes, set_note_tags};
use crate::error::{AppError, Result};
use crate::models::note::{
    CreateNoteRequest, Note, NoteFilter, NoteType, SearchScope, UpdateNoteRequest,
};
use rusqlite::{params, params_from_iter, Connection, OptionalExtension, ToSql};
use uuid::Uuid;

const NOTE_COLUMNS: &str = "n.id, n.user_id, n.title, n.content, n.note_type,
        n.is_pinned, n.is_archived, n.is_deleted, n.color, n.icon, n.folder_id,
        f.name as folder_name,
        n.created_at, n.updated_at, n.deleted_at,
        (SELECT COUNT(*) FROM reminders r WHERE r.note_id = n.id AND r.state != 'dismissed') as reminder_count";

const NOTE_TABLES: &str = "FROM notes n LEFT JOIN folders f ON f.id = n.folder_id";

/// FTS match scoped to specific columns: `title: "x"* OR content: "y"*`.
fn build_scoped_fts_match(query: &str, scopes: &[SearchScope]) -> Option<String> {
    let tokens: Vec<String> = query
        .split_whitespace()
        .filter_map(|t| {
            let cleaned: String = t
                .chars()
                .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
                .collect();
            if cleaned.is_empty() {
                None
            } else {
                Some(format!("\"{}\"*", cleaned))
            }
        })
        .take(8)
        .collect();
    if tokens.is_empty() {
        return None;
    }
    let term = tokens.join(" ");
    let parts: Vec<String> = scopes
        .iter()
        .filter_map(|s| match s {
            SearchScope::Title => Some(format!("title: ({term})")),
            SearchScope::Content => Some(format!("content: ({term})")),
            _ => None,
        })
        .collect();
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" OR "))
    }
}

fn note_from_row(row: &rusqlite::Row) -> rusqlite::Result<Note> {
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
        icon: row.get(9)?,
        folder_id: row.get(10)?,
        folder_name: row.get(11)?,
        tags: Vec::new(),
        created_at: row.get(12)?,
        updated_at: row.get(13)?,
        deleted_at: row.get(14)?,
        reminder_count: Some(row.get(15)?),
    })
}

fn attach_tags(conn: &Connection, user_id: &str, notes: &mut [Note]) -> Result<()> {
    if notes.is_empty() {
        return Ok(());
    }
    let ids: Vec<String> = notes.iter().map(|n| n.id.clone()).collect();
    let map = get_tags_for_notes(conn, user_id, &ids)?;
    for note in notes.iter_mut() {
        note.tags = map.get(&note.id).cloned().unwrap_or_default();
    }
    Ok(())
}

pub fn create_note(conn: &mut Connection, user_id: &str, req: CreateNoteRequest) -> Result<Note> {
    let id = Uuid::new_v4().to_string();

    // Validate folder ownership
    if let Some(ref fid) = req.folder_id {
        let ok: Option<i64> = conn
            .query_row(
                "SELECT 1 FROM folders WHERE id = ?1 AND user_id = ?2",
                params![fid, user_id],
                |r| r.get(0),
            )
            .optional()?;
        if ok.is_none() {
            return Err(AppError::NotFound(format!("Folder {fid} not found")));
        }
    }

    let tx = conn.transaction()?;
    tx.execute(
        "INSERT INTO notes (id, user_id, title, content, note_type, color, icon, folder_id, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, datetime('now'), datetime('now'))",
        params![
            id,
            user_id,
            req.title.trim(),
            req.content,
            req.note_type.as_str(),
            req.color,
            req.icon,
            req.folder_id,
        ],
    )?;

    if !req.tags.is_empty() {
        set_note_tags(&tx, user_id, &id, &req.tags)?;
    }
    tx.commit()?;

    get_note_by_id(conn, user_id, &id)?
        .ok_or_else(|| AppError::Internal("Note creation failed".into()))
}

pub fn get_note_by_id(conn: &Connection, user_id: &str, note_id: &str) -> Result<Option<Note>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {NOTE_COLUMNS} {NOTE_TABLES}
         WHERE n.id = ?1 AND n.user_id = ?2 AND n.is_deleted = 0"
    ))?;

    let mut note = stmt
        .query_row(params![note_id, user_id], note_from_row)
        .optional()?;

    if let Some(ref mut note) = note {
        let mut tag_stmt = conn.prepare(
            "SELECT t.name
             FROM note_tags nt
             JOIN tags t ON t.id = nt.tag_id
             WHERE nt.note_id = ?1 AND t.user_id = ?2
             ORDER BY t.name COLLATE NOCASE ASC",
        )?;
        note.tags = tag_stmt
            .query_map(params![note_id, user_id], |row| row.get::<_, String>(0))?
            .filter_map(std::result::Result::ok)
            .collect();
    }

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

    let is_archived_filter = if filter.is_archived == Some(true) {
        1
    } else {
        0
    };

    // WHERE clauses + positional params (user_id = ?1, is_archived = ?2)
    let mut clauses: Vec<String> =
        vec!["n.user_id = ?1 AND n.is_deleted = 0 AND n.is_archived = ?2".to_string()];
    let mut query_params: Vec<Box<dyn ToSql>> =
        vec![Box::new(user_id.to_string()), Box::new(is_archived_filter)];

    // ---- Search across title / content / tags / folders (OR semantics) ----
    if let Some(raw) = filter.search.as_ref() {
        let q = raw.trim();
        if !q.is_empty() {
            let scopes = filter
                .search_in
                .clone()
                .unwrap_or_else(|| {
                    vec![
                        SearchScope::Title,
                        SearchScope::Content,
                        SearchScope::Tags,
                        SearchScope::Folders,
                    ]
                })
                .into_iter()
                .collect::<Vec<_>>();

            let mut or_parts: Vec<String> = Vec::new();

            let has_title = scopes.contains(&SearchScope::Title);
            let has_content = scopes.contains(&SearchScope::Content);
            if has_title || has_content {
                let mut fts_scopes = Vec::new();
                if has_title {
                    fts_scopes.push(SearchScope::Title);
                }
                if has_content {
                    fts_scopes.push(SearchScope::Content);
                }
                if let Some(match_str) = build_scoped_fts_match(q, &fts_scopes) {
                    let idx = query_params.len() + 1;
                    or_parts.push(format!(
                        "n.rowid IN (SELECT rowid FROM notes_fts WHERE notes_fts MATCH ?{idx})"
                    ));
                    query_params.push(Box::new(match_str));
                } else {
                    let pattern = format!("%{}%", q.replace(['%', '_'], ""));
                    let idx = query_params.len() + 1;
                    or_parts.push(format!("(n.title LIKE ?{idx} OR n.content LIKE ?{idx})"));
                    query_params.push(Box::new(pattern));
                }
            }

            if scopes.contains(&SearchScope::Tags) {
                let pattern = format!("%{}%", q.replace(['%', '_'], ""));
                let idx = query_params.len() + 1;
                or_parts.push(format!(
                    "n.id IN (
                        SELECT nt.note_id FROM note_tags nt
                        JOIN tags tg ON tg.id = nt.tag_id
                        WHERE tg.user_id = ?1 AND tg.name LIKE ?{idx} COLLATE NOCASE
                     )"
                ));
                query_params.push(Box::new(pattern));
            }

            if scopes.contains(&SearchScope::Folders) {
                let pattern = format!("%{}%", q.replace(['%', '_'], ""));
                let idx = query_params.len() + 1;
                or_parts.push(format!(
                    "n.id IN (
                        WITH RECURSIVE matching(id) AS (
                            SELECT f.id FROM folders f
                            WHERE f.user_id = ?1 AND f.name LIKE ?{idx} COLLATE NOCASE
                            UNION ALL
                            SELECT f.id FROM folders f JOIN matching m ON f.parent_id = m.id
                            WHERE f.user_id = ?1
                        )
                        SELECT n2.id FROM notes n2 WHERE n2.folder_id IN (SELECT id FROM matching)
                     )"
                ));
                query_params.push(Box::new(pattern));
            }

            if !or_parts.is_empty() {
                clauses.push(format!("({})", or_parts.join(" OR ")));
            }
        }
    }

    // ---- Exact multi-tag filter (AND semantics) ----
    for tag in filter
        .tags
        .iter()
        .map(|t| t.trim())
        .filter(|t| !t.is_empty())
    {
        let idx = query_params.len() + 1;
        clauses.push(format!(
            "n.id IN (
                SELECT nt.note_id FROM note_tags nt
                JOIN tags tg ON tg.id = nt.tag_id
                WHERE tg.user_id = ?1 AND tg.name = ?{idx} COLLATE NOCASE
             )"
        ));
        query_params.push(Box::new(tag.to_string()));
    }

    if let Some(note_type) = filter.note_type {
        let idx = query_params.len() + 1;
        clauses.push(format!("n.note_type = ?{idx}"));
        query_params.push(Box::new(note_type.as_str().to_string()));
    }

    if let Some(pinned) = filter.is_pinned {
        let idx = query_params.len() + 1;
        clauses.push(format!("n.is_pinned = ?{idx}"));
        query_params.push(Box::new(if pinned { 1 } else { 0 }));
    }

    // ---- Folder filter (includes subfolders) ----
    if let Some(folder_ref) = filter.folder_id.as_ref().map(|f| f.trim()) {
        if !folder_ref.is_empty() {
            if folder_ref == "root" {
                clauses.push("n.folder_id IS NULL".to_string());
            } else {
                let idx = query_params.len() + 1;
                clauses.push(format!(
                    "n.folder_id IN (
                        WITH RECURSIVE sub(id) AS (
                            SELECT ?{idx}
                            UNION ALL
                            SELECT f.id FROM folders f JOIN sub s ON f.parent_id = s.id
                        )
                        SELECT id FROM sub
                     )"
                ));
                query_params.push(Box::new(folder_ref.to_string()));
            }
        }
    }

    let where_sql = clauses.join(" AND ");

    // Count
    let count_sql = format!("SELECT COUNT(*) {NOTE_TABLES} WHERE {where_sql}");
    let total: i64 = {
        let mut stmt = conn.prepare(&count_sql)?;
        stmt.query_row(params_from_iter(query_params.iter()), |r| r.get(0))?
    };

    // Data
    let limit_idx = query_params.len() + 1;
    let offset_idx = limit_idx + 1;
    let data_sql = format!(
        "SELECT {NOTE_COLUMNS} {NOTE_TABLES}
         WHERE {where_sql}
         ORDER BY n.is_pinned DESC, n.updated_at DESC
         LIMIT ?{limit_idx} OFFSET ?{offset_idx}"
    );
    query_params.push(Box::new(limit as i64));
    query_params.push(Box::new(offset as i64));

    let mut stmt = conn.prepare(&data_sql)?;
    let mut notes: Vec<Note> = stmt
        .query_map(params_from_iter(query_params.iter()), |row| {
            note_from_row(row)
        })?
        .filter_map(std::result::Result::ok)
        .collect();

    attach_tags(conn, user_id, &mut notes)?;

    Ok((notes, total))
}

pub fn update_note(
    conn: &mut Connection,
    user_id: &str,
    note_id: &str,
    req: UpdateNoteRequest,
    version_min_interval_secs: i64,
    version_max_keep: usize,
) -> Result<Note> {
    let existing = get_note_by_id(conn, user_id, note_id)?
        .ok_or_else(|| AppError::NotFound(format!("Note {note_id} not found")))?;

    // Snapshot the PREVIOUS state into the version history (throttled)
    crate::db::versions::record_version_if_due(
        conn,
        note_id,
        &existing.title,
        &existing.content,
        existing.note_type.as_str(),
        version_min_interval_secs,
        version_max_keep,
    )?;

    let title = req.title.unwrap_or(existing.title);
    let content = req.content.unwrap_or(existing.content);
    let note_type = req.note_type.unwrap_or(existing.note_type);
    let color = req.color.or(existing.color);
    let icon = req.icon.or(existing.icon);
    let is_pinned = req.is_pinned.unwrap_or(existing.is_pinned);
    let is_archived = req.is_archived.unwrap_or(existing.is_archived);
    let folder_id = match req.folder_id {
        Some(f) => f,
        None => existing.folder_id,
    };

    // Validate folder ownership
    if let Some(ref fid) = folder_id {
        let ok: Option<i64> = conn
            .query_row(
                "SELECT 1 FROM folders WHERE id = ?1 AND user_id = ?2",
                params![fid, user_id],
                |r| r.get(0),
            )
            .optional()?;
        if ok.is_none() {
            return Err(AppError::NotFound(format!("Folder {fid} not found")));
        }
    }

    let tx = conn.transaction()?;
    tx.execute(
        "UPDATE notes
         SET title = ?1, content = ?2, note_type = ?3, color = ?4, icon = ?5,
             is_pinned = ?6, is_archived = ?7, folder_id = ?8, updated_at = datetime('now')
         WHERE id = ?9 AND user_id = ?10 AND is_deleted = 0",
        params![
            title.trim(),
            content,
            note_type.as_str(),
            color,
            icon,
            if is_pinned { 1 } else { 0 },
            if is_archived { 1 } else { 0 },
            folder_id,
            note_id,
            user_id
        ],
    )?;

    if let Some(tags) = req.tags {
        set_note_tags(&tx, user_id, note_id, &tags)?;
    }
    tx.commit()?;

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
