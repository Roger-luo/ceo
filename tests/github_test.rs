use ceo::github::{Issue, IssueDetail};
use chrono::Utc;

#[test]
fn parse_gh_issue_list_json() {
    let json = r#"[
        {
            "number": 42,
            "title": "Fix login redirect",
            "labels": [{"name": "bug"}, {"name": "priority"}],
            "assignees": [{"login": "alice"}],
            "updatedAt": "2026-03-04T10:00:00Z",
            "createdAt": "2026-02-20T08:00:00Z"
        },
        {
            "number": 58,
            "title": "Refactor auth module",
            "labels": [],
            "assignees": [],
            "updatedAt": "2026-03-05T14:00:00Z",
            "createdAt": "2026-03-01T09:00:00Z"
        }
    ]"#;

    let issues = Issue::parse_gh_list(json, "org/frontend").unwrap();
    assert_eq!(issues.len(), 2);
    assert_eq!(issues[0].number, 42);
    assert_eq!(issues[0].title, "Fix login redirect");
    assert_eq!(issues[0].labels, vec!["bug", "priority"]);
    assert_eq!(issues[0].assignees, vec!["alice"]);
    assert_eq!(issues[0].repo, "org/frontend");
    assert_eq!(issues[1].assignees, Vec::<String>::new());
}

#[test]
fn parse_gh_issue_detail_json() {
    let json = r#"{
        "body": "This issue is about the login redirect loop.",
        "comments": [
            {
                "author": {"login": "alice"},
                "body": "I think this is caused by the SSO config.",
                "createdAt": "2026-03-03T12:00:00Z"
            }
        ]
    }"#;

    let detail = IssueDetail::parse_gh_view(json).unwrap();
    assert_eq!(detail.body, "This issue is about the login redirect loop.");
    assert_eq!(detail.comments.len(), 1);
    assert_eq!(detail.comments[0].author, "alice");
}

#[test]
fn issue_missing_required_labels() {
    let issue = Issue {
        number: 1,
        title: "Test".into(),
        labels: vec!["bug".into()],
        assignees: vec![],
        updated_at: Utc::now(),
        created_at: Utc::now(),
        repo: "org/repo".into(),
    };
    let required = &["priority".to_string()];
    assert!(issue.missing_labels(required).contains(&"priority".to_string()));

    let required2 = &["bug".to_string()];
    assert!(issue.missing_labels(required2).is_empty());
}
