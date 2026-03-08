use ceo::db;
use std::path::PathBuf;

#[test]
fn open_db_creates_tables() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    let conn = db::open_db_at(&path).unwrap();

    let tables: Vec<String> = conn
        .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert!(tables.contains(&"issues".to_string()));
    assert!(tables.contains(&"comments".to_string()));
    assert!(tables.contains(&"sync_log".to_string()));
}

#[test]
fn open_db_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    let _conn1 = db::open_db_at(&path).unwrap();
    let _conn2 = db::open_db_at(&path).unwrap();
}

#[test]
fn db_path_returns_platform_path() {
    let path = db::db_path();
    assert!(path.ends_with("ceo/ceo.db") || path.ends_with("ceo\\ceo.db"));
}
