use std::collections::HashMap;

use chrono::{DateTime, Utc};

use crate::config::Config;
use crate::db::{self, CommentRow, CommitRow, IssueRow};
use crate::error::{GhError, SyncError};
use crate::gh::GhRunner;

type Result<T> = std::result::Result<T, SyncError>;

// --- Public types ---

pub struct SyncResult {
    pub repos: Vec<RepoSyncResult>,
}

pub struct RepoSyncResult {
    pub name: String,
    pub issues_synced: usize,
    pub comments_synced: usize,
    pub commits_synced: usize,
}

struct SyncIssue {
    number: u64,
    title: String,
    labels: Vec<String>,
    assignees: Vec<String>,
    updated_at: DateTime<Utc>,
    created_at: DateTime<Utc>,
    state: String,
    body: Option<String>,
    kind: String,
}

// --- Private types for project items ---

pub(crate) struct ProjectItemFields {
    pub status: Option<String>,
    pub start_date: Option<String>,
    pub target_date: Option<String>,
    pub priority: Option<String>,
}

// --- Helper ---

fn get_field_ci(value: &serde_json::Value, names: &[&str]) -> Option<String> {
    for name in names {
        if let Some(v) = value.get(name)
            && let Some(s) = v.as_str()
            && !s.is_empty()
        {
            return Some(s.to_string());
        }
    }
    None
}

// --- Private fetch functions ---

/// Fetch all issues and PRs for a repo via REST API (no GraphQL).
/// The REST `/issues` endpoint returns both issues and PRs; PRs have a
/// `pull_request` key. Paginates and supports incremental `since`.
fn fetch_issues_and_prs_rest(
    gh_runner: &dyn GhRunner,
    repo: &str,
    since: Option<&str>,
) -> std::result::Result<Vec<SyncIssue>, GhError> {
    let mut all_items = Vec::new();
    let mut page = 1u32;

    loop {
        let mut endpoint = format!(
            "repos/{repo}/issues?state=all&per_page=100&page={page}&sort=updated&direction=desc"
        );
        if let Some(ts) = since {
            endpoint.push_str(&format!("&since={ts}"));
        }

        let json = gh_runner.run_gh(&["api", &endpoint])?;
        let parsed: Vec<serde_json::Value> = serde_json::from_str(&json)?;

        if parsed.is_empty() {
            break;
        }

        for item in &parsed {
            let number = item.get("number").and_then(|v| v.as_u64()).unwrap_or(0);
            if number == 0 {
                continue;
            }

            let title = item.get("title").and_then(|v| v.as_str()).unwrap_or_default();
            let state = item.get("state").and_then(|v| v.as_str()).unwrap_or("open");
            let body = item.get("body").and_then(|v| v.as_str()).map(|s| s.to_string());
            let updated_at = item.get("updated_at").and_then(|v| v.as_str()).unwrap_or_default();
            let created_at = item.get("created_at").and_then(|v| v.as_str()).unwrap_or_default();
            let kind = if item.get("pull_request").is_some() { "pr" } else { "issue" };

            let labels: Vec<String> = item
                .get("labels")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|l| l.get("name").and_then(|n| n.as_str()))
                        .map(|s| s.to_string())
                        .collect()
                })
                .unwrap_or_default();

            let assignees: Vec<String> = item
                .get("assignees")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|a| a.get("login").and_then(|n| n.as_str()))
                        .map(|s| s.to_string())
                        .collect()
                })
                .unwrap_or_default();

            let parse_dt = |s: &str| -> DateTime<Utc> {
                chrono::DateTime::parse_from_rfc3339(s)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now())
            };

            all_items.push(SyncIssue {
                number,
                title: title.to_string(),
                labels,
                assignees,
                updated_at: parse_dt(updated_at),
                created_at: parse_dt(created_at),
                state: state.to_uppercase(),
                body,
                kind: kind.to_string(),
            });
        }

        if parsed.len() < 100 {
            break;
        }
        page += 1;
    }

    Ok(all_items)
}

