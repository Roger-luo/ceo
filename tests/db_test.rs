use ceo::db::{self, CommentRow, CommitStatsRow, IssueRow};

fn make_issue(repo: &str, number: u64, title: &str, updated_at: &str) -> IssueRow {
    IssueRow {
        repo: repo.to_string(),
        number,
        title: title.to_string(),
        body: Some("body".to_string()),
        state: Some("open".to_string()),
        kind: "issue".to_string(),
        labels: "[]".to_string(),
        assignees: "[]".to_string(),
        created_at: "2026-03-01T00:00:00Z".to_string(),
        updated_at: updated_at.to_string(),
        project_status: None,
        project_start_date: None,
        project_target_date: None,
        project_priority: None,
    }
}

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
    assert!(tables.contains(&"commit_stats".to_string()));
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

// --- Task 4: Upsert tests ---

#[test]
fn upsert_issues_inserts_and_replaces() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    let conn = db::open_db_at(&path).unwrap();

    let issues = vec![make_issue("org/repo", 1, "First", "2026-03-05T00:00:00Z")];

    let count = db::upsert_issues(&conn, &issues).unwrap();
    assert_eq!(count, 1);

    // Upsert again with updated title
    let updated = vec![IssueRow {
        title: "Updated".to_string(),
        ..issues[0].clone()
    }];
    let count = db::upsert_issues(&conn, &updated).unwrap();
    assert_eq!(count, 1);

    // Verify the title was updated
    let title: String = conn
        .query_row("SELECT title FROM issues WHERE number = 1", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(title, "Updated");
}

#[test]
fn upsert_comments_inserts_and_replaces() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    let conn = db::open_db_at(&path).unwrap();

    // Insert parent issue first (FK constraint)
    let issues = vec![make_issue("org/repo", 1, "Issue", "2026-03-05T00:00:00Z")];
    db::upsert_issues(&conn, &issues).unwrap();

    let comments = vec![CommentRow {
        repo: "org/repo".to_string(),
        issue_number: 1,
        comment_id: 100,
        author: "alice".to_string(),
        body: "first comment".to_string(),
        created_at: "2026-03-05T00:00:00Z".to_string(),
    }];

    let count = db::upsert_comments(&conn, &comments).unwrap();
    assert_eq!(count, 1);

    // Replace with updated body
    let updated = vec![CommentRow {
        body: "updated comment".to_string(),
        ..comments[0].clone()
    }];
    let count = db::upsert_comments(&conn, &updated).unwrap();
    assert_eq!(count, 1);

    let body: String = conn
        .query_row(
            "SELECT body FROM comments WHERE comment_id = 100",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(body, "updated comment");
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

    let (repo, issues, comments): (String, i64, i64) = conn
        .query_row(
            "SELECT repo, issues_synced, comments_synced FROM sync_log",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(repo, "org/repo");
    assert_eq!(issues, 10);
    assert_eq!(comments, 25);
}

// --- Task 5: Query tests ---

#[test]
fn query_recent_issues_filters_by_date_and_repo() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    let conn = db::open_db_at(&path).unwrap();

    let issues = vec![
        make_issue("org/repo", 1, "Recent", "2026-03-05T00:00:00Z"),
        make_issue("org/repo", 2, "Old", "2026-01-01T00:00:00Z"),
        make_issue("org/other", 3, "Other repo recent", "2026-03-05T00:00:00Z"),
    ];
    db::upsert_issues(&conn, &issues).unwrap();

    let results = db::query_recent_issues(
        &conn,
        &["org/repo".to_string()],
        "2026-03-01T00:00:00Z",
    )
    .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].title, "Recent");
    assert_eq!(results[0].number, 1);
}

#[test]
fn query_recent_issues_multiple_repos() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    let conn = db::open_db_at(&path).unwrap();

    let issues = vec![
        make_issue("org/repo", 1, "A", "2026-03-05T00:00:00Z"),
        make_issue("org/other", 2, "B", "2026-03-05T00:00:00Z"),
    ];
    db::upsert_issues(&conn, &issues).unwrap();

    let results = db::query_recent_issues(
        &conn,
        &["org/repo".to_string(), "org/other".to_string()],
        "2026-03-01T00:00:00Z",
    )
    .unwrap();
    assert_eq!(results.len(), 2);
}

#[test]
fn query_recent_issues_empty_repos() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    let conn = db::open_db_at(&path).unwrap();

    let results = db::query_recent_issues(&conn, &[], "2026-03-01T00:00:00Z").unwrap();
    assert!(results.is_empty());
}

#[test]
fn query_comments_for_issues_returns_ordered() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    let conn = db::open_db_at(&path).unwrap();

    // Insert parent issue
    db::upsert_issues(
        &conn,
        &[make_issue("org/repo", 1, "Issue", "2026-03-05T00:00:00Z")],
    )
    .unwrap();

    let comments = vec![
        CommentRow {
            repo: "org/repo".to_string(),
            issue_number: 1,
            comment_id: 200,
            author: "bob".to_string(),
            body: "second".to_string(),
            created_at: "2026-03-05T02:00:00Z".to_string(),
        },
        CommentRow {
            repo: "org/repo".to_string(),
            issue_number: 1,
            comment_id: 100,
            author: "alice".to_string(),
            body: "first".to_string(),
            created_at: "2026-03-05T01:00:00Z".to_string(),
        },
    ];
    db::upsert_comments(&conn, &comments).unwrap();

    let results = db::query_comments_for_issues(&conn, "org/repo", &[1]).unwrap();
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].body, "first");
    assert_eq!(results[1].body, "second");
}

