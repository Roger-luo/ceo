use std::fmt::Write;

pub struct Report {
    pub date: String,
    pub repos: Vec<RepoSection>,
    pub team_stats: Vec<TeamStats>,
}

pub struct RepoSection {
    pub name: String,
    pub progress: String,
    pub big_updates: String,
    pub planned_next: String,
    pub flagged_issues: Vec<FlaggedIssue>,
}

pub struct FlaggedIssue {
    pub number: u64,
    pub title: String,
    pub missing_labels: Vec<String>,
    pub summary: String,
}

pub struct TeamStats {
    pub name: String,
    pub active: usize,
    pub closed_this_week: usize,
}

pub fn render_markdown(report: &Report) -> String {
    let mut out = String::new();
    writeln!(out, "# Weekly Project Report — {}\n", report.date).unwrap();

    for repo in &report.repos {
        writeln!(out, "## {}\n", repo.name).unwrap();
        writeln!(out, "### Progress This Week\n").unwrap();
        writeln!(out, "{}\n", repo.progress).unwrap();
        writeln!(out, "### Big Updates\n").unwrap();
        writeln!(out, "{}\n", repo.big_updates).unwrap();
        writeln!(out, "### Planned Next\n").unwrap();
        writeln!(out, "{}\n", repo.planned_next).unwrap();

        if !repo.flagged_issues.is_empty() {
            writeln!(out, "### Needs Attention\n").unwrap();
            for issue in &repo.flagged_issues {
                let missing = issue.missing_labels.join(", ");
                writeln!(
                    out,
                    "- **#{}**: \"{}\" — Missing {} label. *{}*\n",
                    issue.number, issue.title, missing, issue.summary
                )
                .unwrap();
            }
        }
    }

    if !report.team_stats.is_empty() {
        writeln!(out, "## Team Overview\n").unwrap();
        writeln!(out, "| Person | Issues Active | Issues Closed This Week |").unwrap();
        writeln!(out, "|--------|--------------|------------------------|").unwrap();
        for member in &report.team_stats {
            writeln!(
                out,
                "| {} | {} | {} |",
                member.name, member.active, member.closed_this_week
            )
            .unwrap();
        }
        writeln!(out).unwrap();
    }

    out
}
