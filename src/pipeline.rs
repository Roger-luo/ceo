use chrono::Utc;
use log::{debug, info};

use crate::agent::Agent;
use crate::config::Config;
use crate::error::PipelineError;
use crate::filter;
use crate::gh::{self, GhRunner};
use crate::prompt::{IssueSummaryPrompt, IssueTriagePrompt, WeeklySummaryPrompt};
use crate::report::{FlaggedIssue, Report, RepoSection, TeamStats};

type Result<T> = std::result::Result<T, PipelineError>;

pub fn run_pipeline(
    config: &Config,
    gh_runner: &dyn GhRunner,
    agent: &dyn Agent,
    days: i64,
) -> Result<Report> {
    let mut repo_sections = Vec::new();
    let mut all_recent_issues = Vec::new();

    for repo_config in &config.repos {
        eprintln!("Fetching issues from {}...", repo_config.name);
        info!("Processing repo: {}", repo_config.name);
        let all_issues = gh::fetch_issues(gh_runner, &repo_config.name)?;
        let recent = filter::filter_recent(&all_issues, days);
        debug!("Fetched {} total issues, {} recent (last {} days)", all_issues.len(), recent.len(), days);
        eprintln!("  Found {} recent issues (last {} days)", recent.len(), days);

        for issue in &recent {
            all_recent_issues.push((*issue).clone());
        }

        // Summarize each issue individually to keep context windows small
        let mut per_issue_summaries = Vec::new();
        for (i, issue) in recent.iter().enumerate() {
            eprintln!("  [{}/{}] Summarizing #{} {}...", i + 1, recent.len(), issue.number, issue.title);
            debug!("Fetching detail for issue #{}", issue.number);

            let (body, comments_text) = match gh::fetch_issue_detail(gh_runner, &repo_config.name, issue.number) {
                Ok(detail) => {
                    let comments = detail.comments
                        .iter()
                        .map(|c| format!("{}: {}", c.author, c.body))
                        .collect::<Vec<_>>()
                        .join("\n");
                    (detail.body, comments)
                }
                Err(e) => {
                    debug!("Failed to fetch detail for #{}: {e}", issue.number);
                    (String::new(), String::new())
                }
            };

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
        let flagged_refs = filter::find_flagged_issues(&recent, &repo_config.labels_required);
        debug!("Found {} flagged issues in {}", flagged_refs.len(), repo_config.name);
        let mut flagged_issues = Vec::new();

        if !flagged_refs.is_empty() {
            eprintln!("  Triaging {} flagged issues...", flagged_refs.len());
        }

        for (i, issue) in flagged_refs.iter().enumerate() {
            eprintln!("    [{}/{}] #{} {}...", i + 1, flagged_refs.len(), issue.number, issue.title);
            debug!("Triaging issue #{}: {}", issue.number, issue.title);
            let triage_summary =
                match gh::fetch_issue_detail(gh_runner, &repo_config.name, issue.number) {
                    Ok(detail) => {
                        let comments_text: String = detail
                            .comments
                            .iter()
                            .map(|c| format!("{}: {}", c.author, c.body))
                            .collect::<Vec<_>>()
                            .join("\n");
                        let triage_prompt = IssueTriagePrompt {
                            title: issue.title.clone(),
                            body: detail.body,
                            comments: comments_text,
                        };
                        match agent.invoke(&triage_prompt) {
                            Ok(s) => s,
                            Err(e) => format!("Analysis unavailable: {e}"),
                        }
                    }
                    Err(e) => format!("Could not fetch issue detail: {e}"),
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
