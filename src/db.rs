use std::path::{Path, PathBuf};

use chrono::Utc;
use rusqlite::{Connection, OptionalExtension};

use crate::error::DbError;

pub use ceo_schema::{CommentRow, CommitRow, CommitStatsRow, ContributorStatsRow, EmailMappingRow, IssueRow};

type Result<T> = std::result::Result<T, DbError>;

/// Bulk upsert issues. Returns count of rows written.
pub fn upsert_issues(conn: &Connection, issues: &[IssueRow]) -> Result<usize> {
    let mut count = 0;
    let mut stmt = conn.prepare(
        "INSERT OR REPLACE INTO issues (
            repo, number, title, body, state, kind, labels, assignees,
            created_at, updated_at, project_status, project_start_date,
            project_target_date, project_priority, author,
            pr_additions, pr_deletions, synced_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
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
            issue.author,
            issue.pr_additions,
            issue.pr_deletions,
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

/// Query contributor stats since `since` (ISO 8601 date, e.g. "2026-03-01") for the given repos.
/// Aggregates from `commit_stats` table, joining with `email_to_github` to resolve handles.
/// Falls back to email prefix when no mapping exists.
pub fn query_contributor_stats(
    conn: &Connection,
    repos: &[String],
    since: &str,
) -> Result<Vec<ContributorStatsRow>> {
    if repos.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders: Vec<String> = (1..=repos.len()).map(|i| format!("?{i}")).collect();
    let sql = format!(
        "SELECT cs.repo,
                COALESCE(e.github, SUBSTR(cs.author_email, 1, INSTR(cs.author_email, '@') - 1)) as author,
                MIN(cs.committed_at) as week_start,
                SUM(cs.additions), SUM(cs.deletions), COUNT(*) as commits
         FROM commit_stats cs
         LEFT JOIN email_to_github e ON cs.author_email = e.email
         WHERE cs.repo IN ({}) AND cs.committed_at >= ?{}
         GROUP BY cs.repo, author
         ORDER BY commits DESC",
        placeholders.join(", "),
        repos.len() + 1,
    );
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> =
        repos.iter().map(|r| Box::new(r.clone()) as Box<dyn rusqlite::types::ToSql>).collect();
    params.push(Box::new(since.to_string()));
    let params_ref: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_ref.as_slice(), |row| {
        Ok(ContributorStatsRow {
            repo: row.get(0)?,
            author: row.get(1)?,
            week_start: row.get(2)?,
            additions: row.get(3)?,
            deletions: row.get(4)?,
            commits: row.get(5)?,
        })
    })?;
    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

/// Bulk upsert commit-level stats (from git log). Returns count of rows written.
pub fn upsert_commit_stats(conn: &Connection, stats: &[CommitStatsRow]) -> Result<usize> {
    let mut count = 0;
    let mut stmt = conn.prepare(
        "INSERT OR REPLACE INTO commit_stats (
            repo, sha, author_email, committed_at, additions, deletions, branch, synced_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
    )?;
    let now = Utc::now().to_rfc3339();
    for row in stats {
        stmt.execute(rusqlite::params![
            row.repo,
            row.sha,
            row.author_email,
            row.committed_at,
            row.additions,
            row.deletions,
            row.branch,
            now,
        ])?;
        count += 1;
    }
    Ok(count)
}

/// Query commit-level stats since `since` (ISO 8601 date) for the given repos.
pub fn query_commit_stats(
    conn: &Connection,
    repos: &[String],
    since: &str,
) -> Result<Vec<CommitStatsRow>> {
    if repos.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders: Vec<String> = (1..=repos.len()).map(|i| format!("?{i}")).collect();
    let sql = format!(
        "SELECT repo, sha, author_email, committed_at, additions, deletions, branch
         FROM commit_stats
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
        Ok(CommitStatsRow {
            repo: row.get(0)?,
            sha: row.get(1)?,
            author_email: row.get(2)?,
            committed_at: row.get(3)?,
            additions: row.get(4)?,
            deletions: row.get(5)?,
            branch: row.get(6)?,
        })
    })?;
    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

/// Cache a resolved email→GitHub handle mapping.
pub fn upsert_email_mapping(conn: &Connection, email: &str, github: &str) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO email_to_github (email, github, resolved_at)
         VALUES (?1, ?2, ?3)",
        rusqlite::params![email, github, Utc::now().to_rfc3339()],
    )?;
    Ok(())
}

