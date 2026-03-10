# Batch & Parallel Summarization Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Reduce LLM API call count via batch prompting, then add async parallelism to run remaining calls concurrently.

**Architecture:** Phase 1 adds a `BatchIssueDescriptionPrompt` that sends multiple issues in one LLM call with XML-tagged responses. Phase 2 converts the pipeline to async with tokio, adding a configurable concurrency semaphore for parallel agent calls while keeping DB access serial on the main task.

**Tech Stack:** Rust, tokio (rt-multi-thread), rusqlite, existing `Agent` trait + CLI subprocess agents

---

### Task 1: Add `batch_size` config field

**Files:**
- Modify: `src/config.rs`
- Test: `tests/config_test.rs`

**Step 1: Write the failing test**

Add to `tests/config_test.rs`:

```rust
#[test]
fn batch_size_defaults_to_10() {
    let config: Config = toml::from_str(r#"
        [[repos]]
        name = "org/repo"
    "#).unwrap();
    assert_eq!(config.batch_size(), 10);
}

#[test]
fn batch_size_is_configurable() {
    let config: Config = toml::from_str(r#"
        batch_size = 5
        [[repos]]
        name = "org/repo"
    "#).unwrap();
    assert_eq!(config.batch_size(), 5);
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test batch_size -- --nocapture`
Expected: FAIL — `batch_size` method doesn't exist

**Step 3: Add `batch_size` field to Config**

In `src/config.rs`, add to the `Config` struct:

```rust
/// Number of issues to batch into a single LLM description call. Default: 10.
#[serde(default, skip_serializing_if = "Option::is_none")]
pub batch_size: Option<usize>,
```

Add a helper method:

```rust
pub fn batch_size(&self) -> usize {
    self.batch_size.unwrap_or(10)
}
```

Also add get/set support in `get_field` and `set_field`:

In `get_field`, add before the `_ =>` arm:
```rust
"batch_size" => Ok(self.batch_size().to_string()),
```

In `set_field`, add before the `_ =>` arm:
```rust
"batch_size" => {
    let n: usize = value.parse().map_err(|_| ConfigError::InvalidValue {
        key: key.to_string(),
        message: format!("expected positive integer, got: {value}"),
    })?;
    self.batch_size = if n == 10 { None } else { Some(n) };
}
```

**Step 4: Run tests**

Run: `cargo test batch_size -- --nocapture`
Expected: PASS

**Step 5: Commit**

```bash
git add src/config.rs tests/config_test.rs
git commit -m "feat: add batch_size config field (default 10)"
```

---

### Task 2: Add `BatchIssueDescriptionPrompt` and XML parsing

**Files:**
- Modify: `src/prompt.rs`
- Modify: `src/report.rs` (add `extract_xml_tag_with_attr`)
- Test: `tests/report_test.rs`

**Step 1: Write tests for XML extraction with attributes**

Add to `tests/report_test.rs`:

```rust
use ceo::report::extract_all_summary_tags;

#[test]
fn extract_all_summary_tags_parses_batch_response() {
    let input = r#"
<summary id="42">Auth bug fix summary.</summary>
<summary id="57">New dashboard feature.</summary>
    "#;
    let results = extract_all_summary_tags(input);
    assert_eq!(results.len(), 2);
    assert_eq!(results[0], (42, "Auth bug fix summary.".to_string()));
    assert_eq!(results[1], (57, "New dashboard feature.".to_string()));
}

#[test]
fn extract_all_summary_tags_handles_multiline() {
    let input = r#"<summary id="1">Line one.
Line two.</summary>"#;
    let results = extract_all_summary_tags(input);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0], (1, "Line one.\nLine two.".to_string()));
}

#[test]
fn extract_all_summary_tags_handles_empty() {
    let results = extract_all_summary_tags("no tags here");
    assert!(results.is_empty());
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test extract_all_summary_tags -- --nocapture`
Expected: FAIL — function doesn't exist

