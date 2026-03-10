# Task-Based Pipeline Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Extract the monolithic `run_pipeline` function into 7 self-contained `Task` implementations in `src/tasks/`, with a shared `PipelineContext` and a thin executor loop.

**Architecture:** A `Task` trait defines the interface (name, description, step_count, should_skip, run). A `PipelineContext` struct carries all shared state between tasks. The executor in `pipeline.rs` builds a `Vec<Box<dyn Task>>` and runs them in sequence, calling progress hooks before/after each task. Each task lives in its own file under `src/tasks/`.

**Tech Stack:** Rust, rusqlite, chrono, existing `Agent`/`Prompt` traits

---

### Task 1: Create `tasks/mod.rs` with `Task` trait and `PipelineContext`

**Files:**
- Create: `src/tasks/mod.rs`
- Modify: `src/lib.rs`

**Step 1: Create `src/tasks/mod.rs` with trait and context**

```rust
use std::collections::HashMap;

use crate::agent::Agent;
use crate::config::Config;
use crate::db::CommentRow;
use crate::error::PipelineError;
use crate::github::Issue;
use crate::report::{FlaggedIssue, RepoSection, TeamStats};
use crate::roadmap::Roadmap;

pub mod fetch_data;
pub mod summarize;
pub mod commit_log;
pub mod repo_summary;
pub mod triage;
pub mod team_stats;
pub mod executive;

type Result<T> = std::result::Result<T, PipelineError>;

pub trait Task {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn step_count(&self, ctx: &PipelineContext) -> usize;
    fn should_skip(&self, ctx: &PipelineContext) -> bool;
    fn run(&self, ctx: &mut PipelineContext) -> Result<()>;
}

/// Shared mutable state passed between pipeline tasks.
pub struct PipelineContext<'a> {
    // Inputs (immutable after construction)
    pub config: &'a Config,
    pub conn: &'a rusqlite::Connection,
    pub agent: &'a dyn Agent,
    pub since: String,
    pub date_label: String,
    pub template: Option<String>,
    pub roadmap: Roadmap,
    pub summary_length: String,

    // Per-repo data populated by FetchDataTask
    pub issues: HashMap<String, Vec<Issue>>,
    pub issue_rows: HashMap<String, Vec<crate::db::IssueRow>>,
    pub issue_bodies: HashMap<(String, u64), String>,
    pub comments: HashMap<(String, u64), Vec<CommentRow>>,

    // Per-repo summaries populated by SummarizeIssuesTask
    pub per_issue_summaries: HashMap<String, Vec<String>>,

    // Per-repo commit logs populated by BuildCommitLogTask
    pub commit_logs: HashMap<String, String>,
    pub commit_rows: HashMap<String, Vec<crate::db::CommitRow>>,

    // Final outputs
    pub repo_sections: Vec<RepoSection>,
    pub all_recent_issues: Vec<Issue>,
    pub team_stats: Vec<TeamStats>,
    pub executive_summary: Option<String>,
}

impl<'a> PipelineContext<'a> {
    pub fn new(
        config: &'a Config,
        conn: &'a rusqlite::Connection,
        agent: &'a dyn Agent,
        since: &str,
        date_label: &str,
        template: Option<&str>,
    ) -> Self {
        Self {
            config,
            conn,
            agent,
            since: since.to_string(),
            date_label: date_label.to_string(),
            template: template.map(String::from),
            roadmap: Roadmap::load(),
            summary_length: config.summary_length().to_string(),
            issues: HashMap::new(),
            issue_rows: HashMap::new(),
            issue_bodies: HashMap::new(),
            comments: HashMap::new(),
            per_issue_summaries: HashMap::new(),
            commit_logs: HashMap::new(),
            commit_rows: HashMap::new(),
            repo_sections: Vec::new(),
            all_recent_issues: Vec::new(),
            team_stats: Vec::new(),
            executive_summary: None,
        }
    }
}
```

**Step 2: Add `pub mod tasks;` to `src/lib.rs`**

Add one line after the existing modules:

```rust
pub mod tasks;
```

**Step 3: Create stub files for all task modules**

Create each of these with just a comment placeholder so the module compiles:

