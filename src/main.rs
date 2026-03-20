mod self_update;
mod tui;

use std::sync::Mutex;

use anyhow::Result;
use clap::{Parser, Subcommand};
use indicatif::{ProgressBar, ProgressStyle};

use ceo::pipeline::PipelineProgress;

#[derive(Parser)]
#[command(name = "ceo", about = "Weekly project summary from GitHub issues")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Generate a report (prints markdown to stdout)
    Report {
        /// Number of days to look back (default: 7)
        #[arg(long, default_value = "7")]
        days: i64,
        /// Generate report for a specific month (YYYY-MM, e.g. 2026-03)
        #[arg(long)]
        month: Option<String>,
        /// Executive summary template (executive, technical, standup, or custom name)
        #[arg(long)]
        template: Option<String>,
        /// Send report to Slack (requires $CEO_SLACK_WEBHOOK)
        #[arg(long)]
        slack: bool,
        /// Print the Slack JSON payload without sending (for debugging)
        #[arg(long)]
        slack_dry_run: bool,
    },
    /// Launch interactive TUI mode
    Interactive,
    /// Configure CEO CLI (interactive wizard or set/get/show)
    Config {
        #[command(subcommand)]
        action: Option<ConfigAction>,
    },
    /// Sync GitHub data to local database
    Sync,
    /// Clear generated summary caches (forces re-generation on next report)
    ClearCache,
    /// Manage roadmap / initiatives
    Roadmap {
        #[command(subcommand)]
        action: RoadmapAction,
    },
    /// Show GitHub API rate limit status
    RateLimit,
    /// Show team overview (contributor stats from local DB, no agent calls)
    Team {
        /// Number of days to look back (default: 7)
        #[arg(long, default_value = "7")]
        days: i64,
    },
    /// Manage the ceo binary (info, check for updates, self-update)
    #[command(name = "self")]
    Self_ {
        #[command(subcommand)]
        action: SelfAction,
    },
    /// Generate an example config file (alias for `config`)
    #[command(hide = true)]
    Init,
}

#[derive(Subcommand)]
enum SelfAction {
    /// Show version, target, and executable path
    Info,
    /// Check if a newer version is available
    Check,
    /// Download and install the latest version
    Update {
        /// Install a specific version (e.g. 0.2.0)
        #[arg(long)]
        version: Option<String>,
    },
}

#[derive(Subcommand)]
enum ConfigAction {
    /// Set a config field: ceo config set agent.type codex
    Set {
        key: String,
        value: Vec<String>,
    },
    /// Get a config field: ceo config get agent.type
    Get {
        key: String,
    },
    /// Show full config as TOML
    Show,
}

#[derive(Subcommand)]
enum RoadmapAction {
    /// Show current roadmap
    Show,
    /// Open roadmap in $EDITOR
    Edit,
    /// Add an initiative
    Add {
        /// Initiative name
        name: String,
        /// Timeframe (e.g. "Q1 2026", "2026")
        #[arg(long)]
        timeframe: Option<String>,
        /// Comma-separated list of repos
        #[arg(long, value_delimiter = ',')]
        repos: Vec<String>,
        /// Description of the initiative
        #[arg(long)]
        description: String,
    },
    /// Remove an initiative by name
    Remove {
        /// Initiative name to remove
        name: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    let cli = Cli::parse();

    let is_self_cmd = matches!(cli.command, Commands::Self_ { .. });

    let result = match cli.command {
        Commands::Report { days, month, template, slack, slack_dry_run } => cmd_report(days, month, template, slack, slack_dry_run).await,
        Commands::Interactive => cmd_interactive().await,
        Commands::Sync => cmd_sync(),
        Commands::ClearCache => cmd_clear_cache(),
        Commands::Config { action } => cmd_config(action),
        Commands::Roadmap { action } => cmd_roadmap(action),
        Commands::RateLimit => cmd_rate_limit(),
        Commands::Team { days } => cmd_team(days),
        Commands::Self_ { action } => {
            match action {
                SelfAction::Info => self_update::info(),
                SelfAction::Check => self_update::check(),
                SelfAction::Update { version } => self_update::update(version.as_deref()),
            }
        }
        Commands::Init => cmd_config(None),
    };

    // Show update hint after non-self commands (best-effort, never fails)
    if !is_self_cmd {
        self_update::check_for_update_hint();
    }

    result
}

struct ReportProgress {
    bar: Mutex<Option<ProgressBar>>,
}

impl ReportProgress {
    fn new() -> Self {
        Self { bar: Mutex::new(None) }
    }

