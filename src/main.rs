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
    env_logger::init();
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
    use rustyline::DefaultEditor;

    let mut rl = DefaultEditor::new()?;

    let mut config = ceo::config::Config::load()
        .unwrap_or_else(|_| toml::from_str("repos = []").unwrap());

    eprintln!("\n--- Agent ---");
    // Agent type
    let line = rl.readline(&format!("Agent type [{}]: ", config.agent.agent_type))?;
    let line = line.trim();
    if !line.is_empty() {
        config.agent.agent_type = line.to_string();
    }

    // Timeout
    let line = rl.readline(&format!("Timeout in seconds [{}]: ", config.agent.timeout_secs))?;
    let line = line.trim();
    if !line.is_empty() {
        config.agent.timeout_secs = line.parse()
            .context("Invalid number for timeout")?;
    }

    // Models
    eprintln!("\n--- Models ---");
    let default_model_display = if config.agent.model.is_empty() { "agent default".to_string() } else { config.agent.model.clone() };
    eprintln!("Default model: {default_model_display}");
    if !config.agent.models.is_empty() {
        for (kind, model) in &config.agent.models {
            eprintln!("  {kind}: {model}");
        }
    }
    let line = rl.readline(&format!("Default model [{}]: ", default_model_display))?;
    let line = line.trim();
    if !line.is_empty() {
        config.agent.model = line.to_string();
    }

    // Per-prompt model overrides
    for kind in &["summary", "triage"] {
        let current = config.agent.models.get(*kind)
            .map(|s| s.as_str())
            .unwrap_or("(default)");
        let line = rl.readline(&format!("Model for {kind} [{}]: ", current))?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if line == "-" {
            config.agent.models.remove(*kind);
        } else {
            config.agent.models.insert(kind.to_string(), line.to_string());
        }
    }

    // Per-prompt extra tools
    eprintln!("\n--- Extra tools ---");
    eprintln!("  Some prompts have built-in tool requirements (e.g. triage always gets gh).");
    eprintln!("  Add extra tools per prompt type here, e.g. Read,WebSearch");
    for kind in &["summary", "triage"] {
        let current = config.agent.tools.get(*kind)
            .map(|v| if v.is_empty() { "(none)".to_string() } else { v.join(", ") })
            .unwrap_or("(none)".to_string());
        let line = rl.readline(&format!("Extra tools for {kind} [{current}]: "))?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if line == "-" {
            config.agent.tools.remove(*kind);
        } else {
            let tools: Vec<String> = line.split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            config.agent.tools.insert(kind.to_string(), tools);
        }
    }

    // Repos
    eprintln!("\n--- Repos ---");
    if !config.repos.is_empty() {
        for (i, repo) in config.repos.iter().enumerate() {
            let labels = if repo.labels_required.is_empty() {
                "all issues".to_string()
            } else {
                format!("required labels: {}", repo.labels_required.join(", "))
            };
            eprintln!("  {}. {} ({})", i + 1, repo.name, labels);
        }
        let line = rl.readline("Remove repos by number (e.g. 1,3), or Enter to keep all: ")?;
        let line = line.trim();
        if !line.is_empty() {
            let indices: Vec<usize> = line.split(',')
                .filter_map(|s| s.trim().parse::<usize>().ok())
                .collect();
            // Remove in reverse order to keep indices stable
            let mut to_remove: Vec<usize> = indices.into_iter()
                .filter(|&i| i >= 1 && i <= config.repos.len())
                .map(|i| i - 1)
                .collect();
            to_remove.sort();
            to_remove.dedup();
            for i in to_remove.into_iter().rev() {
                eprintln!("  Removed: {}", config.repos[i].name);
                config.repos.remove(i);
            }
        }
    }
    loop {
        let line = rl.readline("Add a repo (org/name), or Enter to finish: ")?;
        let line = line.trim();
        if line.is_empty() { break; }

        let labels_line = rl.readline("  Only flag issues missing these labels, e.g. priority,bug (Enter to track all): ")?;
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
    eprintln!("\n--- Team ---");
    if !config.team.is_empty() {
        for (i, member) in config.team.iter().enumerate() {
            let role = if member.role.is_empty() { "" } else { &member.role };
            eprintln!("  {}. @{} — {} {}", i + 1, member.github, member.name, role);
        }
        let line = rl.readline("Remove members by number (e.g. 1,3), or Enter to keep all: ")?;
        let line = line.trim();
        if !line.is_empty() {
            let indices: Vec<usize> = line.split(',')
                .filter_map(|s| s.trim().parse::<usize>().ok())
                .collect();
            let mut to_remove: Vec<usize> = indices.into_iter()
                .filter(|&i| i >= 1 && i <= config.team.len())
                .map(|i| i - 1)
                .collect();
            to_remove.sort();
            to_remove.dedup();
            for i in to_remove.into_iter().rev() {
                eprintln!("  Removed: @{}", config.team[i].github);
                config.team.remove(i);
            }
        }
    }
    loop {
        let line = rl.readline("Add team member (github username), or Enter to finish: ")?;
        let github = line.trim().to_string();
        if github.is_empty() { break; }

        let name_line = rl.readline("  Full name: ")?;
        let role_line = rl.readline("  Role: ")?;

        config.team.push(ceo::config::TeamMember {
            github,
            name: name_line.trim().to_string(),
            role: role_line.trim().to_string(),
        });
    }

    // Project
    eprintln!("\n--- Project ---");
    eprintln!("  GitHub Projects board for tracking issue status, dates, priority.");
    if let Some(ref project) = config.project {
        eprintln!("  Current: org={}, number={}", project.org, project.number);
    } else {
        eprintln!("  Not configured.");
    }

    let org_default = config.project.as_ref()
        .map(|p| p.org.as_str())
        .unwrap_or("");
    let line = rl.readline(&format!("Project org [{}] (- to clear): ", if org_default.is_empty() { "none" } else { org_default }))?;
    let line = line.trim();
    if line == "-" {
        config.project = None;
    } else if !line.is_empty() {
        let org = line.to_string();
        let num_default = config.project.as_ref().map(|p| p.number).unwrap_or(0);
        let num_line = rl.readline(&format!("Project number [{}]: ", num_default))?;
        let num_line = num_line.trim();
        let number = if num_line.is_empty() {
            num_default
        } else {
            num_line.parse().context("Invalid number for project number")?
        };
        config.project = Some(ceo::config::ProjectConfig { org, number });
    }

    config.save()?;
    let path = ceo::config::Config::config_path();
    eprintln!("\nConfig saved to {}", path.display());
    Ok(())
}
