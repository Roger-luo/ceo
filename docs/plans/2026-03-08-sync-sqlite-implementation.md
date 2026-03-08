# SQLite Sync Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add a local SQLite database with `ceo sync` command that fetches GitHub issues, comments, and project board data, and migrate `ceo report` to read from the database instead of fetching live.

**Architecture:** New `src/db.rs` module handles all SQLite operations (open, schema creation, upserts, queries). New `src/sync.rs` orchestrates fetching from GitHub and writing to the database. Config gains an optional `[project]` section. Pipeline reads from DB instead of `GhRunner`. Error types extended with `DbError` and `SyncError`.

**Tech Stack:** rusqlite (bundled), dirs, chrono, serde_json, thiserror

---

### Task 1: Add rusqlite Dependency

**Files:**
- Modify: `Cargo.toml`

**Step 1: Add rusqlite to dependencies**

In `Cargo.toml`, add after the `thiserror` line:

```toml
rusqlite = { version = "0.33", features = ["bundled"] }
```

**Step 2: Verify it compiles**

Run: `cargo check`
Expected: Compiles successfully (bundled SQLite builds from source, may take a moment first time)

**Step 3: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "feat: add rusqlite dependency for SQLite sync"
```

---

### Task 2: Add DbError and SyncError Types

**Files:**
- Modify: `src/error.rs`
- Test: `tests/error_test.rs` (create)

**Step 1: Write the failing test**

Create `tests/error_test.rs`:

```rust
use ceo::error::{DbError, SyncError};
use std::path::PathBuf;

#[test]
fn db_error_not_found_displays_path_and_suggestion() {
    let err = DbError::NotFound(PathBuf::from("/tmp/ceo.db"));
    let msg = err.to_string();
    assert!(msg.contains("/tmp/ceo.db"), "should contain the path");
    assert!(msg.contains("ceo sync"), "should suggest running ceo sync");
}

#[test]
fn sync_error_from_db_error() {
    let db_err = DbError::NotFound(PathBuf::from("/tmp/ceo.db"));
    let sync_err: SyncError = db_err.into();
    assert!(sync_err.to_string().contains("ceo sync"));
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --test error_test`
Expected: FAIL — `DbError` and `SyncError` not found

**Step 3: Write the error types**

Add to `src/error.rs`, after the existing `PipelineError` enum:

```rust
#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("Database not found at {0}. Run `ceo sync` first.")]
    NotFound(std::path::PathBuf),
    #[error("Failed to create data directory {path}")]
    CreateDir { path: std::path::PathBuf, source: std::io::Error },
}

#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error(transparent)]
    Gh(#[from] GhError),
    #[error(transparent)]
    Db(#[from] DbError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}
```

Also add `DbError` variant to `PipelineError`:

```rust
#[derive(Debug, thiserror::Error)]
pub enum PipelineError {
    #[error(transparent)]
    Gh(#[from] GhError),
    #[error(transparent)]
    Agent(#[from] AgentError),
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error(transparent)]
    Db(#[from] DbError),
}
```

**Step 4: Run test to verify it passes**

Run: `cargo test --test error_test`
Expected: PASS

**Step 5: Run all existing tests to check nothing broke**

Run: `cargo test`
Expected: All tests pass

**Step 6: Commit**

```bash
git add src/error.rs tests/error_test.rs
git commit -m "feat: add DbError and SyncError types"
```

---

### Task 3: Create Database Module — Schema and Open

**Files:**
- Create: `src/db.rs`
- Modify: `src/lib.rs` (add `pub mod db;`)
- Test: `tests/db_test.rs` (create)

**Step 1: Write the failing test**

Create `tests/db_test.rs`:

```rust
use ceo::db;
use std::path::PathBuf;

#[test]
fn open_db_creates_tables() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    let conn = db::open_db_at(&path).unwrap();

    // Verify tables exist by querying sqlite_master
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
    // No error on second open
}

#[test]
fn db_path_returns_platform_path() {
    let path = db::db_path();
    assert!(path.ends_with("ceo/ceo.db") || path.ends_with("ceo\\ceo.db"));
}
```

**Step 2: Add tempfile dev-dependency**

In `Cargo.toml`, add at the end:

```toml
[dev-dependencies]
tempfile = "3"
```

**Step 3: Run test to verify it fails**

Run: `cargo test --test db_test`
Expected: FAIL — `db` module not found

**Step 4: Write the db module with schema creation**

Create `src/db.rs`:

```rust
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
```

Add to `src/lib.rs`:

```rust
pub mod db;
```

**Step 5: Run test to verify it passes**

Run: `cargo test --test db_test`
Expected: PASS

**Step 6: Commit**

```bash
git add src/db.rs src/lib.rs Cargo.toml tests/db_test.rs
git commit -m "feat: add db module with schema creation"
```

---

### Task 4: Database Upsert Functions

**Files:**
- Modify: `src/db.rs`
- Modify: `tests/db_test.rs`

**Step 1: Write the failing tests**

Add to `tests/db_test.rs`:

```rust
use ceo::db::{IssueRow, CommentRow};

#[test]
fn upsert_issues_inserts_and_replaces() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    let conn = db::open_db_at(&path).unwrap();

    let issues = vec![
        IssueRow {
            repo: "org/repo".to_string(),
            number: 1,
            title: "First".to_string(),
            body: Some("body".to_string()),
            state: Some("open".to_string()),
            labels: "[]".to_string(),
            assignees: "[]".to_string(),
            created_at: "2026-03-01T00:00:00Z".to_string(),
            updated_at: "2026-03-05T00:00:00Z".to_string(),
            project_status: None,
            project_start_date: None,
            project_target_date: None,
            project_priority: None,
        },
    ];

    let count = db::upsert_issues(&conn, &issues).unwrap();
    assert_eq!(count, 1);

    // Upsert again with updated title
    let updated = vec![IssueRow { title: "Updated".to_string(), ..issues[0].clone() }];
    let count = db::upsert_issues(&conn, &updated).unwrap();
    assert_eq!(count, 1);

    // Verify the title was updated
    let title: String = conn
        .query_row("SELECT title FROM issues WHERE number = 1", [], |row| row.get(0))
        .unwrap();
    assert_eq!(title, "Updated");
}

#[test]
fn upsert_comments_inserts_and_replaces() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    let conn = db::open_db_at(&path).unwrap();

    // Insert parent issue first
    let issues = vec![IssueRow {
        repo: "org/repo".to_string(),
        number: 1,
        title: "Issue".to_string(),
        body: None,
        state: Some("open".to_string()),
        labels: "[]".to_string(),
        assignees: "[]".to_string(),
        created_at: "2026-03-01T00:00:00Z".to_string(),
        updated_at: "2026-03-05T00:00:00Z".to_string(),
        project_status: None,
        project_start_date: None,
        project_target_date: None,
        project_priority: None,
    }];
    db::upsert_issues(&conn, &issues).unwrap();

    let comments = vec![CommentRow {
        repo: "org/repo".to_string(),
        issue_number: 1,
        comment_id: 100,
        author: "alice".to_string(),
        body: "Looks good".to_string(),
        created_at: "2026-03-02T00:00:00Z".to_string(),
    }];

    let count = db::upsert_comments(&conn, &comments).unwrap();
    assert_eq!(count, 1);
}

#[test]
fn log_sync_records_entry() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    let conn = db::open_db_at(&path).unwrap();

    db::log_sync(&conn, "org/repo", 10, 25).unwrap();

    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM sync_log", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 1);
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --test db_test`
Expected: FAIL — `IssueRow`, `CommentRow`, `upsert_issues`, etc. not found

**Step 3: Implement row types and upsert functions**

Add to `src/db.rs`:

```rust
use chrono::Utc;

/// One row in the `issues` table.
#[derive(Debug, Clone)]
pub struct IssueRow {
    pub repo: String,
    pub number: u64,
    pub title: String,
    pub body: Option<String>,
    pub state: Option<String>,
    pub labels: String,       // JSON array string
    pub assignees: String,    // JSON array string
    pub created_at: String,   // ISO 8601
    pub updated_at: String,   // ISO 8601
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
    pub created_at: String, // ISO 8601
}

/// Bulk upsert issues. Returns count of rows written.
pub fn upsert_issues(conn: &Connection, issues: &[IssueRow]) -> Result<usize> {
    let now = Utc::now().to_rfc3339();
    let mut stmt = conn.prepare(
        "INSERT OR REPLACE INTO issues
            (repo, number, title, body, state, labels, assignees,
             created_at, updated_at, project_status, project_start_date,
             project_target_date, project_priority, synced_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)"
    )?;

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
    }
    Ok(issues.len())
}

