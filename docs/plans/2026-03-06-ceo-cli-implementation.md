# CEO CLI Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Build a Rust CLI that fetches GitHub issue data via `gh`, analyzes it with a configurable agent CLI, and produces weekly project summary reports in batch or interactive TUI mode.

**Architecture:** Sequential pipeline — config loading → `gh` data fetch → filter/group → agent analysis → markdown report → display (stdout or ratatui TUI). Each stage is a separate module with clear inputs/outputs, tested independently with mock data.

**Tech Stack:** Rust 2024 edition, clap (CLI), serde + toml (config), serde_json (gh output parsing), chrono (dates), ratatui + crossterm (TUI)

---

### Task 1: Project Dependencies

**Files:**
- Modify: `Cargo.toml`

**Step 1: Add all dependencies to Cargo.toml**

```toml
[package]
name = "ceo"
version = "0.1.0"
edition = "2024"

[dependencies]
clap = { version = "4", features = ["derive"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "0.8"
chrono = { version = "0.4", features = ["serde"] }
ratatui = "0.29"
crossterm = "0.28"
dirs = "6"
anyhow = "1"
```

**Step 2: Verify it compiles**

Run: `cargo check`
Expected: compiles with no errors (warnings about unused deps are fine)

**Step 3: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "feat: add project dependencies"
```

---

### Task 2: Config Data Model & Parsing

**Files:**
- Create: `src/config.rs`
- Modify: `src/main.rs` (add `mod config;`)
- Create: `tests/config_test.rs`

**Step 1: Write the failing test**

Create `tests/config_test.rs`:

```rust
use ceo::config::Config;

#[test]
fn parse_full_config() {
    let toml_str = r#"
[agent]
command = "claude"
args = ["-p"]
timeout_secs = 120

[[repos]]
name = "org/frontend"
labels_required = ["priority"]

[[repos]]
name = "org/backend"

[[team]]
github = "alice"
name = "Alice Smith"
role = "Frontend Lead"

[[team]]
github = "bob"
name = "Bob Jones"
role = "Backend"
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert_eq!(config.agent.command, "claude");
    assert_eq!(config.agent.args, vec!["-p"]);
    assert_eq!(config.agent.timeout_secs, 120);
    assert_eq!(config.repos.len(), 2);
    assert_eq!(config.repos[0].name, "org/frontend");
    assert_eq!(config.repos[0].labels_required, vec!["priority"]);
    assert!(config.repos[1].labels_required.is_empty());
    assert_eq!(config.team.len(), 2);
    assert_eq!(config.team[0].github, "alice");
}

#[test]
fn parse_minimal_config() {
    let toml_str = r#"
[[repos]]
name = "org/myrepo"
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert_eq!(config.agent.command, "claude");
    assert_eq!(config.agent.timeout_secs, 120);
    assert_eq!(config.repos.len(), 1);
    assert!(config.team.is_empty());
}

#[test]
fn config_load_from_string() {
    let toml_str = r#"
[[repos]]
name = "org/test"
"#;
    let config = Config::load_from_str(toml_str).unwrap();
    assert_eq!(config.repos[0].name, "org/test");
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --test config_test`
Expected: FAIL — module `ceo::config` not found

**Step 3: Create the config module**

Create `src/config.rs`:

```rust
use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub agent: AgentConfig,
    pub repos: Vec<RepoConfig>,
    #[serde(default)]
    pub team: Vec<TeamMember>,
}

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

fn default_agent_command() -> String {
    "claude".to_string()
}

fn default_timeout() -> u64 {
    120
}

#[derive(Debug, Deserialize)]
pub struct RepoConfig {
    pub name: String,
    #[serde(default)]
    pub labels_required: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct TeamMember {
    pub github: String,
    pub name: String,
    #[serde(default)]
    pub role: String,
}

impl Config {
    pub fn load_from_str(s: &str) -> Result<Self> {
        toml::from_str(s).context("Failed to parse config TOML")
    }

    pub fn load() -> Result<Self> {
        let path = Self::find_config_path()?;
        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read config file: {}", path.display()))?;
        Self::load_from_str(&content)
    }

    fn find_config_path() -> Result<PathBuf> {
        // 1. $CEO_CONFIG env var
        if let Ok(path) = std::env::var("CEO_CONFIG") {
            let p = PathBuf::from(path);
            if p.exists() {
                return Ok(p);
            }
        }

        // 2. ~/.config/ceo/config.toml
        if let Some(config_dir) = dirs::config_dir() {
            let p = config_dir.join("ceo").join("config.toml");
            if p.exists() {
                return Ok(p);
            }
        }

        // 3. ./ceo.toml
        let p = PathBuf::from("ceo.toml");
        if p.exists() {
            return Ok(p);
        }

        anyhow::bail!(
            "No config file found. Looked in:\n  \
             1. $CEO_CONFIG env var\n  \
             2. ~/.config/ceo/config.toml\n  \
             3. ./ceo.toml\n\n\
             Run `ceo init` to generate an example config."
        )
    }
}
```

**Step 4: Expose the module as a library**

Modify `src/main.rs` to:

```rust
mod config;

fn main() {
    println!("Hello, world!");
}
```

Create `src/lib.rs`:

```rust
pub mod config;
```

**Step 5: Run tests to verify they pass**

Run: `cargo test --test config_test`
Expected: 3 tests PASS

**Step 6: Commit**

```bash
git add src/config.rs src/lib.rs src/main.rs tests/config_test.rs
git commit -m "feat: add config parsing with TOML support"
```

---

### Task 3: Data Model for GitHub Issues

**Files:**
- Create: `src/github.rs`
- Modify: `src/lib.rs` (add `pub mod github;`)
- Create: `tests/github_test.rs`

**Step 1: Write the failing test**

Create `tests/github_test.rs`:

```rust
use ceo::github::{Issue, IssueDetail, Comment};
use chrono::{Utc, TimeZone};

#[test]
fn parse_gh_issue_list_json() {
    let json = r#"[
        {
            "number": 42,
            "title": "Fix login redirect",
            "labels": [{"name": "bug"}, {"name": "priority"}],
            "assignees": [{"login": "alice"}],
            "updatedAt": "2026-03-04T10:00:00Z",
            "createdAt": "2026-02-20T08:00:00Z"
        },
        {
            "number": 58,
            "title": "Refactor auth module",
            "labels": [],
            "assignees": [],
            "updatedAt": "2026-03-05T14:00:00Z",
            "createdAt": "2026-03-01T09:00:00Z"
        }
    ]"#;

