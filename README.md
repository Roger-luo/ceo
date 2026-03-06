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

See `ceo init` for an example config file, or create `~/.config/ceo/config.toml`:

```toml
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
```

## Usage

### Batch mode

```bash
# Default 7-day lookback
ceo report

# Custom lookback period
ceo report --days 14
```

### Interactive mode

```bash
ceo interactive
```

Split-pane TUI with scrollable report and command REPL. Commands: `help`, `refresh`, `show #N`, `analyze #N`, `repos`, `quit`.
