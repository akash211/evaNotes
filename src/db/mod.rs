pub mod migrations;
pub mod notes;
pub mod reminders;
pub mod users;

use crate::error::{AppError, Result};
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use std::path::Path;

pub type DbPool = Pool<SqliteConnectionManager>;

pub fn init_db(db_path: &Path) -> Result<DbPool> {
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| AppError::Internal(format!("Failed to create database directory: {e}")))?;
    }

    let manager = SqliteConnectionManager::file(db_path);
    let pool = Pool::builder()
        .max_size(10)
        .build(manager)
        .map_err(|e| AppError::Internal(format!("Failed to build connection pool: {e}")))?;

    // Configure connection and run migrations
    {
        let mut conn = pool.get()?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA foreign_keys = ON;
             PRAGMA busy_timeout = 5000;",
        )?;

        migrations::run_migrations(&mut conn)
            .map_err(|e| AppError::Internal(format!("Failed to run migrations: {e}")))?;
    }

    tracing::info!("Database initialized at {:?}", db_path);
    Ok(pool)
}

/// Helper for tests to initialize an in-memory or temp SQLite pool
pub fn init_memory_db() -> Result<DbPool> {
    let manager = SqliteConnectionManager::memory();
    let pool = Pool::builder()
        .max_size(1)
        .build(manager)
        .map_err(|e| AppError::Internal(format!("Failed to build in-memory pool: {e}")))?;

    {
        let mut conn = pool.get()?;
        conn.execute_batch(
            "PRAGMA foreign_keys = ON;
             PRAGMA busy_timeout = 5000;",
        )?;

        migrations::run_migrations(&mut conn)
            .map_err(|e| AppError::Internal(format!("Failed to run migrations: {e}")))?;
    }

    Ok(pool)
}
