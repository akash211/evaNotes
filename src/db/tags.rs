use crate::error::{AppError, Result};
use crate::models::tag::Tag;
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use std::collections::HashMap;

fn row_to_tag(row: &rusqlite::Row) -> rusqlite::Result<Tag> {
    Ok(Tag {
        id: row.get(0)?,
        user_id: row.get(1)?,
        name: row.get(2)?,
        color: row.get(3)?,
        note_count: row.get(4)?,
    })
}

/// Normalize a tag name: trim, collapse inner whitespace. Case is preserved
/// (uniqueness is enforced case-insensitively at the DB level).
pub fn normalize_tag_name(name: &str) -> String {
    name.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn list_tags_with_counts(conn: &Connection, user_id: &str) -> Result<Vec<Tag>> {
    let mut stmt = conn.prepare(
        "SELECT t.id, t.user_id, t.name, t.color, COUNT(nt.note_id) AS note_count
         FROM tags t
         LEFT JOIN note_tags nt ON nt.tag_id = t.id
         WHERE t.user_id = ?1
         GROUP BY t.id
         ORDER BY t.name COLLATE NOCASE ASC",
    )?;
    let tags = stmt
        .query_map(params![user_id], row_to_tag)?
        .filter_map(std::result::Result::ok)
        .collect();
    Ok(tags)
}

/// Fast prefix-based tag autocomplete (uses idx_tags_user_name).
pub fn suggest_tags(
    conn: &Connection,
    user_id: &str,
    prefix: &str,
    limit: usize,
) -> Result<Vec<Tag>> {
    let prefix = prefix.trim();
    let limit = limit.clamp(1, 50) as i64;

    let tags: Vec<Tag> = if prefix.is_empty() {
        // Empty prefix: return most-used tags
        let mut stmt = conn.prepare(
            "SELECT t.id, t.user_id, t.name, t.color, COUNT(nt.note_id) AS note_count
             FROM tags t
             LEFT JOIN note_tags nt ON nt.tag_id = t.id
             WHERE t.user_id = ?1
             GROUP BY t.id
             ORDER BY note_count DESC, t.name COLLATE NOCASE ASC
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![user_id, limit], row_to_tag)?;
        rows.filter_map(std::result::Result::ok).collect()
    } else {
        let pattern = format!("{}%", prefix);
        let mut stmt = conn.prepare(
            "SELECT t.id, t.user_id, t.name, t.color, COUNT(nt.note_id) AS note_count
             FROM tags t
             LEFT JOIN note_tags nt ON nt.tag_id = t.id
             WHERE t.user_id = ?1 AND t.name LIKE ?2 COLLATE NOCASE
             GROUP BY t.id
             ORDER BY (t.name = ?3 COLLATE NOCASE) DESC, note_count DESC, t.name COLLATE NOCASE ASC
             LIMIT ?4",
        )?;
        let rows = stmt.query_map(params![user_id, pattern, prefix, limit], |row| {
            row_to_tag(row)
        })?;
        rows.filter_map(std::result::Result::ok).collect()
    };

    Ok(tags)
}

pub fn get_tag_by_name(conn: &Connection, user_id: &str, name: &str) -> Result<Option<Tag>> {
    let mut stmt = conn.prepare(
        "SELECT id, user_id, name, color, NULL
         FROM tags
         WHERE user_id = ?1 AND name = ?2 COLLATE NOCASE",
    )?;
    let tag = stmt
        .query_row(params![user_id, name], row_to_tag)
        .optional()?;
    Ok(tag)
}

pub fn get_tag_by_id(conn: &Connection, user_id: &str, tag_id: i64) -> Result<Option<Tag>> {
    let mut stmt = conn.prepare(
        "SELECT id, user_id, name, color, NULL
         FROM tags
         WHERE user_id = ?1 AND id = ?2",
    )?;
    let tag = stmt
        .query_row(params![user_id, tag_id], row_to_tag)
        .optional()?;
    Ok(tag)
}

pub fn create_tag(
    conn: &Connection,
    user_id: &str,
    name: &str,
    color: Option<String>,
) -> Result<Tag> {
    let name = normalize_tag_name(name);
    if name.is_empty() || name.len() > 64 {
        return Err(AppError::BadRequest(
            "Tag name must be between 1 and 64 characters".into(),
        ));
    }

    if let Some(existing) = get_tag_by_name(conn, user_id, &name)? {
        return Ok(existing);
    }

    conn.execute(
        "INSERT INTO tags (user_id, name, color) VALUES (?1, ?2, ?3)",
        params![user_id, name, color],
    )?;
    let id = conn.last_insert_rowid();

    get_tag_by_id(conn, user_id, id)?
        .ok_or_else(|| AppError::Internal("Tag creation failed".into()))
}

pub fn rename_tag(
    conn: &Connection,
    user_id: &str,
    tag_id: i64,
    new_name: Option<String>,
    color: Option<String>,
) -> Result<Tag> {
    let existing = get_tag_by_id(conn, user_id, tag_id)?
        .ok_or_else(|| AppError::NotFound(format!("Tag {tag_id} not found")))?;

    let name = match new_name {
        Some(n) => {
            let normalized = normalize_tag_name(&n);
            if normalized.is_empty() || normalized.len() > 64 {
                return Err(AppError::BadRequest(
                    "Tag name must be between 1 and 64 characters".into(),
                ));
            }
            if let Some(clash) = get_tag_by_name(conn, user_id, &normalized)? {
                if clash.id != tag_id {
                    return Err(AppError::Conflict(format!(
                        "Tag '{}' already exists",
                        normalized
                    )));
                }
            }
            normalized
        }
        None => existing.name,
    };

    let color = color.or(existing.color);

    conn.execute(
        "UPDATE tags SET name = ?1, color = ?2 WHERE id = ?3 AND user_id = ?4",
        params![name, color, tag_id, user_id],
    )?;

    get_tag_by_id(conn, user_id, tag_id)?
        .ok_or_else(|| AppError::Internal("Tag update failed".into()))
}

pub fn delete_tag(conn: &Connection, user_id: &str, tag_id: i64) -> Result<()> {
    // note_tags rows are removed via ON DELETE CASCADE
    let affected = conn.execute(
        "DELETE FROM tags WHERE id = ?1 AND user_id = ?2",
        params![tag_id, user_id],
    )?;
    if affected == 0 {
        return Err(AppError::NotFound(format!("Tag {tag_id} not found")));
    }
    Ok(())
}

/// Replace a note's tag set. Unknown tag names are created on the fly.
pub fn set_note_tags(
    tx: &Transaction,
    user_id: &str,
    note_id: &str,
    names: &[String],
) -> Result<Vec<String>> {
    // Ownership check
    let exists: Option<i64> = tx
        .query_row(
            "SELECT 1 FROM notes WHERE id = ?1 AND user_id = ?2 AND is_deleted = 0",
            params![note_id, user_id],
            |row| row.get(0),
        )
        .optional()?;
    if exists.is_none() {
        return Err(AppError::NotFound(format!("Note {note_id} not found")));
    }

    tx.execute("DELETE FROM note_tags WHERE note_id = ?1", params![note_id])?;

    let mut result = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for raw in names {
        let name = normalize_tag_name(raw);
        if name.is_empty() || !seen.insert(name.to_lowercase()) {
            continue;
        }
        let tag = match get_tag_by_name(tx, user_id, &name)? {
            Some(t) => t,
            None => {
                tx.execute(
                    "INSERT INTO tags (user_id, name) VALUES (?1, ?2)",
                    params![user_id, name],
                )?;
                let id = tx.last_insert_rowid();
                get_tag_by_id(tx, user_id, id)?
                    .ok_or_else(|| AppError::Internal("Tag creation failed".into()))?
            }
        };
        tx.execute(
            "INSERT OR IGNORE INTO note_tags (note_id, tag_id) VALUES (?1, ?2)",
            params![note_id, tag.id],
        )?;
        result.push(tag.name);
    }

    Ok(result)
}

/// Fetch tags for many notes in one query: note_id -> ordered tag names.
pub fn get_tags_for_notes(
    conn: &Connection,
    user_id: &str,
    note_ids: &[String],
) -> Result<HashMap<String, Vec<String>>> {
    let mut map: HashMap<String, Vec<String>> = HashMap::new();
    if note_ids.is_empty() {
        return Ok(map);
    }

    let placeholders = vec!["?"; note_ids.len()].join(",");
    let sql = format!(
        "SELECT nt.note_id, t.name
         FROM note_tags nt
         JOIN tags t ON t.id = nt.tag_id
         JOIN notes n ON n.id = nt.note_id AND n.user_id = ?1
         WHERE nt.note_id IN ({placeholders})
         ORDER BY t.name COLLATE NOCASE ASC"
    );

    let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(user_id.to_string())];
    for id in note_ids {
        params_vec.push(Box::new(id.clone()));
    }

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(params_vec.iter()), |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;

    for (note_id, name) in rows.flatten() {
        map.entry(note_id).or_default().push(name);
    }

    Ok(map)
}
