use ceo::report::{Report, RepoSection, FlaggedIssue, TeamStats, render_markdown};

#[test]
fn render_report_contains_header() {
    let report = Report {
        date: "2026-03-06".to_string(),
        repos: vec![],
        team_stats: vec![],
    };
    let md = render_markdown(&report);
    assert!(md.contains("# Weekly Project Report — 2026-03-06"));
}

#[test]
fn render_report_with_repo_section() {
    let report = Report {
        date: "2026-03-06".to_string(),
        repos: vec![RepoSection {
            name: "org/frontend".to_string(),
            progress: "Fixed 3 bugs.".to_string(),
            big_updates: "Migrated to new auth.".to_string(),
            planned_next: "Start v2 redesign.".to_string(),
            flagged_issues: vec![FlaggedIssue {
                number: 42,
                title: "Fix login redirect".to_string(),
                missing_labels: vec!["priority".to_string()],
                summary: "Issue about SSO redirect loop.".to_string(),
            }],
        }],
        team_stats: vec![TeamStats {
            name: "Alice Smith".to_string(),
            active: 5,
            closed_this_week: 2,
        }],
    };
    let md = render_markdown(&report);
    assert!(md.contains("## org/frontend"));
    assert!(md.contains("Fixed 3 bugs."));
    assert!(md.contains("Migrated to new auth."));
    assert!(md.contains("#42"));
    assert!(md.contains("Missing priority label"));
    assert!(md.contains("Alice Smith"));
    assert!(md.contains("| 5"));
}

#[test]
fn render_report_no_flagged_issues_omits_section() {
    let report = Report {
        date: "2026-03-06".to_string(),
        repos: vec![RepoSection {
            name: "org/backend".to_string(),
            progress: "All good.".to_string(),
            big_updates: "Nothing major.".to_string(),
            planned_next: "Continue work.".to_string(),
            flagged_issues: vec![],
        }],
        team_stats: vec![],
    };
    let md = render_markdown(&report);
    assert!(!md.contains("Needs Attention"));
}
