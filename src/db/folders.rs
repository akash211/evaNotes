use crate::error::{AppError, Result};
use crate::models::folder::Folder;
use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

fn row_to_folder(row: &rusqlite::Row) -> rusqlite::Result<Folder> {
    Ok(Folder {
        id: row.get(0)?,
        user_id: row.get(1)?,
        name: row.get(2)?,
        parent_id: row.get(3)?,
        icon: row.get(4)?,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
        note_count: row.get(7)?,
        children: None,
    })
}

pub fn get_folder(conn: &Connection, user_id: &str, folder_id: &str) -> Result<Option<Folder>> {
    let mut stmt = conn.prepare(
        "SELECT f.id, f.user_id, f.name, f.parent_id, f.icon, f.created_at, f.updated_at,
                (SELECT COUNT(*) FROM notes n WHERE n.folder_id = f.id AND n.is_deleted = 0) AS note_count
         FROM folders f
         WHERE f.id = ?1 AND f.user_id = ?2",
    )?;
    let folder = stmt
        .query_row(params![folder_id, user_id], row_to_folder)
        .optional()?;
    Ok(folder)
}

pub fn list_folders(conn: &Connection, user_id: &str) -> Result<Vec<Folder>> {
    let mut stmt = conn.prepare(
        "SELECT f.id, f.user_id, f.name, f.parent_id, f.icon, f.created_at, f.updated_at,
                (SELECT COUNT(*) FROM notes n WHERE n.folder_id = f.id AND n.is_deleted = 0) AS note_count
         FROM folders f
         WHERE f.user_id = ?1
         ORDER BY f.name COLLATE NOCASE ASC",
    )?;
    let folders = stmt
        .query_map(params![user_id], row_to_folder)?
        .filter_map(std::result::Result::ok)
        .collect();
    Ok(folders)
}

/// Build the nested tree (folders with `children`).
pub fn list_folder_tree(conn: &Connection, user_id: &str) -> Result<Vec<Folder>> {
    let flat = list_folders(conn, user_id)?;
    let mut root: Vec<Folder> = Vec::new();
    let mut children_map: std::collections::HashMap<String, Vec<Folder>> =
        std::collections::HashMap::new();

    for folder in flat {
        match &folder.parent_id {
            Some(pid) => children_map.entry(pid.clone()).or_default().push(folder),
            None => root.push(folder),
        }
    }

    fn attach_children(
        list: &mut [Folder],
        map: &mut std::collections::HashMap<String, Vec<Folder>>,
    ) {
        for f in list.iter_mut() {
            if let Some(kids) = map.remove(&f.id) {
                let mut kids = kids;
                attach_children(&mut kids, map);
                f.children = Some(kids);
            }
        }
    }

    attach_children(&mut root, &mut children_map);

    // Orphans whose parent was missing (should not happen with FKs, but be safe)
    for (_, orphans) in children_map {
        root.extend(orphans);
    }

    Ok(root)
}

pub fn create_folder(
    conn: &Connection,
    user_id: &str,
    name: &str,
    parent_id: Option<String>,
    icon: Option<String>,
) -> Result<Folder> {
    let name = name.trim();
    if name.is_empty() || name.len() > 128 {
        return Err(AppError::BadRequest(
            "Folder name must be between 1 and 128 characters".into(),
        ));
    }

    if let Some(ref pid) = parent_id {
        if get_folder(conn, user_id, pid)?.is_none() {
            return Err(AppError::NotFound(format!("Parent folder {pid} not found")));
        }
    }

    let id = Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO folders (id, user_id, name, parent_id, icon)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![id, user_id, name, parent_id, icon],
    )?;

    get_folder(conn, user_id, &id)?
        .ok_or_else(|| AppError::Internal("Folder creation failed".into()))
}

pub fn update_folder(
    conn: &Connection,
    user_id: &str,
    folder_id: &str,
    name: Option<String>,
    parent_id: Option<Option<String>>,
    icon: Option<String>,
) -> Result<Folder> {
    let existing = get_folder(conn, user_id, folder_id)?
        .ok_or_else(|| AppError::NotFound(format!("Folder {folder_id} not found")))?;

    let name = match name {
        Some(n) => {
            let n = n.trim().to_string();
            if n.is_empty() || n.len() > 128 {
                return Err(AppError::BadRequest(
                    "Folder name must be between 1 and 128 characters".into(),
                ));
            }
            n
        }
        None => existing.name,
    };

    let new_parent = match parent_id {
        Some(p) => p,
        None => existing.parent_id,
    };

    // Validate new parent exists and avoid cycles
    if let Some(ref pid) = new_parent {
        if pid == folder_id {
            return Err(AppError::BadRequest(
                "A folder cannot be its own parent".into(),
            ));
        }
        if get_folder(conn, user_id, pid)?.is_none() {
            return Err(AppError::NotFound(format!("Parent folder {pid} not found")));
        }
        let descendants = subtree_ids(conn, user_id, folder_id)?;
        if descendants.contains(pid) {
            return Err(AppError::BadRequest(
                "A folder cannot be moved into its own subfolder".into(),
            ));
        }
    }

    let icon = icon.or(existing.icon);

    conn.execute(
        "UPDATE folders SET name = ?1, parent_id = ?2, icon = ?3, updated_at = datetime('now')
         WHERE id = ?4 AND user_id = ?5",
        params![name, new_parent, icon, folder_id, user_id],
    )?;

    get_folder(conn, user_id, folder_id)?
        .ok_or_else(|| AppError::Internal("Folder update failed".into()))
}

/// Delete a folder: child folders move up one level, notes move to top level
/// (notes.folder_id is SET NULL by the FK).
pub fn delete_folder(conn: &Connection, user_id: &str, folder_id: &str) -> Result<()> {
    let existing = get_folder(conn, user_id, folder_id)?
        .ok_or_else(|| AppError::NotFound(format!("Folder {folder_id} not found")))?;

    // Re-home subfolders to this folder's parent
    conn.execute(
        "UPDATE folders SET parent_id = ?1, updated_at = datetime('now')
         WHERE parent_id = ?2 AND user_id = ?3",
        params![existing.parent_id, folder_id, user_id],
    )?;

    // FK ON DELETE SET NULL lifts the folder's own notes to top level
    conn.execute(
        "DELETE FROM folders WHERE id = ?1 AND user_id = ?2",
        params![folder_id, user_id],
    )?;

    Ok(())
}

/// All descendant ids of a folder (including itself).
pub fn subtree_ids(conn: &Connection, _user_id: &str, folder_id: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "WITH RECURSIVE sub(id) AS (
            SELECT ?1
            UNION ALL
            SELECT f.id FROM folders f JOIN sub s ON f.parent_id = s.id
         )
         SELECT id FROM sub",
    )?;
    let ids = stmt
        .query_map(params![folder_id], |row| row.get::<_, String>(0))?
        .filter_map(std::result::Result::ok)
        .collect();
    Ok(ids)
}