    fn set_spinner(&self, msg: String) {
        let mut guard = self.bar.lock().unwrap();
        if let Some(pb) = guard.take() {
            pb.finish_and_clear();
        }
        let pb = ProgressBar::new_spinner();
        pb.set_style(
            ProgressStyle::with_template("  {spinner:.cyan} {msg}")
                .unwrap()
                .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏ "),
        );
        pb.set_message(msg);
        pb.enable_steady_tick(std::time::Duration::from_millis(80));
        *guard = Some(pb);
    }

    fn set_bar(&self, total: u64, msg: String) {
        let mut guard = self.bar.lock().unwrap();
        if let Some(pb) = guard.take() {
            pb.finish_and_clear();
        }
        let pb = ProgressBar::new(total);
        pb.set_style(
            ProgressStyle::with_template("  {bar:30.cyan/dim} {pos}/{len} {msg}")
                .unwrap()
                .progress_chars("━╸─"),
        );
        pb.set_message(msg);
        *guard = Some(pb);
    }
}

impl PipelineProgress for ReportProgress {
    fn task_start(&self, name: &str, step_count: usize) {
        if step_count > 0 {
            self.set_bar(step_count as u64, name.to_string());
        } else {
            self.set_spinner(name.to_string());
        }
    }

    fn task_done(&self, name: &str) {
        let mut guard = self.bar.lock().unwrap();
        if let Some(pb) = guard.take() {
            pb.finish_and_clear();
        }
        eprintln!("  ✓ {name}");
    }

    fn repo_start(&self, repo: &str, issue_count: usize) {
        if issue_count == 0 {
            self.set_spinner(format!("{repo}: no recent issues"));
        } else {
            self.set_bar(issue_count as u64, format!("{repo}"));
        }
    }

    fn issue_step(&self, _index: usize, _total: usize, number: u64, title: &str) {
        let guard = self.bar.lock().unwrap();
        if let Some(pb) = guard.as_ref() {
            pb.set_message(format!("#{number} {title}"));
            pb.inc(1);
        }
    }

    fn phase(&self, msg: &str) {
        let guard = self.bar.lock().unwrap();
        if let Some(pb) = guard.as_ref() {
            pb.set_message(msg.to_string());
        } else {
            drop(guard);
            self.set_spinner(msg.to_string());
        }
    }

    fn repo_done(&self, repo: &str) {
        let mut guard = self.bar.lock().unwrap();
        if let Some(pb) = guard.take() {
            pb.finish_and_clear();
        }
        eprintln!("  ✓ {repo}");
    }

