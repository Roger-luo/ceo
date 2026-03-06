use anyhow::{Context, Result};
use crate::config::AgentConfig;
use std::process::{Command, Stdio};

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
        let child = Command::new(&self.command)
            .args(&self.args)
            .arg(prompt)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("Agent command '{}' not found. Check your config.", self.command))?;

        let output = child
            .wait_with_output()
            .context("Failed to read agent output")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("Agent exited with error: {}", stderr);
        }

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }
}

pub fn run_agent(runner: &dyn AgentRunner, prompt: &str) -> Result<String> {
    runner.invoke(prompt)
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
