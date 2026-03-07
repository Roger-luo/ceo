# Agent Abstraction Refactor — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Refactor the agent/prompt system into a Prompt trait with typed prompt structs and an Agent trait with AgentKind enum dispatch (Claude, Codex, Generic).

**Architecture:** The `Prompt` trait provides `render() -> String`. The `Agent` trait provides `invoke(&dyn Prompt) -> Result<String>`. `AgentKind` is an enum with variants for Claude, Codex, and Generic, each knowing their CLI conventions. A factory method on `AgentKind` reads the config `type` field to construct the right variant.

**Tech Stack:** Rust 2024, anyhow, serde, toml, std::process::Command

---

### Task 1: Create Prompt Trait & Types

**Files:**
- Create: `src/prompt.rs`
- Modify: `src/lib.rs`
- Create: `tests/prompt_test.rs`

**Step 1: Write the failing test**

Create `tests/prompt_test.rs`:

```rust
use ceo::prompt::{Prompt, WeeklySummaryPrompt, IssueTriagePrompt};

#[test]
fn weekly_summary_prompt_renders_with_repo_and_issues() {
    let prompt = WeeklySummaryPrompt {
        repo: "org/frontend".to_string(),
        issue_summaries: "- #1 Fix bug\n- #2 Add feature".to_string(),
    };
    let rendered = prompt.render();
    assert!(rendered.contains("org/frontend"));
    assert!(rendered.contains("Fix bug"));
    assert!(rendered.contains("Add feature"));
    assert!(rendered.contains("Key progress"));
}

#[test]
fn issue_triage_prompt_renders_with_all_fields() {
    let prompt = IssueTriagePrompt {
        title: "Fix login redirect".to_string(),
        body: "The login page redirects in a loop.".to_string(),
        comments: "alice: I think it's SSO.".to_string(),
    };
    let rendered = prompt.render();
    assert!(rendered.contains("Fix login redirect"));
    assert!(rendered.contains("login page redirects"));
    assert!(rendered.contains("SSO"));
    assert!(rendered.contains("labels"));
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --test prompt_test`
Expected: FAIL — module `ceo::prompt` not found

**Step 3: Create the prompt module**

Create `src/prompt.rs`:

```rust
pub trait Prompt {
    fn render(&self) -> String;
}

pub struct WeeklySummaryPrompt {
    pub repo: String,
    pub issue_summaries: String,
}

impl Prompt for WeeklySummaryPrompt {
    fn render(&self) -> String {
        format!(
            "Summarize the past week's progress for repo {}. \
             Here are the issues updated this week:\n\
             {}\n\n\
             Provide:\n\
             1) Key progress and completed work\n\
             2) Big updates or decisions\n\
             3) What people are planning to work on next",
            self.repo, self.issue_summaries
        )
    }
}

pub struct IssueTriagePrompt {
    pub title: String,
    pub body: String,
    pub comments: String,
}

impl Prompt for IssueTriagePrompt {
    fn render(&self) -> String {
        format!(
            "Analyze this GitHub issue. It lacks proper labels/status. \
             Summarize what the issue is about in 2-3 sentences and suggest \
             appropriate priority and status labels.\n\n\
             Issue: {}\n\n\
             {}\n\n\
             Comments:\n{}",
            self.title, self.body, self.comments
        )
    }
}
```

**Step 4: Add module to lib.rs**

Change `src/lib.rs` from:

```rust
pub mod agent;
pub mod config;
pub mod filter;
pub mod gh;
pub mod github;
pub mod pipeline;
pub mod report;
```

To:

```rust
pub mod agent;
pub mod config;
pub mod filter;
pub mod gh;
pub mod github;
pub mod pipeline;
pub mod prompt;
pub mod report;
```

**Step 5: Run tests**

Run: `cargo test --test prompt_test`
Expected: 2 tests PASS

**Step 6: Commit**

```bash
git add src/prompt.rs src/lib.rs tests/prompt_test.rs
git commit -m "feat: add Prompt trait with WeeklySummaryPrompt and IssueTriagePrompt"
```

---

### Task 2: Add `agent_type` to Config

