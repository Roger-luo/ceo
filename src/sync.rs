use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;

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
    author: Option<String>,
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

            let author = item
                .get("user")
                .and_then(|u| u.get("login"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

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
                author,
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

// --- Email → GitHub handle resolution ---

/// Resolve a set of git author emails to GitHub handles.
/// Checks DB cache first, then calls `gh api /search/users?q={email}+in:email`
/// for uncached emails. Results are cached for future syncs.
/// `gh_runner` is optional — if None, only cached lookups are used (useful for tests).
fn resolve_emails(
    conn: &rusqlite::Connection,
    emails: &[String],
    gh_runner: Option<&dyn GhRunner>,
) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let mut uncached = Vec::new();

    for email in emails {
        match db::query_email_mapping(conn, email) {
            Ok(Some(github)) => {
                map.insert(email.clone(), github);
            }
            _ => {
                uncached.push(email.clone());
            }
        }
    }

    let Some(gh) = gh_runner else {
        return map;
    };

    for (idx, email) in uncached.iter().enumerate() {
        // GitHub noreply emails: extract handle directly (e.g. "12345678+jdoe@users.noreply.github.com")
        if email.ends_with("@users.noreply.github.com") {
            let prefix = email.split('@').next().unwrap_or(email);
            // Strip numeric ID prefix: "12345678+jdoe" -> "jdoe"
            let handle = prefix.rsplit('+').next().unwrap_or(prefix);
            // Strip "[bot]" suffix for bot accounts
            let handle = handle.strip_suffix("[bot]")
                .map(|h| format!("{h}[bot]"))
                .unwrap_or_else(|| handle.to_string());
            let _ = db::upsert_email_mapping(conn, email, &handle);
            map.insert(email.clone(), handle);
            continue;
        }

        // GitHub search API rate limit: 30 req/min. Throttle to stay under.
        if idx > 0 {
            std::thread::sleep(std::time::Duration::from_secs(2));
        }
        // URL-encode the email to handle special chars like '+'
        let encoded_email: String = email.chars().map(|c| match c {
            '+' => "%2B".to_string(),
            '@' => "%40".to_string(),
            _ => c.to_string(),
        }).collect();
        let endpoint = format!("search/users?q={}+in:email", encoded_email);
        match gh.run_gh(&["api", &endpoint]) {
            Ok(json) => {
                let parsed: serde_json::Value = match serde_json::from_str(&json) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                if let Some(login) = parsed
                    .get("items")
                    .and_then(|v| v.as_array())
                    .and_then(|arr| arr.first())
                    .and_then(|u| u.get("login"))
                    .and_then(|v| v.as_str())
                {
                    let _ = db::upsert_email_mapping(conn, email, login);
                    map.insert(email.clone(), login.to_string());
                } else {
                    // No match — use email prefix as fallback (don't cache to allow retry)
                    let fallback = email.split('@').next().unwrap_or(email).to_string();
                    map.insert(email.clone(), fallback);
                }
            }
            Err(e) => {
                log::warn!("Failed to resolve email {email}: {e}");
                let fallback = email.split('@').next().unwrap_or(email).to_string();
                map.insert(email.clone(), fallback);
            }
        }
    }

    map
}

// --- Git clone/fetch and local git log ---

/// Returns the directory for bare git clones: ~/.local/share/ceo/repos/
fn repos_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("ceo")
        .join("repos")
}

/// Clone a repo as a bare repository (first time) or fetch updates.
/// Returns the path to the bare repo directory.
fn clone_or_fetch_repo(repo: &str) -> std::result::Result<PathBuf, SyncError> {
    let repo_path = repos_dir().join(format!("{}.git", repo));

    if repo_path.exists() {
        // Ensure the fetch refspec is set. `git clone --bare` does not configure one,
        // so without this, `git fetch` only updates FETCH_HEAD and never updates
        // local branch refs — making new commits invisible to `git log`.
        ensure_fetch_refspec(&repo_path, repo)?;
        let output = Command::new("git")
            .args(["fetch", "--prune"])
            .current_dir(&repo_path)
            .output()
            .map_err(|e| SyncError::Git(format!("git fetch failed for {repo}: {e}")))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(SyncError::Git(format!("git fetch failed for {repo}: {stderr}")));
        }
    } else {
        if let Some(parent) = repo_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| SyncError::Git(format!("failed to create dir for {repo}: {e}")))?;
        }
        let url = format!("https://github.com/{repo}.git");
        let output = Command::new("git")
            .args(["clone", "--bare", &url, &repo_path.to_string_lossy()])
            .output()
            .map_err(|e| SyncError::Git(format!("git clone failed for {repo}: {e}")))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(SyncError::Git(format!("git clone failed for {repo}: {stderr}")));
        }
        // Set the fetch refspec right after cloning so future fetches update branches.
        ensure_fetch_refspec(&repo_path, repo)?;
    }

    Ok(repo_path)
}