    let issues: Vec<Issue> = Issue::parse_gh_list(json, "org/frontend").unwrap();
    assert_eq!(issues.len(), 2);
    assert_eq!(issues[0].number, 42);
    assert_eq!(issues[0].title, "Fix login redirect");
    assert_eq!(issues[0].labels, vec!["bug", "priority"]);
    assert_eq!(issues[0].assignees, vec!["alice"]);
    assert_eq!(issues[0].repo, "org/frontend");
    assert_eq!(issues[1].assignees, Vec::<String>::new());
}

#[test]
fn parse_gh_issue_detail_json() {
    let json = r#"{
        "body": "This issue is about the login redirect loop.",
        "comments": [
            {
                "author": {"login": "alice"},
                "body": "I think this is caused by the SSO config.",
                "createdAt": "2026-03-03T12:00:00Z"
            }
        ]
    }"#;

    let detail = IssueDetail::parse_gh_view(json).unwrap();
    assert_eq!(detail.body, "This issue is about the login redirect loop.");
    assert_eq!(detail.comments.len(), 1);
    assert_eq!(detail.comments[0].author, "alice");
}

#[test]
fn issue_missing_required_labels() {
    let issue = Issue {
        number: 1,
        title: "Test".into(),
        labels: vec!["bug".into()],
        assignees: vec![],
        updated_at: Utc::now(),
        created_at: Utc::now(),
        repo: "org/repo".into(),
    };
    let required = &["priority".to_string()];
    assert!(issue.missing_labels(required).contains(&"priority".to_string()));

    let required2 = &["bug".to_string()];
    assert!(issue.missing_labels(required2).is_empty());
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --test github_test`
Expected: FAIL — module not found

**Step 3: Implement the github module**

Create `src/github.rs`:

```rust
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::Deserialize;

#[derive(Debug, Clone)]
pub struct Issue {
    pub number: u64,
    pub title: String,
    pub labels: Vec<String>,
    pub assignees: Vec<String>,
    pub updated_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub repo: String,
}

#[derive(Deserialize)]
struct GhIssue {
    number: u64,
    title: String,
    labels: Vec<GhLabel>,
    assignees: Vec<GhUser>,
    #[serde(rename = "updatedAt")]
    updated_at: DateTime<Utc>,
    #[serde(rename = "createdAt")]
    created_at: DateTime<Utc>,
}

#[derive(Deserialize)]
struct GhLabel {
    name: String,
}

#[derive(Deserialize)]
struct GhUser {
    login: String,
}

impl Issue {
    pub fn parse_gh_list(json: &str, repo: &str) -> Result<Vec<Self>> {
        let gh_issues: Vec<GhIssue> =
            serde_json::from_str(json).context("Failed to parse gh issue list JSON")?;
        Ok(gh_issues
            .into_iter()
            .map(|gi| Issue {
                number: gi.number,
                title: gi.title,
                labels: gi.labels.into_iter().map(|l| l.name).collect(),
                assignees: gi.assignees.into_iter().map(|a| a.login).collect(),
                updated_at: gi.updated_at,
                created_at: gi.created_at,
                repo: repo.to_string(),
            })
            .collect())
    }

    pub fn missing_labels(&self, required: &[String]) -> Vec<String> {
        required
            .iter()
            .filter(|r| !self.labels.iter().any(|l| l == *r))
            .cloned()
            .collect()
    }
}

#[derive(Debug, Clone)]
pub struct IssueDetail {
    pub body: String,
    pub comments: Vec<Comment>,
}

#[derive(Debug, Clone)]
pub struct Comment {
    pub author: String,
    pub body: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Deserialize)]
struct GhIssueDetail {
    body: String,
    comments: Vec<GhComment>,
}

#[derive(Deserialize)]
struct GhComment {
    author: GhUser,
    body: String,
    #[serde(rename = "createdAt")]
    created_at: DateTime<Utc>,
}