**Files:**
- Modify: `src/config.rs`
- Modify: `tests/config_test.rs`

**Step 1: Write the failing test**

Add to `tests/config_test.rs`:

```rust
#[test]
fn parse_config_with_agent_type() {
    let toml_str = r#"
[agent]
type = "codex"
command = "codex"
args = ["-q"]
timeout_secs = 60

[[repos]]
name = "org/repo"
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert_eq!(config.agent.agent_type, "codex");
    assert_eq!(config.agent.command, "codex");
}

#[test]
fn agent_type_defaults_to_claude() {
    let toml_str = r#"
[[repos]]
name = "org/repo"
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert_eq!(config.agent.agent_type, "claude");
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --test config_test`
Expected: FAIL — no field `agent_type` on `AgentConfig`

**Step 3: Add `agent_type` field to AgentConfig**

In `src/config.rs`, modify the `AgentConfig` struct. Change it from:

```rust
#[derive(Debug, Deserialize)]
pub struct AgentConfig {
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
            command: default_agent_command(),
            args: Vec::new(),
            timeout_secs: default_timeout(),
        }
    }
}
```

To:

```rust
#[derive(Debug, Deserialize)]
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
```

And add the default function:

```rust
fn default_agent_type() -> String {
    "claude".to_string()
}
```

**Step 4: Run tests**

Run: `cargo test --test config_test`
Expected: all config tests PASS (including existing ones)

**Step 5: Commit**

```bash
git add src/config.rs tests/config_test.rs
git commit -m "feat: add agent_type field to AgentConfig (defaults to claude)"
```

---

### Task 3: Rewrite Agent Module with Trait & Enum Dispatch

**Files:**
- Rewrite: `src/agent.rs`
- Rewrite: `tests/agent_test.rs`

**Step 1: Write the failing tests**

Replace `tests/agent_test.rs` entirely with:

```rust
use ceo::agent::{Agent, AgentKind, ClaudeAgent, CodexAgent, GenericAgent};
use ceo::prompt::{Prompt, WeeklySummaryPrompt, IssueTriagePrompt};
use ceo::config::AgentConfig;

// Mock prompt for testing
struct TestPrompt(String);

impl Prompt for TestPrompt {
    fn render(&self) -> String {
        self.0.clone()
    }
}

#[test]
fn agent_kind_from_config_claude() {
    let config = AgentConfig {
        agent_type: "claude".to_string(),
        command: "claude".to_string(),
        args: vec!["-p".to_string()],
        timeout_secs: 120,
    };
    let agent = AgentKind::from_config(&config);
    assert!(matches!(agent, AgentKind::Claude(_)));
}

#[test]
fn agent_kind_from_config_codex() {
    let config = AgentConfig {
        agent_type: "codex".to_string(),
        command: "codex".to_string(),
        args: vec![],
        timeout_secs: 120,
    };
    let agent = AgentKind::from_config(&config);
    assert!(matches!(agent, AgentKind::Codex(_)));
}

#[test]
fn agent_kind_from_config_unknown_falls_back_to_generic() {
    let config = AgentConfig {
        agent_type: "llama".to_string(),
        command: "llama-cli".to_string(),
        args: vec!["--prompt".to_string()],
        timeout_secs: 60,
    };
    let agent = AgentKind::from_config(&config);
    assert!(matches!(agent, AgentKind::Generic(_)));
}

#[test]
fn agent_kind_default_config_is_claude() {
    let config = AgentConfig::default();
    let agent = AgentKind::from_config(&config);
    assert!(matches!(agent, AgentKind::Claude(_)));
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --test agent_test`
Expected: FAIL — `Agent`, `AgentKind`, `ClaudeAgent`, etc. not found

**Step 3: Rewrite the agent module**

Replace `src/agent.rs` entirely with:

```rust
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
```

**Step 4: Run tests**

Run: `cargo test --test agent_test`
Expected: 4 tests PASS

Note: The existing `tests/pipeline_test.rs` and `tests/integration_test.rs` will now fail because they still use the old `AgentRunner` trait. That is expected and fixed in Task 4.

**Step 5: Commit**