- `src/tasks/fetch_data.rs` — `// FetchDataTask`
- `src/tasks/summarize.rs` — `// SummarizeIssuesTask`
- `src/tasks/commit_log.rs` — `// BuildCommitLogTask`
- `src/tasks/repo_summary.rs` — `// RepoSummaryTask`
- `src/tasks/triage.rs` — `// TriageTask`
- `src/tasks/team_stats.rs` — `// TeamStatsTask`
- `src/tasks/executive.rs` — `// ExecutiveSummaryTask`

**Step 4: Verify it compiles**

Run: `cargo build`
Expected: Compiles with no errors (warnings about unused fields are OK)

**Step 5: Commit**

```
feat: add Task trait and PipelineContext for task-based pipeline
```

---

### Task 2: Implement `FetchDataTask`

**Files:**
- Modify: `src/tasks/fetch_data.rs`

This task extracts lines 195–221 of the current `pipeline.rs` — the per-repo DB queries that populate issues, comments, and bodies.

**Step 1: Write `FetchDataTask`**

```rust
use log::{debug, info};

use crate::db::{self, CommentRow};
use crate::github::Issue;
use crate::tasks::{PipelineContext, Result, Task};

fn row_to_issue(row: &db::IssueRow) -> Issue {
    let labels: Vec<String> = serde_json::from_str(&row.labels).unwrap_or_default();
    let assignees: Vec<String> = serde_json::from_str(&row.assignees).unwrap_or_default();
    Issue {
        number: row.number,
        title: row.title.clone(),
        kind: row.kind.clone(),
        state: row.state.clone().unwrap_or_else(|| "OPEN".to_string()),
        labels,
        assignees,
        updated_at: chrono::DateTime::parse_from_rfc3339(&row.updated_at)
            .map(|dt| dt.with_timezone(&chrono::Utc))
            .unwrap_or_else(|_| chrono::Utc::now()),
        created_at: chrono::DateTime::parse_from_rfc3339(&row.created_at)
            .map(|dt| dt.with_timezone(&chrono::Utc))
            .unwrap_or_else(|_| chrono::Utc::now()),
        repo: row.repo.clone(),
    }
}

pub struct FetchDataTask;

impl Task for FetchDataTask {
    fn name(&self) -> &str { "fetch-data" }
    fn description(&self) -> &str { "Fetch issues, comments, and commits from database" }
    fn step_count(&self, ctx: &PipelineContext) -> usize { ctx.config.repos.len() }
    fn should_skip(&self, _ctx: &PipelineContext) -> bool { false }

    fn run(&self, ctx: &mut PipelineContext) -> Result<()> {
        for repo_config in &ctx.config.repos {
            info!("Fetching data for repo: {}", repo_config.name);
            let repo = &repo_config.name;
            let repo_names = vec![repo.clone()];

            // Issues
            let issue_rows = db::query_recent_issues(ctx.conn, &repo_names, &ctx.since)?;
            let issues: Vec<Issue> = issue_rows.iter().map(row_to_issue).collect();
            debug!("Found {} recent issues (since {})", issues.len(), ctx.since);

            // Comments
            let issue_numbers: Vec<u64> = issue_rows.iter().map(|r| r.number).collect();
            let comment_rows = db::query_comments_for_issues(ctx.conn, repo, &issue_numbers)?;
            for c in &comment_rows {
                ctx.comments
                    .entry((repo.clone(), c.issue_number))
                    .or_default()
                    .push(c.clone());
            }

            // Bodies
            for r in &issue_rows {
                ctx.issue_bodies
                    .insert((repo.clone(), r.number), r.body.clone().unwrap_or_default());
            }

            // Commits
            let commit_rows = db::query_recent_commits(ctx.conn, &repo_names, &ctx.since)?;
            debug!("Found {} recent commits for {}", commit_rows.len(), repo);
            ctx.commit_rows.insert(repo.clone(), commit_rows);

            // Accumulate all issues for team stats
            for issue in &issues {
                ctx.all_recent_issues.push(issue.clone());
            }

            ctx.issue_rows.insert(repo.clone(), issue_rows);
            ctx.issues.insert(repo.clone(), issues);
        }
        Ok(())
    }
}
```

**Step 2: Verify it compiles**

Run: `cargo build`
Expected: Compiles (warnings OK)

**Step 3: Commit**

```
feat: implement FetchDataTask
```

---

### Task 3: Implement `SummarizeIssuesTask`

**Files:**
- Modify: `src/tasks/summarize.rs`

