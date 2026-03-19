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
    #[serde(default)]
    pub project: Option<ProjectConfig>,
    /// Preferred editor for `ceo roadmap edit` etc. Falls back to $EDITOR, then "vi".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub editor: Option<String>,
    /// Per-issue summary length guidance (e.g. "1 sentence", "2-3 sentences", "50 words max").
    /// Default: "1-2 sentences".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary_length: Option<String>,
    /// Number of issues to batch into a single LLM description call. Default: 10.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub batch_size: Option<usize>,
    /// Maximum number of concurrent agent calls. Default: 4.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub concurrency: Option<usize>,
    /// Slack integration settings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slack: Option<SlackConfig>,
}

// --- Per-agent-type config structs ---

#[derive(Debug, Clone)]
pub struct ClaudeAgentConfig {
    pub command: String,
    pub timeout_secs: u64,
    pub model: String,
    pub models: HashMap<String, String>,
    pub tools: HashMap<String, Vec<String>>,
    pub effort: String,
    pub effort_by_kind: HashMap<String, String>,
}

impl Default for ClaudeAgentConfig {
    fn default() -> Self {
        Self {
            command: "claude".to_string(),
            timeout_secs: default_timeout(),
            model: String::new(),
            models: HashMap::new(),
            tools: HashMap::new(),
            effort: String::new(),
            effort_by_kind: HashMap::new(),
        }
    }
}

impl ClaudeAgentConfig {
    pub fn model_for(&self, kind: &str) -> &str {
        if let Some(m) = self.models.get(kind) {
            return m.as_str();
        }
        if !self.model.is_empty() {
            return &self.model;
        }
        match kind {
            "triage" => "haiku",
            _ => "sonnet",
        }
    }

    pub fn tools_for(&self, kind: &str) -> Option<&Vec<String>> {
        self.tools.get(kind)
    }

    pub fn effort_for(&self, kind: &str) -> &str {
        if let Some(e) = self.effort_by_kind.get(kind) {
            return e.as_str();
        }
        &self.effort
    }
}

#[derive(Debug, Clone)]
pub struct CodexAgentConfig {
    pub command: String,
    pub timeout_secs: u64,
    pub model: String,
    pub models: HashMap<String, String>,
    pub sandbox: String,
    pub effort: String,
    pub effort_by_kind: HashMap<String, String>,
}

impl Default for CodexAgentConfig {
    fn default() -> Self {
        Self {
            command: "codex".to_string(),
            timeout_secs: default_timeout(),
            model: String::new(),
            models: HashMap::new(),
            sandbox: String::new(),
            effort: String::new(),
            effort_by_kind: HashMap::new(),
        }
    }
}

impl CodexAgentConfig {
    pub fn model_for(&self, kind: &str) -> &str {
        if let Some(m) = self.models.get(kind) {
            return m.as_str();
        }
        if !self.model.is_empty() {
            return &self.model;
        }
        match kind {
            "triage" => "gpt-5.1-codex-mini",
            _ => "gpt-5.3-codex",
        }
    }

    pub fn effort_for(&self, kind: &str) -> &str {
        if let Some(e) = self.effort_by_kind.get(kind) {
            return e.as_str();
        }
        &self.effort
    }
}

#[derive(Debug, Clone)]
pub struct GenericAgentConfig {
    pub command: String,
    pub args: Vec<String>,
    pub timeout_secs: u64,
}

impl Default for GenericAgentConfig {
    fn default() -> Self {
        Self {
            command: String::new(),
            args: Vec::new(),
            timeout_secs: default_timeout(),
        }
    }
}

// --- Typed enum ---

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(from = "AgentConfigHelper", into = "AgentConfigHelper")]
pub enum AgentConfig {
    Claude(ClaudeAgentConfig),
    Codex(CodexAgentConfig),
    Generic(GenericAgentConfig),
}

impl Default for AgentConfig {
    fn default() -> Self {
        AgentConfig::Claude(ClaudeAgentConfig::default())
    }
}

impl AgentConfig {
    pub fn agent_type(&self) -> &str {
        match self {
            AgentConfig::Claude(_) => "claude",
            AgentConfig::Codex(_) => "codex",
            AgentConfig::Generic(_) => "generic",
        }
    }

    pub fn command(&self) -> &str {
        match self {
            AgentConfig::Claude(c) => &c.command,
            AgentConfig::Codex(c) => &c.command,
            AgentConfig::Generic(c) => &c.command,
        }
    }

