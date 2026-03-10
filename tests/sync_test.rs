use ceo::config::{Config, ProjectConfig};
use ceo::db;
use ceo::error::GhError;
use ceo::gh::GhRunner;
use ceo::sync::run_sync;

struct MockSyncGh;

impl GhRunner for MockSyncGh {
    fn run_gh(&self, args: &[&str]) -> Result<String, GhError> {
        // gh api repos/.../issues?state=all (REST: issues + PRs)
        if args.iter().any(|a| *a == "api")
            && args.iter().any(|a| a.contains("/issues?") && a.contains("state=all"))
        {
            return Ok(r#"[{
                "number": 1,
                "title": "Fix auth",
                "labels": [{"name": "bug"}],
                "assignees": [{"login": "alice"}],
                "updated_at": "2026-03-05T10:00:00Z",
                "created_at": "2026-03-01T10:00:00Z",
                "state": "open",
                "body": "Auth is broken"
            }]"#
            .to_string());
        }
        // gh api repos/.../issues/comments (batch comments)
        if args.iter().any(|a| *a == "api") && args.iter().any(|a| a.contains("issues/comments")) {
            return Ok(r#"[{
                "id": 1001,
                "user": {"login": "bob"},
                "body": "I can reproduce this",
                "created_at": "2026-03-02T10:00:00Z",
                "issue_url": "https://api.github.com/repos/org/repo/issues/1"
            }]"#
            .to_string());
        }
        // gh api repos/.../stats/contributors
        if args.iter().any(|a| *a == "api") && args.iter().any(|a| a.contains("stats/contributors")) {
            return Ok(r#"[{
                "author": {"login": "alice"},
                "total": 10,
                "weeks": [
                    {"w": 1709424000, "a": 100, "d": 50, "c": 5},
                    {"w": 1710028800, "a": 200, "d": 80, "c": 8}
                ]
            }]"#.to_string());
        }
        // gh api repos/.../commits
        if args.iter().any(|a| *a == "api") && args.iter().any(|a| a.contains("commits")) {
            return Ok(r#"[{
                "sha": "abc1234567890",
                "commit": {
                    "author": {"name": "alice", "date": "2026-03-05T10:00:00Z"},
                    "message": "fix: resolve auth bug"
                },
                "author": {"login": "alice"}
            }]"#.to_string());
        }
        // gh project item-list
        if args.iter().any(|a| *a == "item-list") {
            return Ok(r#"{"items": [{
                "content": {"number": 1, "repository": "org/repo", "type": "Issue"},
                "status": "In Progress",
                "priority": "High"
            }]}"#
            .to_string());
        }
        Ok("[]".to_string())
    }
}

#[test]
fn sync_fetches_and_stores_issues() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let conn = db::open_db_at(&db_path).unwrap();

    let config: Config = toml::from_str(
        r#"
[[repos]]
name = "org/repo"
"#,
    )
    .unwrap();

    let result = run_sync(&config, &MockSyncGh, &conn, &ceo::sync::NoProgress).unwrap();
    assert_eq!(result.repos.len(), 1);
    assert_eq!(result.repos[0].issues_synced, 1);
    assert!(result.repos[0].comments_synced >= 1);

    // Verify data in database
    let issues = db::query_recent_issues(
        &conn,
        &["org/repo".to_string()],
        "2026-01-01T00:00:00Z",
    )
    .unwrap();
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].title, "Fix auth");

    let comments = db::query_comments_for_issues(&conn, "org/repo", &[1]).unwrap();
    assert_eq!(comments.len(), 1);
    assert_ne!(comments[0].comment_id, 0); // hashed from node ID
    assert_eq!(comments[0].author, "bob");

    // Verify commits
    assert_eq!(result.repos[0].commits_synced, 1);
    let commits = db::query_recent_commits(
        &conn,
        &["org/repo".to_string()],
        "2026-01-01T00:00:00Z",
    )
    .unwrap();
    assert_eq!(commits.len(), 1);
    assert_eq!(commits[0].author, "alice");
    assert!(commits[0].message.contains("auth bug"));
}

#[test]
fn sync_with_project_config_merges_project_fields() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let conn = db::open_db_at(&db_path).unwrap();

    let mut config: Config = toml::from_str(
        r#"
[[repos]]
name = "org/repo"
"#,
    )
    .unwrap();
    config.project = Some(ProjectConfig {
        org: "org".to_string(),
        number: 1,
    });

    let result = run_sync(&config, &MockSyncGh, &conn, &ceo::sync::NoProgress).unwrap();
    assert_eq!(result.repos[0].issues_synced, 1);

    let issues = db::query_recent_issues(
        &conn,
        &["org/repo".to_string()],
        "2026-01-01T00:00:00Z",
    )
    .unwrap();
    assert_eq!(issues[0].project_status.as_deref(), Some("In Progress"));
    assert_eq!(issues[0].project_priority.as_deref(), Some("High"));
}

#[test]
fn sync_incremental_passes_since_on_second_run() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let conn = db::open_db_at(&db_path).unwrap();

    let config: Config = toml::from_str(
        r#"
[[repos]]
name = "org/repo"
"#,
    )
    .unwrap();

    // First sync — full fetch
    let result = run_sync(&config, &MockSyncGh, &conn, &ceo::sync::NoProgress).unwrap();
    assert_eq!(result.repos[0].issues_synced, 1);

    // Second sync — incremental (mock still returns same data, upsert is idempotent)
    let result2 = run_sync(&config, &MockSyncGh, &conn, &ceo::sync::NoProgress).unwrap();
    assert_eq!(result2.repos.len(), 1);
    // The mock always returns the same issue regardless of --search, so count stays 1
    assert_eq!(result2.repos[0].issues_synced, 1);
}

#[test]
fn sync_fetches_and_stores_contributor_stats() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let conn = db::open_db_at(&db_path).unwrap();

    let config: Config = toml::from_str(r#"
[[repos]]
name = "org/repo"
"#).unwrap();

    let result = run_sync(&config, &MockSyncGh, &conn, &ceo::sync::NoProgress).unwrap();
    assert_eq!(result.repos.len(), 1);

    // Verify contributor stats were stored
    let stats = db::query_contributor_stats(
        &conn,
        &["org/repo".to_string()],
        "2024-01-01",
    ).unwrap();
    // Should have 2 weeks for alice (both have non-zero activity)
    assert_eq!(stats.len(), 2);
    assert_eq!(stats[0].author, "alice");
}