/// Ensure the bare repo has a fetch refspec configured so `git fetch` updates branch refs.
fn ensure_fetch_refspec(repo_path: &std::path::Path, repo: &str) -> std::result::Result<(), SyncError> {
    let output = Command::new("git")
        .args(["config", "--get", "remote.origin.fetch"])
        .current_dir(repo_path)
        .output()
        .map_err(|e| SyncError::Git(format!("git config failed for {repo}: {e}")))?;
    if !output.status.success() || output.stdout.is_empty() {
        let set_output = Command::new("git")
            .args(["config", "remote.origin.fetch", "+refs/heads/*:refs/heads/*"])
            .current_dir(repo_path)
            .output()
            .map_err(|e| SyncError::Git(format!("git config set failed for {repo}: {e}")))?;
        if !set_output.status.success() {
            let stderr = String::from_utf8_lossy(&set_output.stderr);
            return Err(SyncError::Git(format!("failed to set fetch refspec for {repo}: {stderr}")));
        }
        log::info!("Set fetch refspec for {repo} bare repo");
    }
    Ok(())
}

struct GitCommitStat {
    sha: String,
    email: String,
    date: String,
    additions: i64,
    deletions: i64,
}

fn parse_git_log_output(output: &str) -> Vec<GitCommitStat> {
    let mut results = Vec::new();
    let lines: Vec<&str> = output.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        if lines[i].trim().is_empty() {
            i += 1;
            continue;
        }

        if i + 3 >= lines.len() {
            break;
        }

        let sha = lines[i].trim().to_string();
        let email = lines[i + 1].trim().to_string();
        let _author_name = lines[i + 2].trim();
        let date = lines[i + 3].trim().to_string();
        i += 4;

        while i < lines.len() && lines[i].trim().is_empty() {
            i += 1;
        }

        let mut additions = 0i64;
        let mut deletions = 0i64;
        if i < lines.len() {
            let stat_line = lines[i].trim();
            if stat_line.contains("changed") {
                for part in stat_line.split(", ") {
                    let part = part.trim();
                    if part.contains("insertion") {
                        additions = part
                            .split_whitespace()
                            .next()
                            .and_then(|n| n.parse().ok())
                            .unwrap_or(0);
                    } else if part.contains("deletion") {
                        deletions = part
                            .split_whitespace()
                            .next()
                            .and_then(|n| n.parse().ok())
                            .unwrap_or(0);
                    }
                }
                i += 1;
            }
        }

        if sha.len() >= 40 {
            results.push(GitCommitStat {
                sha,
                email,
                date,
                additions,
                deletions,
            });
        }
    }

    results
}

