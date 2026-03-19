use ceo::report::{Report, RepoSection, FlaggedIssue, TeamStats, RefLookup, render_markdown, extract_xml_tag, expand_github_tags};

#[test]
fn render_report_contains_header() {
    let report = Report {
        executive_summary: None,
        date: "2026-03-06".to_string(),
        repos: vec![],
        team_stats: vec![],
        refs: RefLookup::default(),
    };
    let md = render_markdown(&report);
    assert!(md.contains("# Project Report — 2026-03-06"));
}

#[test]
fn render_report_with_repo_section() {
    let report = Report {
        executive_summary: None,
        date: "2026-03-06".to_string(),
        repos: vec![RepoSection {
            name: "org/frontend".to_string(),
            done: Some("Fixed 3 bugs. Migrated to new auth.".to_string()),
            in_progress: Some("Working on dark mode.".to_string()),
            next: None,
            flagged_issues: vec![FlaggedIssue {
                number: 42,
                title: "Fix login redirect".to_string(),
                missing_labels: vec!["priority".to_string()],
                summary: "Issue about SSO redirect loop.".to_string(),
            }],
        }],
        team_stats: vec![TeamStats {
            name: "Alice Smith".to_string(),
            github: "alice".to_string(),
            active: 5,
            closed_this_week: 2,
            additions: 0,
            deletions: 0,
        }],
        refs: RefLookup::default(),
    };
    let md = render_markdown(&report);
    assert!(md.contains("## org/frontend"));
    assert!(md.contains("**Done:** Fixed 3 bugs."));
    assert!(md.contains("**In Progress:** Working on dark mode."));
    assert!(!md.contains("**Next:**"));
    assert!(md.contains("#42"));
    assert!(md.contains("missing priority label"));
    assert!(md.contains("Alice Smith"));
    assert!(md.contains("| 5"));
}

#[test]
fn render_report_no_flagged_issues_omits_section() {
    let report = Report {
        executive_summary: None,
        date: "2026-03-06".to_string(),
        repos: vec![RepoSection {
            name: "org/backend".to_string(),
            done: Some("All good.".to_string()),
            in_progress: None,
            next: None,
            flagged_issues: vec![],
        }],
        team_stats: vec![],
        refs: RefLookup::default(),
    };
    let md = render_markdown(&report);
    assert!(!md.contains("Needs Attention"));
}

#[test]
fn render_report_inactive_repos_as_compact_list() {
    let report = Report {
        executive_summary: None,
        date: "2026-03-06".to_string(),
        repos: vec![
            RepoSection { name: "org/active".to_string(), done: Some("Some work done.".to_string()), in_progress: None, next: None, flagged_issues: vec![] },
            RepoSection { name: "org/idle-1".to_string(), done: None, in_progress: None, next: None, flagged_issues: vec![] },
            RepoSection { name: "org/idle-2".to_string(), done: None, in_progress: None, next: None, flagged_issues: vec![] },
        ],
        team_stats: vec![],
        refs: RefLookup::default(),
    };
    let md = render_markdown(&report);
    assert!(md.contains("## org/active"));
    assert!(md.contains("Some work done."));
    assert!(md.contains("## No Recent Activity"));
    assert!(md.contains("- org/idle-1"));
    assert!(md.contains("- org/idle-2"));
    assert!(!md.contains("## org/idle-1"));
}

#[test]
fn render_report_inactive_team_members_listed_separately() {
    let report = Report {
        executive_summary: None,
        date: "2026-03-06".to_string(),
        repos: vec![],
        team_stats: vec![
            TeamStats { name: "Alice".to_string(), github: "alice".to_string(), active: 3, closed_this_week: 1, additions: 0, deletions: 0 },
            TeamStats { name: "Bob".to_string(), github: "bob".to_string(), active: 0, closed_this_week: 0, additions: 0, deletions: 0 },
        ],
        refs: RefLookup::default(),
    };
    let md = render_markdown(&report);
    assert!(md.contains("Alice"));
    assert!(md.contains("| 3 |"));
    assert!(md.contains("No activity:"));
    assert!(md.contains("Bob"));
}

#[test]
fn extract_xml_tag_parses_valid_tags() {
    let text = "<done>Fixed auth bug.</done>\n<in_progress>Working on dark mode.</in_progress>";
    assert_eq!(extract_xml_tag(text, "done"), Some("Fixed auth bug.".to_string()));
    assert_eq!(extract_xml_tag(text, "in_progress"), Some("Working on dark mode.".to_string()));
    assert_eq!(extract_xml_tag(text, "next"), None);
}

#[test]
fn extract_xml_tag_trims_whitespace() {
    let text = "<done>\n  Built new feature.\n</done>";
    assert_eq!(extract_xml_tag(text, "done"), Some("Built new feature.".to_string()));
}

#[test]
fn extract_xml_tag_returns_none_for_empty_content() {
    let text = "<done></done>";
    assert_eq!(extract_xml_tag(text, "done"), None);
}

#[test]
fn has_activity_detects_active_and_inactive() {
    let active = RepoSection { name: "r".to_string(), done: Some("x".to_string()), in_progress: None, next: None, flagged_issues: vec![] };
    let inactive = RepoSection { name: "r".to_string(), done: None, in_progress: None, next: None, flagged_issues: vec![] };
    assert!(active.has_activity());
    assert!(!inactive.has_activity());
}

#[test]
fn render_report_team_stats_includes_lines() {
    let report = Report {
        executive_summary: None,
        date: "2026-03-10".to_string(),
        repos: vec![],
        team_stats: vec![TeamStats {
            name: "Alice".to_string(), github: "alice".to_string(),
            active: 3, closed_this_week: 1, additions: 500, deletions: 120,
        }],
        refs: RefLookup::default(),
    };
    let md = render_markdown(&report);
    assert!(md.contains("+500"));
    assert!(md.contains("-120"));
}

#[test]
fn render_report_inactive_member_with_lines_shown_as_active() {
    let report = Report {
        executive_summary: None,
        date: "2026-03-10".to_string(),
        repos: vec![],
        team_stats: vec![TeamStats {
            name: "Carol".to_string(), github: "carol".to_string(),
            active: 0, closed_this_week: 0, additions: 200, deletions: 50,
        }],
        refs: RefLookup::default(),
    };
    let md = render_markdown(&report);
    assert!(md.contains("Carol"));
    assert!(md.contains("+200"));
    assert!(!md.contains("No activity:"));
}

#[test]
fn expand_github_tags_replaces_all_tag_types() {
    let text = "Fixed by <gh>alice</gh> in <pr>42</pr>, see <issue>10</issue> for context.";
    let result = expand_github_tags(text, "org/repo");
    assert_eq!(
        result,
        "Fixed by [@alice](https://github.com/alice) in [#42](https://github.com/org/repo/pull/42), see [#10](https://github.com/org/repo/issues/10) for context."
    );
}

#[test]
fn expand_github_tags_handles_no_tags() {
    let text = "No tags here.";
    assert_eq!(expand_github_tags(text, "org/repo"), "No tags here.");
}

#[test]
fn expand_github_tags_in_rendered_report() {
    // Note: render_markdown no longer expands tags (linkify does it in the pipeline),
    // but expand_github_tags itself should still work for backward compat with cached data.
    let text = "Merged <pr>42</pr> by <gh>alice</gh>.";
    let result = expand_github_tags(text, "org/repo");
    assert!(result.contains("[#42](https://github.com/org/repo/pull/42)"));
    assert!(result.contains("[@alice](https://github.com/alice)"));
    assert!(!result.contains("<pr>"));
    assert!(!result.contains("<gh>"));
}
