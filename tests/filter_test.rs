use ceo::filter::{filter_recent, group_by_repo, group_by_assignee, find_flagged_issues};
use ceo::github::Issue;
use chrono::{Utc, Duration};

fn make_issue(number: u64, repo: &str, assignees: Vec<&str>, labels: Vec<&str>, days_ago: i64) -> Issue {
    Issue {
        number,
        title: format!("Issue #{number}"),
        kind: "issue".to_string(),
        state: "OPEN".to_string(),
        labels: labels.into_iter().map(String::from).collect(),
        assignees: assignees.into_iter().map(String::from).collect(),
        updated_at: Utc::now() - Duration::days(days_ago),
        created_at: Utc::now() - Duration::days(days_ago + 10),
        repo: repo.to_string(),
    }
}

#[test]
fn filter_recent_issues() {
    let issues = vec![
        make_issue(1, "org/repo", vec!["alice"], vec![], 2),
        make_issue(2, "org/repo", vec!["bob"], vec![], 10),
        make_issue(3, "org/repo", vec![], vec![], 6),
    ];
    let recent = filter_recent(&issues, 7);
    assert_eq!(recent.len(), 2);
    assert_eq!(recent[0].number, 1);
    assert_eq!(recent[1].number, 3);
}

#[test]
fn group_issues_by_repo() {
    let issues = vec![
        make_issue(1, "org/frontend", vec![], vec![], 1),
        make_issue(2, "org/backend", vec![], vec![], 1),
        make_issue(3, "org/frontend", vec![], vec![], 1),
    ];
    let refs: Vec<&Issue> = issues.iter().collect();
    let grouped = group_by_repo(&refs);
    assert_eq!(grouped.len(), 2);
    assert_eq!(grouped["org/frontend"].len(), 2);
    assert_eq!(grouped["org/backend"].len(), 1);
}

#[test]
fn group_issues_by_assignee() {
    let issues = vec![
        make_issue(1, "org/repo", vec!["alice"], vec![], 1),
        make_issue(2, "org/repo", vec!["bob"], vec![], 1),
        make_issue(3, "org/repo", vec!["alice"], vec![], 1),
        make_issue(4, "org/repo", vec![], vec![], 1),
    ];
    let refs: Vec<&Issue> = issues.iter().collect();
    let grouped = group_by_assignee(&refs);
    assert_eq!(grouped["alice"].len(), 2);
    assert_eq!(grouped["bob"].len(), 1);
    assert_eq!(grouped["unassigned"].len(), 1);
}

#[test]
fn find_issues_missing_required_labels() {
    let issues = vec![
        make_issue(1, "org/repo", vec![], vec!["priority", "bug"], 1),
        make_issue(2, "org/repo", vec![], vec!["bug"], 1),
        make_issue(3, "org/repo", vec![], vec![], 1),
    ];
    let refs: Vec<&Issue> = issues.iter().collect();
    let required = vec!["priority".to_string()];
    let flagged = find_flagged_issues(&refs, &required);
    assert_eq!(flagged.len(), 2);
    assert_eq!(flagged[0].number, 2);
    assert_eq!(flagged[1].number, 3);
}
