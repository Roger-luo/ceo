use std::hash::{Hash, Hasher};

use chrono::Utc;
use log::{debug, info};

use crate::agent::Agent;
use crate::config::Config;
use crate::db::{self, CommentRow, IssueRow};
use crate::error::PipelineError;
use crate::filter;
use crate::github::Issue;
use crate::prompt::{DiscussionSummaryPrompt, IssueDescriptionPrompt, IssueTriagePrompt, WeeklySummaryPrompt};
use crate::report::{extract_xml_tag, github_link, FlaggedIssue, Report, RepoSection, TeamStats};
use crate::roadmap::Roadmap;

type Result<T> = std::result::Result<T, PipelineError>;

/// Progress reporting for the report pipeline.
pub trait PipelineProgress {
    /// Starting work on a repo.
    fn repo_start(&self, repo: &str, issue_count: usize);
    /// Progress on individual issue summarization.
    fn issue_step(&self, index: usize, total: usize, number: u64, title: &str);
    /// Generic phase message (repo summary, triage step, etc.)
    fn phase(&self, msg: &str);
    /// Finished processing a repo.
    fn repo_done(&self, repo: &str);
    /// All repos done.
    fn finish(&self);
}

/// No-op progress for library/test use.
pub struct NullProgress;

impl PipelineProgress for NullProgress {
    fn repo_start(&self, _repo: &str, _issue_count: usize) {}
    fn issue_step(&self, _index: usize, _total: usize, _number: u64, _title: &str) {}
    fn phase(&self, _msg: &str) {}
    fn repo_done(&self, _repo: &str) {}
    fn finish(&self) {}
}

/// Compute a hash of the raw data feeding into a repo summary so we can
/// skip the agent call when nothing has changed.
fn compute_data_hash(issue_rows: &[IssueRow], commit_rows: &[db::CommitRow]) -> String {
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

/// Compute a hash of the discussion (comments) for an issue.
fn compute_discussion_hash(comments: &[CommentRow]) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for c in comments {
        c.comment_id.hash(&mut hasher);
        c.body.hash(&mut hasher);
    }
    format!("{:x}", hasher.finish())
}

fn row_to_issue(row: &IssueRow) -> Issue {
    let labels: Vec<String> = serde_json::from_str(&row.labels).unwrap_or_default();
    let assignees: Vec<String> = serde_json::from_str(&row.assignees).unwrap_or_default();
    Issue {
        number: row.number,
        title: row.title.clone(),
        kind: row.kind.clone(),
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

/// Summarize a single issue using two-part caching:
/// 1. issue_summary — what the issue is about (generated once)
/// 2. discussion_summary — current discussion state (updated incrementally)
fn summarize_issue(
    conn: &rusqlite::Connection,
    agent: &dyn Agent,
    repo: &str,
    issue: &Issue,
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
            // Nothing changed — use both cached summaries
            debug!("Issue #{}: using cached summary (no changes)", issue.number);
            debug!("Issue #{}: cache hit, no changes", issue.number);
            (cache.issue_summary, cache.discussion_summary)
        }
        Some(cache) => {
            // Discussion changed — keep issue_summary, update discussion incrementally
            debug!("Issue #{}: updating discussion summary (comments changed)", issue.number);
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
                conn, repo, issue.number, &cache.issue_summary, &new_disc, &discussion_hash,
            );
            (cache.issue_summary, new_disc)
        }
        None => {
            // First time — generate both
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
                conn, repo, issue.number, &issue_sum, &discussion_sum, &discussion_hash,
            );
            (issue_sum, discussion_sum)
        }
    };

    Ok(format!("{issue_summary} {discussion_summary}"))
}

