use ceo::prompt::{Prompt, IssueDescriptionPrompt, DiscussionSummaryPrompt, WeeklySummaryPrompt, IssueTriagePrompt};

#[test]
fn weekly_summary_prompt_renders_with_repo_and_issues() {
    let prompt = WeeklySummaryPrompt {
        repo: "org/frontend".to_string(),
        issue_summaries: "- #1 Fix bug\n- #2 Add feature".to_string(),
        commit_log: String::new(),
        previous_summary: None,
        initiatives: String::new(),
    };
    let rendered = prompt.render();
    assert!(rendered.contains("org/frontend"));
    assert!(rendered.contains("Fix bug"));
    assert!(rendered.contains("Add feature"));
    assert!(rendered.contains("<done>"));
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
fn issue_description_prompt_renders_without_comments() {
    let prompt = IssueDescriptionPrompt {
        repo: "org/frontend".to_string(),
        number: 42,
        title: "Add dark mode".to_string(),
        kind: "issue".to_string(),
        labels: "feature, ui".to_string(),
        assignees: "alice".to_string(),
        body: "We need dark mode support.".to_string(),
    };
    let rendered = prompt.render();
    assert!(rendered.contains("org/frontend"));
    assert!(rendered.contains("#42"));
    assert!(rendered.contains("Add dark mode"));
    assert!(rendered.contains("dark mode support"));
    // Should not include any actual comment content (only the issue body)
    assert!(!rendered.contains("help with CSS"), "description prompt should not include comment content");
    assert_eq!(prompt.kind(), "summary");
}

#[test]
fn discussion_summary_prompt_renders_with_comments() {
    let prompt = DiscussionSummaryPrompt {
        repo: "org/frontend".to_string(),
        number: 42,
        title: "Add dark mode".to_string(),
        comments: "bob: I can help with CSS.".to_string(),
        previous_summary: None,
    };
    let rendered = prompt.render();
    assert!(rendered.contains("#42"));
    assert!(rendered.contains("help with CSS"));
    assert_eq!(prompt.kind(), "summary");
}

#[test]
fn discussion_summary_prompt_includes_previous_when_updating() {
    let prompt = DiscussionSummaryPrompt {
        repo: "org/frontend".to_string(),
        number: 42,
        title: "Add dark mode".to_string(),
        comments: "bob: Updated the PR.".to_string(),
        previous_summary: Some("Bob offered to help with CSS.".to_string()),
    };
    let rendered = prompt.render();
    assert!(rendered.contains("Bob offered to help with CSS."));
    assert!(rendered.contains("Updated the PR"));
    assert!(rendered.contains("Update it with any new information"));
}
