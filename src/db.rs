use std::path::PathBuf;

use rusqlite::Connection;

use crate::error::DbError;

type Result<T> = std::result::Result<T, DbError>;

/// Returns the default database path using the platform data directory.
/// macOS: ~/Library/Application Support/ceo/ceo.db
/// Linux: ~/.local/share/ceo/ceo.db
pub fn db_path() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("ceo")
        .join("ceo.db")
}

/// Open (or create) the database at the given path and ensure schema exists.
pub fn open_db_at(path: &PathBuf) -> Result<Connection> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| DbError::CreateDir { path: parent.to_path_buf(), source: e })?;
    }
    let conn = Connection::open(path)?;
    create_schema(&conn)?;
    Ok(conn)
}

/// Open the database at the default path.
pub fn open_db() -> Result<Connection> {
    open_db_at(&db_path())
}

/// Open an existing database. Returns NotFound if the file doesn't exist.
pub fn open_existing_db() -> Result<Connection> {
    let path = db_path();
    if !path.exists() {
        return Err(DbError::NotFound(path));
    }
    open_db_at(&path)
}

fn create_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS issues (
            repo            TEXT NOT NULL,
            number          INTEGER NOT NULL,
            title           TEXT NOT NULL,
            body            TEXT,
            state           TEXT,
            labels          TEXT,
            assignees       TEXT,
            created_at      TEXT,
            updated_at      TEXT,
            project_status  TEXT,
            project_start_date TEXT,
            project_target_date TEXT,
            project_priority TEXT,
            synced_at       TEXT NOT NULL,
            PRIMARY KEY (repo, number)
        );

        CREATE TABLE IF NOT EXISTS comments (
            repo            TEXT NOT NULL,
            issue_number    INTEGER NOT NULL,
            comment_id      INTEGER NOT NULL,
            author          TEXT NOT NULL,
            body            TEXT NOT NULL,
            created_at      TEXT NOT NULL,
            synced_at       TEXT NOT NULL,
            PRIMARY KEY (repo, issue_number, comment_id),
            FOREIGN KEY (repo, issue_number) REFERENCES issues(repo, number)
        );

        CREATE TABLE IF NOT EXISTS sync_log (
            repo            TEXT NOT NULL,
            synced_at       TEXT NOT NULL,
            issues_synced   INTEGER,
            comments_synced INTEGER
        );"
    )?;
    Ok(())
}