pub fn run_pipeline(
    config: &Config,
    conn: &rusqlite::Connection,
    agent: &dyn Agent,
    since: &str,
    date_label: &str,
    progress: &dyn PipelineProgress,
    template: Option<&str>,
) -> Result<Report> {
    let cutoff = since.to_string();
    let roadmap = Roadmap::load();
    let summary_length = config.summary_length();
    let mut repo_sections = Vec::new();
    let mut all_recent_issues = Vec::new();

    for repo_config in &config.repos {
        info!("Processing repo: {}", repo_config.name);
        let repo_names = vec![repo_config.name.clone()];
        let issue_rows = db::query_recent_issues(conn, &repo_names, &cutoff)?;
        let issues: Vec<Issue> = issue_rows.iter().map(row_to_issue).collect();
        debug!("Found {} recent issues (since {})", issues.len(), since);
        progress.repo_start(&repo_config.name, issues.len());

        // Collect issue numbers for comment lookup
        let issue_numbers: Vec<u64> = issue_rows.iter().map(|r| r.number).collect();
        let comment_rows = db::query_comments_for_issues(conn, &repo_config.name, &issue_numbers)?;

        // Build a map of issue_number -> comment rows
        let mut comments_by_issue: std::collections::HashMap<u64, Vec<&CommentRow>> = std::collections::HashMap::new();
        for c in &comment_rows {
            comments_by_issue.entry(c.issue_number).or_default().push(c);
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

        // Summarize each issue individually with two-part caching
        let mut per_issue_summaries = Vec::new();
        for (i, issue) in issues.iter().enumerate() {
            progress.issue_step(i + 1, issues.len(), issue.number, &issue.title);

            let body = body_by_issue.get(&issue.number).cloned().unwrap_or_default();
            let issue_comments: Vec<CommentRow> = comments_by_issue
                .get(&issue.number)
                .map(|v| v.iter().map(|c| (*c).clone()).collect())
                .unwrap_or_default();
            let comments_text: String = issue_comments
                .iter()
                .map(|c| format!("{}: {}", github_link(&c.author), c.body))
                .collect::<Vec<_>>()
                .join("\n");

            let summary = summarize_issue(
                conn, agent, &repo_config.name, issue, &body, &issue_comments, &comments_text,
                summary_length,
            )?;
            debug!("Issue #{} summary: {} chars", issue.number, summary.len());

            let prefix = if issue.kind == "pr" { "PR" } else { "Issue" };
            per_issue_summaries.push(format!("- {prefix} #{} {}: {}", issue.number, issue.title, summary));
        }

        // Query recent commits for this repo
        let repo_names_for_commits = vec![repo_config.name.clone()];
        let commit_rows = db::query_recent_commits(conn, &repo_names_for_commits, &cutoff)?;
        debug!("Found {} recent commits for {}", commit_rows.len(), repo_config.name);

        // Group commits by branch for clearer context
        let commit_log: String = {
            let mut by_branch: std::collections::BTreeMap<&str, Vec<String>> = std::collections::BTreeMap::new();
            for c in &commit_rows {
                let short_sha = &c.sha[..c.sha.len().min(7)];
                let first_line = c.message.lines().next().unwrap_or("");
                let line = format!("- {} {}: {}", short_sha, github_link(&c.author), first_line);
                let branch_key = if c.branch.is_empty() { "default" } else { &c.branch };
                by_branch.entry(branch_key).or_default().push(line);
            }
            if by_branch.len() <= 1 {
                // Single branch (or no commits): flat list, no headers
                by_branch.into_values().flatten().collect::<Vec<_>>().join("\n")
            } else {
                // Multiple branches: add headers
                by_branch.into_iter().map(|(branch, lines)| {
                    format!("[{branch}]\n{}", lines.join("\n"))
                }).collect::<Vec<_>>().join("\n\n")
            }
        };

        // Check report cache: skip agent call if data hasn't changed
        let data_hash = compute_data_hash(&issue_rows, &commit_rows);
        let cached = db::query_report_cache(conn, &repo_config.name).ok().flatten();
        let has_activity = !per_issue_summaries.is_empty() || !commit_log.is_empty();

        // Build initiatives context for this repo
        let repo_initiatives = roadmap.for_repo(&repo_config.name);
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
                    progress.phase("Using cached summary");
                    cached_summary.clone()
                } else {
                    progress.phase("Generating repo summary...");
                    let aggregated = per_issue_summaries.join("\n");
                    let prompt = WeeklySummaryPrompt {
                        repo: repo_config.name.clone(),
                        issue_summaries: aggregated,
                        commit_log,
                        previous_summary: Some(cached_summary.clone()),
                        initiatives: initiatives_text.clone(),
                    };
                    let summary = agent.invoke(&prompt).map_err(|e| {
                        eprintln!("  Error generating repo summary: {e}");
                        e
                    })?;
                    let _ = db::save_report_cache(conn, &repo_config.name, &summary, &data_hash);
                    summary
                }
            } else {
                progress.phase("Generating repo summary...");
                let aggregated = per_issue_summaries.join("\n");
                let prompt = WeeklySummaryPrompt {
                    repo: repo_config.name.clone(),
                    issue_summaries: aggregated,
                    commit_log,
                    previous_summary: None,
                    initiatives: initiatives_text.clone(),
                };
                let summary = agent.invoke(&prompt).map_err(|e| {
                    eprintln!("  Error generating repo summary: {e}");
                    e
                })?;
                let _ = db::save_report_cache(conn, &repo_config.name, &summary, &data_hash);
                summary
            };
            (
                extract_xml_tag(&raw, "done"),
                extract_xml_tag(&raw, "in_progress"),
                extract_xml_tag(&raw, "next"),
            )
        };

        // Triage flagged issues
        let flagged_refs = filter::find_flagged_issues(&issue_refs, &repo_config.labels_required);
        debug!("Found {} flagged issues in {}", flagged_refs.len(), repo_config.name);
        let mut flagged_issues = Vec::new();

        if !flagged_refs.is_empty() {
            progress.phase(&format!("Triaging {} flagged issues...", flagged_refs.len()));
        }

        for (_i, issue) in flagged_refs.iter().enumerate() {
            progress.phase(&format!("Triaging #{} {}...", issue.number, issue.title));
            debug!("Triaging issue #{}: {}", issue.number, issue.title);

            let body = body_by_issue.get(&issue.number).cloned().unwrap_or_default();
            let comments_text = comments_by_issue
                .get(&issue.number)
                .map(|v| v.iter().map(|c| format!("{}: {}", github_link(&c.author), c.body)).collect::<Vec<_>>().join("\n"))
                .unwrap_or_default();

            let triage_prompt = IssueTriagePrompt {
                title: issue.title.clone(),
                body,
                comments: comments_text,
            };
            let triage_summary = agent.invoke(&triage_prompt).map_err(|e| {
                eprintln!("  Error triaging #{}: {e}", issue.number);
                e
            })?;

            flagged_issues.push(FlaggedIssue {
                number: issue.number,
                title: issue.title.clone(),
                missing_labels: issue.missing_labels(&repo_config.labels_required),
                summary: triage_summary,
            });
        }

        repo_sections.push(RepoSection {
            name: repo_config.name.clone(),
            done,
            in_progress,
            next,
            flagged_issues,
        });
        progress.repo_done(&repo_config.name);
    }

    progress.phase("Computing team stats...");
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
                github: member.github.clone(),
                active,
                closed_this_week: 0,
            }
        })
        .collect();

    // Executive summary — aggregate all repo sections if a template is requested
    let executive_summary = if let Some(template_name) = template {
        let template_text = crate::prompt::resolve_template(template_name)
            .ok_or_else(|| PipelineError::Agent(
                crate::error::AgentError::ExitError(format!("Unknown template: {template_name}"))
            ))?;

        // Build text from all repo sections
        let mut repo_text = String::new();
        for section in &repo_sections {
            use std::fmt::Write;
            writeln!(repo_text, "## {}", section.name).unwrap();
            if let Some(done) = &section.done {
                writeln!(repo_text, "Done: {done}").unwrap();
            }
            if let Some(ip) = &section.in_progress {
                writeln!(repo_text, "In Progress: {ip}").unwrap();
            }
            if let Some(next) = &section.next {
                writeln!(repo_text, "Next: {next}").unwrap();
            }
            writeln!(repo_text).unwrap();
        }

        progress.phase("Generating executive summary...");
        let prompt = crate::prompt::ExecutiveSummaryPrompt {
            repo_summaries: repo_text,
            template: template_text,
        };
        let summary = agent.invoke(&prompt).map_err(|e| {
            eprintln!("  Error generating executive summary: {e}");
            e
        })?;
        Some(summary)
    } else {
        None
    };

    progress.finish();
    Ok(Report {
        date: date_label.to_string(),
        executive_summary,
        repos: repo_sections,
        team_stats,
    })
}

