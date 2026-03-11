use std::collections::HashMap;
use std::future::Future;
use std::hash::{Hash, Hasher};
use std::pin::Pin;
use std::sync::Arc;

use futures::stream::{FuturesUnordered, StreamExt};
use log::debug;
use tokio::sync::Semaphore;

use crate::db::{self, CommentRow};
use crate::error::PipelineError;
use crate::github::Issue;
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

            // 2 + 3. Concurrent processing of discussion_changed and needs_description
            let concurrency = ctx.config.concurrency();
            let agent = ctx.agent;
            let uncached_count = discussion_changed.len() + needs_description.len();

            if uncached_count > 0 {
                let effective = concurrency.min(uncached_count);
                ctx.progress.repo_start(repo_name, uncached_count);
                ctx.progress.phase(&format!(
                    "{repo_name}: {uncached_count} to summarize ({effective}x concurrent)"
                ));
            }

            type AgentResult = std::result::Result<(u64, String, String, String), PipelineError>;
            type AgentFut<'f> = Pin<Box<dyn Future<Output = AgentResult> + 'f>>;

            let semaphore = Arc::new(Semaphore::new(concurrency));
            let mut futs: FuturesUnordered<AgentFut<'_>> = FuturesUnordered::new();

            // Push futures for discussion_changed issues
            for issue in &discussion_changed {
                let data = &issue_data[&issue.number];
                let cache = data.cached.as_ref().unwrap();
                let sem = semaphore.clone();
                let number = issue.number;
                let title = issue.title.clone();
                let repo_name_owned = repo_name.to_string();
                let disc_prompt = DiscussionSummaryPrompt {
                    repo: repo_name_owned,
                    number,
                    title: title.clone(),
                    comments: data.comments_text.clone(),
                    previous_summary: Some(cache.discussion_summary.clone()),
                    summary_length: ctx.summary_length.clone(),
                };
                let issue_sum = cache.issue_summary.clone();
                let disc_hash = data.discussion_hash.clone();

                futs.push(Box::pin(async move {
                    let _permit = sem.acquire().await.unwrap();
                    let new_disc = agent.invoke(&disc_prompt).await?;
                    Ok((number, issue_sum, new_disc, disc_hash))
                }));
            }

            // Push futures for needs_description issues
            for issue in &needs_description {
                let data = &issue_data[&issue.number];
                let sem = semaphore.clone();
                let number = issue.number;
                let title = issue.title.clone();
                let repo_name_owned = repo_name.to_string();
                let linked_assignees: Vec<String> = issue.assignees.iter().map(|a| github_link(a)).collect();
                let desc_prompt = IssueDescriptionPrompt {
                    repo: repo_name_owned.clone(),
                    number,
                    title: title.clone(),
                    kind: issue.kind.clone(),
                    labels: issue.labels.join(", "),
                    assignees: linked_assignees.join(", "),
                    body: data.body.clone(),
                    summary_length: ctx.summary_length.clone(),
                };
                let comments_text = data.comments_text.clone();
                let disc_hash = data.discussion_hash.clone();
                let summary_length = ctx.summary_length.clone();

                futs.push(Box::pin(async move {
                    let _permit = sem.acquire().await.unwrap();
                    let issue_sum = agent.invoke(&desc_prompt).await?;

                    let discussion_sum = if comments_text.is_empty() {
                        "No discussion yet.".to_string()
                    } else {
                        let disc_prompt = DiscussionSummaryPrompt {
                            repo: repo_name_owned,
                            number,
                            title: title.clone(),
                            comments: comments_text,
                            previous_summary: None,
                            summary_length,
                        };
                        agent.invoke(&disc_prompt).await?
                    };

                    Ok((number, issue_sum, discussion_sum, disc_hash))
                }));
            }

            // Drain futures as they complete
            let mut completed = 0usize;
            while let Some(result) = futs.next().await {
                let (number, issue_sum, disc_sum, disc_hash) = result?;
                completed += 1;

                let title = issues.iter().find(|i| i.number == number)
                    .map(|i| i.title.as_str()).unwrap_or("");
                ctx.progress.issue_step(completed, uncached_count, number, title);

                let _ = db::save_issue_cache(ctx.conn, repo_name, number, &issue_sum, &disc_sum, &disc_hash);
                issue_summaries.insert(number, issue_sum);
                discussion_summaries.insert(number, disc_sum);
            }

            if uncached_count > 0 {
                ctx.progress.repo_done(repo_name);
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
