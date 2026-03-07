use ceo::agent::Agent;
use ceo::config::Config;
use ceo::gh::GhRunner;
use ceo::pipeline::run_pipeline;
use ceo::prompt::Prompt;
use ceo::report::render_markdown;

struct MockGh;

impl GhRunner for MockGh {
    fn run_gh(&self, args: &[&str]) -> anyhow::Result<String> {
        if args.iter().any(|a| *a == "list") {
            Ok(r#"[
                {
                    "number": 10,
                    "title": "Add dark mode",
                    "labels": [{"name": "feature"}, {"name": "priority"}],
                    "assignees": [{"login": "alice"}],
                    "updatedAt": "2026-03-05T10:00:00Z",
                    "createdAt": "2026-02-25T10:00:00Z"
                },
                {
                    "number": 11,
                    "title": "Fix memory leak",
                    "labels": [{"name": "bug"}],
                    "assignees": [{"login": "bob"}],
                    "updatedAt": "2026-03-04T10:00:00Z",
                    "createdAt": "2026-02-28T10:00:00Z"
                },
                {
                    "number": 12,
                    "title": "Update docs",
                    "labels": [],
                    "assignees": [],
                    "updatedAt": "2026-03-03T10:00:00Z",
                    "createdAt": "2026-03-01T10:00:00Z"
                }
            ]"#.to_string())
        } else {
            Ok(r#"{
                "body": "This issue needs triage.",
                "comments": [
                    {"author": {"login": "bob"}, "body": "I'll look into this.", "createdAt": "2026-03-03T12:00:00Z"}
                ]
            }"#.to_string())
        }
    }
}

struct MockAgent;

impl Agent for MockAgent {
    fn invoke(&self, prompt: &dyn Prompt) -> anyhow::Result<String> {
        let rendered = prompt.render();
        if rendered.contains("Summarize the past week") {
            Ok("Great progress on dark mode. Memory leak identified and being fixed.".to_string())
        } else {
            Ok("This issue is about updating documentation. Suggest adding priority:low label.".to_string())
        }
    }
}

#[test]
fn full_pipeline_produces_valid_markdown() {
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

    let report = run_pipeline(&config, &MockGh, &MockAgent, 7).unwrap();
    let markdown = render_markdown(&report);

    assert!(markdown.contains("org/frontend"));
    assert!(markdown.contains("Great progress on dark mode"));
    assert!(markdown.contains("Needs Attention"));
    assert!(markdown.contains("#11") || markdown.contains("#12"));
    assert!(markdown.contains("Alice Smith"));
    assert!(markdown.contains("Bob Jones"));
}