/// Look up a cached email→GitHub handle mapping. Returns None if not cached.
pub fn query_email_mapping(conn: &Connection, email: &str) -> Result<Option<String>> {
    let mut stmt = conn.prepare(
        "SELECT github FROM email_to_github WHERE email = ?1",
    )?;
    let result = stmt
        .query_row(rusqlite::params![email], |row| row.get::<_, String>(0))
        .optional()?;
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
                project_target_date, project_priority, author,
                pr_additions, pr_deletions
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
            author: row.get(14)?,
            pr_additions: row.get(15)?,
            pr_deletions: row.get(16)?,
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

/// Check if the stored schema version matches the current version.
/// Returns true if the database needs to be reset (version mismatch).
fn check_schema_version(conn: &Connection) -> Result<bool> {
    let table_exists: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='schema_version'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .map(|count| count > 0)?;

    if !table_exists {
        return Ok(true);
    }

    let stored: Option<u32> = conn
        .query_row("SELECT version FROM schema_version LIMIT 1", [], |row| row.get(0))
        .optional()?;

    match stored {
        Some(v) if v == ceo_schema::SCHEMA_VERSION => Ok(false),
        _ => Ok(true),
    }
}

fn store_schema_version(conn: &Connection) -> Result<()> {
    conn.execute(
        "INSERT INTO schema_version (version) VALUES (?1)",
        rusqlite::params![ceo_schema::SCHEMA_VERSION],
    )?;
    Ok(())
}

/// Open (or create) the database at the given path and ensure schema exists.
pub fn open_db_at(path: &Path) -> Result<Connection> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| DbError::CreateDir { path: parent.to_path_buf(), source: e })?;
    }

    if path.exists() {
        let conn = Connection::open(path)?;
        let needs_reset = check_schema_version(&conn)?;
        if needs_reset {
            let old_version: u32 = conn
                .query_row("SELECT version FROM schema_version LIMIT 1", [], |row| row.get(0))
                .unwrap_or(0);
            drop(conn);
            eprintln!(
                "Database schema updated (v{} → v{}), clearing cache...",
                old_version,
                ceo_schema::SCHEMA_VERSION,
            );
            std::fs::remove_file(path)
                .map_err(|e| DbError::DeleteFailed { path: path.to_path_buf(), source: e })?;
        } else {
            return Ok(conn);
        }
    }

    let conn = Connection::open(path)?;
    create_schema(&conn)?;
    store_schema_version(&conn)?;
    Ok(conn)
}

/// Open the database at the default path.
pub fn open_db() -> Result<Connection> {
    open_db_at(&db_path())
}

/// Open an existing database at the given path. Returns NotFound if the file doesn't exist.
/// If the schema version mismatches, the database is deleted and recreated.
pub fn open_existing_db_at(path: &Path) -> Result<Connection> {
    if !path.exists() {
        return Err(DbError::NotFound(path.to_path_buf()));
    }

    let conn = Connection::open(path)?;
    let needs_reset = check_schema_version(&conn)?;
    if needs_reset {
        let old_version: u32 = conn
            .query_row("SELECT version FROM schema_version LIMIT 1", [], |row| row.get(0))
            .unwrap_or(0);
        drop(conn);
        eprintln!(
            "Database schema updated (v{} → v{}), clearing cache...",
            old_version,
            ceo_schema::SCHEMA_VERSION,
        );
        std::fs::remove_file(path)
            .map_err(|e| DbError::DeleteFailed { path: path.to_path_buf(), source: e })?;
        let conn = Connection::open(path)?;
        create_schema(&conn)?;
        store_schema_version(&conn)?;
        return Ok(conn);
    }

    Ok(conn)
}

