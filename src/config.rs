use log::debug;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

use crate::error::ConfigError;

type Result<T> = std::result::Result<T, ConfigError>;

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
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub models: HashMap<String, String>,
    #[serde(default)]
    pub tools: HashMap<String, Vec<String>>,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            agent_type: default_agent_type(),
            command: default_agent_command(),
            args: Vec::new(),
            timeout_secs: default_timeout(),
            model: String::new(),
            models: HashMap::new(),
            tools: HashMap::new(),
        }
    }
}

impl AgentConfig {
    /// Returns the model to use for a given prompt kind.
    /// Checks `models` map first, falls back to `model`, then empty (agent default).
    pub fn model_for(&self, kind: &str) -> &str {
        if let Some(m) = self.models.get(kind) {
            return m.as_str();
        }
        &self.model
    }

    /// Returns the allowed tools for a given prompt kind.
    /// Returns None if no tools are configured (agent decides).
    pub fn tools_for(&self, kind: &str) -> Option<&Vec<String>> {
        self.tools.get(kind)
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
        Ok(toml::from_str(s)?)
    }

    /// Load config by checking (in order):
    /// 1. $CEO_CONFIG environment variable
    /// 2. ~/.config/ceo/config.toml
    /// 3. ./ceo.toml
    pub fn load() -> Result<Self> {
        let path = Self::find_config_path()
            .ok_or(ConfigError::NotFound)?;
        debug!("Loading config from {}", path.display());
        let contents = std::fs::read_to_string(&path)
            .map_err(|e| ConfigError::ReadFile { path: path.clone(), source: e })?;
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
                .map_err(|e| ConfigError::CreateDir { path: parent.to_path_buf(), source: e })?;
        }
        let toml_str = toml::to_string_pretty(self)?;
        std::fs::write(&path, toml_str)
            .map_err(|e| ConfigError::WriteFile { path: path.clone(), source: e })?;
        Ok(())
    }

    pub fn get_field(&self, key: &str) -> Result<String> {
        match key {
            "agent.type" => Ok(self.agent.agent_type.clone()),
            "agent.command" => Ok(self.agent.command.clone()),
            "agent.timeout_secs" => Ok(self.agent.timeout_secs.to_string()),
            "agent.args" => Ok(self.agent.args.join(",")),
            "agent.model" => Ok(self.agent.model.clone()),
            k if k.starts_with("agent.models.") => {
                let kind = &k["agent.models.".len()..];
                self.agent.models.get(kind)
                    .cloned()
                    .ok_or_else(|| ConfigError::NoModel(kind.to_string()))
            }
            k if k.starts_with("agent.tools.") => {
                let kind = &k["agent.tools.".len()..];
                self.agent.tools.get(kind)
                    .map(|v| v.join(","))
                    .ok_or_else(|| ConfigError::NoTools(kind.to_string()))
            }
            _ => Err(ConfigError::UnknownKey(key.to_string())),
        }
    }

    pub fn set_field(&mut self, key: &str, value: &str) -> Result<()> {
        match key {
            "agent.type" => self.agent.agent_type = value.to_string(),
            "agent.command" => self.agent.command = value.to_string(),
            "agent.timeout_secs" => {
                self.agent.timeout_secs = value.parse()
                    .map_err(|_| ConfigError::InvalidValue {
                        key: key.to_string(),
                        message: format!("expected integer, got: {value}"),
                    })?;
            }
            "agent.args" => {
                self.agent.args = value.split(',').map(|s| s.trim().to_string()).collect();
            }
            "agent.model" => self.agent.model = value.to_string(),
            k if k.starts_with("agent.models.") => {
                let kind = k["agent.models.".len()..].to_string();
                self.agent.models.insert(kind, value.to_string());
            }
            k if k.starts_with("agent.tools.") => {
                let kind = k["agent.tools.".len()..].to_string();
                let tools: Vec<String> = value.split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                self.agent.tools.insert(kind, tools);
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
            _ => return Err(ConfigError::UnknownKey(key.to_string())),
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