/// Bulk upsert comments. Returns count of rows written.
pub fn upsert_comments(conn: &Connection, comments: &[CommentRow]) -> Result<usize> {
    let now = Utc::now().to_rfc3339();
    let mut stmt = conn.prepare(
        "INSERT OR REPLACE INTO comments
            (repo, issue_number, comment_id, author, body, created_at, synced_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)"
    )?;

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
    }
    Ok(comments.len())
}

/// Record a sync log entry.
pub fn log_sync(conn: &Connection, repo: &str, issues_count: usize, comments_count: usize) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO sync_log (repo, synced_at, issues_synced, comments_synced)
         VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![repo, now, issues_count as i64, comments_count as i64],
    )?;
    Ok(())
}
```

**Step 4: Run test to verify it passes**

Run: `cargo test --test db_test`
Expected: PASS

**Step 5: Commit**

```bash
git add src/db.rs tests/db_test.rs
git commit -m "feat: add issue/comment upsert and sync logging"
```

---

### Task 5: Database Query Functions (for Pipeline)

**Files:**
- Modify: `src/db.rs`
- Modify: `tests/db_test.rs`

**Step 1: Write the failing tests**

Add to `tests/db_test.rs`:

```rust
#[test]
fn query_recent_issues_filters_by_date_and_repo() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    let conn = db::open_db_at(&path).unwrap();

    let issues = vec![
        IssueRow {
            repo: "org/repo".to_string(),
            number: 1,
            title: "Recent".to_string(),
            body: None,
            state: Some("open".to_string()),
            labels: r#"["bug"]"#.to_string(),
            assignees: r#"["alice"]"#.to_string(),
            created_at: "2026-03-01T00:00:00Z".to_string(),
            updated_at: "2026-03-07T00:00:00Z".to_string(),
            project_status: None,
            project_start_date: None,
            project_target_date: None,
            project_priority: None,
        },
        IssueRow {
            repo: "org/repo".to_string(),
            number: 2,
            title: "Old".to_string(),
            body: None,
            state: Some("open".to_string()),
            labels: "[]".to_string(),
            assignees: "[]".to_string(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-01-15T00:00:00Z".to_string(),
            project_status: None,
            project_start_date: None,
            project_target_date: None,
            project_priority: None,
        },
        IssueRow {
            repo: "org/other".to_string(),
            number: 3,
            title: "Different repo".to_string(),
            body: None,
            state: Some("open".to_string()),
            labels: "[]".to_string(),
            assignees: "[]".to_string(),
            created_at: "2026-03-01T00:00:00Z".to_string(),
            updated_at: "2026-03-07T00:00:00Z".to_string(),
            project_status: None,
            project_start_date: None,
            project_target_date: None,
            project_priority: None,
        },
    ];
    db::upsert_issues(&conn, &issues).unwrap();

    let results = db::query_recent_issues(&conn, &["org/repo".to_string()], "2026-03-01T00:00:00Z").unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].title, "Recent");
}

