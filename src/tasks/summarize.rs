use std::collections::HashMap;
use std::future::Future;
use std::hash::{Hash, Hasher};
use std::pin::Pin;

use log::debug;

use crate::db::{self, CommentRow};
use crate::github::Issue;
use crate::prompt::{
    BatchIssueDescriptionPrompt, BatchIssueEntry, DiscussionSummaryPrompt, IssueDescriptionPrompt,
};
use crate::report::{extract_all_summary_tags, github_link};

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

    fn run<'a, 'ctx>(&'a self, ctx: &'a mut PipelineContext<'ctx>) -> Pin<Box<dyn Future<Output = Result<()>> + 'a>>
    where 'ctx: 'a {
        Box::pin(async move {
        let batch_size = ctx.config.batch_size();

        for repo_config in &ctx.config.repos {
            let repo_name = &repo_config.name;
            let issues = match ctx.issues.get(repo_name) {
                Some(issues) => issues.clone(),
                None => continue,
            };

            // Pre-compute per-issue data: body, comments, comments_text, discussion_hash, cache
            struct IssueData {
                body: String,
                comments_text: String,
                discussion_hash: String,
                cached: Option<db::IssueCacheRow>,
            }

            let mut issue_data: HashMap<u64, IssueData> = HashMap::new();
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

                let discussion_hash = compute_discussion_hash(&issue_comments);
                let cached = db::query_issue_cache(ctx.conn, repo_name, issue.number)
                    .ok()
                    .flatten();

                issue_data.insert(
                    issue.number,
                    IssueData {
                        body,
                        comments_text,
                        discussion_hash,
                        cached,
                    },
                );
            }

            // Partition issues into three categories
            let mut fully_cached: Vec<&Issue> = Vec::new();
            let mut discussion_changed: Vec<&Issue> = Vec::new();
            let mut needs_description: Vec<&Issue> = Vec::new();

            for issue in &issues {
                let data = &issue_data[&issue.number];
                match &data.cached {
                    Some(cache) if cache.discussion_hash == data.discussion_hash => {
                        debug!("Issue #{}: using cached summary (no changes)", issue.number);
                        fully_cached.push(issue);
                    }
                    Some(_cache) => {
                        debug!(
                            "Issue #{}: updating discussion summary (comments changed)",
                            issue.number
                        );
                        discussion_changed.push(issue);
                    }
                    None => {
                        debug!("Issue #{}: generating initial summaries", issue.number);
                        needs_description.push(issue);
                    }
                }
            }

            // Store issue_summary per issue number as we resolve them
            let mut issue_summaries: HashMap<u64, String> = HashMap::new();
            let mut discussion_summaries: HashMap<u64, String> = HashMap::new();

            // 1. Fully cached: just use both cached summaries
            for issue in &fully_cached {
                let data = &issue_data[&issue.number];
                let cache = data.cached.as_ref().unwrap();
                issue_summaries.insert(issue.number, cache.issue_summary.clone());
                discussion_summaries.insert(issue.number, cache.discussion_summary.clone());
            }

            // 2. Discussion changed: keep cached issue_summary, update discussion
            for issue in &discussion_changed {
                let data = &issue_data[&issue.number];
                let cache = data.cached.as_ref().unwrap();
                issue_summaries.insert(issue.number, cache.issue_summary.clone());

                let disc_prompt = DiscussionSummaryPrompt {
                    repo: repo_name.to_string(),
                    number: issue.number,
                    title: issue.title.clone(),
                    comments: data.comments_text.clone(),
                    previous_summary: Some(cache.discussion_summary.clone()),
                    summary_length: ctx.summary_length.clone(),
                };
                let new_disc = ctx.agent.invoke(&disc_prompt).await.map_err(|e| {
                    eprintln!("  Error updating discussion #{}: {e}", issue.number);
                    e
                })?;
                let _ = db::save_issue_cache(
                    ctx.conn,
                    repo_name,
                    issue.number,
                    &cache.issue_summary,
                    &new_disc,
                    &data.discussion_hash,
                );
                discussion_summaries.insert(issue.number, new_disc);
            }

            // 3. Uncached: batch description prompts, then individual discussion prompts
            // Batch the description calls
            for chunk in needs_description.chunks(batch_size) {
                let desc_results: HashMap<u64, String> = if chunk.len() == 1 {
                    // Single issue: use individual prompt
                    let issue = chunk[0];
                    let data = &issue_data[&issue.number];
                    let linked_assignees: Vec<String> =
                        issue.assignees.iter().map(|a| github_link(a)).collect();
                    let desc_prompt = IssueDescriptionPrompt {
                        repo: repo_name.to_string(),
                        number: issue.number,
                        title: issue.title.clone(),
                        kind: issue.kind.clone(),
                        labels: issue.labels.join(", "),
                        assignees: linked_assignees.join(", "),
                        body: data.body.clone(),
                        summary_length: ctx.summary_length.clone(),
                    };
                    let issue_sum = ctx.agent.invoke(&desc_prompt).await.map_err(|e| {
                        eprintln!("  Error summarizing #{}: {e}", issue.number);
                        e
                    })?;
                    let mut m = HashMap::new();
                    m.insert(issue.number, issue_sum);
                    m
                } else {
                    // Multiple issues: use batch prompt
                    let entries: Vec<BatchIssueEntry> = chunk
                        .iter()
                        .map(|issue| {
                            let data = &issue_data[&issue.number];
                            let linked_assignees: Vec<String> =
                                issue.assignees.iter().map(|a| github_link(a)).collect();
                            BatchIssueEntry {
                                repo: repo_name.to_string(),
                                number: issue.number,
                                title: issue.title.clone(),
                                kind: issue.kind.clone(),
                                labels: issue.labels.join(", "),
                                assignees: linked_assignees.join(", "),
                                body: data.body.clone(),
                            }
                        })
                        .collect();

                    let batch_prompt = BatchIssueDescriptionPrompt {
                        issues: entries,
                        summary_length: ctx.summary_length.clone(),
                    };
                    let response = ctx.agent.invoke(&batch_prompt).await.map_err(|e| {
                        eprintln!("  Error in batch summarize: {e}");
                        e
                    })?;

                    let parsed: HashMap<u64, String> =
                        extract_all_summary_tags(&response).into_iter().collect();

                    // Fall back to individual prompts for any missing issues
                    let mut results = parsed;
                    for issue in chunk.iter() {
                        if !results.contains_key(&issue.number) {
                            debug!(
                                "Issue #{}: missing from batch response, falling back to individual",
                                issue.number
                            );
                            let data = &issue_data[&issue.number];
                            let linked_assignees: Vec<String> =
                                issue.assignees.iter().map(|a| github_link(a)).collect();
                            let desc_prompt = IssueDescriptionPrompt {
                                repo: repo_name.to_string(),
                                number: issue.number,
                                title: issue.title.clone(),
                                kind: issue.kind.clone(),
                                labels: issue.labels.join(", "),
                                assignees: linked_assignees.join(", "),
                                body: data.body.clone(),
                                summary_length: ctx.summary_length.clone(),
                            };
                            let issue_sum = ctx.agent.invoke(&desc_prompt).await.map_err(|e| {
                                eprintln!("  Error summarizing #{}: {e}", issue.number);
                                e
                            })?;
                            results.insert(issue.number, issue_sum);
                        }
                    }
                    results
                };

                // Now handle discussion summaries and caching for each issue in the chunk
                for issue in chunk.iter() {
                    let data = &issue_data[&issue.number];
                    let issue_sum = desc_results[&issue.number].clone();

                    let discussion_sum = if data.comments_text.is_empty() {
                        "No discussion yet.".to_string()
                    } else {
                        let disc_prompt = DiscussionSummaryPrompt {
                            repo: repo_name.to_string(),
                            number: issue.number,
                            title: issue.title.clone(),
                            comments: data.comments_text.clone(),
                            previous_summary: None,
                            summary_length: ctx.summary_length.clone(),
                        };
                        ctx.agent.invoke(&disc_prompt).await.map_err(|e| {
                            eprintln!("  Error summarizing discussion #{}: {e}", issue.number);
                            e
                        })?
                    };

                    let _ = db::save_issue_cache(
                        ctx.conn,
                        repo_name,
                        issue.number,
                        &issue_sum,
                        &discussion_sum,
                        &data.discussion_hash,
                    );
                    issue_summaries.insert(issue.number, issue_sum);
                    discussion_summaries.insert(issue.number, discussion_sum);
                }
            }

            // Build final per-issue summary strings in original order
            let mut per_issue_summaries = Vec::new();
            for issue in &issues {
                let issue_sum = &issue_summaries[&issue.number];
                let disc_sum = &discussion_summaries[&issue.number];
                let combined = format!("{issue_sum} {disc_sum}");
                debug!("Issue #{} summary: {} chars", issue.number, combined.len());

                let prefix = if issue.kind == "pr" { "PR" } else { "Issue" };
                per_issue_summaries.push(format!(
                    "- {prefix} #{} {}: {}",
                    issue.number, issue.title, combined
                ));
            }

            ctx.per_issue_summaries
                .insert(repo_name.clone(), per_issue_summaries);
        }

        Ok(())
        })
    }
}