fn collect_git_stats(
    repo_path: &std::path::Path,
    repo_name: &str,
    branches: &[String],
    since: &str,
) -> std::result::Result<Vec<(GitCommitStat, String)>, SyncError> {
    // In bare clones, branches are stored directly (not as origin/*).
    let branches_to_scan: Vec<String> = if branches.is_empty() {
        vec!["HEAD".to_string()]
    } else {
        branches.to_vec()
    };

    let mut seen_shas = std::collections::HashSet::new();
    let mut all_stats = Vec::new();

    for branch in &branches_to_scan {
        let output = Command::new("git")
            .args([
                "log",
                "--format=%H%n%ae%n%an%n%aI",
                "--shortstat",
                &format!("--since={since}"),
                branch,
            ])
            .env("LC_ALL", "C")
            .current_dir(repo_path)
            .output()
            .map_err(|e| {
                SyncError::Git(format!(
                    "git log failed for {repo_name} branch {branch}: {e}"
                ))
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            log::warn!("git log failed for {repo_name} branch {branch}: {stderr}");
            continue;
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let parsed = parse_git_log_output(&stdout);

        let branch_label = branch.clone();
        for stat in parsed {
            if seen_shas.insert(stat.sha.clone()) {
                all_stats.push((stat, branch_label.clone()));
            }
        }
    }

    Ok(all_stats)
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

        // 4. Collect contributor stats via local git clone
        progress.phase(repo, "Cloning/fetching git repo");
        let commit_stats_rows = match clone_or_fetch_repo(repo) {
            Ok(repo_path) => {
                let since_30d = (Utc::now() - chrono::Duration::days(30))
                    .format("%Y-%m-%d")
                    .to_string();
                progress.phase(repo, "Collecting git stats");
                match collect_git_stats(
                    &repo_path,
                    repo,
                    &repo_config.branches,
                    &since_30d,
                ) {
                    Ok(stats) => {
                        // Gather unique emails for resolution
                        let emails: Vec<String> = stats
                            .iter()
                            .map(|(s, _)| s.email.clone())
                            .collect::<std::collections::HashSet<_>>()
                            .into_iter()
                            .collect();

                        progress.phase(repo, &format!("Resolving {} author emails", emails.len()));
                        // Resolve emails to GitHub handles (populates email_to_github cache)
                        let _ = resolve_emails(conn, &emails, Some(gh_runner));

                        // Build CommitStatsRows with raw email (resolution happens at query time)
                        let rows: Vec<db::CommitStatsRow> = stats
                            .into_iter()
                            .map(|(stat, branch)| {
                                db::CommitStatsRow {
                                    repo: repo.clone(),
                                    sha: stat.sha,
                                    author_email: stat.email,
                                    committed_at: stat.date.get(..10).unwrap_or(&stat.date).to_string(),
                                    additions: stat.additions,
                                    deletions: stat.deletions,
                                    branch,
                                }
                            })
                            .collect();

                        log::info!("Collected {} commit stats for {repo}", rows.len());
                        rows
                    }
                    Err(e) => {
                        log::warn!("Failed to collect git stats for {repo}: {e}");
                        progress.warn(&format!("Failed to collect git stats for {repo}: {e}"));
                        Vec::new()
                    }
                }
            }
            Err(e) => {
                log::warn!("Failed to clone/fetch {repo}: {e}");
                progress.warn(&format!("Failed to clone/fetch {repo}: {e}"));
                Vec::new()
            }
        };

        // 5. Fetch additions/deletions for open PRs
        let open_prs: Vec<u64> = items
            .iter()
            .filter(|i| i.kind == "pr" && i.state == "OPEN")
            .map(|i| i.number)
            .collect();
        let mut pr_stats: HashMap<u64, (i64, i64)> = HashMap::new();
        if !open_prs.is_empty() {
            progress.phase(repo, &format!("Fetching stats for {} open PRs", open_prs.len()));
            for &pr_num in &open_prs {
                let endpoint = format!("repos/{repo}/pulls/{pr_num}");
                match gh_runner.run_gh(&["api", &endpoint]) {
                    Ok(json) => {
                        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&json) {
                            let adds = parsed.get("additions").and_then(|v| v.as_i64()).unwrap_or(0);
                            let dels = parsed.get("deletions").and_then(|v| v.as_i64()).unwrap_or(0);
                            pr_stats.insert(pr_num, (adds, dels));
                        }
                    }
                    Err(e) => log::warn!("Failed to fetch PR stats for {repo}#{pr_num}: {e}"),
                }
            }
        }

        // 6. Build IssueRows, merging project data and PR stats
        progress.phase(repo, "Saving to database");
        let issue_rows: Vec<IssueRow> = items
            .into_iter()
            .map(|issue| {
                let project_fields = project_map.get(&(repo.clone(), issue.number));
                let (pr_additions, pr_deletions) = pr_stats
                    .get(&issue.number)
                    .map(|&(a, d)| (Some(a), Some(d)))
                    .unwrap_or((None, None));
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
                    author: issue.author,
                    pr_additions,
                    pr_deletions,
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
            db::upsert_commit_stats(&tx, &commit_stats_rows)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_git_log_output_works() {
        let output = "\
abc1234567890abcdef1234567890abcdef123456
alice@example.com
Alice Smith
2026-03-05T10:00:00+00:00

 3 files changed, 100 insertions(+), 50 deletions(-)

def1234567890abcdef1234567890abcdef654321
bob@example.com
Bob Jones
2026-03-04T10:00:00+00:00

 1 file changed, 10 insertions(+)
";
        let stats = parse_git_log_output(output);
        assert_eq!(stats.len(), 2);
        assert_eq!(stats[0].email, "alice@example.com");
        assert_eq!(stats[0].additions, 100);
        assert_eq!(stats[0].deletions, 50);
        assert_eq!(stats[1].email, "bob@example.com");
        assert_eq!(stats[1].additions, 10);
        assert_eq!(stats[1].deletions, 0);
    }

    #[test]
    fn resolve_emails_uses_cache() {
        let dir = tempfile::tempdir().unwrap();
        let conn = db::open_db_at(&dir.path().join("test.db")).unwrap();

        // Pre-populate cache
        db::upsert_email_mapping(&conn, "alice@example.com", "alice").unwrap();

        let emails = vec!["alice@example.com".to_string()];
        let map = resolve_emails(&conn, &emails, None);
        assert_eq!(map.get("alice@example.com"), Some(&"alice".to_string()));
    }

    #[test]
    fn resolve_emails_handles_noreply() {
        let dir = tempfile::tempdir().unwrap();
        let conn = db::open_db_at(&dir.path().join("test.db")).unwrap();

        let emails = vec![
            "12345678+jdoe@users.noreply.github.com".to_string(),
            "someuser@users.noreply.github.com".to_string(),
        ];
        // gh_runner=None means no API calls — noreply should still resolve
        // We need a mock gh_runner since the function returns early without one
        struct NoOpGh;
        impl GhRunner for NoOpGh {
            fn run_gh(&self, _args: &[&str]) -> std::result::Result<String, GhError> {
                panic!("should not call API for noreply emails");
            }
        }
        let map = resolve_emails(&conn, &emails, Some(&NoOpGh));
        assert_eq!(map.get("12345678+jdoe@users.noreply.github.com"), Some(&"jdoe".to_string()));
        assert_eq!(map.get("someuser@users.noreply.github.com"), Some(&"someuser".to_string()));

        // Verify it was cached in DB
        let cached = db::query_email_mapping(&conn, "12345678+jdoe@users.noreply.github.com").unwrap();
        assert_eq!(cached, Some("jdoe".to_string()));
    }

    #[test]
    fn parse_git_log_handles_merge_commits_no_stats() {
        let output = "\
abc1234567890abcdef1234567890abcdef123456
alice@example.com
Alice Smith
2026-03-05T10:00:00+00:00

";
        let stats = parse_git_log_output(output);
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].additions, 0);
        assert_eq!(stats[0].deletions, 0);
    }

    #[test]
    fn ensure_fetch_refspec_sets_missing_refspec() {
        let dir = tempfile::tempdir().unwrap();
        let bare_path = dir.path().join("test.git");

        // Create a bare repo without a fetch refspec (mimics `git clone --bare`)
        let output = Command::new("git")
            .args(["init", "--bare", &bare_path.to_string_lossy()])
            .output()
            .unwrap();
        assert!(output.status.success());

        // Add a remote origin, then remove the fetch refspec to simulate `git clone --bare`
        Command::new("git")
            .args(["remote", "add", "origin", "https://github.com/test/repo.git"])
            .current_dir(&bare_path)
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "--unset", "remote.origin.fetch"])
            .current_dir(&bare_path)
            .output()
            .unwrap();

        // Verify no fetch refspec exists
        let output = Command::new("git")
            .args(["config", "--get", "remote.origin.fetch"])
            .current_dir(&bare_path)
            .output()
            .unwrap();
        assert!(!output.status.success(), "Should have no fetch refspec initially");

        // Run ensure_fetch_refspec
        ensure_fetch_refspec(&bare_path, "test/repo").unwrap();

        // Verify refspec was set
        let output = Command::new("git")
            .args(["config", "--get", "remote.origin.fetch"])
            .current_dir(&bare_path)
            .output()
            .unwrap();
        assert!(output.status.success(), "fetch refspec should now be set");
        let refspec = String::from_utf8_lossy(&output.stdout).trim().to_string();
        assert_eq!(refspec, "+refs/heads/*:refs/heads/*");
    }

    #[test]
    fn ensure_fetch_refspec_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let bare_path = dir.path().join("test.git");

        Command::new("git")
            .args(["init", "--bare", &bare_path.to_string_lossy()])
            .output()
            .unwrap();
        Command::new("git")
            .args(["remote", "add", "origin", "https://github.com/test/repo.git"])
            .current_dir(&bare_path)
            .output()
            .unwrap();

        // Set refspec twice — should not error or duplicate
        ensure_fetch_refspec(&bare_path, "test/repo").unwrap();
        ensure_fetch_refspec(&bare_path, "test/repo").unwrap();

        // Should still have exactly one refspec line
        let output = Command::new("git")
            .args(["config", "--get-all", "remote.origin.fetch"])
            .current_dir(&bare_path)
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let lines: Vec<&str> = stdout.trim().lines().collect();
        assert_eq!(lines.len(), 1, "Should have exactly one fetch refspec, not duplicates");
    }
}
