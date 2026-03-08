use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::config::Config;
use crate::db::{self, CommentRow, IssueRow};
use crate::error::{GhError, SyncError};
use crate::gh::{self, GhRunner};

type Result<T> = std::result::Result<T, SyncError>;

// --- Public types ---

pub struct SyncResult {
    pub repos: Vec<RepoSyncResult>,
}

pub struct RepoSyncResult {
    pub name: String,
    pub issues_synced: usize,
    pub comments_synced: usize,
}

// --- Private types for issue deserialization ---

#[derive(Deserialize)]
struct GhLabel {
    name: String,
}

#[derive(Deserialize)]
struct GhUser {
    login: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GhSyncIssue {
    number: u64,
    title: String,
    labels: Vec<GhLabel>,
    assignees: Vec<GhUser>,
    updated_at: DateTime<Utc>,
    created_at: DateTime<Utc>,
    state: String,
    body: Option<String>,
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
        if let Some(v) = value.get(name) {
            if let Some(s) = v.as_str() {
                if !s.is_empty() {
                    return Some(s.to_string());
                }
            }
        }
    }
    None
}

// --- Private fetch functions ---

fn fetch_issues_for_sync(
    gh_runner: &dyn GhRunner,
    repo: &str,
) -> std::result::Result<Vec<SyncIssue>, GhError> {
    let json = gh_runner.run_gh(&[
        "issue",
        "list",
        "--repo",
        repo,
        "--state",
        "open",
        "--json",
        "number,title,labels,assignees,updatedAt,createdAt,state,body",
        "--limit",
        "500",
    ])?;
    let gh_issues: Vec<GhSyncIssue> = serde_json::from_str(&json)?;
    let issues = gh_issues
        .into_iter()
        .map(|gh| SyncIssue {
            number: gh.number,
            title: gh.title,
            labels: gh.labels.into_iter().map(|l| l.name).collect(),
            assignees: gh.assignees.into_iter().map(|a| a.login).collect(),
            updated_at: gh.updated_at,
            created_at: gh.created_at,
            state: gh.state,
            body: gh.body,
        })
        .collect();
    Ok(issues)
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

// --- Public API ---

pub fn run_sync(
    config: &Config,
    gh_runner: &dyn GhRunner,
    conn: &rusqlite::Connection,
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
                log::info!("Failed to fetch project items: {e}");
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

        // 1. Fetch issues
        let issues = fetch_issues_for_sync(gh_runner, repo)?;
        log::info!("Fetched {} issues for {repo}", issues.len());

        // 2. Fetch comments for each issue
        let mut all_comments: Vec<CommentRow> = Vec::new();
        for issue in &issues {
            log::debug!("Fetching comments for {repo}#{}", issue.number);
            let detail = gh::fetch_issue_detail(gh_runner, repo, issue.number)?;
            for (idx, comment) in detail.comments.into_iter().enumerate() {
                all_comments.push(CommentRow {
                    repo: repo.clone(),
                    issue_number: issue.number,
                    comment_id: idx as u64,
                    author: comment.author,
                    body: comment.body,
                    created_at: comment.created_at.to_rfc3339(),
                });
            }
        }

        // 3. Build IssueRows, merging project data
        let issue_rows: Vec<IssueRow> = issues
            .into_iter()
            .map(|issue| {
                let project_fields = project_map.get(&(repo.clone(), issue.number));
                IssueRow {
                    repo: repo.clone(),
                    number: issue.number,
                    title: issue.title,
                    body: issue.body,
                    state: Some(issue.state),
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

        // 4. Upsert in a transaction
        let issues_count = issue_rows.len();
        let comments_count = all_comments.len();
        {
            let tx = conn
                .unchecked_transaction()
                .map_err(crate::error::DbError::from)?;
            db::upsert_issues(&tx, &issue_rows)?;
            db::upsert_comments(&tx, &all_comments)?;
            db::log_sync(&tx, repo, issues_count, comments_count)?;
            tx.commit().map_err(crate::error::DbError::from)?;
        }

        log::info!("Synced {repo}: {issues_count} issues, {comments_count} comments");

        repo_results.push(RepoSyncResult {
            name: repo.clone(),
            issues_synced: issues_count,
            comments_synced: comments_count,
        });
    }

    Ok(SyncResult {
        repos: repo_results,
    })
}