#[test]
fn query_comments_for_issues_empty() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    let conn = db::open_db_at(&path).unwrap();

    let results = db::query_comments_for_issues(&conn, "org/repo", &[]).unwrap();
    assert!(results.is_empty());
}

#[test]
fn query_contributor_stats_filters_by_date_and_repo() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    let conn = db::open_db_at(&path).unwrap();

    // Insert commit_stats rows with raw emails
    let commit_rows = vec![
        CommitStatsRow {
            repo: "org/repo".to_string(),
            sha: "aaa".to_string(),
            author_email: "alice@example.com".to_string(),
            committed_at: "2026-03-02".to_string(),
            additions: 100,
            deletions: 50,
            branch: "main".to_string(),
        },
        CommitStatsRow {
            repo: "org/repo".to_string(),
            sha: "bbb".to_string(),
            author_email: "alice@example.com".to_string(),
            committed_at: "2026-01-06".to_string(),
            additions: 30,
            deletions: 10,
            branch: "main".to_string(),
        },
        CommitStatsRow {
            repo: "org/other".to_string(),
            sha: "ccc".to_string(),
            author_email: "bob@example.com".to_string(),
            committed_at: "2026-03-02".to_string(),
            additions: 200,
            deletions: 80,
            branch: "main".to_string(),
        },
    ];
    db::upsert_commit_stats(&conn, &commit_rows).unwrap();
    db::upsert_email_mapping(&conn, "alice@example.com", "alice").unwrap();

    let results = db::query_contributor_stats(
        &conn,
        &["org/repo".to_string()],
        "2026-03-01",
    )
    .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].author, "alice");
    assert_eq!(results[0].additions, 100);
}

#[test]
fn query_contributor_stats_multiple_repos() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    let conn = db::open_db_at(&path).unwrap();

    let commit_rows = vec![
        CommitStatsRow {
            repo: "org/repo".to_string(),
            sha: "aaa".to_string(),
            author_email: "alice@example.com".to_string(),
            committed_at: "2026-03-02".to_string(),
            additions: 100,
            deletions: 50,
            branch: "main".to_string(),
        },
        CommitStatsRow {
            repo: "org/other".to_string(),
            sha: "bbb".to_string(),
            author_email: "alice@example.com".to_string(),
            committed_at: "2026-03-02".to_string(),
            additions: 50,
            deletions: 20,
            branch: "main".to_string(),
        },
    ];
    db::upsert_commit_stats(&conn, &commit_rows).unwrap();
    db::upsert_email_mapping(&conn, "alice@example.com", "alice").unwrap();

    let results = db::query_contributor_stats(
        &conn,
        &["org/repo".to_string(), "org/other".to_string()],
        "2026-03-01",
    )
    .unwrap();
    assert_eq!(results.len(), 2);
}

#[test]
fn query_contributor_stats_empty_repos() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    let conn = db::open_db_at(&path).unwrap();

    let results = db::query_contributor_stats(&conn, &[], "2026-03-01").unwrap();
    assert!(results.is_empty());
}

// --- Schema version tests ---

#[test]
fn open_db_stores_schema_version() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    let _conn = db::open_db_at(&path).unwrap();

    let conn = rusqlite::Connection::open(&path).unwrap();
    let version: u32 = conn
        .query_row("SELECT version FROM schema_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, ceo_schema::SCHEMA_VERSION);
}

#[test]
fn open_db_clears_on_version_mismatch() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");

    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE schema_version (version INTEGER NOT NULL);
             INSERT INTO schema_version (version) VALUES (999);
             CREATE TABLE issues (id INTEGER PRIMARY KEY);"
        ).unwrap();
    }

    let conn = db::open_db_at(&path).unwrap();

    let version: u32 = conn
        .query_row("SELECT version FROM schema_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, ceo_schema::SCHEMA_VERSION);

    let col_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('issues')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(col_count > 1, "issues table should have full schema, not just id");
}

#[test]
fn open_db_clears_pre_versioning_database() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");

    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE issues (
                repo TEXT NOT NULL,
                number INTEGER NOT NULL,
                title TEXT NOT NULL,
                PRIMARY KEY (repo, number)
            );"
        ).unwrap();
    }

    let conn = db::open_db_at(&path).unwrap();

    let version: u32 = conn
        .query_row("SELECT version FROM schema_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, ceo_schema::SCHEMA_VERSION);
}

#[test]
fn open_existing_db_at_clears_on_version_mismatch() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");

    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE schema_version (version INTEGER NOT NULL);
             INSERT INTO schema_version (version) VALUES (999);
             CREATE TABLE issues (id INTEGER PRIMARY KEY);"
        ).unwrap();
    }

    let conn = db::open_existing_db_at(&path).unwrap();

    let version: u32 = conn
        .query_row("SELECT version FROM schema_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, ceo_schema::SCHEMA_VERSION);
}
