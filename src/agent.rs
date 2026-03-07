use anyhow::{Context, Result};
use crate::config::AgentConfig;
use crate::prompt::Prompt;
use std::process::{Command, Stdio};

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
        }
    }
}

impl Agent for ClaudeAgent {
    fn invoke(&self, prompt: &dyn Prompt) -> Result<String> {
        let rendered = prompt.render();
        run_cli_agent(&self.command, &["-p"], &rendered)
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
    let child = Command::new(command)
        .args(args)
        .arg(prompt_text)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("Agent command '{}' not found. Check your config.", command))?;

    let output = child
        .wait_with_output()
        .context("Failed to read agent output")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Agent exited with error: {}", stderr);
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

// --- Backward-compatible shims (removed in later tasks) ---

pub trait AgentRunner {
    fn invoke(&self, prompt: &str) -> Result<String>;
}

pub struct RealAgentRunner {
    pub command: String,
    pub args: Vec<String>,
    pub timeout_secs: u64,
}

impl RealAgentRunner {
    pub fn from_config(config: &AgentConfig) -> Self {
        Self {
            command: config.command.clone(),
            args: config.args.clone(),
            timeout_secs: config.timeout_secs,
        }
    }
}

impl AgentRunner for RealAgentRunner {
    fn invoke(&self, prompt: &str) -> Result<String> {
        let args_refs: Vec<&str> = self.args.iter().map(|s| s.as_str()).collect();
        run_cli_agent(&self.command, &args_refs, prompt)
    }
}

pub fn build_weekly_summary_prompt(repo: &str, issue_summaries: &str) -> String {
    format!(
        "Summarize the past week's progress for repo {repo}. \
         Here are the issues updated this week:\n\
         {issue_summaries}\n\n\
         Provide:\n\
         1) Key progress and completed work\n\
         2) Big updates or decisions\n\
         3) What people are planning to work on next"
    )
}

pub fn build_triage_prompt(title: &str, body: &str, comments: &str) -> String {
    format!(
        "Analyze this GitHub issue. It lacks proper labels/status. \
         Summarize what the issue is about in 2-3 sentences and suggest \
         appropriate priority and status labels.\n\n\
         Issue: {title}\n\n\
         {body}\n\n\
         Comments:\n{comments}"
    )
}