**Step 3: Implement `extract_all_summary_tags` in `src/report.rs`**

```rust
/// Extract all `<summary id="N">...</summary>` tags from a batch response.
/// Returns a vec of (issue_number, summary_text) tuples.
pub fn extract_all_summary_tags(text: &str) -> Vec<(u64, String)> {
    let mut results = Vec::new();
    let mut search_from = 0;
    let open_prefix = "<summary id=\"";

    while let Some(tag_start) = text[search_from..].find(open_prefix) {
        let abs_start = search_from + tag_start;
        let after_prefix = abs_start + open_prefix.len();

        // Find the closing quote of the id attribute
        let Some(quote_end) = text[after_prefix..].find('"') else { break };
        let id_str = &text[after_prefix..after_prefix + quote_end];
        let Ok(id) = id_str.parse::<u64>() else {
            search_from = after_prefix;
            continue;
        };

        // Find the end of the opening tag ">"
        let after_id = after_prefix + quote_end + 1;
        let Some(gt_offset) = text[after_id..].find('>') else { break };
        let content_start = after_id + gt_offset + 1;

        // Find the closing tag
        let close_tag = "</summary>";
        let Some(close_offset) = text[content_start..].find(close_tag) else { break };
        let content = text[content_start..content_start + close_offset].trim();
        if !content.is_empty() {
            results.push((id, content.to_string()));
        }

        search_from = content_start + close_offset + close_tag.len();
    }

    results
}
```

**Step 4: Run tests**

Run: `cargo test extract_all_summary_tags -- --nocapture`
Expected: PASS

**Step 5: Add `BatchIssueDescriptionPrompt` to `src/prompt.rs`**

```rust
/// Batch-summarize multiple issue descriptions in a single LLM call.
/// Returns XML-tagged summaries: `<summary id="N">...</summary>` per issue.
pub struct BatchIssueDescriptionPrompt {
    pub issues: Vec<BatchIssueEntry>,
    pub summary_length: String,
}

pub struct BatchIssueEntry {
    pub repo: String,
    pub number: u64,
    pub title: String,
    pub kind: String,
    pub labels: String,
    pub assignees: String,
    pub body: String,
}

impl Prompt for BatchIssueDescriptionPrompt {
    fn kind(&self) -> &str { "summary" }

    fn render(&self) -> String {
        let mut out = format!(
            "Summarize each of the following GitHub issues/PRs in {}. \
             Focus on purpose and scope — do NOT summarize discussion or comments.\n\
             All information you need is provided below — do NOT fetch external data.\n\
             When referencing GitHub entities, use short tags: \
             <gh>handle</gh> for users, <issue>N</issue> for issues, <pr>N</pr> for PRs.\n\n\
             Respond with exactly one <summary id=\"N\">...</summary> tag per issue, \
             where N is the issue number.\n\n",
            self.summary_length
        );
        for entry in &self.issues {
            let kind_label = if entry.kind == "pr" { "PR" } else { "Issue" };
            out.push_str(&format!(
                "<issue id=\"{}\">\n\
                 Repo: {}\n\
                 {} #{}: {}\n\
                 Labels: {}\n\
                 Assignees: {}\n\
                 Description:\n{}\n\
                 </issue>\n\n",
                entry.number, entry.repo,
                kind_label, entry.number, entry.title,
                entry.labels, entry.assignees,
                entry.body,
            ));
        }
        out
    }
}
```

**Step 6: Verify it compiles**

Run: `cargo check`
Expected: PASS

**Step 7: Commit**

```bash
git add src/prompt.rs src/report.rs tests/report_test.rs
git commit -m "feat: add BatchIssueDescriptionPrompt and XML batch parsing"
```

---

### Task 3: Wire batch prompting into SummarizeIssuesTask

**Files:**
- Modify: `src/tasks/summarize.rs`

**Step 1: Read the current code**

Read `src/tasks/summarize.rs` fully (already provided above).

