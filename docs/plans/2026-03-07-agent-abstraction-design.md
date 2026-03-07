# Agent Abstraction Refactor — Design Document

## Overview

Refactor the agent and prompt system from a single generic shell-out into a proper abstraction with:
- A `Prompt` trait with typed prompt structs (`WeeklySummaryPrompt`, `IssueTriagePrompt`)
- An `Agent` trait with an `AgentKind` enum dispatch over `ClaudeAgent`, `CodexAgent`, and `GenericAgent`

## Prompt Trait

```rust
// src/prompt.rs

pub trait Prompt {
    fn render(&self) -> String;
}

pub struct WeeklySummaryPrompt {
    pub repo: String,
    pub issue_summaries: String,
}

pub struct IssueTriagePrompt {
    pub title: String,
    pub body: String,
    pub comments: String,
}
```

Each prompt type holds its data and implements `render()` to produce the final prompt string. Replaces the free functions `build_weekly_summary_prompt` and `build_triage_prompt`.

## Agent Trait & Enum Dispatch

```rust
// src/agent.rs

pub trait Agent {
    fn invoke(&self, prompt: &dyn Prompt) -> Result<String>;
}

pub enum AgentKind {
    Claude(ClaudeAgent),
    Codex(CodexAgent),
    Generic(GenericAgent),
}
```

`AgentKind` implements `Agent` by matching and delegating to the inner type.

### ClaudeAgent

Runs `claude -p "{prompt}"`. Knows Claude CLI conventions: `-p` for print mode, stdout capture.

### CodexAgent

Runs `codex -q "{prompt}"`. Knows Codex CLI conventions: `-q` for quiet/non-interactive mode.

### GenericAgent

Runs `{command} {args} "{prompt}"`. Fallback for any CLI tool. This is the current `RealAgentRunner` behavior preserved.

### Factory

```rust
impl AgentKind {
    pub fn from_config(config: &AgentConfig) -> Self {
        match config.agent_type.as_str() {
            "claude" => AgentKind::Claude(ClaudeAgent::from_config(config)),
            "codex" => AgentKind::Codex(CodexAgent::from_config(config)),
            _ => AgentKind::Generic(GenericAgent::from_config(config)),
        }
    }
}
```

## Config Changes

Add `type` field to `[agent]` section:

```toml
[agent]
type = "claude"
command = "claude"
args = ["-p"]
timeout_secs = 120
```

- `type` defaults to `"claude"`
- `command` is optional — each agent type has a sensible default binary name
- Unknown `type` falls back to `GenericAgent` using `command` + `args`

## Migration from Current Code

| Before | After |
|--------|-------|
| `AgentRunner` trait | `Agent` trait (takes `&dyn Prompt`) |
| `RealAgentRunner` struct | `ClaudeAgent`, `CodexAgent`, `GenericAgent` behind `AgentKind` |
| `build_weekly_summary_prompt()` | `WeeklySummaryPrompt { ... }` struct |
| `build_triage_prompt()` | `IssueTriagePrompt { ... }` struct |
| `run_agent()` free function | Removed — call `agent.invoke(&prompt)` directly |
| `config.agent.command` for type selection | `config.agent.agent_type` field |

## Files Changed

- **Create**: `src/prompt.rs` — Prompt trait and prompt types
- **Rewrite**: `src/agent.rs` — Agent trait, AgentKind enum, Claude/Codex/Generic implementations
- **Modify**: `src/config.rs` — Add `agent_type` field to `AgentConfig`
- **Modify**: `src/pipeline.rs` — Use new Agent/Prompt types
- **Modify**: `src/main.rs` — Use `AgentKind::from_config`
- **Rewrite**: `tests/agent_test.rs` — Test new trait/enum
- **Modify**: `tests/pipeline_test.rs` — Update mock to new trait
- **Modify**: `tests/integration_test.rs` — Update mock to new trait
- **Add**: `src/lib.rs` — Add `pub mod prompt;`

## Testing

- Unit tests for each prompt type's `render()` output
- Unit tests for `AgentKind::from_config` factory (claude/codex/unknown)
- Existing mock-based tests updated to implement `Agent` instead of `AgentRunner`
- All existing pipeline and integration tests continue to pass
