mod tui;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "ceo", about = "Weekly project summary from GitHub issues")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Generate a weekly report (prints markdown to stdout)
    Report {
        /// Number of days to look back
        #[arg(long, default_value = "7")]
        days: i64,
    },
    /// Launch interactive TUI mode
    Interactive,
    /// Configure CEO CLI (interactive wizard or set/get/show)
    Config {
        #[command(subcommand)]
        action: Option<ConfigAction>,
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

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Report { days } => cmd_report(days),
        Commands::Interactive => cmd_interactive(),
        Commands::Config { action } => cmd_config(action),
        Commands::Init => cmd_config(None),
    }
}

fn cmd_report(days: i64) -> Result<()> {
    let config = ceo::config::Config::load()?;
    let gh_runner = ceo::gh::RealGhRunner;
    let agent = ceo::agent::AgentKind::from_config(&config.agent);

    let report_data = ceo::pipeline::run_pipeline(&config, &gh_runner, &agent, days)?;
    let markdown = ceo::report::render_markdown(&report_data);
    print!("{markdown}");
    Ok(())
}

fn cmd_interactive() -> Result<()> {
    let config = ceo::config::Config::load()?;
    let gh_runner = ceo::gh::RealGhRunner;
    let agent = ceo::agent::AgentKind::from_config(&config.agent);

    eprintln!("Fetching data and generating report...");
    let report_data = ceo::pipeline::run_pipeline(&config, &gh_runner, &agent, 7)?;
    let markdown = ceo::report::render_markdown(&report_data);

    tui::run_tui(markdown)?;
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

fn cmd_config_wizard() -> Result<()> {
    use std::io::{self, BufRead, Write};

    let stdin = io::stdin();
    let mut stdout = io::stdout();

    let mut config = ceo::config::Config::load()
        .unwrap_or_else(|_| toml::from_str("repos = []").unwrap());

    // Agent type
    eprint!("Agent type [{}]: ", config.agent.agent_type);
    stdout.flush()?;
    let mut line = String::new();
    stdin.lock().read_line(&mut line)?;
    let line = line.trim();
    if !line.is_empty() {
        config.agent.agent_type = line.to_string();
    }

    // Timeout
    eprint!("Timeout in seconds [{}]: ", config.agent.timeout_secs);
    stdout.flush()?;
    let mut line = String::new();
    stdin.lock().read_line(&mut line)?;
    let line = line.trim();
    if !line.is_empty() {
        config.agent.timeout_secs = line.parse()
            .context("Invalid number for timeout")?;
    }

    // Repos
    if !config.repos.is_empty() {
        eprintln!("Current repos: {}", config.repos.iter().map(|r| r.name.as_str()).collect::<Vec<_>>().join(", "));
    }
    loop {
        eprint!("Add a repo (org/name), or Enter to finish: ");
        stdout.flush()?;
        let mut line = String::new();
        stdin.lock().read_line(&mut line)?;
        let line = line.trim();
        if line.is_empty() { break; }

        eprint!("Required labels (comma-separated, or Enter for none): ");
        stdout.flush()?;
        let mut labels_line = String::new();
        stdin.lock().read_line(&mut labels_line)?;
        let labels: Vec<String> = labels_line.trim()
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        config.repos.push(ceo::config::RepoConfig {
            name: line.to_string(),
            labels_required: labels,
        });
    }

    // Team
    if !config.team.is_empty() {
        eprintln!("Current team: {}", config.team.iter().map(|t| t.github.as_str()).collect::<Vec<_>>().join(", "));
    }
    loop {
        eprint!("Add team member (github username), or Enter to finish: ");
        stdout.flush()?;
        let mut line = String::new();
        stdin.lock().read_line(&mut line)?;
        let github = line.trim().to_string();
        if github.is_empty() { break; }

        eprint!("Full name: ");
        stdout.flush()?;
        let mut name_line = String::new();
        stdin.lock().read_line(&mut name_line)?;

        eprint!("Role: ");
        stdout.flush()?;
        let mut role_line = String::new();
        stdin.lock().read_line(&mut role_line)?;

        config.team.push(ceo::config::TeamMember {
            github,
            name: name_line.trim().to_string(),
            role: role_line.trim().to_string(),
        });
    }

    config.save()?;
    let path = ceo::config::Config::config_path();
    eprintln!("Config saved to {}", path.display());
    Ok(())
}
