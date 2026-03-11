/// Database schema version. Bump this when making breaking schema changes.
/// Any mismatch between this and the stored version triggers a full DB reset.
pub const SCHEMA_VERSION: u32 = 2;

/// One row in the `issues` table. Covers both issues and pull requests.
#[derive(Debug, Clone)]
pub struct IssueRow {
    pub repo: String,
    pub number: u64,
    pub title: String,
    pub body: Option<String>,
    pub state: Option<String>,
    pub kind: String,
    pub labels: String,
    pub assignees: String,
    pub created_at: String,
    pub updated_at: String,
    pub project_status: Option<String>,
    pub project_start_date: Option<String>,
    pub project_target_date: Option<String>,
    pub project_priority: Option<String>,
}

/// One row in the `comments` table.
#[derive(Debug, Clone)]
pub struct CommentRow {
    pub repo: String,
    pub issue_number: u64,
    pub comment_id: u64,
    pub author: String,
    pub body: String,
    pub created_at: String,
}

/// One row in the `commits` table.
#[derive(Debug, Clone)]
pub struct CommitRow {
    pub repo: String,
    pub sha: String,
    pub author: String,
    pub message: String,
    pub committed_at: String,
    /// Which branch this commit was fetched from (empty = default branch).
    pub branch: String,
}

/// One row in the `contributor_stats` table.
#[derive(Debug, Clone)]
pub struct ContributorStatsRow {
    pub repo: String,
    pub author: String,
    pub week_start: String, // ISO 8601 date, e.g. "2026-03-02"
    pub additions: i64,
    pub deletions: i64,
    pub commits: i64,
}

/// One row in the `commit_stats` table — per-commit line stats from git log.
#[derive(Debug, Clone)]
pub struct CommitStatsRow {
    pub repo: String,
    pub sha: String,
    pub author: String,        // GitHub login (resolved from email)
    pub committed_at: String,  // ISO 8601 date
    pub additions: i64,
    pub deletions: i64,
    pub branch: String,        // branch where first seen
}

/// One row in the `email_to_github` cache table.
#[derive(Debug, Clone)]
pub struct EmailMappingRow {
    pub email: String,
    pub github: String,
    pub resolved_at: String,
}
