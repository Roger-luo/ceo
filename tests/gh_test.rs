use ceo::error::GhError;
use ceo::gh::GhRunner;

struct MockGhRunner {
    issue_list_json: String,
    issue_view_json: String,
}

impl MockGhRunner {
    fn new(list_json: &str, view_json: &str) -> Self {
        Self {
            issue_list_json: list_json.to_string(),
            issue_view_json: view_json.to_string(),
        }
    }
}

impl GhRunner for MockGhRunner {
    fn run_gh(&self, args: &[&str]) -> Result<String, GhError> {
        if args.iter().any(|a| *a == "list") {
            Ok(self.issue_list_json.clone())
        } else {
            Ok(self.issue_view_json.clone())
        }
    }
}

#[test]
fn fetch_issues_from_mock() {
    let list_json = r#"[{
        "number": 1,
        "title": "Test issue",
        "labels": [],
        "assignees": [],
        "updatedAt": "2026-03-05T10:00:00Z",
        "createdAt": "2026-03-01T10:00:00Z"
    }]"#;

    let runner = MockGhRunner::new(list_json, "{}");
    let issues = ceo::gh::fetch_issues(&runner, "org/repo").unwrap();
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].number, 1);
    assert_eq!(issues[0].repo, "org/repo");
}

#[test]
fn fetch_issue_detail_from_mock() {
    let view_json = r#"{
        "body": "Description here",
        "comments": []
    }"#;

    let runner = MockGhRunner::new("[]", view_json);
    let detail = ceo::gh::fetch_issue_detail(&runner, "org/repo", 42).unwrap();
    assert_eq!(detail.body, "Description here");
}