This extracts lines 226–249 of the current `pipeline.rs` plus the `summarize_issue` helper (lines 93–178) and `compute_discussion_hash` (lines 61–68).

**Step 1: Write `SummarizeIssuesTask`**

```rust
use std::hash::{Hash, Hasher};

use log::debug;

use crate::agent::Agent;
use crate::db::{self, CommentRow};
use crate::github::Issue;
use crate::prompt::{DiscussionSummaryPrompt, IssueDescriptionPrompt};
use crate::report::github_link;
use crate::tasks::{PipelineContext, Result, Task};

fn compute_discussion_hash(comments: &[CommentRow]) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for c in comments {
        c.comment_id.hash(&mut hasher);
        c.body.hash(&mut hasher);
    }
    format!("{:x}", hasher.finish())
}

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
            debug!("Issue #{}: using cached summary (no changes)", issue.number);
            (cache.issue_summary, cache.discussion_summary)
        }
        Some(cache) => {
            debug!("Issue #{}: updating discussion summary (comments changed)", issue.number);
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

pub struct SummarizeIssuesTask;

impl Task for SummarizeIssuesTask {
    fn name(&self) -> &str { "summarize-issues" }
    fn description(&self) -> &str { "Summarize each issue with two-part caching" }

    fn step_count(&self, ctx: &PipelineContext) -> usize {
        ctx.issues.values().map(|v| v.len()).sum()
    }

    fn should_skip(&self, _ctx: &PipelineContext) -> bool { false }

    fn run(&self, ctx: &mut PipelineContext) -> Result<()> {
        for repo_config in &ctx.config.repos {
            let repo = &repo_config.name;
            let issues = match ctx.issues.get(repo) {
                Some(issues) => issues.clone(),
                None => continue,
            };

            let mut summaries = Vec::new();
            for issue in &issues {
                let body = ctx.issue_bodies
                    .get(&(repo.clone(), issue.number))
                    .cloned()
                    .unwrap_or_default();
                let issue_comments: Vec<CommentRow> = ctx.comments
                    .get(&(repo.clone(), issue.number))
                    .cloned()
                    .unwrap_or_default();
                let comments_text: String = issue_comments
                    .iter()
                    .map(|c| format!("{}: {}", github_link(&c.author), c.body))
                    .collect::<Vec<_>>()
                    .join("\n");

                let summary = summarize_issue(
                    ctx.conn, ctx.agent, repo, issue, &body, &issue_comments,
                    &comments_text, &ctx.summary_length,
                )?;
                debug!("Issue #{} summary: {} chars", issue.number, summary.len());

                let prefix = if issue.kind == "pr" { "PR" } else { "Issue" };
                summaries.push(format!(
                    "- {prefix} #{} {}: {}", issue.number, issue.title, summary
                ));
            }
            ctx.per_issue_summaries.insert(repo.clone(), summaries);
        }
        Ok(())
    }
}
```

**Step 2: Verify it compiles**

Run: `cargo build`

**Step 3: Commit**

```
feat: implement SummarizeIssuesTask
```

---

### Task 4: Implement `BuildCommitLogTask`

**Files:**
- Modify: `src/tasks/commit_log.rs`

Extracts lines 251–275 of the current `pipeline.rs`.

**Step 1: Write `BuildCommitLogTask`**

```rust
use std::collections::BTreeMap;

use crate::report::github_link;
use crate::tasks::{PipelineContext, Result, Task};

pub struct BuildCommitLogTask;

impl Task for BuildCommitLogTask {
    fn name(&self) -> &str { "build-commit-log" }
    fn description(&self) -> &str { "Build per-repo commit logs grouped by branch" }
    fn step_count(&self, ctx: &PipelineContext) -> usize { ctx.config.repos.len() }
    fn should_skip(&self, _ctx: &PipelineContext) -> bool { false }

    fn run(&self, ctx: &mut PipelineContext) -> Result<()> {
        for repo_config in &ctx.config.repos {
            let repo = &repo_config.name;
            let commit_rows = match ctx.commit_rows.get(repo) {
                Some(rows) => rows,
                None => {
                    ctx.commit_logs.insert(repo.clone(), String::new());
                    continue;
                }
            };

            let commit_log = {
                let mut by_branch: BTreeMap<&str, Vec<String>> = BTreeMap::new();
                for c in commit_rows {
                    let short_sha = &c.sha[..c.sha.len().min(7)];
                    let first_line = c.message.lines().next().unwrap_or("");
                    let line = format!("- {} {}: {}", short_sha, github_link(&c.author), first_line);
                    let branch_key = if c.branch.is_empty() { "default" } else { &c.branch };
                    by_branch.entry(branch_key).or_default().push(line);
                }
                if by_branch.len() <= 1 {
                    by_branch.into_values().flatten().collect::<Vec<_>>().join("\n")
                } else {
                    by_branch
                        .into_iter()
                        .map(|(branch, lines)| format!("[{branch}]\n{}", lines.join("\n")))
                        .collect::<Vec<_>>()
                        .join("\n\n")
                }
            };
            ctx.commit_logs.insert(repo.clone(), commit_log);
        }
        Ok(())
    }
}
```