```bash
git add src/agent.rs tests/agent_test.rs
git commit -m "feat: rewrite agent module with Agent trait and AgentKind enum dispatch"
```

---

### Task 4: Migrate Pipeline to New Agent/Prompt Types

**Files:**
- Modify: `src/pipeline.rs`
- Modify: `tests/pipeline_test.rs`

**Step 1: Update the pipeline module**

Replace `src/pipeline.rs` entirely with:

```rust
use anyhow::Result;
use chrono::Utc;

use crate::agent::Agent;
use crate::config::Config;
use crate::filter;
use crate::gh::{self, GhRunner};
use crate::prompt::{IssueTriagePrompt, WeeklySummaryPrompt};
use crate::report::{FlaggedIssue, Report, RepoSection, TeamStats};

pub fn run_pipeline(
    config: &Config,
    gh_runner: &dyn GhRunner,
    agent: &dyn Agent,
    days: i64,
) -> Result<Report> {
    let mut repo_sections = Vec::new();
    let mut all_recent_issues = Vec::new();

    for repo_config in &config.repos {
        let all_issues = gh::fetch_issues(gh_runner, &repo_config.name)?;
        let recent = filter::filter_recent(&all_issues, days);

        // Track for team stats
        for issue in &recent {
            all_recent_issues.push((*issue).clone());
        }

        // Build issue summary text for agent
        let issue_summaries: String = recent
            .iter()
            .map(|i| {
                format!(
                    "- #{}: {} (labels: {}, assignees: {})",
                    i.number,
                    i.title,
                    i.labels.join(", "),
                    i.assignees.join(", ")
                )
            })
            .collect::<Vec<_>>()
            .join("\n");

        // Get weekly summary from agent
        let summary_prompt = WeeklySummaryPrompt {
            repo: repo_config.name.clone(),
            issue_summaries,
        };
        let summary = match agent.invoke(&summary_prompt) {
            Ok(s) => s,
            Err(e) => format!("Analysis unavailable: {e}"),
        };

        // Find flagged issues
        let flagged_refs = filter::find_flagged_issues(&recent, &repo_config.labels_required);
        let mut flagged_issues = Vec::new();

        for issue in flagged_refs {
            let triage_summary =
                match gh::fetch_issue_detail(gh_runner, &repo_config.name, issue.number) {
                    Ok(detail) => {
                        let comments_text: String = detail
                            .comments
                            .iter()
                            .map(|c| format!("{}: {}", c.author, c.body))
                            .collect::<Vec<_>>()
                            .join("\n");
                        let triage_prompt = IssueTriagePrompt {
                            title: issue.title.clone(),
                            body: detail.body,
                            comments: comments_text,
                        };
                        match agent.invoke(&triage_prompt) {
                            Ok(s) => s,
                            Err(e) => format!("Analysis unavailable: {e}"),
                        }
                    }
                    Err(e) => format!("Could not fetch issue detail: {e}"),
                };

            flagged_issues.push(FlaggedIssue {
                number: issue.number,
                title: issue.title.clone(),
                missing_labels: issue.missing_labels(&repo_config.labels_required),
                summary: triage_summary,
            });
        }

        repo_sections.push(RepoSection {
            name: repo_config.name.clone(),
            progress: summary.clone(),
            big_updates: String::new(),
            planned_next: String::new(),
            flagged_issues,
        });
    }

    // Team stats
    let team_stats: Vec<TeamStats> = config
        .team
        .iter()
        .map(|member| {
            let active = all_recent_issues
                .iter()
                .filter(|i| i.assignees.contains(&member.github))
                .count();
            TeamStats {
                name: member.name.clone(),
                active,
                closed_this_week: 0,
            }
        })
        .collect();

    Ok(Report {
        date: Utc::now().format("%Y-%m-%d").to_string(),
        repos: repo_sections,
        team_stats,
    })
}
```

**Step 2: Update the pipeline test**

Replace `tests/pipeline_test.rs` entirely with:

```rust
use ceo::agent::Agent;
use ceo::config::Config;
use ceo::gh::GhRunner;
use ceo::pipeline::run_pipeline;
use ceo::prompt::Prompt;

struct MockGh;

impl GhRunner for MockGh {
    fn run_gh(&self, args: &[&str]) -> anyhow::Result<String> {
        if args.iter().any(|a| *a == "list") {
            Ok(r#"[{
                "number": 1,
                "title": "Implement auth",
                "labels": [{"name": "feature"}],
                "assignees": [{"login": "alice"}],
                "updatedAt": "2026-03-05T10:00:00Z",
                "createdAt": "2026-03-01T10:00:00Z"
            }, {
                "number": 2,
                "title": "Fix CSS bug",
                "labels": [],
                "assignees": [],
                "updatedAt": "2026-03-04T10:00:00Z",
                "createdAt": "2026-02-28T10:00:00Z"
            }]"#
            .to_string())
        } else {
            Ok(r#"{
                "body": "This issue is about fixing CSS.",
                "comments": []
            }"#
            .to_string())
        }
    }
}

struct MockAgent;

impl Agent for MockAgent {
    fn invoke(&self, _prompt: &dyn Prompt) -> anyhow::Result<String> {
        Ok("Mock agent summary.".to_string())
    }
}

#[test]
fn pipeline_produces_report() {
    let config: Config = toml::from_str(
        r#"
        [[repos]]
        name = "org/frontend"
        labels_required = ["priority"]

        [[team]]
        github = "alice"
        name = "Alice Smith"
        role = "Lead"
    "#,
    )
    .unwrap();

    let report = run_pipeline(&config, &MockGh, &MockAgent, 7).unwrap();
    assert_eq!(report.repos.len(), 1);
    assert_eq!(report.repos[0].name, "org/frontend");
    assert!(report.repos[0].progress.contains("Mock agent summary"));
    assert!(!report.repos[0].flagged_issues.is_empty());
    assert_eq!(report.team_stats.len(), 1);
    assert_eq!(report.team_stats[0].active, 1);
}
```

**Step 3: Run tests**

Run: `cargo test --test pipeline_test`
Expected: 1 test PASS

**Step 4: Commit**

```bash
git add src/pipeline.rs tests/pipeline_test.rs
git commit -m "refactor: migrate pipeline to new Agent/Prompt types"
```

---

### Task 5: Update Integration Test & Main

**Files:**
- Modify: `tests/integration_test.rs`
- Modify: `src/main.rs`

**Step 1: Update the integration test**

Replace `tests/integration_test.rs` entirely with:

```rust
use ceo::agent::Agent;
use ceo::config::Config;
use ceo::gh::GhRunner;
use ceo::pipeline::run_pipeline;
use ceo::prompt::Prompt;
use ceo::report::render_markdown;

struct MockGh;

impl GhRunner for MockGh {
    fn run_gh(&self, args: &[&str]) -> anyhow::Result<String> {
        if args.iter().any(|a| *a == "list") {
            Ok(r#"[
                {
                    "number": 10,
                    "title": "Add dark mode",
                    "labels": [{"name": "feature"}, {"name": "priority"}],
                    "assignees": [{"login": "alice"}],
                    "updatedAt": "2026-03-05T10:00:00Z",
                    "createdAt": "2026-02-25T10:00:00Z"
                },
                {
                    "number": 11,
                    "title": "Fix memory leak",
                    "labels": [{"name": "bug"}],
                    "assignees": [{"login": "bob"}],
                    "updatedAt": "2026-03-04T10:00:00Z",
                    "createdAt": "2026-02-28T10:00:00Z"
                },
                {
                    "number": 12,
                    "title": "Update docs",
                    "labels": [],
                    "assignees": [],
                    "updatedAt": "2026-03-03T10:00:00Z",
                    "createdAt": "2026-03-01T10:00:00Z"
                }
            ]"#.to_string())
        } else {
            Ok(r#"{
                "body": "This issue needs triage.",
                "comments": [
                    {"author": {"login": "bob"}, "body": "I'll look into this.", "createdAt": "2026-03-03T12:00:00Z"}
                ]
            }"#.to_string())
        }
    }
}

struct MockAgent;

impl Agent for MockAgent {
    fn invoke(&self, prompt: &dyn Prompt) -> anyhow::Result<String> {
        let rendered = prompt.render();
        if rendered.contains("Summarize the past week") {
            Ok("Great progress on dark mode. Memory leak identified and being fixed.".to_string())
        } else {
            Ok("This issue is about updating documentation. Suggest adding priority:low label.".to_string())
        }
    }
}

#[test]
fn full_pipeline_produces_valid_markdown() {
    let config: Config = toml::from_str(r#"
        [[repos]]
        name = "org/frontend"
        labels_required = ["priority"]

        [[team]]
        github = "alice"
        name = "Alice Smith"
        role = "Lead"

        [[team]]
        github = "bob"
        name = "Bob Jones"
        role = "Backend"
    "#).unwrap();

    let report = run_pipeline(&config, &MockGh, &MockAgent, 7).unwrap();
    let markdown = render_markdown(&report);

    assert!(markdown.contains("org/frontend"));
    assert!(markdown.contains("Great progress on dark mode"));
    assert!(markdown.contains("Needs Attention"));
    assert!(markdown.contains("#11") || markdown.contains("#12"));
    assert!(markdown.contains("Alice Smith"));
    assert!(markdown.contains("Bob Jones"));
}
```

