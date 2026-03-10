use std::hash::{Hash, Hasher};

use crate::db::{self, CommitRow, IssueRow};
use crate::prompt::WeeklySummaryPrompt;
use crate::report::{extract_xml_tag, RepoSection};

use super::{PipelineContext, Result, Task};

/// Compute a hash of the raw data feeding into a repo summary so we can
/// skip the agent call when nothing has changed.
fn compute_data_hash(issue_rows: &[IssueRow], commit_rows: &[CommitRow]) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for row in issue_rows {
        row.number.hash(&mut hasher);
        row.updated_at.hash(&mut hasher);
        row.state.hash(&mut hasher);
        row.title.hash(&mut hasher);
    }
    for row in commit_rows {
        row.sha.hash(&mut hasher);
        row.branch.hash(&mut hasher);
    }
    format!("{:x}", hasher.finish())
}

pub struct RepoSummaryTask;

impl Task for RepoSummaryTask {
    fn name(&self) -> &str {
        "Repo summaries"
    }

    fn description(&self) -> &str {
        "Generating per-repo weekly summaries"
    }

    fn step_count(&self, ctx: &PipelineContext) -> usize {
        ctx.config.repos.len()
    }

    fn should_skip(&self, _ctx: &PipelineContext) -> bool {
        false
    }

    fn run(&self, ctx: &mut PipelineContext) -> Result<()> {
        let repo_names: Vec<String> = ctx.config.repos.iter().map(|r| r.name.clone()).collect();

        for repo_name in &repo_names {
            let per_issue_summaries = ctx
                .per_issue_summaries
                .get(repo_name)
                .cloned()
                .unwrap_or_default();
            let commit_log = ctx
                .commit_logs
                .get(repo_name)
                .cloned()
                .unwrap_or_default();
            let issue_rows = ctx
                .issue_rows
                .get(repo_name)
                .cloned()
                .unwrap_or_default();
            let commit_rows = ctx
                .commit_rows
                .get(repo_name)
                .cloned()
                .unwrap_or_default();

            // Check report cache: skip agent call if data hasn't changed
            let data_hash = compute_data_hash(&issue_rows, &commit_rows);
            let cached = db::query_report_cache(ctx.conn, repo_name)
                .ok()
                .flatten();
            let has_activity = !per_issue_summaries.is_empty() || !commit_log.is_empty();

            // Build initiatives context for this repo
            let repo_initiatives = ctx.roadmap.for_repo(repo_name);
            let initiatives_text: String = repo_initiatives
                .iter()
                .map(|i| {
                    let tf = i.timeframe.as_deref().unwrap_or("ongoing");
                    format!("- {} ({}): {}", i.name, tf, i.description)
                })
                .collect::<Vec<_>>()
                .join("\n");

            let (done, in_progress, next) = if !has_activity {
                (None, None, None)
            } else {
                let raw = if let Some((cached_summary, cached_hash)) = &cached {
                    let has_xml = cached_summary.contains("<done>")
                        || cached_summary.contains("<in_progress>")
                        || cached_summary.contains("<next>");
                    if *cached_hash == data_hash && has_xml {
                        cached_summary.clone()
                    } else {
                        let aggregated = per_issue_summaries.join("\n");
                        let prompt = WeeklySummaryPrompt {
                            repo: repo_name.clone(),
                            issue_summaries: aggregated,
                            commit_log: commit_log.clone(),
                            previous_summary: Some(cached_summary.clone()),
                            initiatives: initiatives_text.clone(),
                        };
                        let summary = ctx.agent.invoke(&prompt).map_err(|e| {
                            eprintln!("  Error generating repo summary: {e}");
                            e
                        })?;
                        let _ = db::save_report_cache(
                            ctx.conn,
                            repo_name,
                            &summary,
                            &data_hash,
                        );
                        summary
                    }
                } else {
                    let aggregated = per_issue_summaries.join("\n");
                    let prompt = WeeklySummaryPrompt {
                        repo: repo_name.clone(),
                        issue_summaries: aggregated,
                        commit_log: commit_log.clone(),
                        previous_summary: None,
                        initiatives: initiatives_text.clone(),
                    };
                    let summary = ctx.agent.invoke(&prompt).map_err(|e| {
                        eprintln!("  Error generating repo summary: {e}");
                        e
                    })?;
                    let _ = db::save_report_cache(
                        ctx.conn,
                        repo_name,
                        &summary,
                        &data_hash,
                    );
                    summary
                };
                (
                    extract_xml_tag(&raw, "done"),
                    extract_xml_tag(&raw, "in_progress"),
                    extract_xml_tag(&raw, "next"),
                )
            };

            ctx.repo_sections.push(RepoSection {
                name: repo_name.clone(),
                done,
                in_progress,
                next,
                flagged_issues: Vec::new(),
            });
        }

        Ok(())
    }
}