**Step 2: Verify it compiles**

Run: `cargo build`

**Step 3: Commit**

```
feat: implement BuildCommitLogTask
```

---

### Task 5: Implement `RepoSummaryTask`

**Files:**
- Modify: `src/tasks/repo_summary.rs`

Extracts lines 277–342 of current `pipeline.rs` plus `compute_data_hash` (lines 45–58).

**Step 1: Write `RepoSummaryTask`**

```rust
use std::hash::{Hash, Hasher};

use log::debug;

use crate::db::{self, CommitRow, IssueRow};
use crate::prompt::WeeklySummaryPrompt;
use crate::report::{extract_xml_tag, RepoSection};
use crate::tasks::{PipelineContext, Result, Task};

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
    fn name(&self) -> &str { "repo-summary" }
    fn description(&self) -> &str { "Generate per-repo weekly summaries with caching" }
    fn step_count(&self, ctx: &PipelineContext) -> usize { ctx.config.repos.len() }
    fn should_skip(&self, _ctx: &PipelineContext) -> bool { false }

    fn run(&self, ctx: &mut PipelineContext) -> Result<()> {
        for repo_config in &ctx.config.repos {
            let repo = &repo_config.name;

            let per_issue_summaries = ctx.per_issue_summaries
                .get(repo)
                .cloned()
                .unwrap_or_default();
            let commit_log = ctx.commit_logs
                .get(repo)
                .cloned()
                .unwrap_or_default();

            let issue_rows = ctx.issue_rows.get(repo).map(|v| v.as_slice()).unwrap_or(&[]);
            let commit_rows = ctx.commit_rows.get(repo).map(|v| v.as_slice()).unwrap_or(&[]);
            let data_hash = compute_data_hash(issue_rows, commit_rows);
            let cached = db::query_report_cache(ctx.conn, repo).ok().flatten();
            let has_activity = !per_issue_summaries.is_empty() || !commit_log.is_empty();

            // Build initiatives context
            let repo_initiatives = ctx.roadmap.for_repo(repo);
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
                        debug!("Using cached summary for {}", repo);
                        cached_summary.clone()
                    } else {
                        let aggregated = per_issue_summaries.join("\n");
                        let prompt = WeeklySummaryPrompt {
                            repo: repo.clone(),
                            issue_summaries: aggregated,
                            commit_log,
                            previous_summary: Some(cached_summary.clone()),
                            initiatives: initiatives_text.clone(),
                        };
                        let summary = ctx.agent.invoke(&prompt).map_err(|e| {
                            eprintln!("  Error generating repo summary: {e}");
                            e
                        })?;
                        let _ = db::save_report_cache(ctx.conn, repo, &summary, &data_hash);
                        summary
                    }
                } else {
                    let aggregated = per_issue_summaries.join("\n");
                    let prompt = WeeklySummaryPrompt {
                        repo: repo.clone(),
                        issue_summaries: aggregated,
                        commit_log,
                        previous_summary: None,
                        initiatives: initiatives_text.clone(),
                    };
                    let summary = ctx.agent.invoke(&prompt).map_err(|e| {
                        eprintln!("  Error generating repo summary: {e}");
                        e
                    })?;
                    let _ = db::save_report_cache(ctx.conn, repo, &summary, &data_hash);
                    summary
                };
                (
                    extract_xml_tag(&raw, "done"),
                    extract_xml_tag(&raw, "in_progress"),
                    extract_xml_tag(&raw, "next"),
                )
            };

            ctx.repo_sections.push(RepoSection {
                name: repo.clone(),
                done,
                in_progress,
                next,
                flagged_issues: Vec::new(), // Filled by TriageTask
            });
        }
        Ok(())
    }
}
```

