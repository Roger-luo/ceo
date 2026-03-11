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
    /// Show team overview (contributor stats from local DB, no agent calls)
    Team {
        /// Number of days to look back (default: 7)
        #[arg(long, default_value = "7")]
        days: i64,
    },
    /// Generate an example config file (alias for `config`)
    #[command(hide = true)]
    Init,
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

    match cli.command {
        Commands::Report { days, month, template } => cmd_report(days, month, template).await,
        Commands::Interactive => cmd_interactive().await,
        Commands::Sync => cmd_sync(),
        Commands::ClearCache => cmd_clear_cache(),
        Commands::Config { action } => cmd_config(action),
        Commands::Roadmap { action } => cmd_roadmap(action),
        Commands::Team { days } => cmd_team(days),
        Commands::Init => cmd_config(None),
    }
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
    use chrono::{Datelike, Duration, NaiveDate, Utc};

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
        let since = (Utc::now() - Duration::days(days)).to_rfc3339();
        let label = Utc::now().format("%Y-%m-%d").to_string();
        Ok((since, label))
    }
}

async fn cmd_report(days: i64, month: Option<String>, template: Option<String>) -> Result<()> {
    let config = ceo::config::Config::load()?;
    let conn = ceo::db::open_existing_db()?;
    let agent = ceo::agent::AgentKind::from_config(&config.agent);
    let progress = ReportProgress::new();
    let (since, label) = resolve_date_range(days, month)?;

    let report_data = ceo::pipeline::run_pipeline(&config, &conn, &agent, &since, &label, &progress, template.as_deref()).await?;
    let markdown = ceo::report::render_markdown(&report_data);
    print!("{markdown}");
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

        let (additions, deletions) = contributor_stats
            .values()
            .flat_map(|rows| rows.iter())
            .filter(|row| row.author.eq_ignore_ascii_case(&member.github))
            .fold((0i64, 0i64), |(a, d), row| {
                (a + row.additions, d + row.deletions)
            });

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
