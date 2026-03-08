# Config Command — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add `ceo config` command with interactive wizard and `set`/`get`/`show` subcommands for managing config.

**Architecture:** Extend Config with Serialize + save/set_field/get_field methods. Add Config subcommand to clap CLI with wizard and set/get/show modes. Replace `ceo init`.

**Tech Stack:** Rust 2024, serde (Serialize+Deserialize), toml, clap, anyhow, std::io for interactive prompts

---

### Task 1: Add Serialize and save/config_path to Config

**Files:**
- Modify: `src/config.rs`
- Modify: `tests/config_test.rs`

**Step 1: Add Serialize derive to all config structs**

In `src/config.rs`, change the derives:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
pub struct Config { ... }

#[derive(Debug, Deserialize, Serialize)]
pub struct AgentConfig { ... }

#[derive(Debug, Deserialize, Serialize)]
pub struct RepoConfig { ... }

#[derive(Debug, Deserialize, Serialize)]
pub struct TeamMember { ... }
```

Note: The `rename = "type"` on `agent_type` handles both serialization and deserialization.

**Step 2: Add `config_path()` and `save()` methods**

Add to the `impl Config` block:

```rust
    pub fn config_path() -> PathBuf {
        if let Some(path) = Self::find_config_path() {
            return path;
        }
        // Default path when no config exists yet
        dirs::config_dir()
            .map(|d| d.join("ceo").join("config.toml"))
            .unwrap_or_else(|| PathBuf::from("ceo.toml"))
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::config_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create config directory: {}", parent.display()))?;
        }
        let toml_str = toml::to_string_pretty(self)
            .context("Failed to serialize config to TOML")?;
        std::fs::write(&path, toml_str)
            .with_context(|| format!("Failed to write config file: {}", path.display()))?;
        Ok(())
    }
