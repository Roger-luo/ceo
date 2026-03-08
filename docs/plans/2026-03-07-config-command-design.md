# Config Command — Design Document

## Overview

Add a `ceo config` command with interactive wizard mode (bare `ceo config`) and non-interactive subcommands (`set`, `get`, `show`). Replaces `ceo init`.

## CLI Interface

```
ceo config                           # interactive step-by-step wizard
ceo config set <key> <value>         # set a field by dotted path
ceo config get <key>                 # get a field by dotted path
ceo config show                      # print full config as TOML
```

## Key Paths

| Key | Example | Notes |
|-----|---------|-------|
| `agent.type` | `ceo config set agent.type codex` | |
| `agent.command` | `ceo config set agent.command /usr/bin/claude` | |
| `agent.timeout_secs` | `ceo config set agent.timeout_secs 60` | Parsed as u64 |
| `agent.args` | `ceo config set agent.args "-p,--verbose"` | Comma-separated |
| `repos.add` | `ceo config set repos.add org/myrepo` | Appends a repo |
| `repos.remove` | `ceo config set repos.remove org/myrepo` | Removes a repo |
| `team.add` | `ceo config set team.add alice "Alice Smith" Engineer` | Appends member |
| `team.remove` | `ceo config set team.remove alice` | Removes by github handle |

## Interactive Wizard

Runs on bare `ceo config`. Step-by-step stdin prompts:

1. Agent type: "Which agent? [claude/codex/other]:" (default: current or "claude")
2. Agent timeout: "Timeout in seconds [120]:"
3. Repos loop: "Add a repo (org/name), or Enter to finish:"
   - Per repo: "Required labels (comma-separated, or Enter for none):"
4. Team loop: "Add team member (github username), or Enter to finish:"
   - Per member: "Full name:", "Role:"

Creates config directory and file if needed. Overwrites existing config.

## Config Module Changes

- Add `Serialize` derive to `Config`, `AgentConfig`, `RepoConfig`, `TeamMember`
- Add `Config::save(&self) -> Result<()>` — serialize to TOML, write to config path
- Add `Config::config_path() -> PathBuf` — returns existing config path or default `~/.config/ceo/config.toml`
- Add `Config::set_field(&mut self, key: &str, value: &str) -> Result<()>` — dot-path setter
- Add `Config::get_field(&self, key: &str) -> Result<String>` — dot-path getter

## Migration

- `ceo init` becomes a hidden alias for `ceo config` (wizard mode)
- Old `cmd_init` function removed

## Files Changed

- Modify: `src/config.rs` — add Serialize, save, set_field, get_field, config_path
- Modify: `src/main.rs` — add Config subcommand, replace Init
- Modify: `tests/config_test.rs` — add tests for set_field, get_field, save
- Modify: `Cargo.toml` — may need serde Serialize feature (already included via `features = ["derive"]`)
