use crate::error::{AppError, Result};
use crate::models::user::{OtpRecord, Session, User};
use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

pub fn get_user_by_email(conn: &Connection, email: &str) -> Result<Option<User>> {
    let mut stmt = conn.prepare(
        "SELECT id, email, auth_method, is_admin, created_at, last_login
         FROM users WHERE email = ?1 COLLATE NOCASE",
    )?;
    let user = stmt
        .query_row(params![email], |row| {
            Ok(User {
                id: row.get(0)?,
                email: row.get(1)?,
                auth_method: row.get(2)?,
                is_admin: row.get::<_, i32>(3)? == 1,
                created_at: row.get(4)?,
                last_login: row.get(5)?,
            })
        })
        .optional()?;
    Ok(user)
}

pub fn get_user_by_id(conn: &Connection, id: &str) -> Result<Option<User>> {
    let mut stmt = conn.prepare(
        "SELECT id, email, auth_method, is_admin, created_at, last_login
         FROM users WHERE id = ?1",
    )?;
    let user = stmt
        .query_row(params![id], |row| {
            Ok(User {
                id: row.get(0)?,
                email: row.get(1)?,
                auth_method: row.get(2)?,
                is_admin: row.get::<_, i32>(3)? == 1,
                created_at: row.get(4)?,
                last_login: row.get(5)?,
            })
        })
        .optional()?;
    Ok(user)
}

pub fn create_or_get_user(
    conn: &Connection,
    email: &str,
    auth_method: &str,
    is_admin: bool,
) -> Result<User> {
    if let Some(user) = get_user_by_email(conn, email)? {
        return Ok(user);
    }

    let id = Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO users (id, email, auth_method, is_admin, created_at)
         VALUES (?1, ?2, ?3, ?4, datetime('now'))",
        params![id, email, auth_method, if is_admin { 1 } else { 0 }],
    )?;

    get_user_by_id(conn, &id)?.ok_or_else(|| AppError::Internal("User creation failed".into()))
}

pub fn update_last_login(conn: &Connection, user_id: &str) -> Result<()> {
    conn.execute(
        "UPDATE users SET last_login = datetime('now') WHERE id = ?1",
        params![user_id],
    )?;
    Ok(())
}

pub fn create_session(
    conn: &Connection,
    token: &str,
    user_id: &str,
    expires_at: &str,
) -> Result<Session> {
    conn.execute(
        "INSERT INTO sessions (token, user_id, expires_at)
         VALUES (?1, ?2, ?3)",
        params![token, user_id, expires_at],
    )?;

    Ok(Session {
        token: token.to_string(),
        user_id: user_id.to_string(),
        created_at: chrono::Utc::now().to_rfc3339(),
        expires_at: expires_at.to_string(),
    })
}

pub fn get_session_with_user(conn: &Connection, token: &str) -> Result<Option<(Session, User)>> {
    let mut stmt = conn.prepare(
        "SELECT s.token, s.user_id, s.created_at, s.expires_at,
                u.id, u.email, u.auth_method, u.is_admin, u.created_at, u.last_login
         FROM sessions s
         JOIN users u ON s.user_id = u.id
         WHERE s.token = ?1 AND s.expires_at > datetime('now')",
    )?;

    let result = stmt
        .query_row(params![token], |row| {
            let session = Session {
                token: row.get(0)?,
                user_id: row.get(1)?,
                created_at: row.get(2)?,
                expires_at: row.get(3)?,
            };
            let user = User {
                id: row.get(4)?,
                email: row.get(5)?,
                auth_method: row.get(6)?,
                is_admin: row.get::<_, i32>(7)? == 1,
                created_at: row.get(8)?,
                last_login: row.get(9)?,
            };
            Ok((session, user))
        })
        .optional()?;

    Ok(result)
}

pub fn delete_session(conn: &Connection, token: &str) -> Result<()> {
    conn.execute("DELETE FROM sessions WHERE token = ?1", params![token])?;
    Ok(())
}

pub fn clean_expired_sessions(conn: &Connection) -> Result<usize> {
    let count = conn.execute(
        "DELETE FROM sessions WHERE expires_at <= datetime('now')",
        [],
    )?;
    Ok(count)
}

pub fn create_otp(conn: &Connection, email: &str, code: &str, expires_at: &str) -> Result<i64> {
    // Invalidate any active unused OTPs for this email first
    conn.execute(
        "UPDATE otps SET used = 1 WHERE email = ?1 COLLATE NOCASE AND used = 0",
        params![email],
    )?;

    conn.execute(
        "INSERT INTO otps (email, code, expires_at)
         VALUES (?1, ?2, ?3)",
        params![email, code, expires_at],
    )?;

    Ok(conn.last_insert_rowid())
}

pub fn get_latest_active_otp(conn: &Connection, email: &str) -> Result<Option<OtpRecord>> {
    let mut stmt = conn.prepare(
        "SELECT id, email, code, created_at, expires_at, used, attempts
         FROM otps
         WHERE email = ?1 COLLATE NOCASE AND used = 0 AND expires_at > datetime('now')
         ORDER BY id DESC LIMIT 1",
    )?;

    let otp = stmt
        .query_row(params![email], |row| {
            Ok(OtpRecord {
                id: row.get(0)?,
                email: row.get(1)?,
                code: row.get(2)?,
                created_at: row.get(3)?,
                expires_at: row.get(4)?,
                used: row.get::<_, i32>(5)? == 1,
                attempts: row.get(6)?,
            })
        })
        .optional()?;

    Ok(otp)
}

pub fn increment_otp_attempts(conn: &Connection, otp_id: i64) -> Result<i32> {
    conn.execute(
        "UPDATE otps SET attempts = attempts + 1 WHERE id = ?1",
        params![otp_id],
    )?;

    let mut stmt = conn.prepare("SELECT attempts FROM otps WHERE id = ?1")?;
    let attempts: i32 = stmt.query_row(params![otp_id], |row| row.get(0))?;
    Ok(attempts)
}

pub fn mark_otp_used(conn: &Connection, otp_id: i64) -> Result<()> {
    conn.execute("UPDATE otps SET used = 1 WHERE id = ?1", params![otp_id])?;
    Ok(())
}
