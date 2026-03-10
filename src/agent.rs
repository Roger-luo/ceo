use log::debug;
use std::process::{Command, Stdio};

use crate::config::{AgentConfig, ClaudeAgentConfig, CodexAgentConfig, GenericAgentConfig};
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
        match config {
            AgentConfig::Claude(c) => AgentKind::Claude(ClaudeAgent::new(c)),
            AgentConfig::Codex(c) => AgentKind::Codex(CodexAgent::new(c)),
            AgentConfig::Generic(c) => AgentKind::Generic(GenericAgent::new(c)),
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
    config: ClaudeAgentConfig,
}

impl ClaudeAgent {
    pub fn new(config: &ClaudeAgentConfig) -> Self {
        Self { config: config.clone() }
    }
}

impl Agent for ClaudeAgent {
    fn invoke(&self, prompt: &dyn Prompt) -> Result<String> {
        let rendered = prompt.render();
        let kind = prompt.kind();
        let model = self.config.model_for(kind);
        let mut args = vec!["-p".to_string(), "--model".to_string(), model.to_string()];

        // Merge prompt's required tools with any user-configured extras
        let required = prompt.required_tools();
        let extra = self.config.tools_for(kind);
        let mut all_tools: Vec<&str> = required.to_vec();
        if let Some(extras) = extra {
            for t in extras {
                if !all_tools.contains(&t.as_str()) {
                    all_tools.push(t.as_str());
                }
            }
        }
        args.push("--allowedTools".to_string());
        if all_tools.is_empty() {
            args.push(String::new());
        } else {
            args.push(all_tools.join(","));
        }

        // Pass effort level for thinking if configured
        let effort = self.config.effort_for(kind);
        if !effort.is_empty() {
            args.push("--effort".to_string());
            args.push(effort.to_string());
        }

        let args_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        run_cli_agent(&self.config.command, &args_refs, &rendered)
    }
}

// --- CodexAgent ---

pub struct CodexAgent {
    config: CodexAgentConfig,
}

impl CodexAgent {
    pub fn new(config: &CodexAgentConfig) -> Self {
        Self { config: config.clone() }
    }
}

impl Agent for CodexAgent {
    fn invoke(&self, prompt: &dyn Prompt) -> Result<String> {
        let rendered = prompt.render();
        let kind = prompt.kind();
        let model = self.config.model_for(kind);

        let mut args = vec!["exec".to_string()];

        // Model selection
        if !model.is_empty() {
            args.push("-m".to_string());
            args.push(model.to_string());
        }

        // Sandbox mode
        if !self.config.sandbox.is_empty() {
            args.push("--sandbox".to_string());
            args.push(self.config.sandbox.clone());
        }

        // Reasoning effort (passed via config override)
        let effort = self.config.effort_for(kind);
        if !effort.is_empty() {
            args.push("-c".to_string());
            args.push(format!("model_reasoning_effort=\"{effort}\""));
        }

        // Skip git repo trust check — ceo uses codex for text generation only
        args.push("--skip-git-repo-check".to_string());

        // Read prompt from stdin (pass "-" as prompt arg)
        args.push("-".to_string());

        let args_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        run_cli_agent(&self.config.command, &args_refs, &rendered)
    }
}

// --- GenericAgent ---

pub struct GenericAgent {
    config: GenericAgentConfig,
}

impl GenericAgent {
    pub fn new(config: &GenericAgentConfig) -> Self {
        Self { config: config.clone() }
    }
}

impl Agent for GenericAgent {
    fn invoke(&self, prompt: &dyn Prompt) -> Result<String> {
        let rendered = prompt.render();
        let args_refs: Vec<&str> = self.config.args.iter().map(|s| s.as_str()).collect();
        run_cli_agent(&self.config.command, &args_refs, &rendered)
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
