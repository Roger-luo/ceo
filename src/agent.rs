use log::debug;
use std::collections::HashMap;
use std::process::{Command, Stdio};

use crate::config::AgentConfig;
use crate::error::AgentError;
use crate::prompt::Prompt;

type Result<T> = std::result::Result<T, AgentError>;

pub trait Agent {
    fn invoke(&self, prompt: &dyn Prompt) -> Result<String>;
}

pub enum AgentKind {
    Claude(ClaudeAgent),
    Codex(CodexAgent),
    Generic(GenericAgent),
}

impl AgentKind {
    pub fn from_config(config: &AgentConfig) -> Self {
        match config.agent_type.as_str() {
            "claude" => AgentKind::Claude(ClaudeAgent::from_config(config)),
            "codex" => AgentKind::Codex(CodexAgent::from_config(config)),
            _ => AgentKind::Generic(GenericAgent::from_config(config)),
        }
    }
}

impl Agent for AgentKind {
    fn invoke(&self, prompt: &dyn Prompt) -> Result<String> {
        match self {
            AgentKind::Claude(a) => a.invoke(prompt),
            AgentKind::Codex(a) => a.invoke(prompt),
            AgentKind::Generic(a) => a.invoke(prompt),
        }
    }
}

// --- ClaudeAgent ---

pub struct ClaudeAgent {
    pub command: String,
    pub timeout_secs: u64,
    pub model: String,
    pub models: HashMap<String, String>,
    pub tools: HashMap<String, Vec<String>>,
}

impl ClaudeAgent {
    pub fn from_config(config: &AgentConfig) -> Self {
        Self {
            command: if config.command.is_empty() {
                "claude".to_string()
            } else {
                config.command.clone()
            },
            timeout_secs: config.timeout_secs,
            model: config.model.clone(),
            models: config.models.clone(),
            tools: config.tools.clone(),
        }
    }

    fn model_for(&self, kind: &str) -> &str {
        if let Some(m) = self.models.get(kind) {
            return m.as_str();
        }
        if !self.model.is_empty() {
            return &self.model;
        }
        // Cost-effective defaults per prompt type
        match kind {
            "triage" => "haiku",
            _ => "sonnet",
        }
    }

    fn tools_for(&self, kind: &str) -> Option<&Vec<String>> {
        self.tools.get(kind)
    }
}

impl Agent for ClaudeAgent {
    fn invoke(&self, prompt: &dyn Prompt) -> Result<String> {
        let rendered = prompt.render();
        let kind = prompt.kind();
        let model = self.model_for(kind);
        let mut args = vec!["-p".to_string(), "--model".to_string(), model.to_string()];

        // Merge prompt's required tools with any user-configured extras
        let required = prompt.required_tools();
        let extra = self.tools_for(kind);
        let mut all_tools: Vec<&str> = required.to_vec();
        if let Some(extras) = extra {
            for t in extras {
                if !all_tools.contains(&t.as_str()) {
                    all_tools.push(t.as_str());
                }
            }
        }
        // Always pass --allowedTools: explicit list if tools needed,
        // empty string to disable all tools otherwise
        args.push("--allowedTools".to_string());
        if all_tools.is_empty() {
            args.push(String::new());
        } else {
            args.push(all_tools.join(","));
        }

        let args_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        run_cli_agent(&self.command, &args_refs, &rendered)
    }
}

// --- CodexAgent ---

pub struct CodexAgent {
    pub command: String,
    pub timeout_secs: u64,
}

impl CodexAgent {
    pub fn from_config(config: &AgentConfig) -> Self {
        Self {
            command: if config.command.is_empty() {
                "codex".to_string()
            } else {
                config.command.clone()
            },
            timeout_secs: config.timeout_secs,
        }
    }
}

impl Agent for CodexAgent {
    fn invoke(&self, prompt: &dyn Prompt) -> Result<String> {
        let rendered = prompt.render();
        run_cli_agent(&self.command, &["-q"], &rendered)
    }
}

// --- GenericAgent ---

pub struct GenericAgent {
    pub command: String,
    pub args: Vec<String>,
    pub timeout_secs: u64,
}

impl GenericAgent {
    pub fn from_config(config: &AgentConfig) -> Self {
        Self {
            command: config.command.clone(),
            args: config.args.clone(),
            timeout_secs: config.timeout_secs,
        }
    }
}

impl Agent for GenericAgent {
    fn invoke(&self, prompt: &dyn Prompt) -> Result<String> {
        let rendered = prompt.render();
        let args_refs: Vec<&str> = self.args.iter().map(|s| s.as_str()).collect();
        run_cli_agent(&self.command, &args_refs, &rendered)
    }
}

// --- Shared CLI execution ---

fn run_cli_agent(command: &str, args: &[&str], prompt_text: &str) -> Result<String> {
    debug!("Running agent: {} {}", command, args.join(" "));
    debug!("Prompt length: {} chars", prompt_text.len());
    let mut child = Command::new(command)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| AgentError::NotFound { command: command.to_string(), source: e })?;

    // Write prompt via stdin to avoid OS argument length limits
    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        stdin.write_all(prompt_text.as_bytes())
            .map_err(AgentError::OutputRead)?;
        // stdin is dropped here, closing the pipe
    }

    let output = child
        .wait_with_output()
        .map_err(AgentError::OutputRead)?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let detail = if !stderr.is_empty() {
            stderr.to_string()
        } else if !stdout.is_empty() {
            stdout.to_string()
        } else {
            format!("exit code: {}", output.status)
        };
        return Err(AgentError::ExitError(detail));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}