**Step 2: Rewrite `SummarizeIssuesTask::run` to use batching**

The key change: instead of calling `summarize_issue` one-by-one in the inner loop, we:
1. Separate issues into two groups: cached (have issue_summary already) and uncached
2. For uncached issues, batch them into groups of `batch_size` and call `BatchIssueDescriptionPrompt`
3. For any issues missing from the batch response, fall back to individual `IssueDescriptionPrompt`
4. Discussion summaries remain individual calls (unchanged)

Replace the `run` method in `SummarizeIssuesTask`:

```rust
fn run(&self, ctx: &mut PipelineContext) -> Result<()> {
    let batch_size = ctx.config.batch_size();

    for repo_config in &ctx.config.repos {
        let repo_name = &repo_config.name;
        let issues = match ctx.issues.get(repo_name) {
            Some(issues) => issues.clone(),
            None => continue,
        };

        // Partition issues by cache status
        let mut cached_summaries: HashMap<u64, (String, String, String)> = HashMap::new();
        let mut needs_description: Vec<&Issue> = Vec::new();
        let mut needs_discussion_update: Vec<(&Issue, String)> = Vec::new();

        for issue in &issues {
            let issue_comments: Vec<CommentRow> = ctx
                .comments
                .get(&(repo_name.clone(), issue.number))
                .cloned()
                .unwrap_or_default();
            let discussion_hash = compute_discussion_hash(&issue_comments);
            let cached = db::query_issue_cache(ctx.conn, repo_name, issue.number)
                .ok()
                .flatten();

            match cached {
                Some(cache) if cache.discussion_hash == discussion_hash => {
                    debug!("Issue #{}: cache hit, no changes", issue.number);
                    cached_summaries.insert(
                        issue.number,
                        (cache.issue_summary, cache.discussion_summary, discussion_hash),
                    );
                }
                Some(cache) => {
                    debug!("Issue #{}: discussion changed, keeping description", issue.number);
                    cached_summaries.insert(
                        issue.number,
                        (cache.issue_summary, String::new(), discussion_hash),
                    );
                    needs_discussion_update.push((issue, cache.issue_summary));
                }
                None => {
                    debug!("Issue #{}: no cache, needs full summarization", issue.number);
                    needs_description.push(issue);
                }
            }
        }

        // Batch-summarize uncached issue descriptions
        let mut description_results: HashMap<u64, String> = HashMap::new();

        for chunk in needs_description.chunks(batch_size) {
            let entries: Vec<BatchIssueEntry> = chunk
                .iter()
                .map(|issue| {
                    let linked_assignees: Vec<String> =
                        issue.assignees.iter().map(|a| github_link(a)).collect();
                    let body = ctx
                        .issue_bodies
                        .get(&(repo_name.clone(), issue.number))
                        .cloned()
                        .unwrap_or_default();
                    BatchIssueEntry {
                        repo: repo_name.clone(),
                        number: issue.number,
                        title: issue.title.clone(),
                        kind: issue.kind.clone(),
                        labels: issue.labels.join(", "),
                        assignees: linked_assignees.join(", "),
                        body,
                    }
                })
                .collect();

            if entries.len() == 1 {
                // Single issue — use individual prompt (simpler, no XML parsing needed)
                let entry = &entries[0];
                let prompt = IssueDescriptionPrompt {
                    repo: entry.repo.clone(),
                    number: entry.number,
                    title: entry.title.clone(),
                    kind: entry.kind.clone(),
                    labels: entry.labels.clone(),
                    assignees: entry.assignees.clone(),
                    body: entry.body.clone(),
                    summary_length: ctx.summary_length.clone(),
                };
                let summary = ctx.agent.invoke(&prompt).map_err(|e| {
                    eprintln!("  Error summarizing #{}: {e}", entry.number);
                    e
                })?;
                description_results.insert(entry.number, summary);
            } else {
                let batch_prompt = BatchIssueDescriptionPrompt {
                    issues: entries,
                    summary_length: ctx.summary_length.clone(),
                };
                let response = ctx.agent.invoke(&batch_prompt).map_err(|e| {
                    eprintln!("  Error in batch summarization: {e}");
                    e
                })?;
                let parsed = extract_all_summary_tags(&response);
                for (number, summary) in parsed {
                    description_results.insert(number, summary);
                }

                // Fallback: any issues missing from batch response get individual calls
                for issue in chunk {
                    if !description_results.contains_key(&issue.number) {
                        debug!(
                            "Issue #{}: missing from batch response, falling back",
                            issue.number
                        );
                        let linked_assignees: Vec<String> =
                            issue.assignees.iter().map(|a| github_link(a)).collect();
                        let body = ctx
                            .issue_bodies
                            .get(&(repo_name.clone(), issue.number))
                            .cloned()
                            .unwrap_or_default();
                        let prompt = IssueDescriptionPrompt {
                            repo: repo_name.clone(),
                            number: issue.number,
                            title: issue.title.clone(),
                            kind: issue.kind.clone(),
                            labels: issue.labels.join(", "),
                            assignees: linked_assignees.join(", "),
                            body,
                            summary_length: ctx.summary_length.clone(),
                        };
                        let summary = ctx.agent.invoke(&prompt).map_err(|e| {
                            eprintln!("  Error summarizing #{}: {e}", issue.number);
                            e
                        })?;
                        description_results.insert(issue.number, summary);
                    }
                }
            }
        }

        // Now handle discussion summaries for all uncached issues + those with changed discussions
        let mut per_issue_summaries = Vec::new();

        for issue in &issues {
            let issue_comments: Vec<CommentRow> = ctx
                .comments
                .get(&(repo_name.clone(), issue.number))
                .cloned()
                .unwrap_or_default();
            let discussion_hash = compute_discussion_hash(&issue_comments);
            let comments_text: String = issue_comments
                .iter()
                .map(|c| format!("{}: {}", github_link(&c.author), c.body))
                .collect::<Vec<_>>()
                .join("\n");

            let (issue_summary, discussion_summary) = if let Some((is, ds, _)) =
                cached_summaries.get(&issue.number)
            {
                if !ds.is_empty() {
                    // Full cache hit — use as-is
                    (is.clone(), ds.clone())
                } else {
                    // Description cached but discussion needs update
                    let disc_prompt = DiscussionSummaryPrompt {
                        repo: repo_name.clone(),
                        number: issue.number,
                        title: issue.title.clone(),
                        comments: comments_text,
                        previous_summary: None, // cache had stale discussion
                        summary_length: ctx.summary_length.clone(),
                    };
                    let new_disc = ctx.agent.invoke(&disc_prompt).map_err(|e| {
                        eprintln!("  Error updating discussion #{}: {e}", issue.number);
                        e
                    })?;
                    let _ = db::save_issue_cache(
                        ctx.conn,
                        repo_name,
                        issue.number,
                        is,
                        &new_disc,
                        &discussion_hash,
                    );
                    (is.clone(), new_disc)
                }
            } else {
                // Newly described issue — get discussion summary too
                let is = description_results
                    .get(&issue.number)
                    .cloned()
                    .unwrap_or_else(|| "Summary unavailable.".to_string());

                let ds = if comments_text.is_empty() {
                    "No discussion yet.".to_string()
                } else {
                    let disc_prompt = DiscussionSummaryPrompt {
                        repo: repo_name.clone(),
                        number: issue.number,
                        title: issue.title.clone(),
                        comments: comments_text,
                        previous_summary: None,
                        summary_length: ctx.summary_length.clone(),
                    };
                    ctx.agent.invoke(&disc_prompt).map_err(|e| {
                        eprintln!("  Error summarizing discussion #{}: {e}", issue.number);
                        e
                    })?
                };

                let _ = db::save_issue_cache(
                    ctx.conn,
                    repo_name,
                    issue.number,
                    &is,
                    &ds,
                    &discussion_hash,
                );
                (is, ds)
            };

            let prefix = if issue.kind == "pr" { "PR" } else { "Issue" };
            per_issue_summaries.push(format!(
                "- {prefix} #{} {}: {} {}",
                issue.number, issue.title, issue_summary, discussion_summary
            ));
        }

        ctx.per_issue_summaries
            .insert(repo_name.clone(), per_issue_summaries);
    }

    Ok(())
}
```

