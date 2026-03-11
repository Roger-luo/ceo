use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

use crate::agent::Agent;
use crate::config::Config;
use crate::db::CommentRow;
use crate::error::PipelineError;
use crate::github::Issue;
use crate::pipeline::PipelineProgress;
use crate::report::{RepoSection, TeamStats};
use crate::roadmap::Roadmap;

pub mod commit_log;
pub mod executive;
pub mod fetch_data;
pub mod repo_summary;
pub mod summarize;
pub mod team_stats;
pub mod triage;

type Result<T> = std::result::Result<T, PipelineError>;

pub trait Task {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn step_count(&self, ctx: &PipelineContext) -> usize;
    fn should_skip(&self, ctx: &PipelineContext) -> bool;
    fn run<'a, 'ctx>(&'a self, ctx: &'a mut PipelineContext<'ctx>) -> Pin<Box<dyn Future<Output = Result<()>> + 'a>>
    where 'ctx: 'a;
}

/// Shared mutable state passed between pipeline tasks.
pub struct PipelineContext<'a> {
    // Inputs (immutable after construction)
    pub config: &'a Config,
    pub conn: &'a rusqlite::Connection,
    pub agent: &'a dyn Agent,
    pub progress: &'a dyn PipelineProgress,
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

    // Per-repo contributor stats populated by FetchDataTask
    pub contributor_stats: HashMap<String, Vec<crate::db::ContributorStatsRow>>,

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
        progress: &'a dyn PipelineProgress,
        since: &str,
        date_label: &str,
        template: Option<&str>,
    ) -> Self {
        Self {
            config,
            conn,
            agent,
            progress,
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
            contributor_stats: HashMap::new(),
            repo_sections: Vec::new(),
            all_recent_issues: Vec::new(),
            team_stats: Vec::new(),
            executive_summary: None,
        }
    }
}
