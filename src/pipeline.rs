use anyhow::Result;
use chrono::Utc;

use crate::agent::Agent;
use crate::config::Config;
use crate::filter;
use crate::gh::{self, GhRunner};
use crate::prompt::{IssueTriagePrompt, WeeklySummaryPrompt};
use crate::report::{FlaggedIssue, Report, RepoSection, TeamStats};

pub fn run_pipeline(
    config: &Config,
    gh_runner: &dyn GhRunner,
    agent: &dyn Agent,
    days: i64,
) -> Result<Report> {
    let mut repo_sections = Vec::new();
    let mut all_recent_issues = Vec::new();

    for repo_config in &config.repos {
        let all_issues = gh::fetch_issues(gh_runner, &repo_config.name)?;
        let recent = filter::filter_recent(&all_issues, days);

        for issue in &recent {
            all_recent_issues.push((*issue).clone());
        }

        let issue_summaries: String = recent
            .iter()
            .map(|i| {
                format!(
                    "- #{}: {} (labels: {}, assignees: {})",
                    i.number,
                    i.title,
                    i.labels.join(", "),
                    i.assignees.join(", ")
                )
            })
            .collect::<Vec<_>>()
            .join("\n");

        let summary_prompt = WeeklySummaryPrompt {
            repo: repo_config.name.clone(),
            issue_summaries,
        };
        let summary = match agent.invoke(&summary_prompt) {
            Ok(s) => s,
            Err(e) => format!("Analysis unavailable: {e}"),
        };

        let flagged_refs = filter::find_flagged_issues(&recent, &repo_config.labels_required);
        let mut flagged_issues = Vec::new();

        for issue in flagged_refs {
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
            progress: summary.clone(),
            big_updates: String::new(),
            planned_next: String::new(),
            flagged_issues,
        });
    }

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

    Ok(Report {
        date: Utc::now().format("%Y-%m-%d").to_string(),
        repos: repo_sections,
        team_stats,
    })
}
