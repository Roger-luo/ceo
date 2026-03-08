use ceo::config::{Config, ProjectConfig};
use ceo::db;
use ceo::error::GhError;
use ceo::gh::GhRunner;
use ceo::sync::run_sync;

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
            }]"#
            .to_string());
        }
        // gh issue view (comments)
        if args.iter().any(|a| *a == "view") {
            return Ok(r#"{
                "body": "Auth is broken",
                "comments": [{
                    "id": 1001,
                    "author": {"login": "bob"},
                    "body": "I can reproduce this",
                    "createdAt": "2026-03-02T10:00:00Z"
                }]
            }"#
            .to_string());
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

    let result = run_sync(&config, &MockSyncGh, &conn).unwrap();
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
    assert_eq!(comments[0].comment_id, 1001);
    assert_eq!(comments[0].author, "bob");
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

    let result = run_sync(&config, &MockSyncGh, &conn).unwrap();
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