impl IssueDetail {
    pub fn parse_gh_view(json: &str) -> Result<Self> {
        let gd: GhIssueDetail =
            serde_json::from_str(json).context("Failed to parse gh issue view JSON")?;
        Ok(IssueDetail {
            body: gd.body,
            comments: gd
                .comments
                .into_iter()
                .map(|c| Comment {
                    author: c.author.login,
                    body: c.body,
                    created_at: c.created_at,
                })
                .collect(),
        })
    }
}
```

**Step 4: Add module to lib.rs**

Add to `src/lib.rs`:

```rust
pub mod github;
```

**Step 5: Run tests**

Run: `cargo test --test github_test`
Expected: 3 tests PASS

**Step 6: Commit**

```bash
git add src/github.rs src/lib.rs tests/github_test.rs
git commit -m "feat: add GitHub issue data model and JSON parsing"
```

---

### Task 4: gh CLI Runner (Fetch Issues)

**Files:**
- Create: `src/gh.rs`
- Modify: `src/lib.rs` (add `pub mod gh;`)
- Create: `tests/gh_test.rs`

**Step 1: Write the failing test**

Create `tests/gh_test.rs`:

```rust
use ceo::gh::GhRunner;
use ceo::github::Issue;
use chrono::Utc;

struct MockGhRunner {
    issue_list_json: String,
    issue_view_json: String,
}

impl MockGhRunner {
    fn new(list_json: &str, view_json: &str) -> Self {
        Self {
            issue_list_json: list_json.to_string(),
            issue_view_json: view_json.to_string(),
        }
    }
}

impl GhRunner for MockGhRunner {
    fn run_gh(&self, args: &[&str]) -> anyhow::Result<String> {
        // If args contain "list", return issue list; if "view", return view
        if args.iter().any(|a| *a == "list") {
            Ok(self.issue_list_json.clone())
        } else {
            Ok(self.issue_view_json.clone())
        }
    }
}

#[test]
fn fetch_issues_from_mock() {
    let list_json = r#"[{
        "number": 1,
        "title": "Test issue",
        "labels": [],
        "assignees": [],
        "updatedAt": "2026-03-05T10:00:00Z",
        "createdAt": "2026-03-01T10:00:00Z"
    }]"#;

    let runner = MockGhRunner::new(list_json, "{}");
    let issues = ceo::gh::fetch_issues(&runner, "org/repo").unwrap();
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].number, 1);
    assert_eq!(issues[0].repo, "org/repo");
}

#[test]
fn fetch_issue_detail_from_mock() {
    let view_json = r#"{
        "body": "Description here",
        "comments": []
    }"#;

    let runner = MockGhRunner::new("[]", view_json);
    let detail = ceo::gh::fetch_issue_detail(&runner, "org/repo", 42).unwrap();
    assert_eq!(detail.body, "Description here");
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --test gh_test`
Expected: FAIL — module not found

**Step 3: Implement the gh runner module**

Create `src/gh.rs`:

```rust
use anyhow::{Context, Result};
use crate::github::{Issue, IssueDetail};
use std::process::Command;

pub trait GhRunner {
    fn run_gh(&self, args: &[&str]) -> Result<String>;
}

pub struct RealGhRunner;

impl GhRunner for RealGhRunner {
    fn run_gh(&self, args: &[&str]) -> Result<String> {
        let output = Command::new("gh")
            .args(args)
            .output()
            .context("Failed to run gh CLI. Is it installed? https://cli.github.com")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if stderr.contains("auth login") || stderr.contains("not logged") {
                anyhow::bail!("gh is not authenticated. Run `gh auth login` first.");
            }
            anyhow::bail!("gh command failed: {}", stderr);
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }
}

pub fn fetch_issues(runner: &dyn GhRunner, repo: &str) -> Result<Vec<Issue>> {
    let json = runner.run_gh(&[
        "issue", "list",
        "--repo", repo,
        "--state", "open",
        "--json", "number,title,labels,assignees,updatedAt,createdAt",
        "--limit", "200",
    ])?;
    Issue::parse_gh_list(&json, repo)
}

pub fn fetch_issue_detail(runner: &dyn GhRunner, repo: &str, number: u64) -> Result<IssueDetail> {
    let json = runner.run_gh(&[
        "issue", "view",
        &number.to_string(),
        "--repo", repo,
        "--json", "body,comments",
    ])?;
    IssueDetail::parse_gh_view(&json)
}
```

**Step 4: Add module to lib.rs**

Add to `src/lib.rs`:

```rust
pub mod gh;
```

**Step 5: Run tests**

Run: `cargo test --test gh_test`
Expected: 2 tests PASS

**Step 6: Commit**

```bash
git add src/gh.rs src/lib.rs tests/gh_test.rs
git commit -m "feat: add gh CLI runner with trait-based mocking"
```

---

### Task 5: Issue Filtering & Grouping

**Files:**
- Create: `src/filter.rs`
- Modify: `src/lib.rs` (add `pub mod filter;`)
- Create: `tests/filter_test.rs`

**Step 1: Write the failing test**

Create `tests/filter_test.rs`:

```rust
use ceo::filter::{filter_recent, group_by_repo, group_by_assignee, find_flagged_issues};
use ceo::github::Issue;
use chrono::{Utc, Duration};

fn make_issue(number: u64, repo: &str, assignees: Vec<&str>, labels: Vec<&str>, days_ago: i64) -> Issue {
    Issue {
        number,
        title: format!("Issue #{number}"),
        labels: labels.into_iter().map(String::from).collect(),
        assignees: assignees.into_iter().map(String::from).collect(),
        updated_at: Utc::now() - Duration::days(days_ago),
        created_at: Utc::now() - Duration::days(days_ago + 10),
        repo: repo.to_string(),
    }
}

#[test]
fn filter_recent_issues() {
    let issues = vec![
        make_issue(1, "org/repo", vec!["alice"], vec![], 2),  // 2 days ago — recent
        make_issue(2, "org/repo", vec!["bob"], vec![], 10),   // 10 days ago — old
        make_issue(3, "org/repo", vec![], vec![], 6),         // 6 days ago — recent
    ];
    let recent = filter_recent(&issues, 7);
    assert_eq!(recent.len(), 2);
    assert_eq!(recent[0].number, 1);
    assert_eq!(recent[1].number, 3);
}