fn fetch_project_items(
    gh_runner: &dyn GhRunner,
    org: &str,
    number: u64,
) -> std::result::Result<HashMap<(String, u64), ProjectItemFields>, GhError> {
    let num_str = number.to_string();
    let json = gh_runner.run_gh(&[
        "project",
        "item-list",
        &num_str,
        "--owner",
        org,
        "--format",
        "json",
        "--limit",
        "1000",
    ])?;

    let parsed: serde_json::Value = serde_json::from_str(&json)?;
    let mut map = HashMap::new();

    if let Some(items) = parsed.get("items").and_then(|v| v.as_array()) {
        for item in items {
            let content = match item.get("content") {
                Some(c) => c,
                None => continue,
            };

            // Skip non-Issue items
            let item_type = content.get("type").and_then(|v| v.as_str()).unwrap_or("");
            if item_type != "Issue" {
                continue;
            }

            let repo = match content.get("repository").and_then(|v| v.as_str()) {
                Some(r) => r.to_string(),
                None => continue,
            };
            let number = match content.get("number").and_then(|v| v.as_u64()) {
                Some(n) => n,
                None => continue,
            };

            let fields = ProjectItemFields {
                status: get_field_ci(item, &["status", "Status"]),
                start_date: get_field_ci(
                    item,
                    &["startDate", "start_date", "Start Date", "Start date"],
                ),
                target_date: get_field_ci(
                    item,
                    &["targetDate", "target_date", "Target Date", "Target date"],
                ),
                priority: get_field_ci(item, &["priority", "Priority"]),
            };

            map.insert((repo, number), fields);
        }
    }

    Ok(map)
}


/// Fetch all issue/PR comments for a repo in one REST API call instead of
/// N+1 GraphQL calls. Uses `GET /repos/{owner}/{repo}/issues/comments` which
/// covers both issues and PRs. Optionally filters by `since`.
fn fetch_comments_batch(
    gh_runner: &dyn GhRunner,
    repo: &str,
    since: Option<&str>,
) -> std::result::Result<Vec<CommentRow>, GhError> {
    let mut all_comments = Vec::new();
    let mut page = 1u32;

    loop {
        let mut endpoint = format!(
            "repos/{repo}/issues/comments?per_page=100&page={page}&sort=created&direction=asc"
        );
        if let Some(ts) = since {
            endpoint.push_str(&format!("&since={ts}"));
        }

        let json = gh_runner.run_gh(&["api", &endpoint])?;
        let parsed: Vec<serde_json::Value> = serde_json::from_str(&json)?;

        if parsed.is_empty() {
            break;
        }

        for item in &parsed {
            let id_num = item.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
            let author = item
                .get("user")
                .and_then(|u| u.get("login"))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let body = item
                .get("body")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let created_at = item
                .get("created_at")
                .and_then(|v| v.as_str())
                .unwrap_or_default();

            // Extract issue number from issue_url: ".../issues/42"
            let issue_number = item
                .get("issue_url")
                .and_then(|v| v.as_str())
                .and_then(|url| url.rsplit('/').next())
                .and_then(|n| n.parse::<u64>().ok())
                .unwrap_or(0);

            if issue_number == 0 {
                continue;
            }

            all_comments.push(CommentRow {
                repo: repo.to_string(),
                issue_number,
                comment_id: id_num,
                author: author.to_string(),
                body: body.to_string(),
                created_at: created_at.to_string(),
            });
        }

        if parsed.len() < 100 {
            break;
        }
        page += 1;
    }

    Ok(all_comments)
}

/// Fetch commit messages for a specific PR to use as a synthetic body
/// when the PR has no description.
fn fetch_pr_commits(
    gh_runner: &dyn GhRunner,
    repo: &str,
    pr_number: u64,
) -> std::result::Result<String, GhError> {
    let endpoint = format!("repos/{repo}/pulls/{pr_number}/commits?per_page=100");
    let json = gh_runner.run_gh(&["api", &endpoint])?;
    let parsed: Vec<serde_json::Value> = serde_json::from_str(&json)?;

    let mut lines = Vec::new();
    for item in &parsed {
        let message = item
            .get("commit")
            .and_then(|c| c.get("message"))
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let first_line = message.lines().next().unwrap_or_default();
        if !first_line.is_empty() {
            lines.push(format!("- {first_line}"));
        }
    }
    Ok(lines.join("\n"))
}

