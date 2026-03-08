use ceo::prompt::{Prompt, IssueSummaryPrompt, WeeklySummaryPrompt, IssueTriagePrompt};

#[test]
fn weekly_summary_prompt_renders_with_repo_and_issues() {
    let prompt = WeeklySummaryPrompt {
        repo: "org/frontend".to_string(),
        issue_summaries: "- #1 Fix bug\n- #2 Add feature".to_string(),
    };
    let rendered = prompt.render();
    assert!(rendered.contains("org/frontend"));
    assert!(rendered.contains("Fix bug"));
    assert!(rendered.contains("Add feature"));
    assert!(rendered.contains("Key progress"));
}

#[test]
fn issue_triage_prompt_renders_with_all_fields() {
    let prompt = IssueTriagePrompt {
        title: "Fix login redirect".to_string(),
        body: "The login page redirects in a loop.".to_string(),
        comments: "alice: I think it's SSO.".to_string(),
    };
    let rendered = prompt.render();
    assert!(rendered.contains("Fix login redirect"));
    assert!(rendered.contains("login page redirects"));
    assert!(rendered.contains("SSO"));
    assert!(rendered.contains("labels"));
}

#[test]
fn issue_summary_prompt_renders_with_all_fields() {
    let prompt = IssueSummaryPrompt {
        repo: "org/frontend".to_string(),
        number: 42,
        title: "Add dark mode".to_string(),
        labels: "feature, ui".to_string(),
        assignees: "alice".to_string(),
        body: "We need dark mode support.".to_string(),
        comments: "bob: I can help with CSS.".to_string(),
    };
    let rendered = prompt.render();
    assert!(rendered.contains("org/frontend"));
    assert!(rendered.contains("#42"));
    assert!(rendered.contains("Add dark mode"));
    assert!(rendered.contains("dark mode support"));
    assert!(rendered.contains("help with CSS"));
    assert_eq!(prompt.kind(), "summary");
}
