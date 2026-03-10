use log::debug;
use std::future::Future;
use std::pin::Pin;
use std::process::Stdio;

use crate::config::{AgentConfig, ClaudeAgentConfig, CodexAgentConfig, GenericAgentConfig};
use crate::error::AgentError;
use crate::prompt::Prompt;

type Result<T> = std::result::Result<T, AgentError>;

pub trait Agent: Send + Sync {
    fn invoke(&self, prompt: &dyn Prompt) -> Pin<Box<dyn Future<Output = Result<String>> + Send + '_>>;
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
    fn invoke(&self, prompt: &dyn Prompt) -> Pin<Box<dyn Future<Output = Result<String>> + Send + '_>> {
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
    fn invoke(&self, prompt: &dyn Prompt) -> Pin<Box<dyn Future<Output = Result<String>> + Send + '_>> {
        let rendered = prompt.render();
        let kind = prompt.kind().to_string();
        let required = prompt.required_tools().iter().map(|s| s.to_string()).collect::<Vec<_>>();

        let model = self.config.model_for(&kind).to_string();
        let extra = self.config.tools_for(&kind).cloned();
        let effort = self.config.effort_for(&kind).to_string();
        let command = self.config.command.clone();

        Box::pin(async move {
            let mut args = vec!["-p".to_string(), "--model".to_string(), model];

            // Merge prompt's required tools with any user-configured extras
            let mut all_tools: Vec<String> = required;
            if let Some(extras) = extra {
                for t in extras {
                    if !all_tools.contains(&t) {
                        all_tools.push(t);
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
            if !effort.is_empty() {
                args.push("--effort".to_string());
                args.push(effort);
            }

            let args_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
            run_cli_agent(&command, &args_refs, &rendered).await
        })
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
    fn invoke(&self, prompt: &dyn Prompt) -> Pin<Box<dyn Future<Output = Result<String>> + Send + '_>> {
        let rendered = prompt.render();
        let kind = prompt.kind().to_string();

        let model = self.config.model_for(&kind).to_string();
        let effort = self.config.effort_for(&kind).to_string();
        let command = self.config.command.clone();
        let sandbox = self.config.sandbox.clone();

        Box::pin(async move {
            let mut args = vec!["exec".to_string()];

            // Model selection
            if !model.is_empty() {
                args.push("-m".to_string());
                args.push(model);
            }

            // Sandbox mode
            if !sandbox.is_empty() {
                args.push("--sandbox".to_string());
                args.push(sandbox);
            }

            // Reasoning effort (passed via config override)
            if !effort.is_empty() {
                args.push("-c".to_string());
                args.push(format!("model_reasoning_effort=\"{effort}\""));
            }

            // Skip git repo trust check — ceo uses codex for text generation only
            args.push("--skip-git-repo-check".to_string());

            // Read prompt from stdin (pass "-" as prompt arg)
            args.push("-".to_string());

            let args_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
            run_cli_agent(&command, &args_refs, &rendered).await
        })
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
    fn invoke(&self, prompt: &dyn Prompt) -> Pin<Box<dyn Future<Output = Result<String>> + Send + '_>> {
        let rendered = prompt.render();
        let command = self.config.command.clone();
        let args_owned: Vec<String> = self.config.args.clone();

        Box::pin(async move {
            let args_refs: Vec<&str> = args_owned.iter().map(|s| s.as_str()).collect();
            run_cli_agent(&command, &args_refs, &rendered).await
        })
    }
}

// --- Shared CLI execution ---

async fn run_cli_agent(command: &str, args: &[&str], prompt_text: &str) -> Result<String> {
    debug!("Running agent: {} {}", command, args.join(" "));
    debug!("Prompt length: {} chars", prompt_text.len());
    let mut child = tokio::process::Command::new(command)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| AgentError::NotFound { command: command.to_string(), source: e })?;

    // Write prompt via stdin and read output concurrently to avoid deadlock.
    // If stdin write blocks (large prompt fills pipe buffer) while child tries
    // to write stdout/stderr, both sides stall. tokio::join! prevents this.
    let mut stdin = child.stdin.take();
    let write_stdin = async {
        if let Some(ref mut stdin) = stdin {
            use tokio::io::AsyncWriteExt;
            let _ = stdin.write_all(prompt_text.as_bytes()).await;
        }
        drop(stdin); // close the pipe so child sees EOF
    };

    let (_, output) = tokio::join!(write_stdin, child.wait_with_output());
    let output = output.map_err(AgentError::OutputRead)?;

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
