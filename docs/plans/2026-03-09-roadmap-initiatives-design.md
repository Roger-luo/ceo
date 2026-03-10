# Roadmap / Initiatives Design

## Goal

Provide high-level initiative context (product lines, quarterly goals) to the agent when summarizing repos, so summaries frame issue-level work in terms of bigger objectives.

## File

`~/.config/ceo/roadmap.toml` — same directory as the main config, separate file.

## Schema

```toml
[[initiatives]]
name = "Platform v2"
timeframe = "Q1 2026"
repos = ["acme-corp/platform"]
description = "Complete API refactor, inspect overhaul, TUI recovery"

[[initiatives]]
name = "Production readiness"
repos = ["acme-corp/platform", "acme-corp/webapp"]
description = "Full test coverage, CI pipeline, public docs"
```

- `name` — required, unique identifier
- `timeframe` — optional free string ("Q1 2026", "2026", "H2 2026")
- `repos` — required, list of repo names this initiative covers
- `description` — required, free text explaining the initiative

## CLI

`ceo roadmap` subcommand:

- `ceo roadmap show` — print current roadmap to stdout (TOML format)
- `ceo roadmap edit` — open in `$EDITOR`; creates file with commented template if it doesn't exist
- `ceo roadmap add "Name" --timeframe "Q1 2026" --repos org/repo1,org/repo2 --description "..."` — add an initiative (agent-friendly, no editor needed)
- `ceo roadmap remove "Name"` — remove an initiative by name

## Pipeline Integration

When generating a repo summary (`WeeklySummaryPrompt`), the pipeline:

1. Loads roadmap from file (skip silently if file doesn't exist)
2. Filters initiatives where `repos` contains the current repo name
3. Injects matching initiatives as context into the prompt

The agent can then frame work like "PR #25 advances the Q1 Platform v2 initiative" instead of describing PRs in isolation.

## No Archiving

Delete initiatives when done. Generated reports serve as the historical record of what got accomplished under each initiative.

## Non-Goals

- No deadline enforcement or reminders
- No progress tracking (that's what GitHub Projects is for)
- No cross-referencing initiatives with specific issues
