use crate::error::Result;
use crate::models::version::NoteVersion;
use rusqlite::{params, Connection, OptionalExtension};

/// Snapshots a note's previous state before an update. Only stores when the
/// latest stored version is at least `min_interval_secs` old, then prunes the
/// history to `max_keep` entries.
pub fn record_version_if_due(
    conn: &Connection,
    note_id: &str,
    title: &str,
    content: &str,
    note_type: &str,
    min_interval_secs: i64,
    max_keep: usize,
) -> Result<()> {
    let newest: Option<String> = conn
        .query_row(
            "SELECT created_at FROM note_versions WHERE note_id = ?1 ORDER BY id DESC LIMIT 1",
            params![note_id],
            |row| row.get(0),
        )
        .ok();

    if let Some(newest) = newest {
        // created_at is "YYYY-MM-DD HH:MM:SS" (UTC)
        let age_secs: Option<i64> = conn
            .query_row(
                "SELECT CAST((julianday('now') - julianday(?1)) * 86400 AS INTEGER)",
                params![newest],
                |row| row.get(0),
            )
            .ok();
        if let Some(age) = age_secs {
            if age < min_interval_secs {
                return Ok(());
            }
        }
    }

    conn.execute(
        "INSERT INTO note_versions (note_id, title, content, note_type)
         VALUES (?1, ?2, ?3, ?4)",
        params![note_id, title, content, note_type],
    )?;

    // Prune to the newest `max_keep` versions
    let max_keep = max_keep.max(1) as i64;
    conn.execute(
        "DELETE FROM note_versions
         WHERE note_id = ?1 AND id NOT IN (
             SELECT id FROM note_versions WHERE note_id = ?1
             ORDER BY id DESC LIMIT ?2
         )",
        params![note_id, max_keep],
    )?;

    Ok(())
}

pub fn list_versions(conn: &Connection, note_id: &str) -> Result<Vec<NoteVersion>> {
    let mut stmt = conn.prepare(
        "SELECT id, note_id, title, content, note_type, created_at
         FROM note_versions
         WHERE note_id = ?1
         ORDER BY id DESC",
    )?;
    let versions = stmt
        .query_map(params![note_id], |row| {
            Ok(NoteVersion {
                id: row.get(0)?,
                note_id: row.get(1)?,
                title: row.get(2)?,
                content: row.get(3)?,
                note_type: row.get(4)?,
                created_at: row.get(5)?,
            })
        })?
        .filter_map(std::result::Result::ok)
        .collect();
    Ok(versions)
}

pub fn get_version(
    conn: &Connection,
    note_id: &str,
    version_id: i64,
) -> Result<Option<NoteVersion>> {
    let mut stmt = conn.prepare(
        "SELECT id, note_id, title, content, note_type, created_at
         FROM note_versions
         WHERE note_id = ?1 AND id = ?2",
    )?;
    let version = stmt
        .query_row(params![note_id, version_id], |row| {
            Ok(NoteVersion {
                id: row.get(0)?,
                note_id: row.get(1)?,
                title: row.get(2)?,
                content: row.get(3)?,
                note_type: row.get(4)?,
                created_at: row.get(5)?,
            })
        })
        .optional()?;
    Ok(version)
}

pub fn clear_versions(conn: &Connection, note_id: &str) -> Result<usize> {
    let count = conn.execute(
        "DELETE FROM note_versions WHERE note_id = ?1",
        params![note_id],
    )?;
    Ok(count)
}