**Step 2: Verify it compiles**

Run: `cargo build`

**Step 3: Commit**

```
feat: implement RepoSummaryTask
```

---

### Task 6: Implement `TriageTask`

**Files:**
- Modify: `src/tasks/triage.rs`

Extracts lines 344–379 of current `pipeline.rs`.

**Step 1: Write `TriageTask`**

```rust
use log::debug;

use crate::filter;
use crate::prompt::IssueTriagePrompt;
use crate::report::{github_link, FlaggedIssue};
use crate::tasks::{PipelineContext, Result, Task};

pub struct TriageTask;

impl Task for TriageTask {
    fn name(&self) -> &str { "triage" }
    fn description(&self) -> &str { "Triage issues missing required labels" }

    fn step_count(&self, ctx: &PipelineContext) -> usize {
        ctx.config.repos.iter().map(|r| {
            let issues = match ctx.issues.get(&r.name) {
                Some(issues) => issues,
                None => return 0,
            };
            let refs: Vec<_> = issues.iter().collect();
            filter::find_flagged_issues(&refs, &r.labels_required).len()
        }).sum()
    }

    fn should_skip(&self, ctx: &PipelineContext) -> bool {
        ctx.config.repos.iter().all(|r| r.labels_required.is_empty())
    }

    fn run(&self, ctx: &mut PipelineContext) -> Result<()> {
        for (section_idx, repo_config) in ctx.config.repos.iter().enumerate() {
            let repo = &repo_config.name;
            let issues = match ctx.issues.get(repo) {
                Some(issues) => issues.clone(),
                None => continue,
            };

            let issue_refs: Vec<_> = issues.iter().collect();
            let flagged_refs = filter::find_flagged_issues(&issue_refs, &repo_config.labels_required);
            debug!("Found {} flagged issues in {}", flagged_refs.len(), repo);

            let mut flagged_issues = Vec::new();
            for issue in &flagged_refs {
                debug!("Triaging issue #{}: {}", issue.number, issue.title);

                let body = ctx.issue_bodies
                    .get(&(repo.clone(), issue.number))
                    .cloned()
                    .unwrap_or_default();
                let comments_text = ctx.comments
                    .get(&(repo.clone(), issue.number))
                    .map(|v| {
                        v.iter()
                            .map(|c| format!("{}: {}", github_link(&c.author), c.body))
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
                    .unwrap_or_default();

                let triage_prompt = IssueTriagePrompt {
                    title: issue.title.clone(),
                    body,
                    comments: comments_text,
                };
                let triage_summary = ctx.agent.invoke(&triage_prompt).map_err(|e| {
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

            // Attach flagged issues to the corresponding repo section
            if let Some(section) = ctx.repo_sections.get_mut(section_idx) {
                section.flagged_issues = flagged_issues;
            }
        }
        Ok(())
    }
}
```

**Step 2: Verify it compiles**

Run: `cargo build`

**Step 3: Commit**

```
feat: implement TriageTask
```

---

### Task 7: Implement `TeamStatsTask`

**Files:**
- Modify: `src/tasks/team_stats.rs`

Extracts lines 391–415 of current `pipeline.rs`.

**Step 1: Write `TeamStatsTask`**

```rust
use crate::report::TeamStats;
use crate::tasks::{PipelineContext, Result, Task};

pub struct TeamStatsTask;

impl Task for TeamStatsTask {
    fn name(&self) -> &str { "team-stats" }
    fn description(&self) -> &str { "Compute per-member active and closed issue counts" }
    fn step_count(&self, ctx: &PipelineContext) -> usize { ctx.config.team.len() }
    fn should_skip(&self, ctx: &PipelineContext) -> bool { ctx.config.team.is_empty() }

    fn run(&self, ctx: &mut PipelineContext) -> Result<()> {
        ctx.team_stats = ctx.config
            .team
            .iter()
            .map(|member| {
                let (active, closed_this_week) = ctx.all_recent_issues
                    .iter()
                    .filter(|i| i.assignees.contains(&member.github))
                    .fold((0, 0), |(open, closed), i| {
                        if i.state.eq_ignore_ascii_case("OPEN") {
                            (open + 1, closed)
                        } else if i.state.eq_ignore_ascii_case("CLOSED") {
                            (open, closed + 1)
                        } else {
                            (open, closed)
                        }
                    });
                TeamStats {
                    name: member.name.clone(),
                    github: member.github.clone(),
                    active,
                    closed_this_week,
                }
            })
            .collect();
        Ok(())
    }
}
```

