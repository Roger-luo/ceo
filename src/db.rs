use std::path::{Path, PathBuf};

use chrono::Utc;
use rusqlite::Connection;

use crate::error::DbError;

type Result<T> = std::result::Result<T, DbError>;

/// One row in the `issues` table.
#[derive(Debug, Clone)]
pub struct IssueRow {
    pub repo: String,
    pub number: u64,
    pub title: String,
    pub body: Option<String>,
    pub state: Option<String>,
    pub labels: String,
    pub assignees: String,
    pub created_at: String,
    pub updated_at: String,
    pub project_status: Option<String>,
    pub project_start_date: Option<String>,
    pub project_target_date: Option<String>,
    pub project_priority: Option<String>,
}

/// One row in the `comments` table.
#[derive(Debug, Clone)]
pub struct CommentRow {
    pub repo: String,
    pub issue_number: u64,
    pub comment_id: u64,
    pub author: String,
    pub body: String,
    pub created_at: String,
}

/// Bulk upsert issues. Returns count of rows written.
pub fn upsert_issues(conn: &Connection, issues: &[IssueRow]) -> Result<usize> {
    let mut count = 0;
    let mut stmt = conn.prepare(
        "INSERT OR REPLACE INTO issues (
            repo, number, title, body, state, labels, assignees,
            created_at, updated_at, project_status, project_start_date,
            project_target_date, project_priority, synced_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
    )?;
    let now = Utc::now().to_rfc3339();
    for issue in issues {
        stmt.execute(rusqlite::params![
            issue.repo,
            issue.number as i64,
            issue.title,
            issue.body,
            issue.state,
            issue.labels,
            issue.assignees,
            issue.created_at,
            issue.updated_at,
            issue.project_status,
            issue.project_start_date,
            issue.project_target_date,
            issue.project_priority,
            now,
        ])?;
        count += 1;
    }
    Ok(count)
}

/// Bulk upsert comments. Returns count of rows written.
pub fn upsert_comments(conn: &Connection, comments: &[CommentRow]) -> Result<usize> {
    let mut count = 0;
    let mut stmt = conn.prepare(
        "INSERT OR REPLACE INTO comments (
            repo, issue_number, comment_id, author, body, created_at, synced_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
    )?;
    let now = Utc::now().to_rfc3339();
    for comment in comments {
        stmt.execute(rusqlite::params![
            comment.repo,
            comment.issue_number as i64,
            comment.comment_id as i64,
            comment.author,
            comment.body,
            comment.created_at,
            now,
        ])?;
        count += 1;
    }
    Ok(count)
}

/// Record a sync log entry.
pub fn log_sync(
    conn: &Connection,
    repo: &str,
    issues_count: usize,
    comments_count: usize,
) -> Result<()> {
    conn.execute(
        "INSERT INTO sync_log (repo, synced_at, issues_synced, comments_synced)
         VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![
            repo,
            Utc::now().to_rfc3339(),
            issues_count as i64,
            comments_count as i64,
        ],
    )?;
    Ok(())
}

/// Query issues updated since `since` (ISO 8601 string) for the given repos.
pub fn query_recent_issues(
    conn: &Connection,
    repos: &[String],
    since: &str,
) -> Result<Vec<IssueRow>> {
    if repos.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders: Vec<String> = (1..=repos.len()).map(|i| format!("?{i}")).collect();
    let sql = format!(
        "SELECT repo, number, title, body, state, labels, assignees,
                created_at, updated_at, project_status, project_start_date,
                project_target_date, project_priority
         FROM issues
         WHERE repo IN ({}) AND updated_at >= ?{}
         ORDER BY updated_at DESC",
        placeholders.join(", "),
        repos.len() + 1,
    );
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> =
        repos.iter().map(|r| Box::new(r.clone()) as Box<dyn rusqlite::types::ToSql>).collect();
    params.push(Box::new(since.to_string()));
    let params_ref: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_ref.as_slice(), |row| {
        Ok(IssueRow {
            repo: row.get(0)?,
            number: row.get::<_, i64>(1)? as u64,
            title: row.get(2)?,
            body: row.get(3)?,
            state: row.get(4)?,
            labels: row.get::<_, Option<String>>(5)?.unwrap_or_default(),
            assignees: row.get::<_, Option<String>>(6)?.unwrap_or_default(),
            created_at: row.get::<_, Option<String>>(7)?.unwrap_or_default(),
            updated_at: row.get::<_, Option<String>>(8)?.unwrap_or_default(),
            project_status: row.get(9)?,
            project_start_date: row.get(10)?,
            project_target_date: row.get(11)?,
            project_priority: row.get(12)?,
        })
    })?;
    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

/// Query comments for specific issues in a repo.
pub fn query_comments_for_issues(
    conn: &Connection,
    repo: &str,
    issue_numbers: &[u64],
) -> Result<Vec<CommentRow>> {
    if issue_numbers.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders: Vec<String> = (2..=issue_numbers.len() + 1).map(|i| format!("?{i}")).collect();
    let sql = format!(
        "SELECT repo, issue_number, comment_id, author, body, created_at
         FROM comments
         WHERE repo = ?1 AND issue_number IN ({})
         ORDER BY issue_number, created_at",
        placeholders.join(", "),
    );
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    params.push(Box::new(repo.to_string()));
    for &n in issue_numbers {
        params.push(Box::new(n as i64));
    }
    let params_ref: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_ref.as_slice(), |row| {
        Ok(CommentRow {
            repo: row.get(0)?,
            issue_number: row.get::<_, i64>(1)? as u64,
            comment_id: row.get::<_, i64>(2)? as u64,
            author: row.get(3)?,
            body: row.get(4)?,
            created_at: row.get(5)?,
        })
    })?;
    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

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
pub fn open_db_at(path: &Path) -> Result<Connection> {
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
    let flags = rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE;
    match Connection::open_with_flags(&path, flags) {
        Ok(conn) => Ok(conn),
        Err(_) => Err(DbError::NotFound(path)),
    }
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
