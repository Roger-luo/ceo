use crate::agent::Agent;
use crate::config::Config;
use crate::error::PipelineError;
use crate::report::{linkify, RefLookup, Report};
use crate::tasks::commit_log::BuildCommitLogTask;
use crate::tasks::executive::ExecutiveSummaryTask;
use crate::tasks::fetch_data::FetchDataTask;
use crate::tasks::repo_summary::RepoSummaryTask;
use crate::tasks::summarize::SummarizeIssuesTask;
use crate::tasks::team_stats::TeamStatsTask;
use crate::tasks::triage::TriageTask;
use crate::tasks::{PipelineContext, Task};

type Result<T> = std::result::Result<T, PipelineError>;

/// Progress reporting for the report pipeline.
pub trait PipelineProgress {
    /// Starting a named task with an estimated step count.
    fn task_start(&self, _name: &str, _step_count: usize) {}
    /// A task was skipped.
    fn task_skipped(&self, _name: &str) {}
    /// A task completed.
    fn task_done(&self, _name: &str) {}
    /// Starting work on a repo.
    fn repo_start(&self, _repo: &str, _issue_count: usize) {}
    /// Progress on individual issue summarization.
    fn issue_step(&self, _index: usize, _total: usize, _number: u64, _title: &str) {}
    /// Generic phase message (repo summary, triage step, etc.)
    fn phase(&self, _msg: &str) {}
    /// Finished processing a repo.
    fn repo_done(&self, _repo: &str) {}
    /// All repos done.
    fn finish(&self) {}
}

/// No-op progress for library/test use.
pub struct NullProgress;

impl PipelineProgress for NullProgress {}

pub async fn run_pipeline(
    config: &Config,
    conn: &rusqlite::Connection,
    agent: &dyn Agent,
    since: &str,
    date_label: &str,
    progress: &dyn PipelineProgress,
    template: Option<&str>,
) -> Result<Report> {
    let mut ctx = PipelineContext::new(config, conn, agent, progress, since, date_label, template);

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

    let refs = RefLookup::from_issue_rows(&ctx.issue_rows);

    // Linkify all text fields in the report
    let executive_summary = ctx.executive_summary.map(|s| linkify(&s, &refs, ""));
    let repos = ctx.repo_sections.into_iter().map(|mut r| {
        let repo = r.name.clone();
        r.done = r.done.map(|s| linkify(&s, &refs, &repo));
        r.in_progress = r.in_progress.map(|s| linkify(&s, &refs, &repo));
        r.next = r.next.map(|s| linkify(&s, &refs, &repo));
        for issue in &mut r.flagged_issues {
            issue.summary = linkify(&issue.summary, &refs, &repo);
        }
        r
    }).collect();

    Ok(Report {
        date: ctx.date_label.clone(),
        generated_at: chrono::Local::now().to_rfc3339(),
        executive_summary,
        repos,
        team_stats: ctx.team_stats,
        refs,
    })
}