/// Open an existing database. Returns NotFound if the file doesn't exist.
pub fn open_existing_db() -> Result<Connection> {
    open_existing_db_at(&db_path())
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
            author          TEXT,
            pr_additions    INTEGER,
            pr_deletions    INTEGER,
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

        CREATE TABLE IF NOT EXISTS commit_stats (
            repo         TEXT NOT NULL,
            sha          TEXT NOT NULL,
            author_email TEXT NOT NULL,
            committed_at TEXT NOT NULL,
            additions    INTEGER NOT NULL DEFAULT 0,
            deletions    INTEGER NOT NULL DEFAULT 0,
            branch       TEXT NOT NULL,
            synced_at    TEXT NOT NULL,
            PRIMARY KEY (repo, sha)
        );

        CREATE TABLE IF NOT EXISTS email_to_github (
            email       TEXT PRIMARY KEY,
            github      TEXT NOT NULL,
            resolved_at TEXT NOT NULL
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
        );

        CREATE TABLE IF NOT EXISTS schema_version (
            version INTEGER NOT NULL
        );"
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commit_stats_table_exists() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let conn = open_db_at(&db_path).unwrap();
        let _ = conn.prepare("SELECT count(*) FROM commit_stats").unwrap().query([]).unwrap();
        let _ = conn.prepare("SELECT count(*) FROM email_to_github").unwrap().query([]).unwrap();
    }

    #[test]
    fn upsert_and_query_commit_stats() {
        let dir = tempfile::tempdir().unwrap();
        let conn = open_db_at(&dir.path().join("test.db")).unwrap();

        let rows = vec![CommitStatsRow {
            repo: "org/repo".into(),
            sha: "abc123".into(),
            author_email: "alice@example.com".into(),
            committed_at: "2026-03-05".into(),
            additions: 100,
            deletions: 50,
            branch: "main".into(),
        }];
        let count = upsert_commit_stats(&conn, &rows).unwrap();
        assert_eq!(count, 1);

        let result = query_commit_stats(&conn, &["org/repo".into()], "2026-03-01").unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].author_email, "alice@example.com");
        assert_eq!(result[0].additions, 100);
    }

    #[test]
    fn query_contributor_stats_aggregates_from_commit_stats() {
        let dir = tempfile::tempdir().unwrap();
        let conn = open_db_at(&dir.path().join("test.db")).unwrap();

        // Store raw emails in commit_stats
        let rows = vec![
            CommitStatsRow {
                repo: "org/repo".into(),
                sha: "aaa".into(),
                author_email: "alice@example.com".into(),
                committed_at: "2026-03-05".into(),
                additions: 100,
                deletions: 50,
                branch: "main".into(),
            },
            CommitStatsRow {
                repo: "org/repo".into(),
                sha: "bbb".into(),
                author_email: "alice@example.com".into(),
                committed_at: "2026-03-06".into(),
                additions: 200,
                deletions: 80,
                branch: "feature".into(),
            },
            CommitStatsRow {
                repo: "org/repo".into(),
                sha: "ccc".into(),
                author_email: "bob@example.com".into(),
                committed_at: "2026-03-04".into(),
                additions: 50,
                deletions: 20,
                branch: "main".into(),
            },
        ];
        upsert_commit_stats(&conn, &rows).unwrap();

        // Add email→GitHub mappings
        upsert_email_mapping(&conn, "alice@example.com", "alice").unwrap();
        upsert_email_mapping(&conn, "bob@example.com", "bob").unwrap();

        let stats = query_contributor_stats(
            &conn,
            &["org/repo".into()],
            "2026-03-01",
        ).unwrap();

        assert_eq!(stats.len(), 2);
        // Resolved via email_to_github join
        let alice = stats.iter().find(|s| s.author == "alice").unwrap();
        assert_eq!(alice.additions, 300);
        assert_eq!(alice.deletions, 130);
        assert_eq!(alice.commits, 2);
        let bob = stats.iter().find(|s| s.author == "bob").unwrap();
        assert_eq!(bob.additions, 50);
        assert_eq!(bob.commits, 1);
    }

    #[test]
    fn query_contributor_stats_falls_back_to_email_without_mapping() {
        let dir = tempfile::tempdir().unwrap();
        let conn = open_db_at(&dir.path().join("test.db")).unwrap();

        let rows = vec![CommitStatsRow {
            repo: "org/repo".into(),
            sha: "aaa".into(),
            author_email: "unknown@example.com".into(),
            committed_at: "2026-03-05".into(),
            additions: 42,
            deletions: 0,
            branch: "main".into(),
        }];
        upsert_commit_stats(&conn, &rows).unwrap();
        // No email mapping — should fall back to email prefix
        let stats = query_contributor_stats(&conn, &["org/repo".into()], "2026-03-01").unwrap();
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].author, "unknown");
        assert_eq!(stats[0].additions, 42);
    }

    #[test]
    fn upsert_and_query_email_mapping() {
        let dir = tempfile::tempdir().unwrap();
        let conn = open_db_at(&dir.path().join("test.db")).unwrap();

        upsert_email_mapping(&conn, "alice@example.com", "alice").unwrap();
        let result = query_email_mapping(&conn, "alice@example.com").unwrap();
        assert_eq!(result, Some("alice".to_string()));

        let missing = query_email_mapping(&conn, "nobody@example.com").unwrap();
        assert_eq!(missing, None);
    }
}
