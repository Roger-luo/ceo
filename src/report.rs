use std::fmt::Write;

/// Format a GitHub handle as a clickable markdown link.
pub fn github_link(handle: &str) -> String {
    format!("[@{handle}](https://github.com/{handle})")
}

/// Expand short GitHub reference tags in LLM output to full markdown links.
///
/// Tags:
/// - `<gh>handle</gh>` → `[@handle](https://github.com/handle)`
/// - `<issue>64</issue>` → `[#64](https://github.com/{repo}/issues/64)`
/// - `<issue>org/repo#64</issue>` → `[org/repo#64](https://github.com/org/repo/issues/64)`
/// - `<pr>32</pr>` → `[#32](https://github.com/{repo}/pull/32)`
/// - `<pr>org/repo#32</pr>` → `[org/repo#32](https://github.com/org/repo/pull/32)`
///
/// Qualified references (`owner/repo#N`) override the `repo` parameter.
pub fn expand_github_tags(text: &str, repo: &str) -> String {
    let mut result = text.to_string();
    // Process each tag type by scanning for open/close pairs
    for (open_tag, close_tag, fmt_fn) in [
        ("<gh>", "</gh>", Box::new(|inner: &str, _repo: &str| {
            format!("[@{inner}](https://github.com/{inner})")
        }) as Box<dyn Fn(&str, &str) -> String>),
        ("<issue>", "</issue>", Box::new(|inner: &str, repo: &str| {
            format_issue_or_pr_link(inner, repo, "issues")
        }) as Box<dyn Fn(&str, &str) -> String>),
        ("<pr>", "</pr>", Box::new(|inner: &str, repo: &str| {
            format_issue_or_pr_link(inner, repo, "pull")
        }) as Box<dyn Fn(&str, &str) -> String>),
    ] {
        loop {
            let Some(start) = result.find(open_tag) else { break };
            let after_open = start + open_tag.len();
            let Some(end_offset) = result[after_open..].find(close_tag) else { break };
            let inner = result[after_open..after_open + end_offset].trim();
            let replacement = fmt_fn(inner, repo);
            result.replace_range(start..after_open + end_offset + close_tag.len(), &replacement);
        }
    }
    result
}

/// Format a qualified or unqualified issue/PR reference as a markdown link.
///
/// - `"64"` + repo `"org/repo"` → `[#64](https://github.com/org/repo/issues/64)`
/// - `"org/repo#64"` (any repo) → `[org/repo#64](https://github.com/org/repo/issues/64)`
fn format_issue_or_pr_link(inner: &str, fallback_repo: &str, path: &str) -> String {
    if let Some((qualified_repo, number)) = inner.split_once('#') {
        if !qualified_repo.is_empty() && qualified_repo.contains('/') {
            return format!("[{inner}](https://github.com/{qualified_repo}/{path}/{number})");
        }
    }
    // Unqualified: just a number
    if fallback_repo.is_empty() {
        format!("#{inner}")
    } else {
        format!("[#{inner}](https://github.com/{fallback_repo}/{path}/{inner})")
    }
}

/// Extract all `<summary id="N">...</summary>` tags from a batch response.
/// Returns a vec of (issue_number, summary_text) tuples.
pub fn extract_all_summary_tags(text: &str) -> Vec<(u64, String)> {
    let mut results = Vec::new();
    let mut search_from = 0;
    let open_prefix = "<summary id=\"";

    while let Some(tag_start) = text[search_from..].find(open_prefix) {
        let abs_start = search_from + tag_start;
        let after_prefix = abs_start + open_prefix.len();

        let Some(quote_end) = text[after_prefix..].find('"') else { break };
        let id_str = &text[after_prefix..after_prefix + quote_end];
        let Ok(id) = id_str.parse::<u64>() else {
            search_from = after_prefix;
            continue;
        };

        let after_id = after_prefix + quote_end + 1;
        let Some(gt_offset) = text[after_id..].find('>') else { break };
        let content_start = after_id + gt_offset + 1;

        let close_tag = "</summary>";
        let Some(close_offset) = text[content_start..].find(close_tag) else { break };
        let content = text[content_start..content_start + close_offset].trim();
        if !content.is_empty() {
            results.push((id, content.to_string()));
        }

        search_from = content_start + close_offset + close_tag.len();
    }

    results
}

/// Extract the text content of an XML tag from a string.
/// Returns `None` if the tag is not found.
pub fn extract_xml_tag(text: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = text.find(&open).map(|i| i + open.len())?;
    let end = text[start..].find(&close).map(|i| i + start)?;
    let content = text[start..end].trim();
    if content.is_empty() { None } else { Some(content.to_string()) }
}

pub struct Report {
    pub date: String,
    pub executive_summary: Option<String>,
    pub repos: Vec<RepoSection>,
    pub team_stats: Vec<TeamStats>,
}

pub struct RepoSection {
    pub name: String,
    pub done: Option<String>,
    pub in_progress: Option<String>,
    pub next: Option<String>,
    pub flagged_issues: Vec<FlaggedIssue>,
}

impl RepoSection {
    pub fn has_activity(&self) -> bool {
        self.done.is_some() || self.in_progress.is_some() || self.next.is_some()
    }
}