#[test]
fn group_issues_by_repo() {
    let issues = vec![
        make_issue(1, "org/frontend", vec![], vec![], 1),
        make_issue(2, "org/backend", vec![], vec![], 1),
        make_issue(3, "org/frontend", vec![], vec![], 1),
    ];
    let grouped = group_by_repo(&issues);
    assert_eq!(grouped.len(), 2);
    assert_eq!(grouped["org/frontend"].len(), 2);
    assert_eq!(grouped["org/backend"].len(), 1);
}

#[test]
fn group_issues_by_assignee() {
    let issues = vec![
        make_issue(1, "org/repo", vec!["alice"], vec![], 1),
        make_issue(2, "org/repo", vec!["bob"], vec![], 1),
        make_issue(3, "org/repo", vec!["alice"], vec![], 1),
        make_issue(4, "org/repo", vec![], vec![], 1),
    ];
    let grouped = group_by_assignee(&issues);
    assert_eq!(grouped["alice"].len(), 2);
    assert_eq!(grouped["bob"].len(), 1);
    assert_eq!(grouped["unassigned"].len(), 1);
}

#[test]
fn find_issues_missing_required_labels() {
    let issues = vec![
        make_issue(1, "org/repo", vec![], vec!["priority", "bug"], 1),
        make_issue(2, "org/repo", vec![], vec!["bug"], 1),
        make_issue(3, "org/repo", vec![], vec![], 1),
    ];
    let required = vec!["priority".to_string()];
    let flagged = find_flagged_issues(&issues, &required);
    assert_eq!(flagged.len(), 2);
    assert_eq!(flagged[0].number, 2);
    assert_eq!(flagged[1].number, 3);
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --test filter_test`
Expected: FAIL — module not found

**Step 3: Implement the filter module**

Create `src/filter.rs`:

```rust
use crate::github::Issue;
use chrono::{Duration, Utc};
use std::collections::HashMap;

pub fn filter_recent(issues: &[Issue], days: i64) -> Vec<&Issue> {
    let cutoff = Utc::now() - Duration::days(days);
    issues.iter().filter(|i| i.updated_at >= cutoff).collect()
}

pub fn group_by_repo<'a>(issues: &'a [&Issue]) -> HashMap<String, Vec<&'a Issue>> {
    let mut map: HashMap<String, Vec<&Issue>> = HashMap::new();
    for issue in issues {
        map.entry(issue.repo.clone()).or_default().push(issue);
    }
    map
}

pub fn group_by_assignee<'a>(issues: &'a [&Issue]) -> HashMap<String, Vec<&'a Issue>> {
    let mut map: HashMap<String, Vec<&Issue>> = HashMap::new();
    for issue in issues {
        if issue.assignees.is_empty() {
            map.entry("unassigned".to_string()).or_default().push(issue);
        } else {
            for assignee in &issue.assignees {
                map.entry(assignee.clone()).or_default().push(issue);
            }
        }
    }
    map
}

pub fn find_flagged_issues<'a>(issues: &'a [&Issue], required_labels: &[String]) -> Vec<&'a Issue> {
    issues
        .iter()
        .filter(|i| !i.missing_labels(required_labels).is_empty())
        .copied()
        .collect()
}
```

**Step 4: Add module to lib.rs**

Add to `src/lib.rs`:

```rust
pub mod filter;
```

**Step 5: Run tests**

Run: `cargo test --test filter_test`
Expected: 4 tests PASS

Note: The test calls `filter_recent` which returns `Vec<&Issue>`, then `group_by_repo` / `group_by_assignee` / `find_flagged_issues` take `&[&Issue]`. The test for `filter_recent` calls it with `&[Issue]` (owned), while `group_by_repo` etc. take `&[&Issue]`. Adjust the test signatures if the compiler complains — the tests above pass `&issues` (slice of owned) to `filter_recent` and the grouped functions take the result references. If `group_by_repo` in the test calls with owned `&[Issue]`, update the test to use references from `filter_recent` or adjust the function signatures to accept `&[Issue]` directly. The simplest fix: make `group_by_repo` and `group_by_assignee` and `find_flagged_issues` all accept `&[Issue]` instead of `&[&Issue]`, and return `Vec<&Issue>`. Adjust as needed to make the tests pass.

**Step 6: Commit**

```bash
git add src/filter.rs src/lib.rs tests/filter_test.rs
git commit -m "feat: add issue filtering and grouping logic"
```

---

### Task 6: Agent CLI Runner

**Files:**
- Create: `src/agent.rs`
- Modify: `src/lib.rs` (add `pub mod agent;`)
- Create: `tests/agent_test.rs`

**Step 1: Write the failing test**

Create `tests/agent_test.rs`:

```rust
use ceo::agent::{AgentRunner, run_agent};
use ceo::config::AgentConfig;

struct MockAgent {
    response: String,
}

impl AgentRunner for MockAgent {
    fn invoke(&self, _prompt: &str) -> anyhow::Result<String> {
        Ok(self.response.clone())
    }
}

#[test]
fn run_agent_returns_response() {
    let agent = MockAgent {
        response: "Summary: things happened.".to_string(),
    };
    let result = run_agent(&agent, "Summarize this").unwrap();
    assert_eq!(result, "Summary: things happened.");
}

