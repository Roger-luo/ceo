use chrono::{Duration, Utc};
use log::{debug, info};

use crate::agent::Agent;
use crate::config::Config;
use crate::db::{self, IssueRow};
use crate::error::PipelineError;
use crate::filter;
use crate::github::Issue;
use crate::prompt::{IssueSummaryPrompt, IssueTriagePrompt, WeeklySummaryPrompt};
use crate::report::{FlaggedIssue, Report, RepoSection, TeamStats};

type Result<T> = std::result::Result<T, PipelineError>;

fn row_to_issue(row: &IssueRow) -> Issue {
    let labels: Vec<String> = serde_json::from_str(&row.labels).unwrap_or_default();
    let assignees: Vec<String> = serde_json::from_str(&row.assignees).unwrap_or_default();
    Issue {
        number: row.number,
        title: row.title.clone(),
        labels,
        assignees,
        updated_at: chrono::DateTime::parse_from_rfc3339(&row.updated_at)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now()),
        created_at: chrono::DateTime::parse_from_rfc3339(&row.created_at)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now()),
        repo: row.repo.clone(),
    }
}

pub fn run_pipeline(
    config: &Config,
    conn: &rusqlite::Connection,
    agent: &dyn Agent,
    days: i64,
) -> Result<Report> {
    let cutoff = (Utc::now() - Duration::days(days)).to_rfc3339();
    let mut repo_sections = Vec::new();
    let mut all_recent_issues = Vec::new();

    for repo_config in &config.repos {
        info!("Processing repo: {}", repo_config.name);
        let repo_names = vec![repo_config.name.clone()];
        let issue_rows = db::query_recent_issues(conn, &repo_names, &cutoff)?;
        let issues: Vec<Issue> = issue_rows.iter().map(row_to_issue).collect();
        debug!("Found {} recent issues (last {} days)", issues.len(), days);
        eprintln!("  {} has {} recent issues (last {} days)", repo_config.name, issues.len(), days);

        // Collect issue numbers for comment lookup
        let issue_numbers: Vec<u64> = issue_rows.iter().map(|r| r.number).collect();
        let comment_rows = db::query_comments_for_issues(conn, &repo_config.name, &issue_numbers)?;

        // Build a map of issue_number -> comments text
        let mut comments_by_issue: std::collections::HashMap<u64, Vec<String>> = std::collections::HashMap::new();
        for c in &comment_rows {
            comments_by_issue
                .entry(c.issue_number)
                .or_default()
                .push(format!("{}: {}", c.author, c.body));
        }

        // Build a map of issue_number -> body from the rows
        let body_by_issue: std::collections::HashMap<u64, String> = issue_rows
            .iter()
            .map(|r| (r.number, r.body.clone().unwrap_or_default()))
            .collect();

        for issue in &issues {
            all_recent_issues.push(issue.clone());
        }

        let issue_refs: Vec<&Issue> = issues.iter().collect();

        // Summarize each issue individually to keep context windows small
        let mut per_issue_summaries = Vec::new();
        for (i, issue) in issues.iter().enumerate() {
            eprintln!("  [{}/{}] Summarizing #{} {}...", i + 1, issues.len(), issue.number, issue.title);

            let body = body_by_issue.get(&issue.number).cloned().unwrap_or_default();
            let comments_text = comments_by_issue
                .get(&issue.number)
                .map(|v| v.join("\n"))
                .unwrap_or_default();

            let prompt = IssueSummaryPrompt {
                repo: repo_config.name.clone(),
                number: issue.number,
                title: issue.title.clone(),
                labels: issue.labels.join(", "),
                assignees: issue.assignees.join(", "),
                body,
                comments: comments_text,
            };

            let summary = match agent.invoke(&prompt) {
                Ok(s) => {
                    debug!("Issue #{} summary: {} chars", issue.number, s.len());
                    s
                }
                Err(e) => {
                    debug!("Issue #{} summary failed: {e}", issue.number);
                    format!("#{} {}: summary unavailable", issue.number, issue.title)
                }
            };

            per_issue_summaries.push(format!("- #{} {}: {}", issue.number, issue.title, summary));
        }

        // Aggregate per-issue summaries into a repo-level report
        let repo_summary = if per_issue_summaries.is_empty() {
            "No recent activity.".to_string()
        } else {
            eprintln!("  Generating repo summary...");
            let aggregated = per_issue_summaries.join("\n");
            let prompt = WeeklySummaryPrompt {
                repo: repo_config.name.clone(),
                issue_summaries: aggregated,
            };
            match agent.invoke(&prompt) {
                Ok(s) => {
                    debug!("Repo summary received ({} chars)", s.len());
                    s
                }
                Err(e) => {
                    debug!("Repo summary failed: {e}");
                    format!("Analysis unavailable: {e}")
                }
            }
        };

        // Triage flagged issues
        let flagged_refs = filter::find_flagged_issues(&issue_refs, &repo_config.labels_required);
        debug!("Found {} flagged issues in {}", flagged_refs.len(), repo_config.name);
        let mut flagged_issues = Vec::new();

        if !flagged_refs.is_empty() {
            eprintln!("  Triaging {} flagged issues...", flagged_refs.len());
        }

        for (i, issue) in flagged_refs.iter().enumerate() {
            eprintln!("    [{}/{}] #{} {}...", i + 1, flagged_refs.len(), issue.number, issue.title);
            debug!("Triaging issue #{}: {}", issue.number, issue.title);

            let body = body_by_issue.get(&issue.number).cloned().unwrap_or_default();
            let comments_text = comments_by_issue
                .get(&issue.number)
                .map(|v| v.join("\n"))
                .unwrap_or_default();

            let triage_prompt = IssueTriagePrompt {
                title: issue.title.clone(),
                body,
                comments: comments_text,
            };
            let triage_summary = match agent.invoke(&triage_prompt) {
                Ok(s) => s,
                Err(e) => format!("Analysis unavailable: {e}"),
            };

            flagged_issues.push(FlaggedIssue {
                number: issue.number,
                title: issue.title.clone(),
                missing_labels: issue.missing_labels(&repo_config.labels_required),
                summary: triage_summary,
            });
        }

        repo_sections.push(RepoSection {
            name: repo_config.name.clone(),
            progress: repo_summary,
            big_updates: String::new(),
            planned_next: String::new(),
            flagged_issues,
        });
    }

    eprintln!("Computing team stats...");
    let team_stats: Vec<TeamStats> = config
        .team
        .iter()
        .map(|member| {
            let active = all_recent_issues
                .iter()
                .filter(|i| i.assignees.contains(&member.github))
                .count();
            TeamStats {
                name: member.name.clone(),
                active,
                closed_this_week: 0,
            }
        })
        .collect();

    eprintln!("Done.");
    Ok(Report {
        date: Utc::now().format("%Y-%m-%d").to_string(),
        repos: repo_sections,
        team_stats,
    })
}
