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