    fn finish(&self) {
        let mut guard = self.bar.lock().unwrap();
        if let Some(pb) = guard.take() {
            pb.finish_and_clear();
        }
    }
}

fn resolve_date_range(days: i64, month: Option<String>) -> Result<(String, String)> {
    use chrono::{Datelike, Duration, Local, NaiveDate};

    if let Some(m) = month {
        let first = NaiveDate::parse_from_str(&format!("{m}-01"), "%Y-%m-%d")
            .map_err(|_| anyhow::anyhow!("Invalid month format '{m}', expected YYYY-MM"))?;
        let next_month = if first.month() == 12 {
            NaiveDate::from_ymd_opt(first.year() + 1, 1, 1).unwrap()
        } else {
            NaiveDate::from_ymd_opt(first.year(), first.month() + 1, 1).unwrap()
        };
        let since = first.and_hms_opt(0, 0, 0).unwrap().and_utc().to_rfc3339();
        let until = next_month.and_hms_opt(0, 0, 0).unwrap().and_utc().to_rfc3339();
        let _ = until; // DB query currently only uses `since`; upper bound enforced by data freshness
        let label = format!("{}", first.format("%B %Y"));
        Ok((since, label))
    } else {
        let now = Local::now();
        let since = (now - Duration::days(days)).to_rfc3339();
        let label = now.format("%Y-%m-%d").to_string();
        Ok((since, label))
    }
}

async fn cmd_report(days: i64, month: Option<String>, template: Option<String>, slack: bool, slack_dry_run: bool) -> Result<()> {
    let config = ceo::config::Config::load()?;
    let conn = ceo::db::open_existing_db()?;
    let agent = ceo::agent::AgentKind::from_config(&config.agent);
    let progress = ReportProgress::new();
    let (since, label) = resolve_date_range(days, month)?;

    let report_data = ceo::pipeline::run_pipeline(&config, &conn, &agent, &since, &label, &progress, template.as_deref()).await?;
    let markdown = ceo::report::render_markdown(&report_data);

    if slack_dry_run {
        let json = ceo::slack::dry_run(&report_data, config.slack.as_ref());
        println!("{json}");
    } else if slack {
        ceo::slack::send_report(&report_data, &markdown, config.slack.as_ref()).await?;
        eprintln!("Report sent to Slack.");
    } else {
        print!("{markdown}");
    }
    Ok(())
}

async fn cmd_interactive() -> Result<()> {
    let config = ceo::config::Config::load()?;
    let conn = ceo::db::open_existing_db()?;
    let agent = ceo::agent::AgentKind::from_config(&config.agent);
    let progress = ReportProgress::new();
    let (since, label) = resolve_date_range(7, None)?;

    let report_data = ceo::pipeline::run_pipeline(&config, &conn, &agent, &since, &label, &progress, None).await?;
    let markdown = ceo::report::render_markdown(&report_data);

    tui::run_tui(markdown)?;
    Ok(())
}

fn cmd_clear_cache() -> Result<()> {
    let conn = ceo::db::open_existing_db()?;
    ceo::db::clear_caches(&conn)?;
    eprintln!("Cleared all summary caches. Next report will regenerate everything.");
    Ok(())
}

fn cmd_rate_limit() -> Result<()> {
    let gh = ceo::gh::RealGhRunner;
    let json = ceo::gh::GhRunner::run_gh(&gh, &["api", "rate_limit"])
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let parsed: serde_json::Value = serde_json::from_str(&json)?;

    let resources = &parsed["resources"];
    let now = chrono::Utc::now().timestamp();

    for (name, display) in [("core", "Core API"), ("search", "Search API"), ("graphql", "GraphQL")] {
        let r = &resources[name];
        let remaining = r["remaining"].as_i64().unwrap_or(0);
        let limit = r["limit"].as_i64().unwrap_or(0);
        let reset = r["reset"].as_i64().unwrap_or(0);
        let wait = (reset - now).max(0);

        let reset_str = if wait > 0 {
            format!(" (resets in {}m {}s)", wait / 60, wait % 60)
        } else {
            String::new()
        };
        let status = if remaining == 0 {
            format!("EXHAUSTED{reset_str}")
        } else {
            format!("{remaining}/{limit} remaining{reset_str}")
        };
        println!("  {display}: {status}");
    }

    // Show email resolution cache stats
    if let Ok(conn) = ceo::db::open_existing_db() {
        let cached: i64 = conn
            .query_row("SELECT COUNT(*) FROM email_to_github", [], |row| row.get(0))
            .unwrap_or(0);
        let unresolved: i64 = conn
            .query_row(
                "SELECT COUNT(DISTINCT author) FROM commit_stats WHERE author NOT IN (SELECT github FROM email_to_github)",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        println!("\n  Email cache: {cached} resolved, {unresolved} pending");
    }

    Ok(())
}

fn cmd_team(days: i64) -> Result<()> {
    use chrono::{Duration, Utc};

    let config = ceo::config::Config::load()?;
    let conn = ceo::db::open_existing_db()?;

    if config.team.is_empty() {
        eprintln!("No team members configured. Use `ceo config` to add team members.");
        return Ok(());
    }

    let now = Utc::now();
    let start = now - Duration::days(days);
    let since = start.format("%Y-%m-%d").to_string();

    // Query contributor stats and recent issues
    let repo_names: Vec<String> = config.repos.iter().map(|r| r.name.clone()).collect();
    let all_issues = ceo::db::query_recent_issues(&conn, &repo_names, &start.to_rfc3339())?;
    let mut contributor_stats = std::collections::HashMap::new();
    for repo_config in &config.repos {
        let stats = ceo::db::query_contributor_stats(&conn, &[repo_config.name.clone()], &since)?;
        contributor_stats.insert(repo_config.name.clone(), stats);
    }

    println!("## Team Overview ({since} to {})\n", now.format("%Y-%m-%d"));
    println!("| Person | Active | Closed | Lines |");
    println!("|--------|--------|--------|-------|");

    for member in &config.team {
        let (active, closed) = all_issues
            .iter()
            .filter(|i| {
                let assignees: Vec<String> = serde_json::from_str(&i.assignees).unwrap_or_default();
                assignees.contains(&member.github)
            })
            .fold((0, 0), |(open, closed), i| {
                let state = i.state.as_deref().unwrap_or("OPEN");
                if state.eq_ignore_ascii_case("OPEN") {
                    (open + 1, closed)
                } else if state.eq_ignore_ascii_case("CLOSED") {
                    (open, closed + 1)
                } else {
                    (open, closed)
                }
            });

        let (mut additions, mut deletions) = contributor_stats
            .values()
            .flat_map(|rows| rows.iter())
            .filter(|row| row.author.eq_ignore_ascii_case(&member.github))
            .fold((0i64, 0i64), |(a, d), row| {
                (a + row.additions, d + row.deletions)
            });

        // Add lines from open (unmerged) PRs authored by this member
        for issue in &all_issues {
            let state = issue.state.as_deref().unwrap_or("OPEN");
            if issue.kind == "pr"
                && state.eq_ignore_ascii_case("OPEN")
                && issue.author.as_deref() == Some(&member.github)
            {
                additions += issue.pr_additions.unwrap_or(0);
                deletions += issue.pr_deletions.unwrap_or(0);
            }
        }

        println!(
            "| {} (@{}) | {} | {} | +{} / -{} |",
            member.name, member.github, active, closed, additions, deletions
        );
    }

    Ok(())
}

fn cmd_sync() -> Result<()> {
    use ceo::sync::{SyncProgress, RepoSyncResult};

    struct IndicatifProgress {
        spinner: Mutex<Option<ProgressBar>>,
    }

    impl IndicatifProgress {
        fn new() -> Self {
            Self {
                spinner: Mutex::new(None),
            }
        }
    }

    impl SyncProgress for IndicatifProgress {
        fn phase(&self, repo: &str, name: &str) {
            if let Some(pb) = self.spinner.lock().unwrap().take() {
                pb.finish_and_clear();
            }
            let pb = ProgressBar::new_spinner();
            pb.set_style(
                ProgressStyle::with_template("  {spinner:.cyan} {msg}")
                    .unwrap()
                    .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏ "),
            );
            pb.set_message(format!("{repo}: {name}..."));
            pb.enable_steady_tick(std::time::Duration::from_millis(80));
            *self.spinner.lock().unwrap() = Some(pb);
        }

        fn repo_done(&self, repo: &str, result: &RepoSyncResult) {
            if let Some(pb) = self.spinner.lock().unwrap().take() {
                pb.finish_and_clear();
            }
            eprintln!(
                "  {repo}: {} issues, {} comments, {} commits",
                result.issues_synced, result.comments_synced, result.commits_synced,
            );
        }

        fn warn(&self, msg: &str) {
            eprintln!("  Warning: {msg}");
        }
    }

    let config = ceo::config::Config::load()?;
    let gh_runner = ceo::gh::RealGhRunner;
    let db_path = ceo::db::db_path();
    let conn = ceo::db::open_db_at(&db_path)?;

    eprintln!("Syncing to {}...", db_path.display());
    let progress = IndicatifProgress::new();
    let _result = ceo::sync::run_sync(&config, &gh_runner, &conn, &progress)?;
    eprintln!("Sync complete.");
    Ok(())
}

fn cmd_config(action: Option<ConfigAction>) -> Result<()> {
    match action {
        None => cmd_config_wizard(),
        Some(ConfigAction::Set { key, value }) => {
            let joined = value.join(" ");
            let mut config = ceo::config::Config::load()
                .unwrap_or_else(|_| toml::from_str("repos = []").unwrap());
            config.set_field(&key, &joined)?;
            config.save()?;
            eprintln!("Set {key} = {joined}");
            Ok(())
        }
        Some(ConfigAction::Get { key }) => {
            let config = ceo::config::Config::load()?;
            let value = config.get_field(&key)?;
            println!("{value}");
            Ok(())
        }
        Some(ConfigAction::Show) => {
            let config = ceo::config::Config::load()?;
            let toml_str = toml::to_string_pretty(&config)?;
            print!("{toml_str}");
            Ok(())
        }
    }
}

fn cmd_roadmap(action: RoadmapAction) -> Result<()> {
    use ceo::roadmap::{Initiative, Roadmap};

    match action {
        RoadmapAction::Show => {
            let path = Roadmap::path();
            if !path.exists() {
                eprintln!("No roadmap file. Create one with: ceo roadmap edit");
                return Ok(());
            }
            let contents = std::fs::read_to_string(&path)?;
            print!("{contents}");
            Ok(())
        }
        RoadmapAction::Edit => {
            let path = Roadmap::path();
            if !path.exists() {
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(&path, Roadmap::template())?;
            }
            let config = ceo::config::Config::load()
                .unwrap_or_else(|_| toml::from_str("repos = []").unwrap());
            let editor = config.editor();
            let status = std::process::Command::new(&editor)
                .arg(&path)
                .status()?;
            if !status.success() {
                anyhow::bail!("Editor exited with error");
            }
            eprintln!("Roadmap saved to {}", path.display());
            Ok(())
        }
        RoadmapAction::Add { name, timeframe, repos, description } => {
            let mut roadmap = Roadmap::load();
            roadmap.add(Initiative { name: name.clone(), timeframe, repos, description })
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            roadmap.save()?;
            eprintln!("Added initiative: {name}");
            Ok(())
        }
        RoadmapAction::Remove { name } => {
            let mut roadmap = Roadmap::load();
            roadmap.remove(&name)
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            roadmap.save()?;
            eprintln!("Removed initiative: {name}");
            Ok(())
        }
    }
}

fn cmd_config_wizard() -> Result<()> {
    let mut config = ceo::config::Config::load()
        .unwrap_or_else(|_| toml::from_str("repos = []").unwrap());

    tui::run_config_editor(&mut config)?;

    config.save()?;
    let path = ceo::config::Config::config_path();
    eprintln!("Config saved to {}", path.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_date_range_daily_uses_local_time() {
        let (since, label) = resolve_date_range(7, None).unwrap();
        // Label should be YYYY-MM-DD matching local date
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        assert_eq!(label, today, "Daily report date should use local time, not UTC");
        // Since should be an RFC 3339 timestamp with timezone offset
        assert!(since.contains('+') || since.contains('-'),
            "since should be RFC 3339 with tz offset, got: {since}");
    }

    #[test]
    fn resolve_date_range_monthly_format() {
        let (since, label) = resolve_date_range(7, Some("2026-03".to_string())).unwrap();
        assert_eq!(label, "March 2026");
        assert!(since.starts_with("2026-03-01"));
    }
}
