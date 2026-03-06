# CEO CLI — Design Document

## Overview

`ceo` is a Rust CLI tool for engineering managers to get weekly project summaries from GitHub issue boards. It operates in two modes: batch (markdown to stdout) and interactive (ratatui TUI with report + REPL).

The MVP fetches issue data via the `gh` CLI, detects poorly-managed issues (missing labels/status), and shells out to a configurable agent CLI (default: `claude`) to generate summaries and triage reports.

## Configuration

TOML config file. Lookup order: `$CEO_CONFIG` env var, `~/.config/ceo/config.toml`, `./ceo.toml`.

```toml
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
```

Parsed with `serde` + `toml`. Missing config produces a helpful error pointing to `ceo init`.

## Architecture

Sequential pipeline: fetch → filter → analyze → render → display.

```
Config
  │
  ▼
gh issue list (per repo)
  │
  ▼
Filter: updated in last N days
  │
  ▼
Group by repo + assignee
  │
  ▼
Detect issues missing required labels
  │
  ▼
Agent CLI: summarize repo progress + triage flagged issues
  │
  ▼
Assemble markdown report
  │
  ├─► Batch mode: print to stdout
  └─► Interactive mode: render in ratatui TUI
```

## Data Fetching

Uses `gh` CLI — no GitHub API SDK needed for MVP.

- `gh issue list --repo {repo} --state open --json number,title,labels,assignees,updatedAt,createdAt --limit 200`
- `gh issue view {number} --repo {repo} --json body,comments` — on-demand for flagged issues

### Data model

```rust
struct Issue {
    number: u64,
    title: String,
    labels: Vec<String>,
    assignees: Vec<String>,
    updated_at: DateTime<Utc>,
    created_at: DateTime<Utc>,
    repo: String,
}

struct IssueDetail {
    body: String,
    comments: Vec<Comment>,
}

struct Comment {
    author: String,
    body: String,
    created_at: DateTime<Utc>,
}

struct TeamMember {
    github: String,
    name: String,
    role: String,
}
```

### Filtering

1. Fetch open issues from each configured repo
2. Keep issues updated within the lookback window (default 7 days, configurable via `--days`)
3. Group by assignee (mapped to team config) and by repo
4. Flag issues missing any label listed in `labels_required`

## Agent Analysis

### When invoked

1. **Issue triage** — for issues missing required labels/status
2. **Weekly summary** — per-repo batch of recent issue activity

### Invocation

```
{agent_command} {agent_args} "{prompt}"
```

Agent's stdout is captured as the result.

### Prompt templates (hardcoded for MVP)

**Issue triage:**
> Analyze this GitHub issue. It lacks proper labels/status. Summarize what the issue is about in 2-3 sentences and suggest appropriate priority and status labels.
> Issue: {title}
> {body}
> Comments: {comments}

**Weekly summary:**
> Summarize the past week's progress for repo {repo}. Here are the issues updated this week:
> {issue_list_with_recent_comments}
> Provide: 1) Key progress and completed work 2) Big updates or decisions 3) What people are planning to work on next

### Error handling

- Agent timeout → "Analysis timed out" in report, continue
- Agent non-zero exit → "Analysis unavailable" in report, continue

## Report Structure

```markdown
# Weekly Project Report — {date}

## {repo}
### Progress This Week
{agent summary}

### Big Updates
{agent summary}

### Planned Next
{agent summary}

### Needs Attention
- **#{number}**: "{title}" — Missing priority label.
  Agent summary: {triage summary}

## Team Overview
| Person | Issues Active | Issues Closed This Week |
|--------|--------------|------------------------|
| Alice  | 5            | 2                      |
```

## CLI Interface

Using `clap`:

```
ceo report [--days 7] [--repo org/repo]    # batch mode, stdout
ceo interactive                              # TUI mode
ceo init                                     # generate example config
```

## Interactive Mode (ratatui TUI)

Split-pane layout:
- **Top pane**: scrollable markdown report
- **Bottom pane**: REPL prompt

REPL commands:
- `refresh` — re-fetch and regenerate report
- `show #42` — display full issue detail
- `analyze #42` — re-run agent on specific issue
- `repos` — list configured repos
- `help` — show available commands
- `quit` / Ctrl-C — exit

## Dependencies

| Crate | Purpose |
|-------|---------|
| `clap` | CLI argument parsing |
| `serde` + `toml` | Config parsing |
| `serde_json` | Parsing `gh` JSON output |
| `ratatui` + `crossterm` | TUI framework |
| `chrono` | Date/time handling |

## Error Handling

- `gh` not installed → "gh CLI not found. Install from https://cli.github.com"
- `gh` not authenticated → detect from stderr, suggest `gh auth login`
- Agent CLI not found → "Agent command '{cmd}' not found. Check your config."
- No config → suggest `ceo init`

## Testing Strategy

- **Unit tests**: config parsing, issue filtering, date logic, report formatting
- **Integration tests**: mock `gh` output (canned JSON), verify report generation
- **Manual testing**: TUI interaction

## Future Enhancements (Backlog)

- Configurable agent prompt templates (user-defined in config)
- Parallel agent invocations via tokio
- Local SQLite cache for incremental analysis
- Slack/email report delivery
- PR review summaries alongside issues