#[test]
fn query_comments_for_issues() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    let conn = db::open_db_at(&path).unwrap();

    // Insert issue
    let issues = vec![IssueRow {
        repo: "org/repo".to_string(),
        number: 1,
        title: "Issue".to_string(),
        body: None,
        state: Some("open".to_string()),
        labels: "[]".to_string(),
        assignees: "[]".to_string(),
        created_at: "2026-03-01T00:00:00Z".to_string(),
        updated_at: "2026-03-05T00:00:00Z".to_string(),
        project_status: None,
        project_start_date: None,
        project_target_date: None,
        project_priority: None,
    }];
    db::upsert_issues(&conn, &issues).unwrap();

    // Insert comments
    let comments = vec![
        CommentRow {
            repo: "org/repo".to_string(),
            issue_number: 1,
            comment_id: 100,
            author: "alice".to_string(),
            body: "First comment".to_string(),
            created_at: "2026-03-02T00:00:00Z".to_string(),
        },
        CommentRow {
            repo: "org/repo".to_string(),
            issue_number: 1,
            comment_id: 101,
            author: "bob".to_string(),
            body: "Second comment".to_string(),
            created_at: "2026-03-03T00:00:00Z".to_string(),
        },
    ];
    db::upsert_comments(&conn, &comments).unwrap();

    let results = db::query_comments_for_issues(&conn, "org/repo", &[1]).unwrap();
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].author, "alice");
    assert_eq!(results[1].author, "bob");
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --test db_test`
Expected: FAIL — `query_recent_issues` and `query_comments_for_issues` not found

**Step 3: Implement query functions**

Add to `src/db.rs`:

```rust
/// Query issues updated since `since` (ISO 8601 string) for the given repos.
pub fn query_recent_issues(conn: &Connection, repos: &[String], since: &str) -> Result<Vec<IssueRow>> {
    // Build placeholders for IN clause
    let placeholders: Vec<String> = (1..=repos.len()).map(|i| format!("?{}", i)).collect();
    let sql = format!(
        "SELECT repo, number, title, body, state, labels, assignees,
                created_at, updated_at, project_status, project_start_date,
                project_target_date, project_priority
         FROM issues
         WHERE repo IN ({}) AND updated_at >= ?{}
         ORDER BY repo, number",
        placeholders.join(", "),
        repos.len() + 1
    );

    let mut stmt = conn.prepare(&sql)?;

    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = repos
        .iter()
        .map(|r| Box::new(r.clone()) as Box<dyn rusqlite::types::ToSql>)
        .collect();
    params.push(Box::new(since.to_string()));

    let params_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();

    let rows = stmt.query_map(params_refs.as_slice(), |row| {
        Ok(IssueRow {
            repo: row.get(0)?,
            number: row.get::<_, i64>(1)? as u64,
            title: row.get(2)?,
            body: row.get(3)?,
            state: row.get(4)?,
            labels: row.get::<_, String>(5)?,
            assignees: row.get::<_, String>(6)?,
            created_at: row.get(7)?,
            updated_at: row.get(8)?,
            project_status: row.get(9)?,
            project_start_date: row.get(10)?,
            project_target_date: row.get(11)?,
            project_priority: row.get(12)?,
        })
    })?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row?);
    }
    Ok(results)
}