    pub fn timeout_secs(&self) -> u64 {
        match self {
            AgentConfig::Claude(c) => c.timeout_secs,
            AgentConfig::Codex(c) => c.timeout_secs,
            AgentConfig::Generic(c) => c.timeout_secs,
        }
    }

    pub fn model(&self) -> &str {
        match self {
            AgentConfig::Claude(c) => &c.model,
            AgentConfig::Codex(c) => &c.model,
            AgentConfig::Generic(_) => "",
        }
    }

    pub fn models(&self) -> Option<&HashMap<String, String>> {
        match self {
            AgentConfig::Claude(c) => Some(&c.models),
            AgentConfig::Codex(c) => Some(&c.models),
            AgentConfig::Generic(_) => None,
        }
    }
}

// --- Serde helper (flat struct for backward-compatible TOML) ---

#[derive(Debug, Deserialize, Serialize)]
struct AgentConfigHelper {
    #[serde(default = "default_agent_type", rename = "type")]
    agent_type: String,
    #[serde(default)]
    command: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default = "default_timeout")]
    timeout_secs: u64,
    #[serde(default)]
    model: String,
    #[serde(default)]
    models: HashMap<String, String>,
    #[serde(default)]
    tools: HashMap<String, Vec<String>>,
    #[serde(default)]
    effort: String,
    #[serde(default)]
    effort_by_kind: HashMap<String, String>,
    #[serde(default)]
    sandbox: String,
}

impl From<AgentConfigHelper> for AgentConfig {
    fn from(h: AgentConfigHelper) -> Self {
        match h.agent_type.as_str() {
            "codex" => AgentConfig::Codex(CodexAgentConfig {
                command: if h.command.is_empty() { "codex".to_string() } else { h.command },
                timeout_secs: h.timeout_secs,
                model: h.model,
                models: h.models,
                sandbox: h.sandbox,
                effort: h.effort,
                effort_by_kind: h.effort_by_kind,
            }),
            "generic" => AgentConfig::Generic(GenericAgentConfig {
                command: h.command,
                args: h.args,
                timeout_secs: h.timeout_secs,
            }),
            // Default: claude (handles "claude" and any unknown type)
            _ => AgentConfig::Claude(ClaudeAgentConfig {
                command: if h.command.is_empty() { "claude".to_string() } else { h.command },
                timeout_secs: h.timeout_secs,
                model: h.model,
                models: h.models,
                tools: h.tools,
                effort: h.effort,
                effort_by_kind: h.effort_by_kind,
            }),
        }
    }
}

impl From<AgentConfig> for AgentConfigHelper {
    fn from(config: AgentConfig) -> Self {
        match config {
            AgentConfig::Claude(c) => AgentConfigHelper {
                agent_type: "claude".to_string(),
                command: c.command,
                args: Vec::new(),
                timeout_secs: c.timeout_secs,
                model: c.model,
                models: c.models,
                tools: c.tools,
                effort: c.effort,
                effort_by_kind: c.effort_by_kind,
                sandbox: String::new(),
            },
            AgentConfig::Codex(c) => AgentConfigHelper {
                agent_type: "codex".to_string(),
                command: c.command,
                args: Vec::new(),
                timeout_secs: c.timeout_secs,
                model: c.model,
                models: c.models,
                tools: HashMap::new(),
                effort: c.effort,
                effort_by_kind: c.effort_by_kind,
                sandbox: c.sandbox,
            },
            AgentConfig::Generic(c) => AgentConfigHelper {
                agent_type: "generic".to_string(),
                command: c.command,
                args: c.args,
                timeout_secs: c.timeout_secs,
                model: String::new(),
                models: HashMap::new(),
                tools: HashMap::new(),
                effort: String::new(),
                effort_by_kind: HashMap::new(),
                sandbox: String::new(),
            },
        }
    }
}

