use std::fmt::Write;

/// Format a GitHub handle as a clickable markdown link.
pub fn github_link(handle: &str) -> String {
    format!("[@{handle}](https://github.com/{handle})")
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
}

pub fn render_markdown(report: &Report) -> String {
    let mut out = String::new();
    writeln!(out, "# Project Report — {}\n", report.date).unwrap();

    // Split repos into active and inactive
    let active: Vec<&RepoSection> = report.repos.iter()
        .filter(|r| r.has_activity())
        .collect();
    let inactive: Vec<&RepoSection> = report.repos.iter()
        .filter(|r| !r.has_activity())
        .collect();

    for repo in &active {
        writeln!(out, "## {}\n", repo.name).unwrap();
        if let Some(done) = &repo.done {
            writeln!(out, "**Done:** {}\n", done).unwrap();
        }
        if let Some(ip) = &repo.in_progress {
            writeln!(out, "**In Progress:** {}\n", ip).unwrap();
        }
        if let Some(next) = &repo.next {
            writeln!(out, "**Next:** {}\n", next).unwrap();
        }

        if !repo.flagged_issues.is_empty() {
            writeln!(out, "**Needs Attention:**\n").unwrap();
            for issue in &repo.flagged_issues {
                let missing = issue.missing_labels.join(", ");
                writeln!(
                    out,
                    "- **#{}** \"{}\" — missing {} label. *{}*",
                    issue.number, issue.title, missing, issue.summary
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
            .filter(|m| m.active > 0 || m.closed_this_week > 0)
            .collect();
        let inactive_members: Vec<&TeamStats> = report.team_stats.iter()
            .filter(|m| m.active == 0 && m.closed_this_week == 0)
            .collect();

        writeln!(out, "## Team Overview\n").unwrap();
        writeln!(out, "| Person | Active | Closed |").unwrap();
        writeln!(out, "|--------|--------|--------|").unwrap();
        for member in &active_members {
            writeln!(
                out,
                "| {} ({}) | {} | {} |",
                member.name, github_link(&member.github), member.active, member.closed_this_week
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