#[test]
fn weekly_summary_prompt_contains_repo() {
    let prompt = ceo::agent::build_weekly_summary_prompt("org/frontend", "- #1 Fix bug\n- #2 Add feature");
    assert!(prompt.contains("org/frontend"));
    assert!(prompt.contains("Fix bug"));
}

#[test]
fn triage_prompt_contains_issue_info() {
    let prompt = ceo::agent::build_triage_prompt("Fix login redirect", "The login page redirects.", "alice: I think it's SSO.");
    assert!(prompt.contains("Fix login redirect"));
    assert!(prompt.contains("login page redirects"));
    assert!(prompt.contains("SSO"));
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --test agent_test`
Expected: FAIL — module not found

**Step 3: Implement the agent module**

Create `src/agent.rs`:

```rust
use anyhow::{Context, Result};
use crate::config::AgentConfig;
use std::io::Write;
use std::process::{Command, Stdio};
use std::time::Duration;

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
        let mut child = Command::new(&self.command)
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
```

**Step 4: Add module to lib.rs**

Add to `src/lib.rs`:

```rust
pub mod agent;
```

**Step 5: Run tests**

Run: `cargo test --test agent_test`
Expected: 3 tests PASS

**Step 6: Commit**

```bash
git add src/agent.rs src/lib.rs tests/agent_test.rs
git commit -m "feat: add agent CLI runner with prompt builders"
```

---

### Task 7: Report Renderer

**Files:**
- Create: `src/report.rs`
- Modify: `src/lib.rs` (add `pub mod report;`)
- Create: `tests/report_test.rs`

**Step 1: Write the failing test**

Create `tests/report_test.rs`:

```rust
use ceo::report::{Report, RepoSection, FlaggedIssue, TeamStats, render_markdown};

#[test]
fn render_report_contains_header() {
    let report = Report {
        date: "2026-03-06".to_string(),
        repos: vec![],
        team_stats: vec![],
    };
    let md = render_markdown(&report);
    assert!(md.contains("# Weekly Project Report — 2026-03-06"));
}

#[test]
fn render_report_with_repo_section() {
    let report = Report {
        date: "2026-03-06".to_string(),
        repos: vec![RepoSection {
            name: "org/frontend".to_string(),
            progress: "Fixed 3 bugs.".to_string(),
            big_updates: "Migrated to new auth.".to_string(),
            planned_next: "Start v2 redesign.".to_string(),
            flagged_issues: vec![FlaggedIssue {
                number: 42,
                title: "Fix login redirect".to_string(),
                missing_labels: vec!["priority".to_string()],
                summary: "Issue about SSO redirect loop.".to_string(),
            }],
        }],
        team_stats: vec![TeamStats {
            name: "Alice Smith".to_string(),
            active: 5,
            closed_this_week: 2,
        }],
    };
    let md = render_markdown(&report);
    assert!(md.contains("## org/frontend"));
    assert!(md.contains("Fixed 3 bugs."));
    assert!(md.contains("Migrated to new auth."));
    assert!(md.contains("#42"));
    assert!(md.contains("Missing priority label"));
    assert!(md.contains("Alice Smith"));
    assert!(md.contains("| 5"));
}

#[test]
fn render_report_no_flagged_issues_omits_section() {
    let report = Report {
        date: "2026-03-06".to_string(),
        repos: vec![RepoSection {
            name: "org/backend".to_string(),
            progress: "All good.".to_string(),
            big_updates: "Nothing major.".to_string(),
            planned_next: "Continue work.".to_string(),
            flagged_issues: vec![],
        }],
        team_stats: vec![],
    };
    let md = render_markdown(&report);
    assert!(!md.contains("Needs Attention"));
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --test report_test`
Expected: FAIL — module not found

**Step 3: Implement the report module**

Create `src/report.rs`:

```rust
use std::fmt::Write;

pub struct Report {
    pub date: String,
    pub repos: Vec<RepoSection>,
    pub team_stats: Vec<TeamStats>,
}

pub struct RepoSection {
    pub name: String,
    pub progress: String,
    pub big_updates: String,
    pub planned_next: String,
    pub flagged_issues: Vec<FlaggedIssue>,
}

pub struct FlaggedIssue {
    pub number: u64,
    pub title: String,
    pub missing_labels: Vec<String>,
    pub summary: String,
}

pub struct TeamStats {
    pub name: String,
    pub active: usize,
    pub closed_this_week: usize,
}

pub fn render_markdown(report: &Report) -> String {
    let mut out = String::new();
    writeln!(out, "# Weekly Project Report — {}\n", report.date).unwrap();

    for repo in &report.repos {
        writeln!(out, "## {}\n", repo.name).unwrap();
        writeln!(out, "### Progress This Week\n").unwrap();
        writeln!(out, "{}\n", repo.progress).unwrap();
        writeln!(out, "### Big Updates\n").unwrap();
        writeln!(out, "{}\n", repo.big_updates).unwrap();
        writeln!(out, "### Planned Next\n").unwrap();
        writeln!(out, "{}\n", repo.planned_next).unwrap();

        if !repo.flagged_issues.is_empty() {
            writeln!(out, "### Needs Attention\n").unwrap();
            for issue in &repo.flagged_issues {
                let missing = issue.missing_labels.join(", ");
                writeln!(
                    out,
                    "- **#{}**: \"{}\" — Missing {} label. *{}*\n",
                    issue.number, issue.title, missing, issue.summary
                )
                .unwrap();
            }
        }
    }

    if !report.team_stats.is_empty() {
        writeln!(out, "## Team Overview\n").unwrap();
        writeln!(out, "| Person | Issues Active | Issues Closed This Week |").unwrap();
        writeln!(out, "|--------|--------------|------------------------|").unwrap();
        for member in &report.team_stats {
            writeln!(
                out,
                "| {} | {} | {} |",
                member.name, member.active, member.closed_this_week
            )
            .unwrap();
        }
        writeln!(out).unwrap();
    }

    out
}
```

**Step 4: Add module to lib.rs**

Add to `src/lib.rs`:

```rust
pub mod report;
```

**Step 5: Run tests**

Run: `cargo test --test report_test`
Expected: 3 tests PASS

**Step 6: Commit**

```bash
git add src/report.rs src/lib.rs tests/report_test.rs
git commit -m "feat: add markdown report renderer"
```

---

### Task 8: Pipeline Orchestrator

**Files:**
- Create: `src/pipeline.rs`
- Modify: `src/lib.rs` (add `pub mod pipeline;`)
- Create: `tests/pipeline_test.rs`

This module ties together: config → fetch → filter → agent → report.

**Step 1: Write the failing test**

Create `tests/pipeline_test.rs`:

```rust
use ceo::pipeline::run_pipeline;
use ceo::config::Config;
use ceo::gh::GhRunner;
use ceo::agent::AgentRunner;

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
            }]"#.to_string())
        } else {
            Ok(r#"{
                "body": "This issue is about fixing CSS.",
                "comments": []
            }"#.to_string())
        }
    }
}