Update imports at the top of `src/tasks/summarize.rs`:

```rust
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use log::debug;

use crate::db::{self, CommentRow};
use crate::github::Issue;
use crate::prompt::{
    BatchIssueDescriptionPrompt, BatchIssueEntry, DiscussionSummaryPrompt,
    IssueDescriptionPrompt,
};
use crate::report::{extract_all_summary_tags, github_link};

use super::{PipelineContext, Result, Task};
```

The old `summarize_issue` function can be removed entirely.

**Step 3: Run tests**

Run: `cargo test -- --nocapture`
Expected: All existing tests pass. The `MockAgent` in tests returns a flat string that works for both individual and batch paths (the fallback handles it).

**Step 4: Commit**

```bash
git add src/tasks/summarize.rs
git commit -m "feat: batch issue description summarization to reduce API calls"
```

---

### Task 4: Add `concurrency` config field

**Files:**
- Modify: `src/config.rs`
- Test: `tests/config_test.rs`

**Step 1: Write the failing test**

Add to `tests/config_test.rs`:

```rust
#[test]
fn concurrency_defaults_to_4() {
    let config: Config = toml::from_str(r#"
        [[repos]]
        name = "org/repo"
    "#).unwrap();
    assert_eq!(config.concurrency(), 4);
}

#[test]
fn concurrency_is_configurable() {
    let config: Config = toml::from_str(r#"
        concurrency = 8
        [[repos]]
        name = "org/repo"
    "#).unwrap();
    assert_eq!(config.concurrency(), 8);
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test concurrency -- --nocapture`
Expected: FAIL

