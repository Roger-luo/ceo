# Task-Based Pipeline Design

## Problem

`run_pipeline` is a ~280-line monolithic function doing everything inline: DB queries, per-issue summarization, commit log building, repo-level summaries, triage, team stats, and executive summaries. This makes it hard to extend with new steps, observe individual task progress, or reason about data flow.

## Goals

- **Extensibility**: Add new pipeline steps (e.g., PR review, dependency audit) without touching the executor.
- **Observability**: Uniform progress tracking, skip conditions, and step count estimates per task.
- **Readability**: Each task is a self-contained unit in its own file with a clear interface.

## Design

### Core Trait

```rust
// src/tasks/mod.rs

pub trait Task {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn step_count(&self, ctx: &PipelineContext) -> usize;
    fn should_skip(&self, ctx: &PipelineContext) -> bool;
    fn run(&self, ctx: &mut PipelineContext) -> Result<()>;
}
```

### PipelineContext

A shared mutable struct carrying all data between tasks:

```rust
pub struct PipelineContext<'a> {
    // Inputs (immutable after construction)
    pub config: &'a Config,
    pub conn: &'a Connection,
    pub agent: &'a dyn Agent,
    pub since: String,
    pub date_label: String,
    pub template: Option<String>,
    pub roadmap: Roadmap,
    pub summary_length: String,

    // Accumulated data (tasks write into these)
    pub issues: HashMap<String, Vec<Issue>>,
    pub issue_bodies: HashMap<(String, u64), String>,
    pub comments: HashMap<(String, u64), Vec<CommentRow>>,
    pub issue_summaries: Vec<String>,       // per-repo accumulator
    pub commit_logs: HashMap<String, String>,
    pub repo_sections: Vec<RepoSection>,
    pub team_stats: Vec<TeamStats>,
    pub executive_summary: Option<String>,
}
```

### Execution Model

Linear sequence. Tasks run in a fixed order. Each task reads from and writes to the shared `PipelineContext`. Per-repo tasks internally loop over `ctx.config.repos`.

### Concrete Tasks

| Task | File | Scope | Agent calls | Cached |
|------|------|-------|-------------|--------|
| `FetchDataTask` | `fetch_data.rs` | Per-repo | No | N/A |
| `SummarizeIssuesTask` | `summarize.rs` | Per-repo | Yes (per issue) | `issue_cache` |
| `BuildCommitLogTask` | `commit_log.rs` | Per-repo | No | N/A |
| `RepoSummaryTask` | `repo_summary.rs` | Per-repo | Yes (per repo) | `report_cache` |
| `TriageTask` | `triage.rs` | Per-repo | Yes (per flagged issue) | No |
| `TeamStatsTask` | `team_stats.rs` | Global | No | N/A |
| `ExecutiveSummaryTask` | `executive.rs` | Global | Yes (once) | No |

### Skip Conditions

- `ExecutiveSummaryTask`: skip when `ctx.template.is_none()`
- `TriageTask`: skip when no repo has `labels_required` configured
- Others: never skip

### Executor

`run_pipeline` becomes a thin executor that builds the task list and runs each task in sequence:

```rust
pub fn run_pipeline(...) -> Result<Report> {
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
    ctx.into_report()
}
```

### PipelineProgress Changes

Add three methods to the existing trait:

```rust
pub trait PipelineProgress {
    // New task-level hooks
    fn task_start(&self, name: &str, step_count: usize);
    fn task_skipped(&self, name: &str);
    fn task_done(&self, name: &str);
    // Existing methods kept for backward compat during migration
    fn repo_start(&self, repo: &str, issue_count: usize);
    fn issue_step(&self, index: usize, total: usize, number: u64, title: &str);
    fn phase(&self, msg: &str);
    fn repo_done(&self, repo: &str);
    fn finish(&self);
}
```

### File Layout

```
src/
  tasks/
    mod.rs              — Task trait, PipelineContext, re-exports
    fetch_data.rs       — DB queries, populates ctx.issues/comments/bodies
    summarize.rs        — Per-issue summarization with caching
    commit_log.rs       — Commit log building
    repo_summary.rs     — Repo-level weekly summary with caching
    triage.rs           — Flagged issue triage
    team_stats.rs       — Team stats computation
    executive.rs        — Executive summary generation
  pipeline.rs           — Executor (build task list, run loop, progress hooks)
```

### Migration Strategy

Extract code from `run_pipeline` into task structs one at a time. Each extraction is a standalone commit that preserves all existing tests. The executor replaces the inline code incrementally — at no point does the pipeline break.
