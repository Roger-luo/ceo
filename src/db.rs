use std::path::{Path, PathBuf};

use chrono::Utc;
use rusqlite::{Connection, OptionalExtension};

use crate::error::DbError;

type Result<T> = std::result::Result<T, DbError>;

/// One row in the `issues` table. Covers both issues and pull requests.
#[derive(Debug, Clone)]
pub struct IssueRow {
    pub repo: String,
    pub number: u64,
    pub title: String,
    pub body: Option<String>,
    pub state: Option<String>,
    pub kind: String,
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

/// One row in the `commits` table.
#[derive(Debug, Clone)]
pub struct CommitRow {
    pub repo: String,
    pub sha: String,
    pub author: String,
    pub message: String,
    pub committed_at: String,
    /// Which branch this commit was fetched from (empty = default branch).
    pub branch: String,
}

/// Bulk upsert issues. Returns count of rows written.
pub fn upsert_issues(conn: &Connection, issues: &[IssueRow]) -> Result<usize> {
    let mut count = 0;
    let mut stmt = conn.prepare(
        "INSERT OR REPLACE INTO issues (
            repo, number, title, body, state, kind, labels, assignees,
            created_at, updated_at, project_status, project_start_date,
            project_target_date, project_priority, synced_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
    )?;
    let now = Utc::now().to_rfc3339();
    for issue in issues {
        stmt.execute(rusqlite::params![
            issue.repo,
            issue.number as i64,
            issue.title,
            issue.body,
            issue.state,
            issue.kind,
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

/// Bulk upsert commits. Returns count of rows written.
pub fn upsert_commits(conn: &Connection, commits: &[CommitRow]) -> Result<usize> {
    let mut count = 0;
    let mut stmt = conn.prepare(
        "INSERT OR REPLACE INTO commits (
            repo, sha, author, message, committed_at, branch, synced_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
    )?;
    let now = Utc::now().to_rfc3339();
    for commit in commits {
        stmt.execute(rusqlite::params![
            commit.repo,
            commit.sha,
            commit.author,
            commit.message,
            commit.committed_at,
            commit.branch,
            now,
        ])?;
        count += 1;
    }
    Ok(count)
}

/// Query commits since `since` (ISO 8601 string) for the given repos.
pub fn query_recent_commits(
    conn: &Connection,
    repos: &[String],
    since: &str,
) -> Result<Vec<CommitRow>> {
    if repos.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders: Vec<String> = (1..=repos.len()).map(|i| format!("?{i}")).collect();
    let sql = format!(
        "SELECT repo, sha, author, message, committed_at, COALESCE(branch, '')
         FROM commits
         WHERE repo IN ({}) AND committed_at >= ?{}
         ORDER BY committed_at DESC",
        placeholders.join(", "),
        repos.len() + 1,
    );
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> =
        repos.iter().map(|r| Box::new(r.clone()) as Box<dyn rusqlite::types::ToSql>).collect();
    params.push(Box::new(since.to_string()));
    let params_ref: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_ref.as_slice(), |row| {
        Ok(CommitRow {
            repo: row.get(0)?,
            sha: row.get(1)?,
            author: row.get(2)?,
            message: row.get(3)?,
            committed_at: row.get(4)?,
            branch: row.get(5)?,
        })
    })?;
    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

/// Per-issue cached summary (issue description + discussion).
#[derive(Debug, Clone)]
pub struct IssueCacheRow {
    pub issue_summary: String,
    pub discussion_summary: String,
    pub discussion_hash: String,
}

/// Query the cached issue summary.
pub fn query_issue_cache(
    conn: &Connection,
    repo: &str,
    issue_number: u64,
) -> Result<Option<IssueCacheRow>> {
    let mut stmt = conn.prepare(
        "SELECT issue_summary, discussion_summary, discussion_hash
         FROM issue_cache WHERE repo = ?1 AND issue_number = ?2",
    )?;
    let result = stmt
        .query_row(rusqlite::params![repo, issue_number as i64], |row| {
            Ok(IssueCacheRow {
                issue_summary: row.get(0)?,
                discussion_summary: row.get(1)?,
                discussion_hash: row.get(2)?,
            })
        })
        .optional()?;
    Ok(result)
}

/// Save or update the cached issue summary.
pub fn save_issue_cache(
    conn: &Connection,
    repo: &str,
    issue_number: u64,
    issue_summary: &str,
    discussion_summary: &str,
    discussion_hash: &str,
) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO issue_cache
         (repo, issue_number, issue_summary, discussion_summary, discussion_hash, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![
            repo,
            issue_number as i64,
            issue_summary,
            discussion_summary,
            discussion_hash,
            Utc::now().to_rfc3339(),
        ],
    )?;
    Ok(())
}

/// Query the cached report summary for a repo.
/// Returns (summary, input_hash) if a cached entry exists.
pub fn query_report_cache(
    conn: &Connection,
    repo: &str,
) -> Result<Option<(String, String)>> {
    let mut stmt = conn.prepare(
        "SELECT summary, input_hash FROM report_cache WHERE repo = ?1",
    )?;
    let result = stmt
        .query_row(rusqlite::params![repo], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .optional()?;
    Ok(result)
}

/// Save or update the cached report summary for a repo.
pub fn save_report_cache(
    conn: &Connection,
    repo: &str,
    summary: &str,
    input_hash: &str,
) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO report_cache (repo, generated_at, summary, input_hash)
         VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![repo, Utc::now().to_rfc3339(), summary, input_hash],
    )?;
    Ok(())
}

/// Clear all generated summary caches (report_cache and issue_cache).
/// Does NOT touch synced data (issues, comments, commits).
pub fn clear_caches(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "DELETE FROM report_cache; DELETE FROM issue_cache;"
    )?;
    Ok(())
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

/// Return the most recent sync timestamp for a repo, or None if never synced.
pub fn query_last_sync(conn: &Connection, repo: &str) -> Result<Option<String>> {
    let mut stmt = conn.prepare(
        "SELECT synced_at FROM sync_log WHERE repo = ?1 ORDER BY synced_at DESC LIMIT 1",
    )?;
    let result = stmt
        .query_row(rusqlite::params![repo], |row| row.get::<_, String>(0))
        .optional()?;
    Ok(result)
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
        "SELECT repo, number, title, body, state, kind, labels, assignees,
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
            kind: row.get::<_, Option<String>>(5)?.unwrap_or_else(|| "issue".to_string()),
            labels: row.get::<_, Option<String>>(6)?.unwrap_or_default(),
            assignees: row.get::<_, Option<String>>(7)?.unwrap_or_default(),
            created_at: row.get::<_, Option<String>>(8)?.unwrap_or_default(),
            updated_at: row.get::<_, Option<String>>(9)?.unwrap_or_default(),
            project_status: row.get(10)?,
            project_start_date: row.get(11)?,
            project_target_date: row.get(12)?,
            project_priority: row.get(13)?,
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
    migrate_schema(&conn);
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
        Ok(conn) => {
            migrate_schema(&conn);
            Ok(conn)
        }
        Err(_) => Err(DbError::NotFound(path)),
    }
}

/// Run schema migrations on an existing database.
fn migrate_schema(conn: &Connection) {
    // Add 'kind' column if it doesn't exist (for databases created before PR sync)
    let has_kind: bool = conn
        .prepare("SELECT COUNT(*) FROM pragma_table_info('issues') WHERE name='kind'")
        .and_then(|mut stmt| stmt.query_row([], |row| row.get::<_, i64>(0)))
        .unwrap_or(0)
        > 0;
    if !has_kind {
        let _ = conn.execute_batch(
            "ALTER TABLE issues ADD COLUMN kind TEXT NOT NULL DEFAULT 'issue';"
        );
    }

    // Create commits table if it doesn't exist (for databases created before commit sync)
    let _ = conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS commits (
            repo            TEXT NOT NULL,
            sha             TEXT NOT NULL,
            author          TEXT NOT NULL,
            message         TEXT NOT NULL,
            committed_at    TEXT NOT NULL,
            branch          TEXT NOT NULL DEFAULT '',
            synced_at       TEXT NOT NULL,
            PRIMARY KEY (repo, sha)
        );"
    );

    // Add branch column to existing commits table
    let _ = conn.execute_batch(
        "ALTER TABLE commits ADD COLUMN branch TEXT NOT NULL DEFAULT '';"
    );

    // Create report_cache table if it doesn't exist
    let _ = conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS report_cache (
            repo            TEXT PRIMARY KEY,
            generated_at    TEXT NOT NULL,
            summary         TEXT NOT NULL,
            input_hash      TEXT NOT NULL
        );"
    );