```

**Step 3: Write tests**

Add to `tests/config_test.rs`:

```rust
#[test]
fn config_round_trip_serialize_deserialize() {
    let config = Config::load_from_str(r#"
[agent]
type = "claude"
command = "claude"
timeout_secs = 120

[[repos]]
name = "org/frontend"
labels_required = ["priority"]

[[team]]
github = "alice"
name = "Alice Smith"
role = "Lead"
"#).unwrap();

    let serialized = toml::to_string_pretty(&config).unwrap();
    let reparsed: Config = toml::from_str(&serialized).unwrap();
    assert_eq!(reparsed.agent.agent_type, "claude");
    assert_eq!(reparsed.repos[0].name, "org/frontend");
    assert_eq!(reparsed.team[0].github, "alice");
}
```

**Step 4: Verify**

Run: `cargo test --test config_test`

**Step 5: Commit**

```bash
git add src/config.rs tests/config_test.rs
git commit -m "feat: add Serialize and save/config_path to Config"
```

---

### Task 2: Add set_field and get_field to Config

**Files:**
- Modify: `src/config.rs`
- Modify: `tests/config_test.rs`

**Step 1: Write tests**

Add to `tests/config_test.rs`:

```rust
#[test]
fn config_get_field() {
    let config = Config::load_from_str(r#"
[agent]
type = "claude"
timeout_secs = 120

[[repos]]
name = "org/frontend"

[[team]]
github = "alice"
name = "Alice Smith"
role = "Lead"
"#).unwrap();

    assert_eq!(config.get_field("agent.type").unwrap(), "claude");
    assert_eq!(config.get_field("agent.timeout_secs").unwrap(), "120");
}

#[test]
fn config_set_agent_field() {
    let mut config = Config::load_from_str(r#"
[[repos]]
name = "org/repo"
"#).unwrap();

    config.set_field("agent.type", "codex").unwrap();
    assert_eq!(config.agent.agent_type, "codex");

    config.set_field("agent.timeout_secs", "60").unwrap();
    assert_eq!(config.agent.timeout_secs, 60);

    config.set_field("agent.command", "/usr/bin/codex").unwrap();
    assert_eq!(config.agent.command, "/usr/bin/codex");

    config.set_field("agent.args", "-q,--verbose").unwrap();
    assert_eq!(config.agent.args, vec!["-q", "--verbose"]);
}

#[test]
fn config_set_repos_add_remove() {
    let mut config = Config::load_from_str(r#"
[[repos]]
name = "org/existing"
"#).unwrap();

    config.set_field("repos.add", "org/new-repo").unwrap();
    assert_eq!(config.repos.len(), 2);
    assert_eq!(config.repos[1].name, "org/new-repo");

    config.set_field("repos.remove", "org/existing").unwrap();
    assert_eq!(config.repos.len(), 1);
    assert_eq!(config.repos[0].name, "org/new-repo");
}

#[test]
fn config_get_field_unknown_key_errors() {
    let config = Config::load_from_str(r#"
[[repos]]
name = "org/repo"
"#).unwrap();

    assert!(config.get_field("nonexistent.field").is_err());
}
```

**Step 2: Implement set_field and get_field**

Add to the `impl Config` block in `src/config.rs`:

```rust
    pub fn get_field(&self, key: &str) -> Result<String> {
        match key {
            "agent.type" => Ok(self.agent.agent_type.clone()),
            "agent.command" => Ok(self.agent.command.clone()),
            "agent.timeout_secs" => Ok(self.agent.timeout_secs.to_string()),
            "agent.args" => Ok(self.agent.args.join(",")),
            _ => anyhow::bail!("Unknown config key: {key}. Valid keys: agent.type, agent.command, agent.timeout_secs, agent.args"),
        }
    }

    pub fn set_field(&mut self, key: &str, value: &str) -> Result<()> {
        match key {
            "agent.type" => self.agent.agent_type = value.to_string(),
            "agent.command" => self.agent.command = value.to_string(),
            "agent.timeout_secs" => {
                self.agent.timeout_secs = value.parse()
                    .with_context(|| format!("Invalid timeout value: {value}"))?;
            }
            "agent.args" => {
                self.agent.args = value.split(',').map(|s| s.trim().to_string()).collect();
            }
            "repos.add" => {
                self.repos.push(RepoConfig {
                    name: value.to_string(),
                    labels_required: Vec::new(),
                });
            }
            "repos.remove" => {
                self.repos.retain(|r| r.name != value);
            }
            "team.add" => {
                // value format: "github_handle Full Name Role"
                // Minimum: just the github handle
                let parts: Vec<&str> = value.splitn(3, ' ').collect();
                let github = parts[0].to_string();
                let name = parts.get(1).unwrap_or(&"").to_string();
                let role = parts.get(2).unwrap_or(&"").to_string();
                self.team.push(TeamMember { github, name, role });
            }
            "team.remove" => {
                self.team.retain(|t| t.github != value);
            }
            _ => anyhow::bail!("Unknown config key: {key}. Valid keys: agent.type, agent.command, agent.timeout_secs, agent.args, repos.add, repos.remove, team.add, team.remove"),
        }
        Ok(())
    }
```

**Step 3: Verify**

Run: `cargo test --test config_test`

**Step 4: Commit**

```bash
git add src/config.rs tests/config_test.rs
git commit -m "feat: add set_field and get_field to Config"
```

---

### Task 3: Add config subcommand to CLI

**Files:**
- Modify: `src/main.rs`

**Step 1: Replace Init with Config in clap**

Replace the `Commands` enum and add the config command handler. The full updated main.rs:

The `Commands` enum becomes:

```rust
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
```

The match becomes:

```rust
    match cli.command {
        Commands::Report { days } => cmd_report(days),
        Commands::Interactive => cmd_interactive(),
        Commands::Config { action } => cmd_config(action),
        Commands::Init => cmd_config(None),
    }
```

**Step 2: Implement cmd_config**

```rust
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
```

**Step 3: Remove old cmd_init function**

Delete the `cmd_init` function entirely.

**Step 4: Verify build**

Run: `cargo build`
Run: `cargo run -- --help`
Run: `cargo run -- config --help`

**Step 5: Commit**

```bash
git add src/main.rs
git commit -m "feat: add ceo config command with wizard and set/get/show"
```

---

### Task 4: Verify and cleanup

**Step 1: Run full test suite**

Run: `cargo test`

**Step 2: Verify CLI commands**

Run: `cargo run -- --help`
Run: `cargo run -- config --help`
Run: `cargo run -- config set --help`

**Step 3: Commit if needed**