/// Query comments for specific issues in a repo.
pub fn query_comments_for_issues(conn: &Connection, repo: &str, issue_numbers: &[u64]) -> Result<Vec<CommentRow>> {
    let placeholders: Vec<String> = (1..=issue_numbers.len()).map(|i| format!("?{}", i + 1)).collect();
    let sql = format!(
        "SELECT repo, issue_number, comment_id, author, body, created_at
         FROM comments
         WHERE repo = ?1 AND issue_number IN ({})
         ORDER BY issue_number, created_at",
        placeholders.join(", ")
    );

    let mut stmt = conn.prepare(&sql)?;

    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    params.push(Box::new(repo.to_string()));
    for &n in issue_numbers {
        params.push(Box::new(n as i64));
    }
    let params_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();

    let rows = stmt.query_map(params_refs.as_slice(), |row| {
        Ok(CommentRow {
            repo: row.get(0)?,
            issue_number: row.get::<_, i64>(1)? as u64,
            comment_id: row.get::<_, i64>(2)? as u64,
            author: row.get(3)?,
            body: row.get(4)?,
            created_at: row.get(5)?,
        })
    })?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row?);
    }
    Ok(results)
}
```

**Step 4: Run test to verify it passes**

Run: `cargo test --test db_test`
Expected: PASS

**Step 5: Commit**

```bash
git add src/db.rs tests/db_test.rs
git commit -m "feat: add query functions for issues and comments"
```

---

### Task 6: Add ProjectConfig to Config

**Files:**
- Modify: `src/config.rs`
- Modify: `tests/config_test.rs`

**Step 1: Write the failing tests**

Add to `tests/config_test.rs`:

```rust
#[test]
fn parse_config_with_project() {
    let config = Config::load_from_str(r#"
[project]
org = "acme-corp"
number = 3

[[repos]]
name = "org/repo"
"#).unwrap();

    let project = config.project.unwrap();
    assert_eq!(project.org, "acme-corp");
    assert_eq!(project.number, 3);
}

#[test]
fn config_without_project_section() {
    let config = Config::load_from_str(r#"
[[repos]]
name = "org/repo"
"#).unwrap();

    assert!(config.project.is_none());
}

#[test]
fn config_get_set_project_fields() {
    let mut config = Config::load_from_str(r#"
[[repos]]
name = "org/repo"
"#).unwrap();

    config.set_field("project.org", "acme-corp").unwrap();
    config.set_field("project.number", "3").unwrap();

    assert_eq!(config.get_field("project.org").unwrap(), "acme-corp");
    assert_eq!(config.get_field("project.number").unwrap(), "3");
    assert_eq!(config.project.unwrap().number, 3);
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --test config_test`
Expected: FAIL — `project` field not found on `Config`

**Step 3: Implement ProjectConfig**

Add to `src/config.rs`, after `TeamMember`:

```rust
#[derive(Debug, Deserialize, Serialize)]
pub struct ProjectConfig {
    pub org: String,
    pub number: u64,
}
```

Add field to `Config`:

```rust
#[derive(Debug, Deserialize, Serialize)]
pub struct Config {
    #[serde(default)]
    pub agent: AgentConfig,
    pub repos: Vec<RepoConfig>,
    #[serde(default)]
    pub team: Vec<TeamMember>,
    #[serde(default)]
    pub project: Option<ProjectConfig>,
}
```

Add to `get_field` match:

```rust
"project.org" => self.project.as_ref()
    .map(|p| p.org.clone())
    .ok_or_else(|| ConfigError::UnknownKey("project.org (not configured)".to_string())),
"project.number" => self.project.as_ref()
    .map(|p| p.number.to_string())
    .ok_or_else(|| ConfigError::UnknownKey("project.number (not configured)".to_string())),
```

Add to `set_field` match:

```rust
"project.org" => {
    if let Some(ref mut p) = self.project {
        p.org = value.to_string();
    } else {
        self.project = Some(ProjectConfig {
            org: value.to_string(),
            number: 0,
        });
    }
}
"project.number" => {
    let n: u64 = value.parse().map_err(|_| ConfigError::InvalidValue {
        key: key.to_string(),
        message: format!("expected integer, got: {value}"),
    })?;
    if let Some(ref mut p) = self.project {
        p.number = n;
    } else {
        self.project = Some(ProjectConfig {
            org: String::new(),
            number: n,
        });
    }
}
```

**Step 4: Run test to verify it passes**

Run: `cargo test --test config_test`
Expected: PASS

**Step 5: Run all tests**

Run: `cargo test`
Expected: All pass

**Step 6: Commit**

```bash
git add src/config.rs tests/config_test.rs
git commit -m "feat: add ProjectConfig to config"
```

---

### Task 7: Add Project Section to Config Wizard

**Files:**
- Modify: `src/main.rs`

**Step 1: Add wizard section**

In `cmd_config_wizard()`, after the Team section and before `config.save()`, add:

```rust
    // Project
    eprintln!("\n--- Project ---");
    eprintln!("  GitHub Projects board for tracking issue status, dates, priority.");
    if let Some(ref project) = config.project {
        eprintln!("  Current: org={}, number={}", project.org, project.number);
    } else {
        eprintln!("  Not configured.");
    }

    let org_default = config.project.as_ref()
        .map(|p| p.org.as_str())
        .unwrap_or("");
    let line = rl.readline(&format!("Project org [{}] (- to clear): ", if org_default.is_empty() { "none" } else { org_default }))?;
    let line = line.trim();
    if line == "-" {
        config.project = None;
    } else if !line.is_empty() {
        let org = line.to_string();
        let num_default = config.project.as_ref().map(|p| p.number).unwrap_or(0);
        let num_line = rl.readline(&format!("Project number [{}]: ", num_default))?;
        let num_line = num_line.trim();
        let number = if num_line.is_empty() {
            num_default
        } else {
            num_line.parse().context("Invalid number for project number")?
        };
        config.project = Some(ceo::config::ProjectConfig { org, number });
    }
```

**Step 2: Verify it compiles**

Run: `cargo check`
Expected: Compiles

**Step 3: Commit**

```bash
git add src/main.rs
git commit -m "feat: add project section to config wizard"
```

---

### Task 8: Create Sync Module

**Files:**
- Create: `src/sync.rs`
- Modify: `src/lib.rs` (add `pub mod sync;`)
- Test: `tests/sync_test.rs` (create)

**Step 1: Write the failing test**

Create `tests/sync_test.rs`:

```rust
use ceo::config::{Config, ProjectConfig};
use ceo::db;
use ceo::error::GhError;
use ceo::gh::GhRunner;
use ceo::sync::{run_sync, SyncResult};

struct MockSyncGh;

impl GhRunner for MockSyncGh {
    fn run_gh(&self, args: &[&str]) -> Result<String, GhError> {
        // gh issue list
        if args.iter().any(|a| *a == "list") && args.iter().any(|a| *a == "issue") {
            return Ok(r#"[{
                "number": 1,
                "title": "Fix auth",
                "labels": [{"name": "bug"}],
                "assignees": [{"login": "alice"}],
                "updatedAt": "2026-03-05T10:00:00Z",
                "createdAt": "2026-03-01T10:00:00Z",
                "state": "OPEN",
                "body": "Auth is broken"
            }]"#.to_string());
        }
        // gh issue view (comments)
        if args.iter().any(|a| *a == "view") {
            return Ok(r#"{
                "body": "Auth is broken",
                "comments": [{
                    "author": {"login": "bob"},
                    "body": "I can reproduce this",
                    "createdAt": "2026-03-02T10:00:00Z"
                }]
            }"#.to_string());
        }
        // gh project item-list
        if args.iter().any(|a| *a == "item-list") {
            return Ok(r#"{"items": [{
                "content": {"number": 1, "repository": "org/repo", "type": "Issue"},
                "status": "In Progress",
                "priority": "High"
            }]}"#.to_string());
        }
        Ok("[]".to_string())
    }
}

#[test]
fn sync_fetches_and_stores_issues() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let conn = db::open_db_at(&db_path).unwrap();

    let config: Config = toml::from_str(r#"
[[repos]]
name = "org/repo"
"#).unwrap();

    let result = run_sync(&config, &MockSyncGh, &conn).unwrap();
    assert_eq!(result.repos.len(), 1);
    assert_eq!(result.repos[0].issues_synced, 1);
    assert!(result.repos[0].comments_synced >= 1);

    // Verify data in database
    let issues = db::query_recent_issues(&conn, &["org/repo".to_string()], "2026-01-01T00:00:00Z").unwrap();
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].title, "Fix auth");

    let comments = db::query_comments_for_issues(&conn, "org/repo", &[1]).unwrap();
    assert_eq!(comments.len(), 1);
    assert_eq!(comments[0].author, "bob");
}

