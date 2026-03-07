use ceo::prompt::{Prompt, WeeklySummaryPrompt, IssueTriagePrompt};

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
