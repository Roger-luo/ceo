use std::future::Future;
use std::pin::Pin;

use ceo::agent::Agent;
use ceo::config::Config;
use ceo::db;
use ceo::error::AgentError;
use ceo::pipeline::{run_pipeline, NullProgress};
use ceo::prompt::Prompt;
use ceo::slack;

struct MockAgent;

impl Agent for MockAgent {
    fn invoke(
        &self,
        prompt: &dyn Prompt,
    ) -> Pin<Box<dyn Future<Output = Result<String, AgentError>> + Send + '_>> {
        let rendered = prompt.render();
        Box::pin(async move {
            if rendered.contains("Write a concise summary for repo") {
                if rendered.contains("org/frontend") {
                    Ok("\
<done>Shipped dark mode (#10) and fixed memory leak (#11) — \
both merged by @alice.</done>
<in_progress>Working on #13 for accessibility improvements.</in_progress>
<next>Performance audit planned for next sprint.</next>"
                        .to_string())
                } else {
                    Ok("\
<done>Released v2.0 API with pagination support (#50).</done>
<in_progress>@charlie investigating rate-limit bug (#55).</in_progress>"
                        .to_string())
                }
            } else if rendered.contains("executive")
                || rendered.contains("standup")
                || rendered.contains("technical")
            {
                Ok("\
## Highlights
- Dark mode shipped on frontend (org/frontend#10) by @alice
- Backend v2.0 API released (org/backend#50)

## Risks
- Rate-limit bug (org/backend#55) may impact production
- Accessibility PR (org/frontend#13) needs review

## Outlook
Team velocity is strong. Focus next week on org/frontend#14 perf audit."
                    .to_string())
            } else {
                Ok("Summary of this issue.".to_string())
            }
        })
    }
}

fn seed_test_db() -> (tempfile::TempDir, rusqlite::Connection, Config) {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let conn = db::open_db_at(&db_path).unwrap();

    let issues = vec![
        db::IssueRow {
            repo: "org/frontend".to_string(),
            number: 10,
            title: "Add dark mode".to_string(),
            body: Some("Implement dark mode theme.".to_string()),
            state: Some("CLOSED".to_string()),
            kind: "pr".to_string(),
            labels: r#"["feature"]"#.to_string(),
            assignees: r#"["alice"]"#.to_string(),
            created_at: "2026-03-01T10:00:00Z".to_string(),
            updated_at: "2026-03-05T10:00:00Z".to_string(),
            project_status: None,
            project_start_date: None,
            project_target_date: None,
            project_priority: None,
            author: Some("alice".to_string()),
            pr_additions: Some(320),
            pr_deletions: Some(45),
        },
        db::IssueRow {
            repo: "org/frontend".to_string(),
            number: 11,
            title: "Fix memory leak in renderer".to_string(),
            body: Some("Memory leak on re-render.".to_string()),
            state: Some("CLOSED".to_string()),
            kind: "issue".to_string(),
            labels: r#"["bug"]"#.to_string(),
            assignees: r#"["alice"]"#.to_string(),
            created_at: "2026-03-02T10:00:00Z".to_string(),
            updated_at: "2026-03-04T10:00:00Z".to_string(),
            project_status: None,
            project_start_date: None,
            project_target_date: None,
            project_priority: None,
            author: Some("alice".to_string()),
            pr_additions: None,
            pr_deletions: None,
        },
        db::IssueRow {
            repo: "org/frontend".to_string(),
            number: 13,
            title: "Accessibility improvements".to_string(),
            body: Some("Add ARIA labels.".to_string()),
            state: Some("OPEN".to_string()),
            kind: "pr".to_string(),
            labels: r#"["a11y"]"#.to_string(),
            assignees: r#"["bob"]"#.to_string(),
            created_at: "2026-03-03T10:00:00Z".to_string(),
            updated_at: "2026-03-05T10:00:00Z".to_string(),
            project_status: None,
            project_start_date: None,
            project_target_date: None,
            project_priority: None,
            author: Some("bob".to_string()),
            pr_additions: Some(80),
            pr_deletions: Some(10),
        },
        db::IssueRow {
            repo: "org/backend".to_string(),
            number: 50,
            title: "v2.0 API with pagination".to_string(),
            body: Some("Paginated endpoints.".to_string()),
            state: Some("CLOSED".to_string()),
            kind: "pr".to_string(),
            labels: r#"["api"]"#.to_string(),
            assignees: r#"["charlie"]"#.to_string(),
            created_at: "2026-03-01T08:00:00Z".to_string(),
            updated_at: "2026-03-04T08:00:00Z".to_string(),
            project_status: None,
            project_start_date: None,
            project_target_date: None,
            project_priority: None,
            author: Some("charlie".to_string()),
            pr_additions: Some(500),
            pr_deletions: Some(120),
        },
        db::IssueRow {
            repo: "org/backend".to_string(),
            number: 55,
            title: "Rate-limit bug".to_string(),
            body: Some("Rate limiter off by one.".to_string()),
            state: Some("OPEN".to_string()),
            kind: "issue".to_string(),
            labels: r#"["bug"]"#.to_string(),
            assignees: r#"["charlie"]"#.to_string(),
            created_at: "2026-03-03T08:00:00Z".to_string(),
            updated_at: "2026-03-05T08:00:00Z".to_string(),
            project_status: None,
            project_start_date: None,
            project_target_date: None,
            project_priority: None,
            author: Some("charlie".to_string()),
            pr_additions: None,
            pr_deletions: None,
        },
    ];
    db::upsert_issues(&conn, &issues).unwrap();

    let config: Config = toml::from_str(
        r#"
        [[repos]]
        name = "org/frontend"

        [[repos]]
        name = "org/backend"

        [[team]]
        github = "alice"
        name = "Alice Smith"
        role = "Frontend Lead"

        [[team]]
        github = "bob"
        name = "Bob Jones"
        role = "Frontend"

        [[team]]
        github = "charlie"
        name = "Charlie Lee"
        role = "Backend"
    "#,
    )
    .unwrap();

    (dir, conn, config)
}

#[tokio::test]
async fn slack_webhook_blocks_snapshot() {
    let (_dir, conn, config) = seed_test_db();
    let since = "2026-03-01T00:00:00Z";
    let report = run_pipeline(
        &config,
        &conn,
        &MockAgent,
        since,
        "2026-03-06",
        &NullProgress,
        Some("executive"),
    )
    .await
    .unwrap();

    let json = slack::dry_run(&report, config.slack.as_ref());
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();

    insta::assert_json_snapshot!("slack_webhook_blocks", value);
}
