use ceo::roadmap::{Initiative, Roadmap};

#[test]
fn parse_roadmap_from_toml() {
    let toml_str = r#"
        [[initiatives]]
        name = "Platform v2"
        timeframe = "Q1 2026"
        repos = ["acme-corp/platform"]
        description = "Complete API refactor"

        [[initiatives]]
        name = "Production readiness"
        repos = ["acme-corp/platform", "acme-corp/webapp"]
        description = "Full test coverage"
    "#;
    let roadmap: Roadmap = toml::from_str(toml_str).unwrap();
    assert_eq!(roadmap.initiatives.len(), 2);
    assert_eq!(roadmap.initiatives[0].name, "Platform v2");
    assert_eq!(roadmap.initiatives[0].timeframe.as_deref(), Some("Q1 2026"));
    assert_eq!(roadmap.initiatives[1].timeframe, None);
}

#[test]
fn for_repo_filters_by_repo_name() {
    let roadmap = Roadmap {
        initiatives: vec![
            Initiative {
                name: "A".to_string(),
                timeframe: None,
                repos: vec!["org/frontend".to_string()],
                description: "Frontend work".to_string(),
            },
            Initiative {
                name: "B".to_string(),
                timeframe: None,
                repos: vec!["org/backend".to_string()],
                description: "Backend work".to_string(),
            },
            Initiative {
                name: "C".to_string(),
                timeframe: None,
                repos: vec!["org/frontend".to_string(), "org/backend".to_string()],
                description: "Cross-cutting".to_string(),
            },
        ],
    };
    let frontend = roadmap.for_repo("org/frontend");
    assert_eq!(frontend.len(), 2);
    assert_eq!(frontend[0].name, "A");
    assert_eq!(frontend[1].name, "C");

    let backend = roadmap.for_repo("org/backend");
    assert_eq!(backend.len(), 2);

    let other = roadmap.for_repo("org/other");
    assert_eq!(other.len(), 0);
}

#[test]
fn add_and_remove_initiatives() {
    let mut roadmap = Roadmap::default();
    roadmap.add(Initiative {
        name: "Test".to_string(),
        timeframe: Some("Q1".to_string()),
        repos: vec!["org/repo".to_string()],
        description: "Testing".to_string(),
    }).unwrap();
    assert_eq!(roadmap.initiatives.len(), 1);

    // Duplicate name fails
    let err = roadmap.add(Initiative {
        name: "Test".to_string(),
        timeframe: None,
        repos: vec![],
        description: "Dup".to_string(),
    });
    assert!(err.is_err());

    // Remove works
    roadmap.remove("Test").unwrap();
    assert_eq!(roadmap.initiatives.len(), 0);

    // Remove non-existent fails
    assert!(roadmap.remove("Nope").is_err());
}

#[test]
fn empty_roadmap_parses_as_default() {
    let roadmap: Roadmap = toml::from_str("").unwrap();
    assert!(roadmap.initiatives.is_empty());
}

#[test]
fn prompt_includes_initiatives_when_present() {
    use ceo::prompt::{Prompt, WeeklySummaryPrompt};
    let prompt = WeeklySummaryPrompt {
        repo: "org/frontend".to_string(),
        issue_summaries: "- #1 Fix bug".to_string(),
        commit_log: String::new(),
        previous_summary: None,
        initiatives: "- Platform v2 (Q1 2026): Complete API refactor".to_string(),
    };
    let rendered = prompt.render();
    assert!(rendered.contains("Platform v2"));
    assert!(rendered.contains("initiatives"));
}

#[test]
fn prompt_omits_initiatives_section_when_empty() {
    use ceo::prompt::{Prompt, WeeklySummaryPrompt};
    let prompt = WeeklySummaryPrompt {
        repo: "org/frontend".to_string(),
        issue_summaries: "- #1 Fix bug".to_string(),
        commit_log: String::new(),
        previous_summary: None,
        initiatives: String::new(),
    };
    let rendered = prompt.render();
    assert!(!rendered.contains("initiatives"));
}
