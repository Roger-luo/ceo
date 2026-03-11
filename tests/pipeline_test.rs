use std::future::Future;
use std::pin::Pin;

use ceo::agent::Agent;
use ceo::config::Config;
use ceo::db;
use ceo::error::AgentError;
use ceo::pipeline::{run_pipeline, NullProgress};
use ceo::prompt::Prompt;

struct MockAgent;

impl Agent for MockAgent {
    fn invoke(&self, _prompt: &dyn Prompt) -> Pin<Box<dyn Future<Output = Result<String, AgentError>> + Send + '_>> {
        Box::pin(async {
            Ok("<done>Mock work completed.</done><in_progress>Mock active work.</in_progress>".to_string())
        })
    }
}

#[tokio::test]
async fn pipeline_reads_from_database() {
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
        kind: "issue".to_string(),
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

    let since = (chrono::Utc::now() - chrono::Duration::days(7)).to_rfc3339();
    let report = run_pipeline(&config, &conn, &MockAgent, &since, "2026-03-09", &NullProgress, None).await.unwrap();
    assert_eq!(report.repos.len(), 1);
    assert_eq!(report.repos[0].name, "org/frontend");
    assert_eq!(report.repos[0].done.as_deref(), Some("Mock work completed."));
    assert_eq!(report.repos[0].in_progress.as_deref(), Some("Mock active work."));
    assert!(!report.repos[0].flagged_issues.is_empty());
    assert_eq!(report.team_stats.len(), 1);
    assert_eq!(report.team_stats[0].active, 1);
}

#[tokio::test]
async fn pipeline_handles_empty_database() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let conn = db::open_db_at(&db_path).unwrap();

    let config: Config = toml::from_str(r#"
        [[repos]]
        name = "org/empty"
    "#).unwrap();

    let since = (chrono::Utc::now() - chrono::Duration::days(7)).to_rfc3339();
    let report = run_pipeline(&config, &conn, &MockAgent, &since, "2026-03-09", &NullProgress, None).await.unwrap();
    assert_eq!(report.repos.len(), 1);
    assert!(!report.repos[0].has_activity());
}

#[tokio::test]
async fn pipeline_includes_contributor_stats_in_team_overview() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let conn = db::open_db_at(&db_path).unwrap();

    // Seed issues
    let issues = vec![db::IssueRow {
        repo: "org/frontend".to_string(),
        number: 1,
        title: "Implement auth".to_string(),
        body: Some("Auth implementation needed.".to_string()),
        state: Some("open".to_string()),
        kind: "issue".to_string(),
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

    // Seed commit-level stats (query_contributor_stats aggregates from commit_stats)
    // Use dates well within the 7-day window so they aren't filtered out by `since`.
    let commit_stats = vec![
        db::CommitStatsRow {
            repo: "org/frontend".to_string(),
            sha: "aaa111".to_string(),
            author: "alice".to_string(),
            committed_at: "2026-03-07".to_string(),
            additions: 80,
            deletions: 20,
            branch: "main".to_string(),
        },
        db::CommitStatsRow {
            repo: "org/frontend".to_string(),
            sha: "bbb222".to_string(),
            author: "alice".to_string(),
            committed_at: "2026-03-08".to_string(),
            additions: 70,
            deletions: 20,
            branch: "main".to_string(),
        },
    ];
    db::upsert_commit_stats(&conn, &commit_stats).unwrap();

    let config: Config = toml::from_str(r#"
        [[repos]]
        name = "org/frontend"
        labels_required = ["priority"]

        [[team]]
        github = "alice"
        name = "Alice Smith"
        role = "Lead"
    "#).unwrap();

    let since = (chrono::Utc::now() - chrono::Duration::days(7)).to_rfc3339();
    let report = run_pipeline(&config, &conn, &MockAgent, &since, "2026-03-09", &NullProgress, None).await.unwrap();
    assert_eq!(report.team_stats[0].additions, 150);
    assert_eq!(report.team_stats[0].deletions, 40);
}
