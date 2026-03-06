use ceo::agent::AgentRunner;
use ceo::config::Config;
use ceo::gh::GhRunner;
use ceo::pipeline::run_pipeline;

struct MockGh;

impl GhRunner for MockGh {
    fn run_gh(&self, args: &[&str]) -> anyhow::Result<String> {
        if args.iter().any(|a| *a == "list") {
            Ok(r#"[{
                "number": 1,
                "title": "Implement auth",
                "labels": [{"name": "feature"}],
                "assignees": [{"login": "alice"}],
                "updatedAt": "2026-03-05T10:00:00Z",
                "createdAt": "2026-03-01T10:00:00Z"
            }, {
                "number": 2,
                "title": "Fix CSS bug",
                "labels": [],
                "assignees": [],
                "updatedAt": "2026-03-04T10:00:00Z",
                "createdAt": "2026-02-28T10:00:00Z"
            }]"#
            .to_string())
        } else {
            Ok(r#"{
                "body": "This issue is about fixing CSS.",
                "comments": []
            }"#
            .to_string())
        }
    }
}

struct MockAgent;

impl AgentRunner for MockAgent {
    fn invoke(&self, _prompt: &str) -> anyhow::Result<String> {
        Ok("Mock agent summary.".to_string())
    }
}

#[test]
fn pipeline_produces_report() {
    let config: Config = toml::from_str(
        r#"
        [[repos]]
        name = "org/frontend"
        labels_required = ["priority"]

        [[team]]
        github = "alice"
        name = "Alice Smith"
        role = "Lead"
    "#,
    )
    .unwrap();

    let report = run_pipeline(&config, &MockGh, &MockAgent, 7).unwrap();
    assert_eq!(report.repos.len(), 1);
    assert_eq!(report.repos[0].name, "org/frontend");
    assert!(report.repos[0].progress.contains("Mock agent summary"));
    assert!(!report.repos[0].flagged_issues.is_empty());
    // alice should have 1 active issue
    assert_eq!(report.team_stats.len(), 1);
    assert_eq!(report.team_stats[0].active, 1);
}