**Step 2: Update main.rs**

In `src/main.rs`, change the two lines that create the agent runner. Replace:

```rust
    let agent_runner = ceo::agent::RealAgentRunner::from_config(&config.agent);
```

With (in both `cmd_report` and `cmd_interactive`):

```rust
    let agent = ceo::agent::AgentKind::from_config(&config.agent);
```

And change the pipeline calls from `&agent_runner` to `&agent`.

Also update the `cmd_init` example config to include the `type` field. Change:

```
[agent]
command = "claude"
args = ["-p"]
timeout_secs = 120
```

To:

```
[agent]
type = "claude"
timeout_secs = 120
```

The full `cmd_report` becomes:

```rust
fn cmd_report(days: i64) -> Result<()> {
    let config = ceo::config::Config::load()?;
    let gh_runner = ceo::gh::RealGhRunner;
    let agent = ceo::agent::AgentKind::from_config(&config.agent);

    let report_data = ceo::pipeline::run_pipeline(&config, &gh_runner, &agent, days)?;
    let markdown = ceo::report::render_markdown(&report_data);
    print!("{markdown}");
    Ok(())
}
```

The full `cmd_interactive` becomes:

```rust
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
```

The full `cmd_init` example string becomes:

```rust
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
```

**Step 3: Run all tests**

Run: `cargo test`
Expected: ALL tests PASS (prompt_test, agent_test, config_test, filter_test, gh_test, github_test, pipeline_test, integration_test, report_test)

**Step 4: Verify build**

Run: `cargo build`
Expected: compiles with no errors

**Step 5: Commit**

```bash
git add src/main.rs tests/integration_test.rs
git commit -m "refactor: update main and integration test for new agent abstraction"
```

---

### Task 6: Final Verification & Cleanup

**Files:**
- No new files

**Step 1: Run full test suite**

Run: `cargo test`
Expected: ALL tests pass, no warnings

**Step 2: Verify CLI still works**

Run: `cargo run -- --help`
Expected: shows help with report, interactive, init subcommands

**Step 3: Check for any remaining references to old types**

Run: `grep -r "AgentRunner\|RealAgentRunner\|run_agent\|build_weekly_summary_prompt\|build_triage_prompt" src/ tests/`
Expected: NO matches — all old types are fully removed

**Step 4: Commit if any cleanup was needed**

```bash
git add -A
git commit -m "chore: clean up remaining references to old agent types"
```

---

## Summary

| Task | What it does | Tests |
|------|-------------|-------|
| 1 | Create Prompt trait & types | 2 unit tests |
| 2 | Add `agent_type` to config | 2 unit tests (+ existing 3) |
| 3 | Rewrite agent module with enum dispatch | 4 unit tests |
| 4 | Migrate pipeline to new types | 1 integration test |
| 5 | Update integration test & main | 1 e2e test + build check |
| 6 | Final verification & cleanup | full suite green |
