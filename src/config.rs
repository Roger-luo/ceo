use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Deserialize, Serialize)]
pub struct Config {
    #[serde(default)]
    pub agent: AgentConfig,
    pub repos: Vec<RepoConfig>,
    #[serde(default)]
    pub team: Vec<TeamMember>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct AgentConfig {
    #[serde(default = "default_agent_type", rename = "type")]
    pub agent_type: String,
    #[serde(default = "default_agent_command")]
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            agent_type: default_agent_type(),
            command: default_agent_command(),
            args: Vec::new(),
            timeout_secs: default_timeout(),
        }
    }
}

fn default_agent_type() -> String {
    "claude".to_string()
}

fn default_agent_command() -> String {
    "claude".to_string()
}

fn default_timeout() -> u64 {
    120
}

#[derive(Debug, Deserialize, Serialize)]
pub struct RepoConfig {
    pub name: String,
    #[serde(default)]
    pub labels_required: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct TeamMember {
    pub github: String,
    pub name: String,
    #[serde(default)]
    pub role: String,
}

impl Config {
    /// Parse a TOML string into a Config.
    pub fn load_from_str(s: &str) -> Result<Self> {
        let config: Config =
            toml::from_str(s).context("Failed to parse TOML config")?;
        Ok(config)
    }

    /// Load config by checking (in order):
    /// 1. $CEO_CONFIG environment variable
    /// 2. ~/.config/ceo/config.toml
    /// 3. ./ceo.toml
    pub fn load() -> Result<Self> {
        let path = Self::find_config_path()
            .context("No config file found. Set $CEO_CONFIG, create ~/.config/ceo/config.toml, or create ./ceo.toml")?;
        let contents = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read config file: {}", path.display()))?;
        Self::load_from_str(&contents)
    }

    pub fn config_path() -> PathBuf {
        if let Some(path) = Self::find_config_path() {
            return path;
        }
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
                let parts: Vec<&str> = value.splitn(3, ' ').collect();
                let github = parts[0].to_string();
                let name = parts.get(1).unwrap_or(&"").to_string();
                let role = parts.get(2).unwrap_or(&"").to_string();
                self.team.push(TeamMember { github, name, role });
            }
            "team.remove" => {
                self.team.retain(|t| t.github != value);
            }
            _ => anyhow::bail!("Unknown config key: {key}"),
        }
        Ok(())
    }

    fn find_config_path() -> Option<PathBuf> {
        // 1. $CEO_CONFIG
        if let Ok(env_path) = std::env::var("CEO_CONFIG") {
            let p = PathBuf::from(env_path);
            if p.exists() {
                return Some(p);
            }
        }

        // 2. ~/.config/ceo/config.toml
        if let Some(config_dir) = dirs::config_dir() {
            let p = config_dir.join("ceo").join("config.toml");
            if p.exists() {
                return Some(p);
            }
        }

        // 3. ./ceo.toml
        let p = PathBuf::from("ceo.toml");
        if p.exists() {
            return Some(p);
        }

        None
    }
}