fn fetch_commits_for_branch(
    gh_runner: &dyn GhRunner,
    repo: &str,
    branch: Option<&str>,
) -> std::result::Result<Vec<CommitRow>, GhError> {
    // Fetch last 30 days of commits via GitHub REST API
    let since = (Utc::now() - chrono::Duration::days(30)).to_rfc3339();
    let mut endpoint = format!("repos/{repo}/commits?since={since}&per_page=100");
    if let Some(b) = branch {
        endpoint.push_str(&format!("&sha={b}"));
    }
    let json = gh_runner.run_gh(&["api", &endpoint])?;

    let parsed: Vec<serde_json::Value> = serde_json::from_str(&json)?;
    let mut commits = Vec::new();
    for item in parsed {
        let sha = item.get("sha").and_then(|v| v.as_str()).unwrap_or_default();
        if sha.is_empty() {
            continue;
        }
        let commit_obj = match item.get("commit") {
            Some(c) => c,
            None => continue,
        };
        let message = commit_obj
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        // Use the GitHub login if available, fall back to git author name
        let author = item
            .get("author")
            .and_then(|a| a.get("login"))
            .and_then(|v| v.as_str())
            .or_else(|| {
                commit_obj
                    .get("author")
                    .and_then(|a| a.get("name"))
                    .and_then(|v| v.as_str())
            })
            .unwrap_or("unknown");
        let date = commit_obj
            .get("author")
            .and_then(|a| a.get("date"))
            .and_then(|v| v.as_str())
            .unwrap_or_default();

        commits.push(CommitRow {
            repo: repo.to_string(),
            sha: sha.to_string(),
            author: author.to_string(),
            message: message.to_string(),
            committed_at: date.to_string(),
            branch: branch.unwrap_or_default().to_string(),
        });
    }
    Ok(commits)
}

/// Fetch commits across all configured branches (or just default if none configured).
/// Deduplicates by SHA since branches may share commits.
fn fetch_commits_for_sync(
    gh_runner: &dyn GhRunner,
    repo: &str,
    branches: &[String],
) -> std::result::Result<Vec<CommitRow>, GhError> {
    if branches.is_empty() {
        return fetch_commits_for_branch(gh_runner, repo, None);
    }

    let mut seen_shas = std::collections::HashSet::new();
    let mut all_commits = Vec::new();

    for branch in branches {
        let commits = fetch_commits_for_branch(gh_runner, repo, Some(branch))?;
        for commit in commits {
            if seen_shas.insert(commit.sha.clone()) {
                all_commits.push(commit);
            }
        }
    }

    Ok(all_commits)
}

/// Fetch contributor stats (weekly additions/deletions/commits per author)
/// via the GitHub REST API. Returns one row per author per week.
fn fetch_contributor_stats(
    gh_runner: &dyn GhRunner,
    repo: &str,
) -> std::result::Result<Vec<db::ContributorStatsRow>, GhError> {
    let endpoint = format!("repos/{repo}/stats/contributors");
    let json = gh_runner.run_gh(&["api", &endpoint])?;

    // The API returns 202 with a non-array body while computing stats — treat as empty
    if json.trim().is_empty() {
        return Ok(Vec::new());
    }
    let value: serde_json::Value = serde_json::from_str(&json)?;
    let parsed = match value.as_array() {
        Some(arr) => arr,
        None => return Ok(Vec::new()), // 202 response: stats not yet computed
    };
    let mut rows = Vec::new();

    for contributor in parsed {
        let author = match contributor
            .get("author")
            .and_then(|a| a.get("login"))
            .and_then(|v| v.as_str())
        {
            Some(login) => login,
            None => continue, // skip deleted accounts / bots with null author
        };

        let weeks = match contributor.get("weeks").and_then(|w| w.as_array()) {
            Some(w) => w,
            None => continue,
        };

        for week in weeks {
            let timestamp = week.get("w").and_then(|v| v.as_i64()).unwrap_or(0);
            let additions = week.get("a").and_then(|v| v.as_i64()).unwrap_or(0);
            let deletions = week.get("d").and_then(|v| v.as_i64()).unwrap_or(0);
            let commits = week.get("c").and_then(|v| v.as_i64()).unwrap_or(0);

            // Skip weeks with zero activity
            if additions == 0 && deletions == 0 && commits == 0 {
                continue;
            }

            // Convert Unix timestamp to ISO 8601 date
            let week_start = chrono::DateTime::from_timestamp(timestamp, 0)
                .map(|dt| dt.format("%Y-%m-%d").to_string())
                .unwrap_or_default();

            if week_start.is_empty() {
                continue;
            }

            rows.push(db::ContributorStatsRow {
                repo: repo.to_string(),
                author: author.to_string(),
                week_start,
                additions,
                deletions,
                commits,
            });
        }
    }

    Ok(rows)
}

