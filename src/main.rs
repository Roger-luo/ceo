mod tui;

use anyhow::Result;
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
    /// Generate an example config file
    Init,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Report { days } => cmd_report(days),
        Commands::Interactive => cmd_interactive(),
        Commands::Init => cmd_init(),
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

fn cmd_init() -> Result<()> {
    let example = r#"# CEO CLI configuration
# Place this file at ~/.config/ceo/config.toml

[agent]
type = "claude"
timeout_secs = 120

# Uncomment to use a different agent:
# type = "codex"
# type = "custom-tool"
# command = "custom-tool"
# args = ["--prompt"]

[[repos]]
name = "org/repo-name"
labels_required = ["priority"]

[[team]]
github = "username"
name = "Full Name"
role = "Role"
"#;

    let config_dir = dirs::config_dir()
        .map(|d| d.join("ceo"))
        .unwrap_or_else(|| std::path::PathBuf::from("."));

    let config_path = config_dir.join("config.toml");

    if config_path.exists() {
        eprintln!("Config already exists at {}", config_path.display());
        eprintln!("Edit it directly or delete it and re-run `ceo init`.");
        return Ok(());
    }

    std::fs::create_dir_all(&config_dir)?;
    std::fs::write(&config_path, example)?;
    eprintln!("Example config written to {}", config_path.display());
    eprintln!("Edit it with your repos and team, then run `ceo report`.");
    Ok(())
}
