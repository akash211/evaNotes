use evanotes::db::{init_db, DbPool};

pub fn init_test_db() -> (DbPool, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("Failed to create tempdir for test DB");
    let db_path = dir.path().join("test_evanotes.db");
    let pool = init_db(&db_path).expect("Failed to init test DB");
    (pool, dir)
}
