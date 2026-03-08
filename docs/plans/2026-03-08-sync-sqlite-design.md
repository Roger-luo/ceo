# Design: Local SQLite Cache with `ceo sync`

**Date:** 2026-03-08
**Status:** Proposed

## Motivation

Currently, `ceo report` fetches all GitHub data live on every run. This is slow (each issue requires a separate `gh issue view` call for comments), rate-limit-prone, and prevents offline use. Adding a local SQLite database with an explicit `ceo sync` step separates data fetching from report generation, making reports fast and repeatable.

This also enables fetching GitHub Project board metadata (status, dates, priority) which the live pipeline cannot access through `gh issue list` alone.

## Overview

1. Add a `ceo sync` command that fetches issues, comments, and project board data from GitHub and stores them in a local SQLite database.
2. Modify `ceo report` to read from the database instead of fetching live.
3. Add a `[project]` config section for GitHub Projects integration.

## Database

### Location

Use `dirs::data_dir()` for the database file:

- macOS: `~/Library/Application Support/ceo/ceo.db`
- Linux: `~/.local/share/ceo/ceo.db`

The directory is created automatically on first sync. The `rusqlite` crate handles all SQLite operations.

### Schema

Three tables, created on first database open:

**issues** -- one row per issue, keyed by repo + number

| Column | Type | Notes |
|---|---|---|
| repo | TEXT NOT NULL | e.g. `acme-corp/Webapp` |
| number | INTEGER NOT NULL | |
| title | TEXT NOT NULL | |
| body | TEXT | |
| state | TEXT | `open` or `closed` |
| labels | TEXT | JSON array, e.g. `["bug","P1"]` |
| assignees | TEXT | JSON array of login strings |
| created_at | TEXT | ISO 8601 |
| updated_at | TEXT | ISO 8601 |
| project_status | TEXT | Nullable. `Todo`, `In Progress`, `Done`, etc. |
| project_start_date | TEXT | Nullable. ISO 8601 date |
| project_target_date | TEXT | Nullable. ISO 8601 date |
| project_priority | TEXT | Nullable. Project board priority field |
| synced_at | TEXT NOT NULL | ISO 8601 timestamp of last upsert |

Primary key: `(repo, number)`

**comments** -- one row per comment

| Column | Type | Notes |
|---|---|---|
| repo | TEXT NOT NULL | |
| issue_number | INTEGER NOT NULL | |
| comment_id | INTEGER NOT NULL | GitHub comment ID |
| author | TEXT NOT NULL | GitHub login |
| body | TEXT NOT NULL | |
| created_at | TEXT NOT NULL | ISO 8601 |
| synced_at | TEXT NOT NULL | |

Primary key: `(repo, issue_number, comment_id)`
Foreign key: `(repo, issue_number)` references `issues(repo, number)`

**sync_log** -- one row per sync run per repo

| Column | Type | Notes |
|---|---|---|
| repo | TEXT NOT NULL | |
| synced_at | TEXT NOT NULL | ISO 8601 |
| issues_synced | INTEGER | Count of issues upserted |
| comments_synced | INTEGER | Count of comments upserted |

### Migrations

For v1, the schema is created with `CREATE TABLE IF NOT EXISTS` on database open. No migration framework yet. If the schema needs to change later, we add a `schema_version` table and run migrations sequentially.

### Upsert Strategy