#[test]
fn sync_with_project_config_merges_project_fields() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let conn = db::open_db_at(&db_path).unwrap();

    let mut config: Config = toml::from_str(r#"
[[repos]]
name = "org/repo"
"#).unwrap();
    config.project = Some(ProjectConfig {
        org: "org".to_string(),
        number: 1,
    });

    let result = run_sync(&config, &MockSyncGh, &conn).unwrap();
    assert_eq!(result.repos[0].issues_synced, 1);

    let issues = db::query_recent_issues(&conn, &["org/repo".to_string()], "2026-01-01T00:00:00Z").unwrap();
    assert_eq!(issues[0].project_status.as_deref(), Some("In Progress"));
    assert_eq!(issues[0].project_priority.as_deref(), Some("High"));
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --test sync_test`
Expected: FAIL — `sync` module not found

**Step 3: Implement sync module**

Create `src/sync.rs`:

```rust
use std::collections::HashMap;

use log::{debug, info};
use serde::Deserialize;

use crate::config::Config;
use crate::db::{self, CommentRow, IssueRow};
use crate::error::SyncError;
use crate::gh::{self, GhRunner};

type Result<T> = std::result::Result<T, SyncError>;

pub struct SyncResult {
    pub repos: Vec<RepoSyncResult>,
}

pub struct RepoSyncResult {
    pub name: String,
    pub issues_synced: usize,
    pub comments_synced: usize,
}

/// Fetch all issues from a repo using `gh issue list` with body included.
/// This differs from gh::fetch_issues by also requesting state and body fields.
fn fetch_issues_for_sync(gh_runner: &dyn GhRunner, repo: &str) -> std::result::Result<Vec<SyncIssue>, crate::error::GhError> {
    let json = gh_runner.run_gh(&[
        "issue", "list",
        "--repo", repo,
        "--state", "open",
        "--json", "number,title,labels,assignees,updatedAt,createdAt,state,body",
        "--limit", "500",
    ])?;
    let issues: Vec<GhSyncIssue> = serde_json::from_str(&json)?;
    Ok(issues.into_iter().map(|i| SyncIssue {
        number: i.number,
        title: i.title,
        body: i.body,
        state: i.state,
        labels: i.labels.into_iter().map(|l| l.name).collect(),
        assignees: i.assignees.into_iter().map(|a| a.login).collect(),
        created_at: i.created_at,
        updated_at: i.updated_at,
    }).collect())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GhSyncIssue {
    number: u64,
    title: String,
    body: String,
    state: String,
    labels: Vec<GhLabel>,
    assignees: Vec<GhUser>,
    created_at: String,
    updated_at: String,
}

#[derive(Deserialize)]
struct GhLabel { name: String }

#[derive(Deserialize)]
struct GhUser { login: String }

struct SyncIssue {
    number: u64,
    title: String,
    body: String,
    state: String,
    labels: Vec<String>,
    assignees: Vec<String>,
    created_at: String,
    updated_at: String,
}

// --- Project board parsing ---

#[derive(Debug)]
struct ProjectItemFields {
    status: Option<String>,
    start_date: Option<String>,
    target_date: Option<String>,
    priority: Option<String>,
}

fn fetch_project_items(
    gh_runner: &dyn GhRunner,
    org: &str,
    number: u64,
) -> std::result::Result<HashMap<(String, u64), ProjectItemFields>, crate::error::GhError> {
    let json = gh_runner.run_gh(&[
        "project", "item-list",
        &number.to_string(),
        "--owner", org,
        "--format", "json",
        "--limit", "1000",
    ])?;

    let parsed: serde_json::Value = serde_json::from_str(&json)
        .map_err(|e| crate::error::GhError::Json(e))?;

    let mut map = HashMap::new();

    if let Some(items) = parsed.get("items").and_then(|v| v.as_array()) {
        for item in items {
            let content = match item.get("content") {
                Some(c) => c,
                None => continue,
            };

            // Skip non-issues (DraftIssue, PullRequest)
            let item_type = content.get("type").and_then(|v| v.as_str()).unwrap_or("");
            if item_type != "Issue" {
                continue;
            }

            let number = match content.get("number").and_then(|v| v.as_u64()) {
                Some(n) => n,
                None => continue,
            };
            let repo = match content.get("repository").and_then(|v| v.as_str()) {
                Some(r) => r.to_string(),
                None => continue,
            };

            let fields = ProjectItemFields {
                status: get_field_ci(item, &["status", "Status"]),
                start_date: get_field_ci(item, &["startDate", "start_date", "Start Date", "Start date"]),
                target_date: get_field_ci(item, &["targetDate", "target_date", "Target Date", "Target date"]),
                priority: get_field_ci(item, &["priority", "Priority"]),
            };

            map.insert((repo, number), fields);
        }
    }

    Ok(map)
}

/// Try multiple field name variations to find a string value.
fn get_field_ci(value: &serde_json::Value, names: &[&str]) -> Option<String> {
    for name in names {
        if let Some(v) = value.get(name).and_then(|v| v.as_str()) {
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    None
}

/// Run the full sync: fetch issues, comments, and optionally project data,
/// then upsert everything into the database.
pub fn run_sync(
    config: &Config,
    gh_runner: &dyn GhRunner,
    conn: &rusqlite::Connection,
) -> Result<SyncResult> {
    // Fetch project board data if configured
    let project_map = if let Some(ref project) = config.project {
        info!("Fetching project board: {}/{}", project.org, project.number);
        match fetch_project_items(gh_runner, &project.org, project.number) {
            Ok(map) => {
                debug!("Project board: {} items", map.len());
                map
            }
            Err(e) => {
                debug!("Failed to fetch project board: {e}");
                HashMap::new()
            }
        }
    } else {
        HashMap::new()
    };

    let mut repo_results = Vec::new();

    for repo_config in &config.repos {
        let repo = &repo_config.name;
        info!("Syncing {repo}...");

        // 1. Fetch issues
        let sync_issues = fetch_issues_for_sync(gh_runner, repo)?;
        debug!("{}: {} issues fetched", repo, sync_issues.len());

        // 2. Fetch comments for each issue
        let mut all_comments = Vec::new();
        for issue in &sync_issues {
            match gh::fetch_issue_detail(gh_runner, repo, issue.number) {
                Ok(detail) => {
                    for (i, comment) in detail.comments.iter().enumerate() {
                        all_comments.push(CommentRow {
                            repo: repo.clone(),
                            issue_number: issue.number,
                            // Use index as fallback comment_id since gh issue view doesn't return IDs
                            comment_id: i as u64,
                            author: comment.author.clone(),
                            body: comment.body.clone(),
                            created_at: comment.created_at.to_rfc3339(),
                        });
                    }
                }
                Err(e) => {
                    debug!("Failed to fetch comments for #{}: {e}", issue.number);
                }
            }
        }
        debug!("{}: {} comments fetched", repo, all_comments.len());

        // 3. Build IssueRows, merging project data
        let issue_rows: Vec<IssueRow> = sync_issues.iter().map(|issue| {
            let project_fields = project_map.get(&(repo.clone(), issue.number));
            IssueRow {
                repo: repo.clone(),
                number: issue.number,
                title: issue.title.clone(),
                body: Some(issue.body.clone()),
                state: Some(issue.state.clone()),
                labels: serde_json::to_string(&issue.labels).unwrap_or_else(|_| "[]".to_string()),
                assignees: serde_json::to_string(&issue.assignees).unwrap_or_else(|_| "[]".to_string()),
                created_at: issue.created_at.clone(),
                updated_at: issue.updated_at.clone(),
                project_status: project_fields.and_then(|f| f.status.clone()),
                project_start_date: project_fields.and_then(|f| f.start_date.clone()),
                project_target_date: project_fields.and_then(|f| f.target_date.clone()),
                project_priority: project_fields.and_then(|f| f.priority.clone()),
            }
        }).collect();

        // 4. Upsert in a transaction
        let tx = conn.unchecked_transaction()
            .map_err(crate::error::DbError::Sqlite)?;
        let issues_synced = db::upsert_issues(&tx, &issue_rows)?;
        let comments_synced = db::upsert_comments(&tx, &all_comments)?;
        db::log_sync(&tx, repo, issues_synced, comments_synced)?;
        tx.commit().map_err(crate::error::DbError::Sqlite)?;

        repo_results.push(RepoSyncResult {
            name: repo.clone(),
            issues_synced,
            comments_synced,
        });
    }

    Ok(SyncResult { repos: repo_results })
}
```

Add to `src/lib.rs`:

```rust
pub mod sync;
```

**Step 4: Run test to verify it passes**

Run: `cargo test --test sync_test`
Expected: PASS

**Step 5: Run all tests**

Run: `cargo test`
Expected: All pass

**Step 6: Commit**

```bash
git add src/sync.rs src/lib.rs tests/sync_test.rs
git commit -m "feat: add sync module with GitHub fetch and DB upsert"
```

---

### Task 9: Add `ceo sync` CLI Command

**Files:**
- Modify: `src/main.rs`

**Step 1: Add Sync variant to Commands enum**

```rust
#[derive(Subcommand)]
enum Commands {
    /// Generate a weekly report (prints markdown to stdout)
    Report {
        #[arg(long, default_value = "7")]
        days: i64,
    },
    /// Launch interactive TUI mode
    Interactive,
    /// Sync GitHub data to local database
    Sync,
    /// Configure CEO CLI (interactive wizard or set/get/show)
    Config {
        #[command(subcommand)]
        action: Option<ConfigAction>,
    },
    #[command(hide = true)]
    Init,
}
```

**Step 2: Add match arm and handler**

In `main()`, add:

```rust
Commands::Sync => cmd_sync(),
```

Add the handler function:

```rust
fn cmd_sync() -> Result<()> {
    let config = ceo::config::Config::load()?;
    let gh_runner = ceo::gh::RealGhRunner;
    let db_path = ceo::db::db_path();
    let conn = ceo::db::open_db_at(&db_path)?;

    eprintln!("Syncing to {}...", db_path.display());
    let result = ceo::sync::run_sync(&config, &gh_runner, &conn)?;

    for repo in &result.repos {
        eprintln!("  {}: {} issues, {} comments", repo.name, repo.issues_synced, repo.comments_synced);
    }
    eprintln!("Sync complete.");
    Ok(())
}
```

**Step 3: Verify it compiles**

Run: `cargo check`
Expected: Compiles

**Step 4: Commit**

```bash
git add src/main.rs
git commit -m "feat: add ceo sync CLI command"
```

---

### Task 10: Migrate Pipeline to Read from Database

**Files:**
- Modify: `src/pipeline.rs`
- Modify: `tests/pipeline_test.rs`

This is the key refactor: `run_pipeline` stops using `GhRunner` and reads from the database instead.

**Step 1: Write the failing test**

Replace `tests/pipeline_test.rs` with:

```rust
use ceo::agent::Agent;
use ceo::config::Config;
use ceo::db;
use ceo::error::AgentError;
use ceo::pipeline::run_pipeline;
use ceo::prompt::Prompt;

struct MockAgent;

impl Agent for MockAgent {
    fn invoke(&self, _prompt: &dyn Prompt) -> Result<String, AgentError> {
        Ok("Mock agent summary.".to_string())
    }
}

#[test]
fn pipeline_reads_from_database() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let conn = db::open_db_at(&db_path).unwrap();

    // Seed the database
    let issues = vec![db::IssueRow {
        repo: "org/frontend".to_string(),
        number: 1,
        title: "Implement auth".to_string(),
        body: Some("Auth implementation needed.".to_string()),
        state: Some("open".to_string()),
        labels: r#"["feature"]"#.to_string(),
        assignees: r#"["alice"]"#.to_string(),
        created_at: "2026-03-01T10:00:00Z".to_string(),
        updated_at: "2026-03-05T10:00:00Z".to_string(),
        project_status: None,
        project_start_date: None,
        project_target_date: None,
        project_priority: None,
    }];
    db::upsert_issues(&conn, &issues).unwrap();

    let comments = vec![db::CommentRow {
        repo: "org/frontend".to_string(),
        issue_number: 1,
        comment_id: 0,
        author: "bob".to_string(),
        body: "I can review this.".to_string(),
        created_at: "2026-03-02T10:00:00Z".to_string(),
    }];
    db::upsert_comments(&conn, &comments).unwrap();

    let config: Config = toml::from_str(r#"
        [[repos]]
        name = "org/frontend"
        labels_required = ["priority"]

        [[team]]
        github = "alice"
        name = "Alice Smith"
        role = "Lead"
    "#).unwrap();

    let report = run_pipeline(&config, &conn, &MockAgent, 7).unwrap();
    assert_eq!(report.repos.len(), 1);
    assert_eq!(report.repos[0].name, "org/frontend");
    assert!(report.repos[0].progress.contains("Mock agent summary"));
    assert!(!report.repos[0].flagged_issues.is_empty());
    assert_eq!(report.team_stats.len(), 1);
    assert_eq!(report.team_stats[0].active, 1);
}

#[test]
fn pipeline_handles_empty_database() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let conn = db::open_db_at(&db_path).unwrap();

    let config: Config = toml::from_str(r#"
        [[repos]]
        name = "org/empty"
    "#).unwrap();

    let report = run_pipeline(&config, &conn, &MockAgent, 7).unwrap();
    assert_eq!(report.repos.len(), 1);
    assert!(report.repos[0].progress.contains("No recent activity"));
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --test pipeline_test`
Expected: FAIL — `run_pipeline` signature doesn't match (expects `GhRunner`, now gets `Connection`)

**Step 3: Rewrite pipeline to read from database**

Replace `src/pipeline.rs`:

```rust
use chrono::{Duration, Utc};
use log::{debug, info};

use crate::agent::Agent;
use crate::config::Config;
use crate::db::{self, IssueRow};
use crate::error::PipelineError;
use crate::github::Issue;
use crate::prompt::{IssueSummaryPrompt, IssueTriagePrompt, WeeklySummaryPrompt};
use crate::report::{FlaggedIssue, Report, RepoSection, TeamStats};

type Result<T> = std::result::Result<T, PipelineError>;

pub fn run_pipeline(
    config: &Config,
    conn: &rusqlite::Connection,
    agent: &dyn Agent,
    days: i64,
) -> Result<Report> {
    let cutoff = (Utc::now() - Duration::days(days)).to_rfc3339();
    let mut repo_sections = Vec::new();
    let mut all_recent_issues: Vec<Issue> = Vec::new();

    for repo_config in &config.repos {
        let repo = &repo_config.name;
        eprintln!("Loading issues from database for {}...", repo);
        info!("Processing repo: {}", repo);

        let issue_rows = db::query_recent_issues(conn, &[repo.clone()], &cutoff)?;
        debug!("Found {} recent issues (last {} days)", issue_rows.len(), days);
        eprintln!("  Found {} recent issues (last {} days)", issue_rows.len(), days);

        // Convert IssueRows to Issue structs for compatibility with filter/report
        let issues: Vec<Issue> = issue_rows.iter().map(|row| row_to_issue(row)).collect();
        let issue_numbers: Vec<u64> = issue_rows.iter().map(|r| r.number).collect();

        // Fetch all comments for these issues in one query
        let all_comments = db::query_comments_for_issues(conn, repo, &issue_numbers)?;

        for issue in &issues {
            all_recent_issues.push(issue.clone());
        }

        // Summarize each issue
        let mut per_issue_summaries = Vec::new();
        for (i, row) in issue_rows.iter().enumerate() {
            eprintln!("  [{}/{}] Summarizing #{} {}...", i + 1, issue_rows.len(), row.number, row.title);

            let body = row.body.clone().unwrap_or_default();
            let comments_text: String = all_comments.iter()
                .filter(|c| c.issue_number == row.number)
                .map(|c| format!("{}: {}", c.author, c.body))
                .collect::<Vec<_>>()
                .join("\n");

            let labels: Vec<String> = serde_json::from_str(&row.labels).unwrap_or_default();
            let assignees: Vec<String> = serde_json::from_str(&row.assignees).unwrap_or_default();

            let prompt = IssueSummaryPrompt {
                repo: repo.clone(),
                number: row.number,
                title: row.title.clone(),
                labels: labels.join(", "),
                assignees: assignees.join(", "),
                body,
                comments: comments_text,
            };

            let summary = match agent.invoke(&prompt) {
                Ok(s) => {
                    debug!("Issue #{} summary: {} chars", row.number, s.len());
                    s
                }
                Err(e) => {
                    debug!("Issue #{} summary failed: {e}", row.number);
                    format!("#{} {}: summary unavailable", row.number, row.title)
                }
            };

            per_issue_summaries.push(format!("- #{} {}: {}", row.number, row.title, summary));
        }

        // Aggregate per-issue summaries into a repo-level report
        let repo_summary = if per_issue_summaries.is_empty() {
            "No recent activity.".to_string()
        } else {
            eprintln!("  Generating repo summary...");
            let aggregated = per_issue_summaries.join("\n");
            let prompt = WeeklySummaryPrompt {
                repo: repo.clone(),
                issue_summaries: aggregated,
            };
            match agent.invoke(&prompt) {
                Ok(s) => {
                    debug!("Repo summary received ({} chars)", s.len());
                    s
                }
                Err(e) => {
                    debug!("Repo summary failed: {e}");
                    format!("Analysis unavailable: {e}")
                }
            }
        };

        // Triage flagged issues
        let flagged_refs = crate::filter::find_flagged_issues(
            &issues.iter().collect::<Vec<_>>(),
            &repo_config.labels_required,
        );
        debug!("Found {} flagged issues in {}", flagged_refs.len(), repo);
        let mut flagged_issues = Vec::new();

        if !flagged_refs.is_empty() {
            eprintln!("  Triaging {} flagged issues...", flagged_refs.len());
        }

        for (i, issue) in flagged_refs.iter().enumerate() {
            eprintln!("    [{}/{}] #{} {}...", i + 1, flagged_refs.len(), issue.number, issue.title);

            let row = issue_rows.iter().find(|r| r.number == issue.number);
            let body = row.and_then(|r| r.body.clone()).unwrap_or_default();
            let comments_text: String = all_comments.iter()
                .filter(|c| c.issue_number == issue.number)
                .map(|c| format!("{}: {}", c.author, c.body))
                .collect::<Vec<_>>()
                .join("\n");

            let triage_prompt = IssueTriagePrompt {
                title: issue.title.clone(),
                body,
                comments: comments_text,
            };

            let triage_summary = match agent.invoke(&triage_prompt) {
                Ok(s) => s,
                Err(e) => format!("Analysis unavailable: {e}"),
            };

            flagged_issues.push(FlaggedIssue {
                number: issue.number,
                title: issue.title.clone(),
                missing_labels: issue.missing_labels(&repo_config.labels_required),
                summary: triage_summary,
            });
        }

        repo_sections.push(RepoSection {
            name: repo.clone(),
            progress: repo_summary,
            big_updates: String::new(),
            planned_next: String::new(),
            flagged_issues,
        });
    }

    eprintln!("Computing team stats...");
    let team_stats: Vec<TeamStats> = config
        .team
        .iter()
        .map(|member| {
            let active = all_recent_issues
                .iter()
                .filter(|i| i.assignees.contains(&member.github))
                .count();
            TeamStats {
                name: member.name.clone(),
                active,
                closed_this_week: 0,
            }
        })
        .collect();

    eprintln!("Done.");
    Ok(Report {
        date: Utc::now().format("%Y-%m-%d").to_string(),
        repos: repo_sections,
        team_stats,
    })
}

/// Convert a database IssueRow to the domain Issue type.
fn row_to_issue(row: &IssueRow) -> Issue {
    let labels: Vec<String> = serde_json::from_str(&row.labels).unwrap_or_default();
    let assignees: Vec<String> = serde_json::from_str(&row.assignees).unwrap_or_default();

    Issue {
        number: row.number,
        title: row.title.clone(),
        labels,
        assignees,
        updated_at: chrono::DateTime::parse_from_rfc3339(&row.updated_at)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now()),
        created_at: chrono::DateTime::parse_from_rfc3339(&row.created_at)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now()),
        repo: row.repo.clone(),
    }
}
```

**Step 4: Run test to verify it passes**

Run: `cargo test --test pipeline_test`
Expected: PASS

**Step 5: Commit**

```bash
git add src/pipeline.rs tests/pipeline_test.rs
git commit -m "feat: migrate pipeline to read from SQLite database"
```

---

### Task 11: Update main.rs Callers (report and interactive)

**Files:**
- Modify: `src/main.rs`

**Step 1: Update cmd_report**

```rust
fn cmd_report(days: i64) -> Result<()> {
    let config = ceo::config::Config::load()?;
    let conn = ceo::db::open_existing_db()?;
    let agent = ceo::agent::AgentKind::from_config(&config.agent);

    let report_data = ceo::pipeline::run_pipeline(&config, &conn, &agent, days)?;
    let markdown = ceo::report::render_markdown(&report_data);
    print!("{markdown}");
    Ok(())
}
```

**Step 2: Update cmd_interactive**

```rust
fn cmd_interactive() -> Result<()> {
    let config = ceo::config::Config::load()?;
    let conn = ceo::db::open_existing_db()?;
    let agent = ceo::agent::AgentKind::from_config(&config.agent);

    eprintln!("Generating report from local database...");
    let report_data = ceo::pipeline::run_pipeline(&config, &conn, &agent, 7)?;
    let markdown = ceo::report::render_markdown(&report_data);

    tui::run_tui(markdown)?;
    Ok(())
}
```

**Step 3: Remove unused GhRunner import if no longer needed in main**

Remove `use ceo::gh::RealGhRunner;` if it's only used in the now-removed cmd_report code. The `cmd_sync` function still needs it.

**Step 4: Verify it compiles**

Run: `cargo check`
Expected: Compiles

**Step 5: Run all tests**

Run: `cargo test`
Expected: All pass

**Step 6: Commit**

```bash
git add src/main.rs
git commit -m "feat: update report and interactive commands to use database"
```

---

### Task 12: Update Integration Test

**Files:**
- Modify: `tests/integration_test.rs`

**Step 1: Read the current integration test**

Check what `tests/integration_test.rs` contains and update it to work with the new database-backed pipeline.

**Step 2: Update integration test**

The integration test likely uses `GhRunner` mocks. Update it to seed a database instead. Follow the same pattern as the pipeline test: create a tempdir, open a database, seed issues/comments, then call `run_pipeline` with `&conn`.

**Step 3: Run all tests**

Run: `cargo test`
Expected: All pass

**Step 4: Commit**

```bash
git add tests/integration_test.rs
git commit -m "feat: update integration test for database-backed pipeline"
```

---

### Task 13: Final Verification

**Step 1: Run full test suite**

Run: `cargo test`
Expected: All tests pass

**Step 2: Run clippy**

Run: `cargo clippy -- -D warnings`
Expected: No warnings

**Step 3: Verify binary builds**

Run: `cargo build`
Expected: Clean build

**Step 4: Quick manual check**

Run: `cargo run -- sync --help`
Expected: Shows sync command help

Run: `cargo run -- report --help`
Expected: Shows report command help

**Step 5: Commit any cleanup**

If clippy found anything, fix and commit.

---

Plan complete and saved to `docs/plans/2026-03-08-sync-sqlite-implementation.md`. Two execution options:

**1. Subagent-Driven (this session)** - I dispatch fresh subagent per task, review between tasks, fast iteration

**2. Parallel Session (separate)** - Open new session with executing-plans, batch execution with checkpoints

Which approach?