struct MockAgent;

impl AgentRunner for MockAgent {
    fn invoke(&self, _prompt: &str) -> anyhow::Result<String> {
        Ok("Mock agent summary.".to_string())
    }
}

#[test]
fn pipeline_produces_report() {
    let config: Config = toml::from_str(r#"
        [[repos]]
        name = "org/frontend"
        labels_required = ["priority"]

        [[team]]
        github = "alice"
        name = "Alice Smith"
        role = "Lead"
    "#).unwrap();

    let report = run_pipeline(&config, &MockGh, &MockAgent, 7).unwrap();
    assert_eq!(report.repos.len(), 1);
    assert_eq!(report.repos[0].name, "org/frontend");
    assert!(report.repos[0].progress.contains("Mock agent summary"));
    // Both issues #1 and #2 are missing "priority" label (only #1 has "feature")
    // Actually #1 has "feature" but not "priority", and #2 has no labels
    assert!(!report.repos[0].flagged_issues.is_empty());
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --test pipeline_test`
Expected: FAIL — module not found

**Step 3: Implement the pipeline module**

Create `src/pipeline.rs`:

```rust
use anyhow::Result;
use chrono::Utc;

use crate::agent::{self, AgentRunner};
use crate::config::Config;
use crate::filter;
use crate::gh::{self, GhRunner};
use crate::report::{FlaggedIssue, Report, RepoSection, TeamStats};

pub fn run_pipeline(
    config: &Config,
    gh_runner: &dyn GhRunner,
    agent_runner: &dyn AgentRunner,
    days: i64,
) -> Result<Report> {
    let mut repo_sections = Vec::new();

    for repo_config in &config.repos {
        let all_issues = gh::fetch_issues(gh_runner, &repo_config.name)?;
        let recent: Vec<_> = filter::filter_recent(&all_issues, days);

        // Build issue summary text for the agent
        let issue_summaries: String = recent
            .iter()
            .map(|i| format!("- #{}: {} (labels: {}, assignees: {})",
                i.number, i.title,
                i.labels.join(", "),
                i.assignees.join(", ")))
            .collect::<Vec<_>>()
            .join("\n");

        // Get weekly summary from agent
        let summary_prompt = agent::build_weekly_summary_prompt(&repo_config.name, &issue_summaries);
        let summary = match agent_runner.invoke(&summary_prompt) {
            Ok(s) => s,
            Err(e) => format!("Analysis unavailable: {e}"),
        };

        // Find flagged issues
        let flagged_refs = filter::find_flagged_issues(&recent, &repo_config.labels_required);
        let mut flagged_issues = Vec::new();

        for issue in flagged_refs {
            let triage_summary = match gh::fetch_issue_detail(gh_runner, &repo_config.name, issue.number) {
                Ok(detail) => {
                    let comments_text: String = detail.comments.iter()
                        .map(|c| format!("{}: {}", c.author, c.body))
                        .collect::<Vec<_>>()
                        .join("\n");
                    let prompt = agent::build_triage_prompt(&issue.title, &detail.body, &comments_text);
                    match agent_runner.invoke(&prompt) {
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

        // Parse the agent summary into sections (simple split approach)
        // The agent returns free-form text; we use the whole thing for each section for MVP
        // A smarter approach would parse numbered sections from the agent output
        repo_sections.push(RepoSection {
            name: repo_config.name.clone(),
            progress: summary.clone(),
            big_updates: String::new(),
            planned_next: String::new(),
            flagged_issues,
        });
    }

    // Team stats
    let team_stats: Vec<TeamStats> = config.team.iter().map(|member| {
        // Count active issues across all repos where this member is assigned
        // For MVP, we don't have closed issue data, so closed_this_week is 0
        TeamStats {
            name: member.name.clone(),
            active: 0, // TODO: compute from fetched issues
            closed_this_week: 0,
        }
    }).collect();

    Ok(Report {
        date: Utc::now().format("%Y-%m-%d").to_string(),
        repos: repo_sections,
        team_stats,
    })
}
```

**Step 4: Add module to lib.rs**

Add to `src/lib.rs`:

```rust
pub mod pipeline;
```

**Step 5: Run tests**

Run: `cargo test --test pipeline_test`
Expected: 1 test PASS

**Step 6: Commit**

```bash
git add src/pipeline.rs src/lib.rs tests/pipeline_test.rs
git commit -m "feat: add pipeline orchestrator connecting fetch-filter-analyze-report"
```

---

### Task 9: CLI Interface with clap (Batch Mode)

**Files:**
- Modify: `src/main.rs`

**Step 1: Implement the CLI with clap**

Replace `src/main.rs` with:

```rust
mod config;
mod github;
mod gh;
mod filter;
mod agent;
mod report;
mod pipeline;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "ceo", about = "Weekly project summary from GitHub issues")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

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
    /// Generate an example config file
    Init,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Report { days } => cmd_report(days),
        Commands::Interactive => cmd_interactive(),
        Commands::Init => cmd_init(),
    }
}

fn cmd_report(days: i64) -> Result<()> {
    let config = config::Config::load()?;
    let gh_runner = gh::RealGhRunner;
    let agent_runner = agent::RealAgentRunner::from_config(&config.agent);

    let report_data = pipeline::run_pipeline(&config, &gh_runner, &agent_runner, days)?;
    let markdown = report::render_markdown(&report_data);
    print!("{markdown}");
    Ok(())
}

fn cmd_interactive() -> Result<()> {
    eprintln!("Interactive mode not yet implemented. Use `ceo report` for now.");
    Ok(())
}

fn cmd_init() -> Result<()> {
    let example = r#"# CEO CLI configuration
# Place this file at ~/.config/ceo/config.toml

[agent]
command = "claude"
args = ["-p"]
timeout_secs = 120

[[repos]]
name = "org/repo-name"
labels_required = ["priority"]

[[team]]
github = "username"
name = "Full Name"
role = "Role"
"#;

    let config_dir = dirs::config_dir()
        .map(|d| d.join("ceo"))
        .unwrap_or_else(|| std::path::PathBuf::from("."));

    let config_path = config_dir.join("config.toml");

    if config_path.exists() {
        eprintln!("Config already exists at {}", config_path.display());
        eprintln!("Edit it directly or delete it and re-run `ceo init`.");
        return Ok(());
    }

    std::fs::create_dir_all(&config_dir)?;
    std::fs::write(&config_path, example)?;
    eprintln!("Example config written to {}", config_path.display());
    eprintln!("Edit it with your repos and team, then run `ceo report`.");
    Ok(())
}
```

**Step 2: Verify it compiles**

Run: `cargo build`
Expected: compiles successfully

**Step 3: Verify help output**

Run: `cargo run -- --help`
Expected: shows "Weekly project summary from GitHub issues" with subcommands

**Step 4: Verify init command**

Run: `cargo run -- init`
Expected: writes example config or says it exists

**Step 5: Commit**

```bash
git add src/main.rs
git commit -m "feat: add CLI interface with report, interactive, and init subcommands"
```

---

### Task 10: Interactive TUI Mode (ratatui)

**Files:**
- Create: `src/tui.rs`
- Modify: `src/lib.rs` (add `pub mod tui;`)
- Modify: `src/main.rs` (wire up `cmd_interactive`)

**Step 1: Implement the TUI module**

Create `src/tui.rs`:

```rust
use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Style},
    text::{Line, Text},
    widgets::{Block, Borders, Paragraph, Wrap},
    Terminal,
};
use std::io;

pub struct TuiApp {
    pub report_text: String,
    pub input: String,
    pub output_lines: Vec<String>,
    pub report_scroll: u16,
    pub should_quit: bool,
}

impl TuiApp {
    pub fn new(report_text: String) -> Self {
        Self {
            report_text,
            input: String::new(),
            output_lines: vec!["Type `help` for commands, `quit` to exit.".to_string()],
            report_scroll: 0,
            should_quit: false,
        }
    }

    pub fn handle_command(&mut self, cmd: &str) -> Option<String> {
        let parts: Vec<&str> = cmd.trim().split_whitespace().collect();
        match parts.first().map(|s| *s) {
            Some("help") => Some(
                "Commands:\n  refresh  — re-fetch report\n  show #N  — show issue detail\n  \
                 analyze #N — re-run agent on issue\n  repos — list repos\n  quit — exit"
                    .to_string(),
            ),
            Some("quit") | Some("exit") => {
                self.should_quit = true;
                None
            }
            Some("repos") => Some("(repos command — not yet wired up)".to_string()),
            Some("refresh") => Some("(refresh — not yet wired up)".to_string()),
            Some("show") => Some("(show — not yet wired up)".to_string()),
            Some("analyze") => Some("(analyze — not yet wired up)".to_string()),
            Some(other) => Some(format!("Unknown command: {other}. Type `help` for commands.")),
            None => None,
        }
    }
}

pub fn run_tui(report_text: String) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = TuiApp::new(report_text);

    loop {
        terminal.draw(|frame| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Percentage(65), Constraint::Percentage(35)])
                .split(frame.area());

            // Top pane: report
            let report_text = Text::raw(&app.report_text);
            let report_widget = Paragraph::new(report_text)
                .block(Block::default().borders(Borders::ALL).title(" Report "))
                .wrap(Wrap { trim: false })
                .scroll((app.report_scroll, 0));
            frame.render_widget(report_widget, chunks[0]);

            // Bottom pane: REPL
            let mut repl_lines: Vec<Line> = app
                .output_lines
                .iter()
                .map(|l| Line::raw(l.as_str()))
                .collect();
            repl_lines.push(Line::styled(
                format!("> {}_", app.input),
                Style::default().fg(Color::Green),
            ));
            let repl_widget = Paragraph::new(Text::from(repl_lines))
                .block(Block::default().borders(Borders::ALL).title(" Commands "))
                .wrap(Wrap { trim: false });
            frame.render_widget(repl_widget, chunks[1]);
        })?;

        if event::poll(std::time::Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        break;
                    }
                    KeyCode::Enter => {
                        let cmd = app.input.clone();
                        app.input.clear();
                        app.output_lines.push(format!("> {cmd}"));
                        if let Some(response) = app.handle_command(&cmd) {
                            for line in response.lines() {
                                app.output_lines.push(line.to_string());
                            }
                        }
                        if app.should_quit {
                            break;
                        }
                    }
                    KeyCode::Char(c) => {
                        app.input.push(c);
                    }
                    KeyCode::Backspace => {
                        app.input.pop();
                    }
                    KeyCode::Up => {
                        app.report_scroll = app.report_scroll.saturating_sub(1);
                    }
                    KeyCode::Down => {
                        app.report_scroll = app.report_scroll.saturating_add(1);
                    }
                    _ => {}
                }
            }
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    Ok(())
}
```

**Step 2: Add module to lib.rs**

Add to `src/lib.rs`:

```rust
pub mod tui;
```

**Step 3: Wire up cmd_interactive in main.rs**

Replace the `cmd_interactive` function in `src/main.rs`:

```rust
fn cmd_interactive() -> Result<()> {
    let config = config::Config::load()?;
    let gh_runner = gh::RealGhRunner;
    let agent_runner = agent::RealAgentRunner::from_config(&config.agent);

    eprintln!("Fetching data and generating report...");
    let report_data = pipeline::run_pipeline(&config, &gh_runner, &agent_runner, 7)?;
    let markdown = report::render_markdown(&report_data);

    tui::run_tui(markdown)?;
    Ok(())
}
```

**Step 4: Verify it compiles**

Run: `cargo build`
Expected: compiles successfully

**Step 5: Commit**

```bash
git add src/tui.rs src/lib.rs src/main.rs
git commit -m "feat: add ratatui interactive TUI with report pane and REPL"
```

---

### Task 11: Integration Test with Full Mock Pipeline

**Files:**
- Create: `tests/integration_test.rs`

**Step 1: Write the integration test**

Create `tests/integration_test.rs`:

```rust
use ceo::config::Config;
use ceo::gh::GhRunner;
use ceo::agent::AgentRunner;
use ceo::pipeline::run_pipeline;
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