**Step 3: Add `concurrency` field to Config**

In `src/config.rs`, add to the `Config` struct:

```rust
/// Maximum number of concurrent agent calls. Default: 4.
#[serde(default, skip_serializing_if = "Option::is_none")]
pub concurrency: Option<usize>,
```

Add helper method:

```rust
pub fn concurrency(&self) -> usize {
    self.concurrency.unwrap_or(4)
}
```

Add get/set support (same pattern as `batch_size`).

**Step 4: Run tests**

Run: `cargo test concurrency -- --nocapture`
Expected: PASS

**Step 5: Commit**

```bash
git add src/config.rs tests/config_test.rs
git commit -m "feat: add concurrency config field (default 4)"
```

---

### Task 5: Add tokio dependency and convert Agent trait to async

**Files:**
- Modify: `Cargo.toml`
- Modify: `src/agent.rs`
- Modify: `src/prompt.rs` (make Prompt Send + Sync)

**Step 1: Add tokio dependency**

In root `Cargo.toml`, add to `[dependencies]`:

```toml
tokio = { version = "1", features = ["rt-multi-thread", "process", "macros", "sync"] }
```

**Step 2: Convert Agent trait to async**

In `src/agent.rs`:

Add at the top:
```rust
use std::future::Future;
use std::pin::Pin;
```

Change the `Agent` trait:

```rust
pub trait Agent: Send + Sync {
    fn invoke(&self, prompt: &dyn Prompt) -> Pin<Box<dyn Future<Output = Result<String>> + Send + '_>>;
}
```

Note: We use `Pin<Box<dyn Future>>` rather than `async fn` in the trait because async trait methods with `dyn` dispatch require boxing. This avoids the `async-trait` crate dependency.

**Step 3: Convert `run_cli_agent` to async**

Replace `std::process::Command` with `tokio::process::Command`:

```rust
async fn run_cli_agent(command: &str, args: &[&str], prompt_text: &str) -> Result<String> {
    debug!("Running agent: {} {}", command, args.join(" "));
    debug!("Prompt length: {} chars", prompt_text.len());
    let mut child = tokio::process::Command::new(command)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| AgentError::NotFound { command: command.to_string(), source: e })?;

    if let Some(mut stdin) = child.stdin.take() {
        use tokio::io::AsyncWriteExt;
        stdin.write_all(prompt_text.as_bytes())
            .await
            .map_err(AgentError::OutputRead)?;
    }

    let output = child
        .wait_with_output()
        .await
        .map_err(AgentError::OutputRead)?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let detail = if !stderr.is_empty() {
            stderr.to_string()
        } else if !stdout.is_empty() {
            stdout.to_string()
        } else {
            format!("exit code: {}", output.status)
        };
        return Err(AgentError::ExitError(detail));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}
```

**Step 4: Update each agent impl**

For `ClaudeAgent`, `CodexAgent`, `GenericAgent`, change `invoke` to return boxed futures:

```rust
impl Agent for ClaudeAgent {
    fn invoke(&self, prompt: &dyn Prompt) -> Pin<Box<dyn Future<Output = Result<String>> + Send + '_>> {
        let rendered = prompt.render();
        let kind = prompt.kind().to_string();
        let model = self.config.model_for(&kind).to_string();
        // ... build args as before ...
        Box::pin(async move {
            let args_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
            run_cli_agent(&command, &args_refs, &rendered).await
        })
    }
}
```

Apply same pattern to `CodexAgent` and `GenericAgent`. Each captures needed values (rendered prompt, args) into the async block.

For `AgentKind`:

```rust
impl Agent for AgentKind {
    fn invoke(&self, prompt: &dyn Prompt) -> Pin<Box<dyn Future<Output = Result<String>> + Send + '_>> {
        match self {
            AgentKind::Claude(a) => a.invoke(prompt),
            AgentKind::Codex(a) => a.invoke(prompt),
            AgentKind::Generic(a) => a.invoke(prompt),
        }
    }
}
```

**Step 5: Make `Prompt` trait `Send + Sync`**

In `src/prompt.rs`, update the trait:

```rust
pub trait Prompt: Send + Sync {
    fn render(&self) -> String;
    fn kind(&self) -> &str;
    fn required_tools(&self) -> &[&str] { &[] }
}
```

**Step 6: Verify it compiles**

Run: `cargo check`
Expected: Compilation errors in pipeline.rs and task files (they call `.invoke()` synchronously). That's expected — we fix those in the next tasks.

**Step 7: Commit**

```bash
git add Cargo.toml src/agent.rs src/prompt.rs
git commit -m "feat: convert Agent trait to async with tokio"
```

---

### Task 6: Convert pipeline and tasks to async

**Files:**
- Modify: `src/pipeline.rs`
- Modify: `src/tasks/mod.rs`
- Modify: `src/tasks/summarize.rs`
- Modify: `src/tasks/repo_summary.rs`
- Modify: `src/tasks/triage.rs`
- Modify: `src/tasks/executive.rs`
- Modify: `src/tasks/fetch_data.rs`
- Modify: `src/tasks/commit_log.rs`
- Modify: `src/tasks/team_stats.rs`

**Step 1: Convert `Task` trait to async**

In `src/tasks/mod.rs`:

```rust
use std::future::Future;
use std::pin::Pin;

pub trait Task {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn step_count(&self, ctx: &PipelineContext) -> usize;
    fn should_skip(&self, ctx: &PipelineContext) -> bool;
    fn run<'a>(&'a self, ctx: &'a mut PipelineContext<'a>) -> Pin<Box<dyn Future<Output = Result<()>> + 'a>>;
}
```

**Step 2: Convert `run_pipeline` to async**

In `src/pipeline.rs`:

