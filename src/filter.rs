use crate::github::Issue;
use chrono::{Duration, Utc};
use std::collections::HashMap;

pub fn filter_recent(issues: &[Issue], days: i64) -> Vec<&Issue> {
    let cutoff = Utc::now() - Duration::days(days);
    issues.iter().filter(|i| i.updated_at >= cutoff).collect()
}

pub fn group_by_repo<'a>(issues: &[&'a Issue]) -> HashMap<String, Vec<&'a Issue>> {
    let mut map: HashMap<String, Vec<&Issue>> = HashMap::new();
    for issue in issues {
        map.entry(issue.repo.clone()).or_default().push(issue);
    }
    map
}

pub fn group_by_assignee<'a>(issues: &[&'a Issue]) -> HashMap<String, Vec<&'a Issue>> {
    let mut map: HashMap<String, Vec<&Issue>> = HashMap::new();
    for issue in issues {
        if issue.assignees.is_empty() {
            map.entry("unassigned".to_string()).or_default().push(issue);
        } else {
            for assignee in &issue.assignees {
                map.entry(assignee.clone()).or_default().push(issue);
            }
        }
    }
    map
}

pub fn find_flagged_issues<'a>(issues: &[&'a Issue], required_labels: &[String]) -> Vec<&'a Issue> {
    if required_labels.is_empty() {
        return vec![];
    }
    issues
        .iter()
        .filter(|i| !i.missing_labels(required_labels).is_empty())
        .copied()
        .collect()
}