**Step 2: Verify it compiles**

Run: `cargo build`

**Step 3: Commit**

```
feat: implement TeamStatsTask
```

---

### Task 8: Implement `ExecutiveSummaryTask`

**Files:**
- Modify: `src/tasks/executive.rs`

Extracts lines 417–453 of current `pipeline.rs`.

**Step 1: Write `ExecutiveSummaryTask`**

```rust
use std::fmt::Write;

use crate::error::{AgentError, PipelineError};
use crate::prompt::{resolve_template, ExecutiveSummaryPrompt};
use crate::tasks::{PipelineContext, Result, Task};

pub struct ExecutiveSummaryTask;

impl Task for ExecutiveSummaryTask {
    fn name(&self) -> &str { "executive-summary" }
    fn description(&self) -> &str { "Generate cross-repo executive summary from template" }
    fn step_count(&self, _ctx: &PipelineContext) -> usize { 1 }

    fn should_skip(&self, ctx: &PipelineContext) -> bool {
        ctx.template.is_none()
    }

    fn run(&self, ctx: &mut PipelineContext) -> Result<()> {
        let template_name = match &ctx.template {
            Some(name) => name.clone(),
            None => return Ok(()),
        };

        let template_text = resolve_template(&template_name)
            .ok_or_else(|| PipelineError::Agent(
                AgentError::ExitError(format!("Unknown template: {template_name}"))
            ))?;

        let mut repo_text = String::new();
        for section in &ctx.repo_sections {
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

        let prompt = ExecutiveSummaryPrompt {
            repo_summaries: repo_text,
            template: template_text,
        };
        let summary = ctx.agent.invoke(&prompt).map_err(|e| {
            eprintln!("  Error generating executive summary: {e}");
            e
        })?;
        ctx.executive_summary = Some(summary);
        Ok(())
    }
}
```

**Step 2: Verify it compiles**

Run: `cargo build`

**Step 3: Commit**

```
feat: implement ExecutiveSummaryTask
```

---

### Task 9: Update `PipelineProgress` trait and rewrite executor

**Files:**
- Modify: `src/pipeline.rs`
- Modify: `src/main.rs`

This is the switchover: replace the monolithic `run_pipeline` body with the task executor loop, add new progress methods.

**Step 1: Update `PipelineProgress` trait in `src/pipeline.rs`**