```rust
pub async fn run_pipeline(
    config: &Config,
    conn: &rusqlite::Connection,
    agent: &dyn Agent,
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
        task.run(&mut ctx).await?;
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

**Step 3: Convert non-LLM tasks (trivial async wrappers)**

For `FetchDataTask`, `BuildCommitLogTask`, `TeamStatsTask` — these make no agent calls, so just wrap the existing body:

```rust
fn run<'a>(&'a self, ctx: &'a mut PipelineContext<'a>) -> Pin<Box<dyn Future<Output = Result<()>> + 'a>> {
    Box::pin(async move {
        // ... existing synchronous code unchanged ...
        Ok(())
    })
}
```

**Step 4: Convert `SummarizeIssuesTask` to use concurrent discussion calls**

The batch description calls are already sequential (batched). The discussion summary calls can be parallelized using `tokio::sync::Semaphore`.

Strategy:
1. Collect all issues needing discussion summaries with their inputs
2. Fan out with semaphore-limited `tokio::spawn` (but since `conn` isn't Send, we call agent outside and save to DB after)
3. Actually: since `agent` is `&dyn Agent` (not owned), we can't easily spawn. Instead, use `futures::stream::FuturesUnordered` or simply call agent.invoke().await concurrently via `tokio::join!` in batches.

Simpler approach: use `futures::future::join_all` with a semaphore:

Add `futures` to dependencies in Cargo.toml:
```toml
futures = "0.3"
```

In the discussion summary loop of `SummarizeIssuesTask`, collect all discussion calls as futures, run them through the semaphore:

```rust
// Collect discussion work items
struct DiscussionWork {
    number: u64,
    issue_summary: String,
    discussion_hash: String,
    comments_text: String,
    title: String,
    has_previous: bool,
}

let mut discussion_work: Vec<DiscussionWork> = Vec::new();
// ... populate from the issue loop ...

// Fan out discussion calls with concurrency limit
let semaphore = Arc::new(tokio::sync::Semaphore::new(ctx.config.concurrency()));
let mut handles = Vec::new();

for work in &discussion_work {
    let permit = semaphore.clone().acquire_owned().await.unwrap();
    let prompt = DiscussionSummaryPrompt { ... };
    let future = ctx.agent.invoke(&prompt);
    handles.push(async move {
        let result = future.await;
        drop(permit);
        (work.number, result)
    });
}

let results = futures::future::join_all(handles).await;

// Write results to DB serially
for (number, result) in results {
    let summary = result?;
    // save to cache + build per_issue_summaries
}
```

However, since `ctx.agent` is `&dyn Agent` and futures borrow it, we can't easily move them into separate tasks. The simpler approach is to use `FuturesUnordered` without `tokio::spawn`:

```rust
use futures::stream::{FuturesUnordered, StreamExt};

let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(ctx.config.concurrency()));
let mut futures = FuturesUnordered::new();

for work in &discussion_work {
    let sem = semaphore.clone();
    futures.push(async move {
        let _permit = sem.acquire().await.unwrap();
        let prompt = DiscussionSummaryPrompt {
            repo: repo_name.clone(),
            number: work.number,
            title: work.title.clone(),
            comments: work.comments_text.clone(),
            previous_summary: if work.has_previous { Some(String::new()) } else { None },
            summary_length: ctx.summary_length.clone(),
        };
        let result = ctx.agent.invoke(&prompt).await;
        (work.number, work.issue_summary.clone(), work.discussion_hash.clone(), result)
    });
}