// --- Progress reporting ---

/// Callback trait for sync progress. All methods have default no-ops so tests
/// can pass `&NoProgress` without noise.
pub trait SyncProgress {
    /// Called per repo phase: "issues", "PRs", "comments", "commits", "saving".
    fn phase(&self, _repo: &str, _name: &str) {}
    /// Called when a repo is fully done.
    fn repo_done(&self, _repo: &str, _result: &RepoSyncResult) {}
    /// Called for non-fatal warnings.
    fn warn(&self, _msg: &str) {}
}

/// No-op progress for tests and non-interactive usage.
pub struct NoProgress;
impl SyncProgress for NoProgress {}

// --- Public API ---

pub fn run_sync(
    config: &Config,
    gh_runner: &dyn GhRunner,
    conn: &rusqlite::Connection,
    progress: &dyn SyncProgress,
) -> Result<SyncResult> {
    // Fetch project data once if configured
    let project_map = if let Some(ref project) = config.project {
        log::info!(
            "Fetching project items for {}/{}",
            project.org,
            project.number
        );
        match fetch_project_items(gh_runner, &project.org, project.number) {
            Ok(map) => {
                log::info!("Fetched {} project items", map.len());
                map
            }
            Err(e) => {
                log::warn!("Failed to fetch project items: {e}");
                progress.warn(&format!("Failed to fetch project board data: {e}"));
                HashMap::new()
            }
        }
    } else {
        HashMap::new()
    };

    let mut repo_results = Vec::new();

    for repo_config in &config.repos {
        let repo = &repo_config.name;
        log::info!("Syncing repo: {repo}");

        // 0. Check last sync time for incremental fetch
        let last_sync = db::query_last_sync(conn, repo).ok().flatten();
        let since = last_sync.as_deref();
        if let Some(ts) = since {
            log::info!("Incremental sync for {repo} since {ts}");
        }

        // 1. Fetch issues and PRs via REST API (single endpoint, no GraphQL)
        progress.phase(repo, "Fetching issues & PRs");
        let mut items = fetch_issues_and_prs_rest(gh_runner, repo, since)?;
        let issue_count = items.iter().filter(|i| i.kind == "issue").count();
        let pr_count = items.iter().filter(|i| i.kind == "pr").count();
        log::info!("Fetched {issue_count} issues and {pr_count} PRs for {repo}");

        // 2. Fetch all comments in one batch REST call
        progress.phase(repo, "Fetching comments");
        let all_comments = match fetch_comments_batch(gh_runner, repo, since) {
            Ok(comments) => {
                log::info!("Fetched {} comments for {repo}", comments.len());
                comments
            }
            Err(e) => {
                log::warn!("Failed to fetch comments for {repo}: {e}");
                progress.warn(&format!("Failed to fetch comments for {repo}: {e}"));
                Vec::new()
            }
        };

        // 2b. For PRs with empty bodies, fetch commit history as synthetic description
        let prs_needing_body: Vec<u64> = items.iter()
            .filter(|i| i.kind == "pr" && i.body.as_deref().unwrap_or("").is_empty())
            .map(|i| i.number)
            .collect();
        if !prs_needing_body.is_empty() {
            progress.phase(repo, &format!("Fetching commits for {} PRs without descriptions", prs_needing_body.len()));
            for &pr_num in &prs_needing_body {
                match fetch_pr_commits(gh_runner, repo, pr_num) {
                    Ok(commit_body) if !commit_body.is_empty() => {
                        if let Some(item) = items.iter_mut().find(|i| i.number == pr_num) {
                            item.body = Some(format!("Commits:\n{commit_body}"));
                        }
                    }
                    Ok(_) => {}
                    Err(e) => {
                        log::warn!("Failed to fetch commits for PR #{pr_num}: {e}");
                    }
                }
            }
        }

        // 3. Fetch commits on configured branches (or default branch)
        if repo_config.branches.is_empty() {
            progress.phase(repo, "Fetching commits");
        } else {
            progress.phase(repo, &format!("Fetching commits ({})", repo_config.branches.join(", ")));
        }
        let commit_rows = match fetch_commits_for_sync(gh_runner, repo, &repo_config.branches) {
            Ok(commits) => {
                log::info!("Fetched {} commits for {repo}", commits.len());
                commits
            }
            Err(e) => {
                log::warn!("Failed to fetch commits for {repo}: {e}");
                progress.warn(&format!("Failed to fetch commits for {repo}: {e}"));
                Vec::new()
            }
        };

        // 4. Fetch contributor stats (weekly additions/deletions per author)
        progress.phase(repo, "Fetching contributor stats");
        let contributor_stats = match fetch_contributor_stats(gh_runner, repo) {
            Ok(stats) => {
                log::info!("Fetched {} contributor stat entries for {repo}", stats.len());
                stats
            }
            Err(e) => {
                log::warn!("Failed to fetch contributor stats for {repo}: {e}");
                progress.warn(&format!("Failed to fetch contributor stats for {repo}: {e}"));
                Vec::new()
            }
        };

        // 5. Build IssueRows, merging project data
        progress.phase(repo, "Saving to database");
        let issue_rows: Vec<IssueRow> = items
            .into_iter()
            .map(|issue| {
                let project_fields = project_map.get(&(repo.clone(), issue.number));
                IssueRow {
                    repo: repo.clone(),
                    number: issue.number,
                    title: issue.title,
                    body: issue.body,
                    state: Some(issue.state),
                    kind: issue.kind,
                    labels: serde_json::to_string(&issue.labels).unwrap_or_default(),
                    assignees: serde_json::to_string(&issue.assignees).unwrap_or_default(),
                    created_at: issue.created_at.to_rfc3339(),
                    updated_at: issue.updated_at.to_rfc3339(),
                    project_status: project_fields.and_then(|f| f.status.clone()),
                    project_start_date: project_fields.and_then(|f| f.start_date.clone()),
                    project_target_date: project_fields.and_then(|f| f.target_date.clone()),
                    project_priority: project_fields.and_then(|f| f.priority.clone()),
                }
            })
            .collect();

        // 6. Upsert in a transaction
        let issues_count = issue_rows.len();
        let comments_count = all_comments.len();
        let commits_count = commit_rows.len();
        {
            let tx = conn
                .unchecked_transaction()
                .map_err(crate::error::DbError::from)?;
            db::upsert_issues(&tx, &issue_rows)?;
            db::upsert_comments(&tx, &all_comments)?;
            db::upsert_commits(&tx, &commit_rows)?;
            db::upsert_contributor_stats(&tx, &contributor_stats)?;
            db::log_sync(&tx, repo, issues_count, comments_count)?;
            tx.commit().map_err(crate::error::DbError::from)?;
        }

        log::info!("Synced {repo}: {issues_count} issues, {comments_count} comments, {commits_count} commits");

        let result = RepoSyncResult {
            name: repo.clone(),
            issues_synced: issues_count,
            comments_synced: comments_count,
            commits_synced: commits_count,
        };
        progress.repo_done(repo, &result);
        repo_results.push(result);
    }

    Ok(SyncResult {
        repos: repo_results,
    })
}