impl AgentRunner for MockAgent {
    fn invoke(&self, prompt: &str) -> anyhow::Result<String> {
        if prompt.contains("Summarize the past week") {
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

    // Report should contain repo section
    assert!(markdown.contains("org/frontend"));
    // Report should contain agent summary
    assert!(markdown.contains("Great progress on dark mode"));
    // Report should flag issues missing "priority" label
    // Issue #11 has "bug" but not "priority", #12 has no labels
    assert!(markdown.contains("Needs Attention"));
    assert!(markdown.contains("#11") || markdown.contains("#12"));
    // Team overview should be present
    assert!(markdown.contains("Alice Smith"));
    assert!(markdown.contains("Bob Jones"));
}
```

**Step 2: Run the test**

Run: `cargo test --test integration_test`
Expected: 1 test PASS

**Step 3: Run all tests together**

Run: `cargo test`
Expected: all tests PASS

**Step 4: Commit**

```bash
git add tests/integration_test.rs
git commit -m "feat: add full integration test with mock gh and agent"
```

---

### Task 12: Final Polish & README

**Files:**
- Modify: `src/main.rs` (add version flag)
- Create: `README.md`

**Step 1: Verify everything builds and tests pass**

Run: `cargo build && cargo test`
Expected: all green

**Step 2: Add a brief README**

Create `README.md`:

```markdown
# ceo

Weekly project summary CLI for engineering managers. Fetches GitHub issues via `gh`, analyzes them with a configurable AI agent, and produces markdown reports.

## Quick Start

```bash
# Install
cargo install --path .

# Generate example config
ceo init

# Edit ~/.config/ceo/config.toml with your repos and team

# Generate weekly report
ceo report

# Launch interactive TUI
ceo interactive
```

## Requirements

- [gh CLI](https://cli.github.com) installed and authenticated
- An agent CLI (default: `claude`) available on PATH

## Configuration

See `ceo init` for an example config file.
```

**Step 3: Commit**

```bash
git add README.md src/main.rs
git commit -m "docs: add README with quick start guide"
```

---

## Summary

| Task | What it builds | Tests |
|------|---------------|-------|
| 1 | Dependencies | compile check |
| 2 | Config parsing | 3 unit tests |
| 3 | GitHub data model | 3 unit tests |
| 4 | gh CLI runner | 2 unit tests |
| 5 | Issue filtering | 4 unit tests |
| 6 | Agent runner | 3 unit tests |
| 7 | Report renderer | 3 unit tests |
| 8 | Pipeline orchestrator | 1 integration test |
| 9 | CLI interface (batch) | manual + compile |
| 10 | Interactive TUI | manual |
| 11 | Full integration test | 1 end-to-end test |
| 12 | Polish + README | all tests green |
