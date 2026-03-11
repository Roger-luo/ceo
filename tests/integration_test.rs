use std::future::Future;
use std::pin::Pin;

use ceo::agent::Agent;
use ceo::config::Config;
use ceo::db;
use ceo::error::AgentError;
use ceo::pipeline::{run_pipeline, NullProgress};
use ceo::prompt::Prompt;
use ceo::report::render_markdown;

struct MockAgent;

impl Agent for MockAgent {
    fn invoke(&self, prompt: &dyn Prompt) -> Pin<Box<dyn Future<Output = Result<String, AgentError>> + Send + '_>> {
        let rendered = prompt.render();
        Box::pin(async move {
            if rendered.contains("Write a concise summary for repo") {
                Ok("<done>Dark mode feature merged.</done>\n<in_progress>Memory leak identified and being fixed.</in_progress>".to_string())
            } else {
                Ok("This issue is about updating documentation. Suggest adding priority:low label.".to_string())
            }
        })
    }
}

#[tokio::test]
async fn full_pipeline_produces_valid_markdown() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let conn = db::open_db_at(&db_path).unwrap();

    // Seed issues
    let issues = vec![
        db::IssueRow {
            repo: "org/frontend".to_string(),
            number: 10,
            title: "Add dark mode".to_string(),
            body: Some("This issue needs triage.".to_string()),
            state: Some("open".to_string()),
            kind: "issue".to_string(),
            labels: r#"["feature","priority"]"#.to_string(),
            assignees: r#"["alice"]"#.to_string(),
            created_at: "2026-02-25T10:00:00Z".to_string(),
            updated_at: "2026-03-05T10:00:00Z".to_string(),
            project_status: None,
            project_start_date: None,
            project_target_date: None,
            project_priority: None,
            author: None,
            pr_additions: None,
            pr_deletions: None,
        },
        db::IssueRow {
            repo: "org/frontend".to_string(),
            number: 11,
            title: "Fix memory leak".to_string(),
            body: Some("This issue needs triage.".to_string()),
            state: Some("open".to_string()),
            kind: "issue".to_string(),
            labels: r#"["bug"]"#.to_string(),
            assignees: r#"["bob"]"#.to_string(),
            created_at: "2026-02-28T10:00:00Z".to_string(),
            updated_at: "2026-03-04T10:00:00Z".to_string(),
            project_status: None,
            project_start_date: None,
            project_target_date: None,
            project_priority: None,
            author: None,
            pr_additions: None,
            pr_deletions: None,
        },
        db::IssueRow {
            repo: "org/frontend".to_string(),
            number: 12,
            title: "Update docs".to_string(),
            body: Some("This issue needs triage.".to_string()),
            state: Some("open".to_string()),
            kind: "issue".to_string(),
            labels: r#"[]"#.to_string(),
            assignees: r#"[]"#.to_string(),
            created_at: "2026-03-01T10:00:00Z".to_string(),
            updated_at: "2026-03-03T10:00:00Z".to_string(),
            project_status: None,
            project_start_date: None,
            project_target_date: None,
            project_priority: None,
            author: None,
            pr_additions: None,
            pr_deletions: None,
        },
    ];
    db::upsert_issues(&conn, &issues).unwrap();

    // Seed comments
    let comments = vec![db::CommentRow {
        repo: "org/frontend".to_string(),
        issue_number: 10,
        comment_id: 0,
        author: "bob".to_string(),
        body: "I'll look into this.".to_string(),
        created_at: "2026-03-03T12:00:00Z".to_string(),
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

        [[team]]
        github = "bob"
        name = "Bob Jones"
        role = "Backend"
    "#).unwrap();

    let since = "2026-03-01T00:00:00Z".to_string();
    let report = run_pipeline(&config, &conn, &MockAgent, &since, "2026-03-06", &NullProgress, None).await.unwrap();
    let markdown = render_markdown(&report);

    assert!(markdown.contains("org/frontend"));
    assert!(markdown.contains("**Done:** Dark mode feature merged."));
    assert!(markdown.contains("**In Progress:** Memory leak identified"));
    assert!(markdown.contains("Needs Attention"));
    assert!(markdown.contains("#11") || markdown.contains("#12"));
    assert!(markdown.contains("Alice Smith"));
    assert!(markdown.contains("Bob Jones"));
}