    // Create issue_cache table if it doesn't exist
    let _ = conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS issue_cache (
            repo                TEXT NOT NULL,
            issue_number        INTEGER NOT NULL,
            issue_summary       TEXT NOT NULL,
            discussion_summary  TEXT NOT NULL,
            discussion_hash     TEXT NOT NULL,
            updated_at          TEXT NOT NULL,
            PRIMARY KEY (repo, issue_number)
        );"
    );
}

fn create_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS issues (
            repo            TEXT NOT NULL,
            number          INTEGER NOT NULL,
            title           TEXT NOT NULL,
            body            TEXT,
            state           TEXT,
            kind            TEXT NOT NULL DEFAULT 'issue',
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

        CREATE TABLE IF NOT EXISTS commits (
            repo            TEXT NOT NULL,
            sha             TEXT NOT NULL,
            author          TEXT NOT NULL,
            message         TEXT NOT NULL,
            committed_at    TEXT NOT NULL,
            branch          TEXT NOT NULL DEFAULT '',
            synced_at       TEXT NOT NULL,
            PRIMARY KEY (repo, sha)
        );

        CREATE TABLE IF NOT EXISTS sync_log (
            repo            TEXT NOT NULL,
            synced_at       TEXT NOT NULL,
            issues_synced   INTEGER,
            comments_synced INTEGER
        );

        CREATE TABLE IF NOT EXISTS report_cache (
            repo            TEXT PRIMARY KEY,
            generated_at    TEXT NOT NULL,
            summary         TEXT NOT NULL,
            input_hash      TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS issue_cache (
            repo                TEXT NOT NULL,
            issue_number        INTEGER NOT NULL,
            issue_summary       TEXT NOT NULL,
            discussion_summary  TEXT NOT NULL,
            discussion_hash     TEXT NOT NULL,
            updated_at          TEXT NOT NULL,
            PRIMARY KEY (repo, issue_number)
        );"
    )?;
    Ok(())
}