while let Some((number, issue_sum, disc_hash, result)) = futures.next().await {
    let disc_sum = result.map_err(|e| {
        eprintln!("  Error summarizing discussion #{number}: {e}");
        e
    })?;
    let _ = db::save_issue_cache(ctx.conn, repo_name, number, &issue_sum, &disc_sum, &disc_hash);
    // Store result for later assembly
}
```

Note: This works because `FuturesUnordered` polls futures on the current task — no need for `Send` bounds. The semaphore limits how many agent subprocesses are running concurrently.

Apply similar pattern to batch description calls — though those are already batched, the batch calls themselves can be parallelized across repos.

**Step 5: Convert `RepoSummaryTask` to concurrent**

Per-repo summary calls are independent. Use the same `FuturesUnordered` + semaphore pattern:

1. Collect all repo inputs (summaries, commit logs, hashes, cache status)
2. Fan out agent calls for repos that need them
3. Collect results
4. Build `repo_sections` serially with DB writes

**Step 6: Convert `TriageTask` to concurrent**

Same pattern — fan out per-issue triage calls with semaphore.

**Step 7: Convert `ExecutiveSummaryTask` to async**

Single call — just wrap with `.await`.

**Step 8: Verify it compiles**

Run: `cargo check`
Expected: PASS

**Step 9: Commit**

```bash
git add Cargo.toml src/pipeline.rs src/tasks/
git commit -m "feat: convert pipeline tasks to async with concurrent agent calls"
```

---

### Task 7: Convert main.rs callers to async

**Files:**
- Modify: `src/main.rs`

**Step 1: Add `#[tokio::main]` to main**

Change `fn main()` to:

```rust
#[tokio::main]
async fn main() {
    // ... existing code ...
}
```

**Step 2: Add `.await` to `run_pipeline` calls**

Find the two `run_pipeline` calls in main.rs and add `.await`:

```rust
let report_data = ceo::pipeline::run_pipeline(&config, &conn, &agent, &since, &label, &progress, template.as_deref()).await?;
```

**Step 3: Verify it compiles and runs**

Run: `cargo build`
Expected: PASS

Run: `cargo test`
Expected: Tests need updating too (next task)

**Step 4: Commit**

```bash
git add src/main.rs
git commit -m "feat: convert main to async with tokio runtime"
```

---

### Task 8: Update tests for async

**Files:**
- Modify: `tests/pipeline_test.rs`
- Modify: any other test files that call agent or pipeline

**Step 1: Update MockAgent for async trait**

In `tests/pipeline_test.rs`:

```rust
use std::future::Future;
use std::pin::Pin;

struct MockAgent;

impl Agent for MockAgent {
    fn invoke(&self, _prompt: &dyn Prompt) -> Pin<Box<dyn Future<Output = Result<String, AgentError>> + Send + '_>> {
        Box::pin(async {
            Ok("<done>Mock work completed.</done><in_progress>Mock active work.</in_progress>".to_string())
        })
    }
}
```

**Step 2: Make test functions async**

Add `tokio` to `[dev-dependencies]`:

```toml
[dev-dependencies]
tempfile = "3"
rusqlite = { version = "0.38", features = ["bundled"] }
ceo-schema = { path = "crates/ceo-schema" }
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
```

Convert each pipeline test:

```rust
#[tokio::test]
async fn pipeline_reads_from_database() {
    // ... same body but add .await to run_pipeline call ...
    let report = run_pipeline(&config, &conn, &MockAgent, &since, "2026-03-09", &NullProgress, None).await.unwrap();
    // ... assertions unchanged ...
}
```

**Step 3: Run tests**

Run: `cargo test -- --nocapture`
Expected: All tests pass

**Step 4: Commit**

```bash
git add tests/ Cargo.toml
git commit -m "test: update tests for async agent and pipeline"
```

---

### Task 9: Final verification

**Step 1: Run full test suite**

Run: `cargo test`
Expected: All tests pass

**Step 2: Run clippy**

Run: `cargo clippy -- -D warnings 2>&1 | grep -v while_let_loop`
Expected: No new warnings (the pre-existing while_let_loop warning is unrelated)

**Step 3: Build release**

Run: `cargo build --release`
Expected: Clean build

**Step 4: Manual smoke test (optional)**

```bash
cargo install --path .
ceo sync
ceo report
```

Verify the report generates correctly and batch prompting is used (check debug logs with `RUST_LOG=debug`).
