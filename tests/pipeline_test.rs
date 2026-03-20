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

    // Use relative dates so the test doesn't break as time passes
    let yesterday = (chrono::Utc::now() - chrono::Duration::days(1)).to_rfc3339();
    let two_days_ago = (chrono::Utc::now() - chrono::Duration::days(2)).to_rfc3339();

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
        created_at: two_days_ago.clone(),
        updated_at: yesterday.clone(),
        project_status: None,
        project_start_date: None,
        project_target_date: None,
        project_priority: None,
        author: None,
        pr_additions: None,
        pr_deletions: None,
    }];
    db::upsert_issues(&conn, &issues).unwrap();

    let comments = vec![db::CommentRow {
        repo: "org/frontend".to_string(),
        issue_number: 1,
        comment_id: 0,
        author: "bob".to_string(),
        body: "I can review this.".to_string(),
        created_at: two_days_ago,
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

    // Use relative dates so the test doesn't break as time passes
    let yesterday = (chrono::Utc::now() - chrono::Duration::days(1)).to_rfc3339();
    let yesterday_date = (chrono::Utc::now() - chrono::Duration::days(1))
        .format("%Y-%m-%d")
        .to_string();
    let two_days_ago = (chrono::Utc::now() - chrono::Duration::days(2)).to_rfc3339();
    let two_days_ago_date = (chrono::Utc::now() - chrono::Duration::days(2))
        .format("%Y-%m-%d")
        .to_string();

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
        created_at: two_days_ago.clone(),
        updated_at: yesterday,
        project_status: None,
        project_start_date: None,
        project_target_date: None,
        project_priority: None,
        author: None,
        pr_additions: None,
        pr_deletions: None,
    }];
    db::upsert_issues(&conn, &issues).unwrap();

    // Seed commit-level stats with raw emails
    let commit_stats = vec![
        db::CommitStatsRow {
            repo: "org/frontend".to_string(),
            sha: "aaa111".to_string(),
            author_email: "alice@example.com".to_string(),
            committed_at: two_days_ago_date,
            additions: 80,
            deletions: 20,
            branch: "main".to_string(),
        },
        db::CommitStatsRow {
            repo: "org/frontend".to_string(),
            sha: "bbb222".to_string(),
            author_email: "alice@example.com".to_string(),
            committed_at: yesterday_date,
            additions: 70,
            deletions: 20,
            branch: "main".to_string(),
        },
    ];
    db::upsert_commit_stats(&conn, &commit_stats).unwrap();
    // Add email mapping so contributor stats resolve correctly
    db::upsert_email_mapping(&conn, "alice@example.com", "alice").unwrap();

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

#[tokio::test]
async fn pipeline_aggregates_multiple_emails_for_same_team_member() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let conn = db::open_db_at(&db_path).unwrap();

    // Use a recent date so commits fall within the 7-day window
    let recent_date = (chrono::Utc::now() - chrono::Duration::days(1))
        .format("%Y-%m-%d")
        .to_string();

    // Same person commits with work and personal emails
    let commit_stats = vec![
        db::CommitStatsRow {
            repo: "org/frontend".to_string(),
            sha: "aaa111".to_string(),
            author_email: "dplankensteiner@company.com".to_string(),
            committed_at: recent_date.clone(),
            additions: 100,
            deletions: 20,
            branch: "main".to_string(),
        },
        db::CommitStatsRow {
            repo: "org/frontend".to_string(),
            sha: "bbb222".to_string(),
            author_email: "david-pl@users.noreply.github.com".to_string(),
            committed_at: recent_date.clone(),
            additions: 50,
            deletions: 10,
            branch: "main".to_string(),
        },
    ];
    db::upsert_commit_stats(&conn, &commit_stats).unwrap();
    // Map both emails to the same GitHub handle
    db::upsert_email_mapping(&conn, "dplankensteiner@company.com", "david-pl").unwrap();
    db::upsert_email_mapping(&conn, "david-pl@users.noreply.github.com", "david-pl").unwrap();

    let config: Config = toml::from_str(r#"
        [[repos]]
        name = "org/frontend"

        [[team]]
        github = "david-pl"
        name = "David Plankensteiner"
        role = "Engineer"
    "#).unwrap();

    let since = (chrono::Utc::now() - chrono::Duration::days(7)).to_rfc3339();
    let report = run_pipeline(&config, &conn, &MockAgent, &since, "2026-03-09", &NullProgress, None).await.unwrap();

    // Both emails should be aggregated under the same team member
    assert_eq!(report.team_stats.len(), 1);
    assert_eq!(report.team_stats[0].github, "david-pl");
    assert_eq!(report.team_stats[0].additions, 150, "Should sum additions from both emails");
    assert_eq!(report.team_stats[0].deletions, 30, "Should sum deletions from both emails");
}

#[tokio::test]
async fn pipeline_unmapped_email_prefix_does_not_match_different_handle() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let conn = db::open_db_at(&db_path).unwrap();

    let recent_date = (chrono::Utc::now() - chrono::Duration::days(1))
        .format("%Y-%m-%d")
        .to_string();

    // Commit with work email that has no mapping
    let commit_stats = vec![db::CommitStatsRow {
        repo: "org/frontend".to_string(),
        sha: "aaa111".to_string(),
        author_email: "dplankensteiner@company.com".to_string(),
        committed_at: recent_date,
        additions: 500,
        deletions: 100,
        branch: "main".to_string(),
    }];
    db::upsert_commit_stats(&conn, &commit_stats).unwrap();
    // Deliberately NO email mapping — email prefix "dplankensteiner" won't match "david-pl"

    let config: Config = toml::from_str(r#"
        [[repos]]
        name = "org/frontend"

        [[team]]
        github = "david-pl"
        name = "David Plankensteiner"
        role = "Engineer"
    "#).unwrap();

    let since = (chrono::Utc::now() - chrono::Duration::days(7)).to_rfc3339();
    let report = run_pipeline(&config, &conn, &MockAgent, &since, "2026-03-09", &NullProgress, None).await.unwrap();

    // Without the email mapping, the commit should NOT be attributed to david-pl
    assert_eq!(report.team_stats.len(), 1);
    assert_eq!(report.team_stats[0].additions, 0,
        "Unmapped email prefix 'dplankensteiner' should not match github handle 'david-pl'");
}

#[tokio::test]
async fn pipeline_report_has_generated_at_timestamp() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let conn = db::open_db_at(&db_path).unwrap();

    let config: Config = toml::from_str(r#"
        [[repos]]
        name = "org/empty"
    "#).unwrap();

    let since = (chrono::Utc::now() - chrono::Duration::days(7)).to_rfc3339();
    let report = run_pipeline(&config, &conn, &MockAgent, &since, "2026-03-19", &NullProgress, None).await.unwrap();

    // generated_at should be a valid RFC 3339 timestamp with timezone
    assert!(!report.generated_at.is_empty(), "generated_at should not be empty");
    assert!(report.generated_at.contains('T'), "generated_at should be RFC 3339 format");
    assert!(report.generated_at.contains('+') || report.generated_at.contains('-'),
        "generated_at should include timezone offset: {}", report.generated_at);
}