pub struct FlaggedIssue {
    pub number: u64,
    pub title: String,
    pub missing_labels: Vec<String>,
    pub summary: String,
}

pub struct TeamStats {
    pub name: String,
    pub github: String,
    pub active: usize,
    pub closed_this_week: usize,
    pub additions: i64,
    pub deletions: i64,
}

pub fn render_markdown(report: &Report) -> String {
    let mut out = String::new();
    writeln!(out, "# Project Report — {}\n", report.date).unwrap();

    // Executive summary (if generated) — expand <gh> tags only (no repo context)
    if let Some(summary) = &report.executive_summary {
        writeln!(out, "{}\n", expand_github_tags(summary, "")).unwrap();
        writeln!(out, "---\n").unwrap();
    }

    // Split repos into active and inactive
    let active: Vec<&RepoSection> = report.repos.iter()
        .filter(|r| r.has_activity())
        .collect();
    let inactive: Vec<&RepoSection> = report.repos.iter()
        .filter(|r| !r.has_activity())
        .collect();

    for repo in &active {
        let expand = |text: &str| expand_github_tags(text, &repo.name);
        writeln!(out, "## {}\n", repo.name).unwrap();
        if let Some(done) = &repo.done {
            writeln!(out, "**Done:** {}\n", expand(done)).unwrap();
        }
        if let Some(ip) = &repo.in_progress {
            writeln!(out, "**In Progress:** {}\n", expand(ip)).unwrap();
        }
        if let Some(next) = &repo.next {
            writeln!(out, "**Next:** {}\n", expand(next)).unwrap();
        }

        if !repo.flagged_issues.is_empty() {
            writeln!(out, "**Needs Attention:**\n").unwrap();
            for issue in &repo.flagged_issues {
                let missing = issue.missing_labels.join(", ");
                writeln!(
                    out,
                    "- **#{}** \"{}\" — missing {} label. *{}*",
                    issue.number, issue.title, missing, expand(&issue.summary)
                ).unwrap();
            }
            writeln!(out).unwrap();
        }
    }

    // Inactive repos as a compact list
    if !inactive.is_empty() {
        writeln!(out, "## No Recent Activity\n").unwrap();
        for repo in &inactive {
            writeln!(out, "- {}", repo.name).unwrap();
        }
        writeln!(out).unwrap();
    }

    if !report.team_stats.is_empty() {
        // Filter out team members with zero activity
        let active_members: Vec<&TeamStats> = report.team_stats.iter()
            .filter(|m| m.active > 0 || m.closed_this_week > 0 || m.additions > 0 || m.deletions > 0)
            .collect();
        let inactive_members: Vec<&TeamStats> = report.team_stats.iter()
            .filter(|m| m.active == 0 && m.closed_this_week == 0 && m.additions == 0 && m.deletions == 0)
            .collect();

        writeln!(out, "## Team Overview\n").unwrap();
        writeln!(out, "| Person | Active | Closed | Lines |").unwrap();
        writeln!(out, "|--------|--------|--------|-------|").unwrap();
        for member in &active_members {
            writeln!(
                out,
                "| {} ({}) | {} | {} | +{} / -{} |",
                member.name, github_link(&member.github),
                member.active, member.closed_this_week,
                member.additions, member.deletions
            ).unwrap();
        }
        if !inactive_members.is_empty() {
            let names: Vec<String> = inactive_members.iter()
                .map(|m| format!("{} ({})", m.name, github_link(&m.github)))
                .collect();
            writeln!(out, "\n*No activity:* {}", names.join(", ")).unwrap();
        }
        writeln!(out).unwrap();
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_unqualified_issue() {
        let result = expand_github_tags("<issue>64</issue>", "org/repo");
        assert_eq!(result, "[#64](https://github.com/org/repo/issues/64)");
    }

    #[test]
    fn expand_qualified_issue() {
        let result = expand_github_tags("<issue>acme/frontend#15</issue>", "");
        assert_eq!(result, "[acme/frontend#15](https://github.com/acme/frontend/issues/15)");
    }

    #[test]
    fn expand_qualified_pr() {
        let result = expand_github_tags("<pr>acme/backend#42</pr>", "other/repo");
        assert_eq!(result, "[acme/backend#42](https://github.com/acme/backend/pull/42)");
    }

    #[test]
    fn expand_gh_tag() {
        let result = expand_github_tags("<gh>alice</gh>", "");
        assert_eq!(result, "[@alice](https://github.com/alice)");
    }

    #[test]
    fn expand_unqualified_no_repo_falls_back() {
        // When no repo context, bare numbers render without link
        let result = expand_github_tags("<issue>99</issue>", "");
        assert_eq!(result, "#99");
    }

    #[test]
    fn expand_mixed_tags() {
        let text = "<gh>alice</gh> fixed <pr>org/repo#42</pr> and <issue>55</issue>";
        let result = expand_github_tags(text, "org/repo");
        assert!(result.contains("[@alice](https://github.com/alice)"));
        assert!(result.contains("[org/repo#42](https://github.com/org/repo/pull/42)"));
        assert!(result.contains("[#55](https://github.com/org/repo/issues/55)"));
    }
}
