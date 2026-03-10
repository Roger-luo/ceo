use std::hash::{Hash, Hasher};

use log::debug;

use crate::db::{self, CommentRow};
use crate::prompt::{DiscussionSummaryPrompt, IssueDescriptionPrompt};
use crate::report::github_link;

use super::{PipelineContext, Result, Task};

/// Compute a hash of the discussion (comments) for an issue.
fn compute_discussion_hash(comments: &[CommentRow]) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for c in comments {
        c.comment_id.hash(&mut hasher);
        c.body.hash(&mut hasher);
    }
    format!("{:x}", hasher.finish())
}

/// Summarize a single issue using two-part caching:
/// 1. issue_summary -- what the issue is about (generated once)
/// 2. discussion_summary -- current discussion state (updated incrementally)
#[allow(clippy::too_many_arguments)]
fn summarize_issue(
    conn: &rusqlite::Connection,
    agent: &dyn crate::agent::Agent,
    repo: &str,
    issue: &crate::github::Issue,
    body: &str,
    issue_comments: &[CommentRow],
    comments_text: &str,
    summary_length: &str,
) -> Result<String> {
    let linked_assignees: Vec<String> = issue.assignees.iter().map(|a| github_link(a)).collect();
    let discussion_hash = compute_discussion_hash(issue_comments);
    let cached = db::query_issue_cache(conn, repo, issue.number).ok().flatten();

    let (issue_summary, discussion_summary) = match cached {
        Some(cache) if cache.discussion_hash == discussion_hash => {
            // Nothing changed -- use both cached summaries
            debug!("Issue #{}: using cached summary (no changes)", issue.number);
            debug!("Issue #{}: cache hit, no changes", issue.number);
            (cache.issue_summary, cache.discussion_summary)
        }
        Some(cache) => {
            // Discussion changed -- keep issue_summary, update discussion incrementally
            debug!(
                "Issue #{}: updating discussion summary (comments changed)",
                issue.number
            );
            debug!("Issue #{}: updating discussion summary", issue.number);
            let disc_prompt = DiscussionSummaryPrompt {
                repo: repo.to_string(),
                number: issue.number,
                title: issue.title.clone(),
                comments: comments_text.to_string(),
                previous_summary: Some(cache.discussion_summary),
                summary_length: summary_length.to_string(),
            };
            let new_disc = agent.invoke(&disc_prompt).map_err(|e| {
                eprintln!("  Error updating discussion #{}: {e}", issue.number);
                e
            })?;
            let _ = db::save_issue_cache(
                conn,
                repo,
                issue.number,
                &cache.issue_summary,
                &new_disc,
                &discussion_hash,
            );
            (cache.issue_summary, new_disc)
        }
        None => {
            // First time -- generate both
            debug!("Issue #{}: generating initial summaries", issue.number);
            let desc_prompt = IssueDescriptionPrompt {
                repo: repo.to_string(),
                number: issue.number,
                title: issue.title.clone(),
                kind: issue.kind.clone(),
                labels: issue.labels.join(", "),
                assignees: linked_assignees.join(", "),
                body: body.to_string(),
                summary_length: summary_length.to_string(),
            };
            let issue_sum = agent.invoke(&desc_prompt).map_err(|e| {
                eprintln!("  Error summarizing #{}: {e}", issue.number);
                e
            })?;

            let discussion_sum = if comments_text.is_empty() {
                "No discussion yet.".to_string()
            } else {
                let disc_prompt = DiscussionSummaryPrompt {
                    repo: repo.to_string(),
                    number: issue.number,
                    title: issue.title.clone(),
                    comments: comments_text.to_string(),
                    previous_summary: None,
                    summary_length: summary_length.to_string(),
                };
                agent.invoke(&disc_prompt).map_err(|e| {
                    eprintln!("  Error summarizing discussion #{}: {e}", issue.number);
                    e
                })?
            };

            let _ = db::save_issue_cache(
                conn,
                repo,
                issue.number,
                &issue_sum,
                &discussion_sum,
                &discussion_hash,
            );
            (issue_sum, discussion_sum)
        }
    };

    Ok(format!("{issue_summary} {discussion_summary}"))
}

pub struct SummarizeIssuesTask;

impl Task for SummarizeIssuesTask {
    fn name(&self) -> &str {
        "Summarize Issues"
    }

    fn description(&self) -> &str {
        "Summarize each issue/PR using two-part caching"
    }

    fn step_count(&self, ctx: &PipelineContext) -> usize {
        ctx.config
            .repos
            .iter()
            .map(|repo_config| {
                ctx.issues
                    .get(&repo_config.name)
                    .map_or(0, |issues| issues.len())
            })
            .sum()
    }

    fn should_skip(&self, _ctx: &PipelineContext) -> bool {
        false
    }

    fn run(&self, ctx: &mut PipelineContext) -> Result<()> {
        for repo_config in &ctx.config.repos {
            let repo_name = &repo_config.name;
            let issues = match ctx.issues.get(repo_name) {
                Some(issues) => issues.clone(),
                None => continue,
            };

            let mut per_issue_summaries = Vec::new();

            for issue in &issues {
                let body = ctx
                    .issue_bodies
                    .get(&(repo_name.clone(), issue.number))
                    .cloned()
                    .unwrap_or_default();

                let issue_comments: Vec<CommentRow> = ctx
                    .comments
                    .get(&(repo_name.clone(), issue.number))
                    .cloned()
                    .unwrap_or_default();

                let comments_text: String = issue_comments
                    .iter()
                    .map(|c| format!("{}: {}", github_link(&c.author), c.body))
                    .collect::<Vec<_>>()
                    .join("\n");

                let summary = summarize_issue(
                    ctx.conn,
                    ctx.agent,
                    repo_name,
                    issue,
                    &body,
                    &issue_comments,
                    &comments_text,
                    &ctx.summary_length,
                )?;
                debug!("Issue #{} summary: {} chars", issue.number, summary.len());

                let prefix = if issue.kind == "pr" { "PR" } else { "Issue" };
                per_issue_summaries.push(format!(
                    "- {prefix} #{} {}: {}",
                    issue.number, issue.title, summary
                ));
            }

            ctx.per_issue_summaries
                .insert(repo_name.clone(), per_issue_summaries);
        }

        Ok(())
    }
}