All writes use `INSERT OR REPLACE` (SQLite's `UPSERT` equivalent keyed on the primary key). Each sync replaces the full row for every issue and comment it touches. This keeps the logic simple: sync always writes the latest state.

Syncs are wrapped in a single transaction per repo for atomicity and performance.

## Config Changes

### New `[project]` Section

```toml
[project]
org = "acme-corp"
number = 3
```

This identifies the GitHub Projects (v2) board to pull metadata from.

### Struct Changes

Add to `src/config.rs`:

- `ProjectConfig` struct with fields `org: String` and `number: u64`, both required when the section is present.
- Add `pub project: Option<ProjectConfig>` to `Config`. The field is optional; sync works without it but skips project board data.

### get_field / set_field

Wire `project.org` and `project.number` into the existing `get_field`/`set_field` dispatch so `ceo config set project.org acme-corp` works.

### Wizard

Add a `--- Project ---` section to `cmd_config_wizard` after the Team section. Prompt for org and project number. Allow clearing with `-`.

## CLI: `ceo sync`

### Command Definition

Add a `Sync` variant to the `Commands` enum in `src/main.rs`:

```
ceo sync          # sync all configured repos
```

No subcommands or flags in v1. Future iterations may add `--repo` filtering or `--since` for incremental sync.

### Sync Flow

For each repo in `config.repos`:

1. **Fetch open issues.** Use `gh issue list --repo <repo> --state open --json number,title,labels,assignees,updatedAt,createdAt,state,body --limit 500`. This is a single call that returns all open issues with bodies included, avoiding per-issue detail fetches.

2. **Fetch comments for each issue.** Use `gh issue view <number> --repo <repo> --json comments`. This is the expensive step (one call per issue). Only fetch for issues whose `updated_at` is within a reasonable window, or all of them in v1.

3. **Fetch project board data** (if `[project]` is configured). Single call: `gh project item-list <number> --owner <org> --format json --limit 1000`. Parse the response and build a lookup map of `(repo, issue_number) -> project fields`.

4. **Upsert into SQLite.** Within a transaction: upsert all issues (merging project fields from step 3), upsert all comments, insert a sync_log entry.

5. **Print progress.** `Syncing org/repo... 42 issues, 156 comments`

### GitHub Project Data

The `gh project item-list` command returns JSON with this shape:

```json
{
  "items": [
    {
      "content": { "number": 42, "repository": "org/repo", "type": "Issue" },
      "status": "In Progress",
      "startDate": "2026-01-15",
      "targetDate": "2026-03-01",
      "priority": "High",
      ...
    }
  ]
}
```

We parse this into a `HashMap<(String, u64), ProjectItemFields>` keyed on `(repo, number)` and look up each issue during the upsert step. Items that are DraftIssues or PRs are ignored.

Field names in the project response vary by board configuration. We look for common field names: `Status`, `Start Date` / `Start date`, `Target Date` / `Target date`, `Priority`. Matching is case-insensitive.

## Pipeline Changes

### `ceo report` Reads from Database

Replace the live-fetch logic in `run_pipeline` with database queries:

1. Open the database. If the file does not exist, return an error suggesting `ceo sync`.
2. Query issues where `updated_at >= <cutoff>` for the configured time range, scoped to repos in config.
3. Query comments joined to those issues.
4. Build the same `Issue` / `IssueDetail` structures the pipeline already uses.
5. Feed into the existing agent summarization pipeline (per-issue summary, then aggregate). No changes to prompts or agent invocation.

This means `run_pipeline` no longer needs the `GhRunner` parameter. It takes a database handle (or path) instead.

### Empty Database Handling

If the database exists but has no issues for the requested repos/time range, print a message: `No issues found. Try running 'ceo sync' or adjusting --days.`

If the database file does not exist at all, return `DbError::NotFound` with the expected path.

## Module Structure

### New Files

**`src/db.rs`** -- Database layer
- `db_path() -> PathBuf` -- resolve the XDG data dir path
- `open_db() -> Result<Connection, DbError>` -- open or create the database, run schema creation
- `upsert_issues(conn, &[IssueRow]) -> Result<usize, DbError>` -- bulk upsert, returns count
- `upsert_comments(conn, &[CommentRow]) -> Result<usize, DbError>` -- bulk upsert, returns count
- `log_sync(conn, repo, issues_count, comments_count) -> Result<(), DbError>`
- `query_recent_issues(conn, repos, since) -> Result<Vec<IssueRow>, DbError>` -- for pipeline
- `query_comments_for_issues(conn, repo, issue_numbers) -> Result<Vec<CommentRow>, DbError>`
- Row types: `IssueRow`, `CommentRow` that map directly to the table schemas

**`src/sync.rs`** -- Sync orchestration
- `run_sync(config, gh_runner) -> Result<SyncResult, SyncError>` -- top-level sync function
- `fetch_project_items(gh_runner, org, number) -> Result<HashMap<...>, GhError>` -- parse project board
- `SyncResult` struct with per-repo counts for display

### Modified Files

- **`src/main.rs`** -- Add `Sync` command variant and `cmd_sync()` handler
- **`src/config.rs`** -- Add `ProjectConfig` struct, wire into `Config`, `get_field`, `set_field`, wizard
- **`src/error.rs`** -- Add `DbError` and `SyncError` enums
- **`src/pipeline.rs`** -- Replace `GhRunner` usage with database reads; keep agent flow unchanged
- **`src/lib.rs`** -- Add `pub mod db;` and `pub mod sync;`

### Dependency Changes

Add to `Cargo.toml`:
- `rusqlite = { version = "0.33", features = ["bundled"] }` -- `bundled` compiles SQLite from source, avoiding system library issues

## Error Types

**DbError** in `src/error.rs`:

- `Sqlite(rusqlite::Error)` -- any rusqlite error, via `#[from]`
- `NotFound(PathBuf)` -- database file does not exist, message suggests `ceo sync`
- `CreateDir { path: PathBuf, source: std::io::Error }` -- failed to create the data directory

**SyncError** in `src/error.rs`:

- `Gh(GhError)` -- GitHub CLI failure, via `#[from]`
- `Db(DbError)` -- database failure, via `#[from]`
- `NoProject` -- `[project]` section not configured, message suggests `ceo config`

**PipelineError** gains:

- `Db(DbError)` -- via `#[from]`, for when report queries fail

## Data Flow Summary

```
ceo sync:
  config.toml -> repos list
                    |
                    v
  gh issue list  -----> issues ----+
  gh issue view  -----> comments --+--> SQLite (upsert)
  gh project item-list -> project -+
                                        |
                                        v
                                    sync_log entry

ceo report:
  SQLite -----> issues + comments (by date range)
                    |
                    v
              agent summarize (per-issue)
                    |
                    v
              agent aggregate (per-repo)
                    |
                    v
              markdown report
```

## What This Does NOT Cover (Future Work)

- **Incremental sync.** V1 fetches all open issues every time. A future version can use `updated_at > last_sync` from sync_log to only fetch changed issues.
- **Interactive mode combining sync + report.** The TUI could trigger a sync before generating the report.
- **Issue analysis.** The database enables queries like "issues missing target dates," "overdue items," or "stale issues with no updates in 30 days." These are deferred to a later iteration.
- **Closed issue tracking.** V1 only syncs open issues. Tracking recently-closed issues would improve report coverage.
- **Database pruning.** No cleanup of old data. The database will grow slowly since we only track open issues; closed issues become stale naturally.