fn default_agent_type() -> String {
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
    /// Branches to track commits from. Empty means default branch only.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub branches: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct TeamMember {
    pub github: String,
    pub name: String,
    #[serde(default)]
    pub role: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ProjectConfig {
    pub org: String,
    pub number: u64,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct SlackConfig {
    /// Incoming webhook URL. Can also be set via $CEO_SLACK_WEBHOOK (env var takes precedence).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub webhook_url: Option<String>,
    /// Bot OAuth token (xoxb-...) for threading and file uploads.
    /// Can also be set via $CEO_SLACK_TOKEN (env var takes precedence).
    /// When set, posts a summary then uploads the full report in a thread.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bot_token: Option<String>,
    /// Channel to post to (e.g. "#engineering-updates" or "C1234567890").
    /// Required when using bot_token. Optional for webhooks (uses webhook default).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel: Option<String>,
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
            "agent.type" => Ok(self.agent.agent_type().to_string()),
            "agent.command" => Ok(self.agent.command().to_string()),
            "agent.timeout_secs" => Ok(self.agent.timeout_secs().to_string()),
            "agent.model" => Ok(self.agent.model().to_string()),
            k if k.starts_with("agent.models.") => {
                let kind = &k["agent.models.".len()..];
                self.agent.models()
                    .and_then(|m| m.get(kind))
                    .cloned()
                    .ok_or_else(|| ConfigError::NoModel(kind.to_string()))
            }
            // Claude-specific
            k if k.starts_with("agent.tools.") => {
                let kind = &k["agent.tools.".len()..];
                match &self.agent {
                    AgentConfig::Claude(c) => c.tools.get(kind)
                        .map(|v| v.join(","))
                        .ok_or_else(|| ConfigError::NoTools(kind.to_string())),
                    _ => Err(ConfigError::UnknownKey(k.to_string())),
                }
            }
            "agent.effort" => match &self.agent {
                AgentConfig::Claude(c) => Ok(c.effort.clone()),
                AgentConfig::Codex(c) => Ok(c.effort.clone()),
                _ => Err(ConfigError::UnknownKey("agent.effort (Claude/Codex only)".to_string())),
            },
            k if k.starts_with("agent.effort_by_kind.") => {
                let kind = &k["agent.effort_by_kind.".len()..];
                match &self.agent {
                    AgentConfig::Claude(c) => c.effort_by_kind.get(kind)
                        .cloned()
                        .ok_or_else(|| ConfigError::UnknownKey(k.to_string())),
                    AgentConfig::Codex(c) => c.effort_by_kind.get(kind)
                        .cloned()
                        .ok_or_else(|| ConfigError::UnknownKey(k.to_string())),
                    _ => Err(ConfigError::UnknownKey(k.to_string())),
                }
            }
            // Codex-specific
            "agent.sandbox" => match &self.agent {
                AgentConfig::Codex(c) => Ok(c.sandbox.clone()),
                _ => Err(ConfigError::UnknownKey("agent.sandbox (Codex only)".to_string())),
            },
            // Generic-specific
            "agent.args" => match &self.agent {
                AgentConfig::Generic(c) => Ok(c.args.join(",")),
                _ => Err(ConfigError::UnknownKey("agent.args (generic only)".to_string())),
            },
            "editor" => Ok(self.editor.clone().unwrap_or_default()),
            "summary_length" => Ok(self.summary_length.clone().unwrap_or_default()),
            "batch_size" => Ok(self.batch_size().to_string()),
            "concurrency" => Ok(self.concurrency().to_string()),
            "project.org" => Ok(self.project.as_ref()
                .map(|p| p.org.clone())
                .unwrap_or_default()),
            "project.number" => Ok(self.project.as_ref()
                .map(|p| if p.number == 0 { String::new() } else { p.number.to_string() })
                .unwrap_or_default()),
            "slack.webhook_url" => Ok(self.slack.as_ref()
                .and_then(|s| s.webhook_url.clone())
                .unwrap_or_default()),
            "slack.bot_token" => Ok(self.slack.as_ref()
                .and_then(|s| s.bot_token.clone())
                .unwrap_or_default()),
            "slack.channel" => Ok(self.slack.as_ref()
                .and_then(|s| s.channel.clone())
                .unwrap_or_default()),
            _ => Err(ConfigError::UnknownKey(key.to_string())),
        }
    }

    pub fn set_field(&mut self, key: &str, value: &str) -> Result<()> {
        match key {
            "agent.type" => {
                // Switch agent type, preserving shared fields
                let model = self.agent.model().to_string();
                let models = self.agent.models().cloned().unwrap_or_default();
                let timeout = self.agent.timeout_secs();
                self.agent = match value {
                    "codex" => AgentConfig::Codex(CodexAgentConfig {
                        command: "codex".to_string(),
                        timeout_secs: timeout,
                        model,
                        models,
                        ..CodexAgentConfig::default()
                    }),
                    "generic" => AgentConfig::Generic(GenericAgentConfig {
                        timeout_secs: timeout,
                        ..GenericAgentConfig::default()
                    }),
                    _ => AgentConfig::Claude(ClaudeAgentConfig {
                        command: "claude".to_string(),
                        timeout_secs: timeout,
                        model,
                        models,
                        ..ClaudeAgentConfig::default()
                    }),
                };
            }
            "agent.command" => match &mut self.agent {
                AgentConfig::Claude(c) => c.command = value.to_string(),
                AgentConfig::Codex(c) => c.command = value.to_string(),
                AgentConfig::Generic(c) => c.command = value.to_string(),
            },
            "agent.timeout_secs" => {
                let t: u64 = value.parse().map_err(|_| ConfigError::InvalidValue {
                    key: key.to_string(),
                    message: format!("expected integer, got: {value}"),
                })?;
                match &mut self.agent {
                    AgentConfig::Claude(c) => c.timeout_secs = t,
                    AgentConfig::Codex(c) => c.timeout_secs = t,
                    AgentConfig::Generic(c) => c.timeout_secs = t,
                }
            }
            "agent.model" => match &mut self.agent {
                AgentConfig::Claude(c) => c.model = value.to_string(),
                AgentConfig::Codex(c) => c.model = value.to_string(),
                AgentConfig::Generic(_) => return Err(ConfigError::UnknownKey("agent.model (generic has no model)".to_string())),
            },
            k if k.starts_with("agent.models.") => {
                let kind = k["agent.models.".len()..].to_string();
                match &mut self.agent {
                    AgentConfig::Claude(c) => { c.models.insert(kind, value.to_string()); }
                    AgentConfig::Codex(c) => { c.models.insert(kind, value.to_string()); }
                    AgentConfig::Generic(_) => return Err(ConfigError::UnknownKey(k.to_string())),
                }
            }
            // Claude-specific
            k if k.starts_with("agent.tools.") => {
                let kind = k["agent.tools.".len()..].to_string();
                let tools: Vec<String> = value.split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                match &mut self.agent {
                    AgentConfig::Claude(c) => { c.tools.insert(kind, tools); }
                    _ => return Err(ConfigError::UnknownKey(k.to_string())),
                }
            }
            "agent.effort" => match &mut self.agent {
                AgentConfig::Claude(c) => c.effort = value.to_string(),
                AgentConfig::Codex(c) => c.effort = value.to_string(),
                _ => return Err(ConfigError::UnknownKey("agent.effort (Claude/Codex only)".to_string())),
            },
            k if k.starts_with("agent.effort_by_kind.") => {
                let kind = k["agent.effort_by_kind.".len()..].to_string();
                match &mut self.agent {
                    AgentConfig::Claude(c) => { c.effort_by_kind.insert(kind, value.to_string()); }
                    AgentConfig::Codex(c) => { c.effort_by_kind.insert(kind, value.to_string()); }
                    _ => return Err(ConfigError::UnknownKey(k.to_string())),
                }
            }
            // Codex-specific
            "agent.sandbox" => match &mut self.agent {
                AgentConfig::Codex(c) => c.sandbox = value.to_string(),
                _ => return Err(ConfigError::UnknownKey("agent.sandbox (Codex only)".to_string())),
            },
            // Generic-specific
            "agent.args" => match &mut self.agent {
                AgentConfig::Generic(c) => {
                    c.args = value.split(',').map(|s| s.trim().to_string()).collect();
                }
                _ => return Err(ConfigError::UnknownKey("agent.args (generic only)".to_string())),
            },
            "repos.add" => {
                self.repos.push(RepoConfig {
                    name: value.to_string(),
                    labels_required: Vec::new(),
                    branches: Vec::new(),
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
            "project.org" => {
                if let Some(ref mut p) = self.project {
                    p.org = value.to_string();
                } else {
                    self.project = Some(ProjectConfig {
                        org: value.to_string(),
                        number: 0,
                    });
                }
            }
            "project.number" => {
                let n: u64 = value.parse().map_err(|_| ConfigError::InvalidValue {
                    key: key.to_string(),
                    message: format!("expected integer, got: {value}"),
                })?;
                if let Some(ref mut p) = self.project {
                    p.number = n;
                } else {
                    self.project = Some(ProjectConfig {
                        org: String::new(),
                        number: n,
                    });
                }
            }
            "editor" => {
                self.editor = if value.is_empty() { None } else { Some(value.to_string()) };
            }
            "summary_length" => {
                self.summary_length = if value.is_empty() { None } else { Some(value.to_string()) };
            }
            "batch_size" => {
                let n: usize = value.parse().map_err(|_| ConfigError::InvalidValue {
                    key: key.to_string(),
                    message: format!("expected positive integer, got: {value}"),
                })?;
                self.batch_size = if n == 10 { None } else { Some(n) };
            }
            "concurrency" => {
                let n: usize = value.parse().map_err(|_| ConfigError::InvalidValue {
                    key: key.to_string(),
                    message: format!("expected positive integer, got: {value}"),
                })?;
                self.concurrency = if n == 4 { None } else { Some(n) };
            }
            "slack.webhook_url" => {
                let url = if value.is_empty() { None } else { Some(value.to_string()) };
                if let Some(ref mut s) = self.slack {
                    s.webhook_url = url;
                } else {
                    self.slack = Some(SlackConfig { webhook_url: url, bot_token: None, channel: None });
                }
            }
            "slack.bot_token" => {
                let token = if value.is_empty() { None } else { Some(value.to_string()) };
                if let Some(ref mut s) = self.slack {
                    s.bot_token = token;
                } else {
                    self.slack = Some(SlackConfig { webhook_url: None, bot_token: token, channel: None });
                }
            }
            "slack.channel" => {
                let channel = if value.is_empty() { None } else { Some(value.to_string()) };
                if let Some(ref mut s) = self.slack {
                    s.channel = channel;
                } else {
                    self.slack = Some(SlackConfig { webhook_url: None, bot_token: None, channel });
                }
            }
            _ => return Err(ConfigError::UnknownKey(key.to_string())),
        }
        Ok(())
    }

    /// Resolve editor: config > $EDITOR > vi
    pub fn editor(&self) -> String {
        self.editor.clone()
            .or_else(|| std::env::var("EDITOR").ok())
            .unwrap_or_else(|| "vi".to_string())
    }

    /// Resolve summary length guidance for per-issue prompts.
    pub fn summary_length(&self) -> &str {
        self.summary_length.as_deref().unwrap_or("1-2 sentences")
    }

    /// Number of issues to batch per LLM description call.
    pub fn batch_size(&self) -> usize {
        self.batch_size.unwrap_or(10)
    }

    /// Maximum concurrent agent calls.
    pub fn concurrency(&self) -> usize {
        self.concurrency.unwrap_or(4)
    }

    /// Returns the UI tab schema for the interactive config editor.
    ///
    /// This is the **single source of truth** for which config fields appear in the
    /// TUI. Each `FormFieldSpec::key` maps directly to `get_field` / `set_field`,
    /// guaranteeing the panel and the config file stay in sync.
    pub fn ui_tabs(&self) -> Vec<TabSpec> {
        let mut tabs = vec![];

        // ── Agent ──────────────────────────────────────────
        tabs.push(TabSpec::Form(FormTab {
            name: "Agent",
            fields: vec![
                FormFieldSpec {
                    key: "agent.type",
                    label: "Type",
                    placeholder: "claude",
                    options: vec!["claude".into(), "codex".into(), "generic".into()],
                },
                FormFieldSpec {
                    key: "agent.command",
                    label: "Command",
                    placeholder: "claude",
                    options: vec![],
                },
                FormFieldSpec {
                    key: "agent.timeout_secs",
                    label: "Timeout (secs)",
                    placeholder: "120",
                    options: vec![],
                },
            ],
        }));

        // ── Models & Tools (dynamic by agent type) ─────────
        let model_opts = model_options_for(self.agent.agent_type());
        let mut mf = Vec::new();
        if !matches!(&self.agent, AgentConfig::Generic(_)) {
            mf.push(FormFieldSpec { key: "agent.model", label: "Default model", placeholder: "agent default", options: model_opts.clone() });
            mf.push(FormFieldSpec { key: "agent.models.summary", label: "Summary model", placeholder: "(default)", options: model_opts.clone() });
            mf.push(FormFieldSpec { key: "agent.models.triage", label: "Triage model", placeholder: "(default)", options: model_opts });
        }
        match &self.agent {
            AgentConfig::Claude(_) => {
                mf.push(FormFieldSpec { key: "agent.tools.summary", label: "Summary tools", placeholder: "(none)", options: vec![] });
                mf.push(FormFieldSpec { key: "agent.tools.triage", label: "Triage tools", placeholder: "(none)", options: vec![] });
                mf.push(FormFieldSpec { key: "agent.effort", label: "Effort", placeholder: "(none)", options: effort_options() });
            }
            AgentConfig::Codex(_) => {
                mf.push(FormFieldSpec { key: "agent.sandbox", label: "Sandbox", placeholder: "(none)", options: vec!["".into(), "read-only".into(), "full".into(), "none".into()] });
                mf.push(FormFieldSpec { key: "agent.effort", label: "Effort", placeholder: "(none)", options: effort_options() });
            }
            AgentConfig::Generic(_) => {
                mf.push(FormFieldSpec { key: "agent.args", label: "Args", placeholder: "--flag1, --flag2", options: vec![] });
            }
        }
        tabs.push(TabSpec::Form(FormTab { name: "Models", fields: mf }));

        // ── Repos (list) ───────────────────────────────────
        tabs.push(TabSpec::List(ListTab {
            name: "Repos",
            add_placeholder: "org/repo",
            detail_labels: vec!["Required labels", "Branches (comma-sep, empty=default)"],
        }));

        // ── Team (list) ────────────────────────────────────
        tabs.push(TabSpec::List(ListTab {
            name: "Team",
            add_placeholder: "@username",
            detail_labels: vec!["Name", "Role"],
        }));

        // ── Project ────────────────────────────────────────
        tabs.push(TabSpec::Form(FormTab {
            name: "Project",
            fields: vec![
                FormFieldSpec { key: "project.org", label: "Organization", placeholder: "org-name", options: vec![] },
                FormFieldSpec { key: "project.number", label: "Project number", placeholder: "1", options: vec![] },
                FormFieldSpec { key: "editor", label: "Editor", placeholder: "$EDITOR or vi", options: vec![] },
            ],
        }));

        // ── Slack ──────────────────────────────────────────
        tabs.push(TabSpec::Form(FormTab {
            name: "Slack",
            fields: vec![
                FormFieldSpec { key: "slack.webhook_url", label: "Webhook URL", placeholder: "https://hooks.slack.com/services/...", options: vec![] },
                FormFieldSpec { key: "slack.bot_token", label: "Bot token", placeholder: "xoxb-... (optional, enables threading)", options: vec![] },
                FormFieldSpec { key: "slack.channel", label: "Channel", placeholder: "#channel (required for bot token)", options: vec![] },
            ],
        }));

        tabs
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

// ========================================================================
// UI schema types — shared between config.rs and tui.rs
// ========================================================================

/// A single field in a config form tab.
pub struct FormFieldSpec {
    /// Key for `get_field` / `set_field` — the canonical config path.
    pub key: &'static str,
    /// Human-readable label shown in the TUI.
    pub label: &'static str,
    /// Placeholder shown when value is empty.
    pub placeholder: &'static str,
    /// If non-empty, the field cycles through these options instead of free text.
    pub options: Vec<String>,
}

/// A form-based tab (key-value fields).
pub struct FormTab {
    pub name: &'static str,
    pub fields: Vec<FormFieldSpec>,
}

/// A list-based tab (repos or team members).
pub struct ListTab {
    pub name: &'static str,
    pub add_placeholder: &'static str,
    pub detail_labels: Vec<&'static str>,
}

/// A tab in the config editor — either a form or a list.
pub enum TabSpec {
    Form(FormTab),
    List(ListTab),
}

pub fn model_options_for(agent_type: &str) -> Vec<String> {
    match agent_type {
        "claude" => vec!["".into(), "haiku".into(), "sonnet".into(), "opus".into()],
        "codex" => vec![
            "".into(),
            "gpt-5.3-codex".into(),
            "gpt-5.4".into(),
            "gpt-5.2-codex".into(),
            "gpt-5.1-codex-max".into(),
            "gpt-5.2".into(),
            "gpt-5.1-codex-mini".into(),
        ],
        _ => vec![],
    }
}

pub fn effort_options() -> Vec<String> {
    vec!["".into(), "low".into(), "medium".into(), "high".into()]
}