Add three new methods with default no-op implementations (so `NullProgress` and `ReportProgress` don't break immediately):

```rust
pub trait PipelineProgress {
    fn task_start(&self, _name: &str, _step_count: usize) {}
    fn task_skipped(&self, _name: &str) {}
    fn task_done(&self, _name: &str) {}
    fn repo_start(&self, _repo: &str, _issue_count: usize) {}
    fn issue_step(&self, _index: usize, _total: usize, _number: u64, _title: &str) {}
    fn phase(&self, _msg: &str) {}
    fn repo_done(&self, _repo: &str) {}
    fn finish(&self) {}
}
```

Remove the explicit `NullProgress` impl block since the defaults now cover it. Keep the `pub struct NullProgress;` and add:

```rust
impl PipelineProgress for NullProgress {}
```

**Step 2: Rewrite `run_pipeline` as task executor**

Replace the entire `run_pipeline` function body (and remove all the now-unused private helper functions `compute_data_hash`, `compute_discussion_hash`, `row_to_issue`, `summarize_issue`) with:

```rust
use crate::error::PipelineError;
use crate::report::Report;
use crate::tasks::{self, PipelineContext, Task};
use crate::tasks::fetch_data::FetchDataTask;
use crate::tasks::summarize::SummarizeIssuesTask;
use crate::tasks::commit_log::BuildCommitLogTask;
use crate::tasks::repo_summary::RepoSummaryTask;
use crate::tasks::triage::TriageTask;
use crate::tasks::team_stats::TeamStatsTask;
use crate::tasks::executive::ExecutiveSummaryTask;

type Result<T> = std::result::Result<T, PipelineError>;

pub trait PipelineProgress {
    fn task_start(&self, _name: &str, _step_count: usize) {}
    fn task_skipped(&self, _name: &str) {}
    fn task_done(&self, _name: &str) {}
    fn repo_start(&self, _repo: &str, _issue_count: usize) {}
    fn issue_step(&self, _index: usize, _total: usize, _number: u64, _title: &str) {}
    fn phase(&self, _msg: &str) {}
    fn repo_done(&self, _repo: &str) {}
    fn finish(&self) {}
}

pub struct NullProgress;
impl PipelineProgress for NullProgress {}

pub fn run_pipeline(
    config: &crate::config::Config,
    conn: &rusqlite::Connection,
    agent: &dyn crate::agent::Agent,
    since: &str,
    date_label: &str,
    progress: &dyn PipelineProgress,
    template: Option<&str>,
) -> Result<Report> {
    let mut ctx = PipelineContext::new(config, conn, agent, since, date_label, template);

    let tasks: Vec<Box<dyn Task>> = vec![
        Box::new(FetchDataTask),
        Box::new(SummarizeIssuesTask),
        Box::new(BuildCommitLogTask),
        Box::new(RepoSummaryTask),
        Box::new(TriageTask),
        Box::new(TeamStatsTask),
        Box::new(ExecutiveSummaryTask),
    ];

    for task in &tasks {
        if task.should_skip(&ctx) {
            progress.task_skipped(task.name());
            continue;
        }
        progress.task_start(task.name(), task.step_count(&ctx));
        task.run(&mut ctx)?;
        progress.task_done(task.name());
    }

    progress.finish();
    Ok(Report {
        date: ctx.date_label.clone(),
        executive_summary: ctx.executive_summary,
        repos: ctx.repo_sections,
        team_stats: ctx.team_stats,
    })
}
```

**Step 3: Add `task_start`/`task_skipped`/`task_done` to `ReportProgress` in `src/main.rs`**

In the `impl PipelineProgress for ReportProgress` block, add:

```rust
fn task_start(&self, name: &str, step_count: usize) {
    if step_count > 0 {
        self.set_bar(step_count as u64, name.to_string());
    } else {
        self.set_spinner(name.to_string());
    }
}

fn task_skipped(&self, _name: &str) {}

fn task_done(&self, name: &str) {
    let mut guard = self.bar.lock().unwrap();
    if let Some(pb) = guard.take() {
        pb.finish_and_clear();
    }
    eprintln!("  ✓ {name}");
}
```

**Step 4: Run tests**

Run: `cargo test`
Expected: All 76 tests pass — the `run_pipeline` signature hasn't changed, just the internals.

**Step 5: Commit**

```
feat: rewrite pipeline as task executor

Replace 280-line monolithic run_pipeline with a thin executor
that runs 7 self-contained Task implementations in sequence,
sharing state via PipelineContext.
```

---

### Task 10: Clean up old imports and dead code

**Files:**
- Modify: `src/pipeline.rs` — remove any leftover unused imports

**Step 1: Run `cargo build` and fix any warnings about unused imports**

The old `pipeline.rs` imported many types (`chrono`, `log`, `filter`, `github`, `prompt`, `report`, `roadmap`, etc.) that are now used inside the task modules instead. Remove all unused imports from `pipeline.rs`.

**Step 2: Run full test suite**

Run: `cargo test`
Expected: All tests pass, no warnings

**Step 3: Commit**

```
chore: remove dead code from pipeline.rs after task extraction
```

---

### Task 11: Verify end-to-end

**Step 1: Run the full test suite one final time**

Run: `cargo test`
Expected: All tests pass

**Step 2: Run clippy for lint check**

Run: `cargo clippy -- -D warnings`
Expected: No warnings

**Step 3: Spot-check the file layout**

Run: `find src/tasks -name '*.rs' | sort`
Expected:
```
src/tasks/commit_log.rs
src/tasks/executive.rs
src/tasks/fetch_data.rs
src/tasks/mod.rs
src/tasks/repo_summary.rs
src/tasks/summarize.rs
src/tasks/team_stats.rs
src/tasks/triage.rs
```

**Step 4: Commit any final fixes if needed**
