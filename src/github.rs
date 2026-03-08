use std::hash::{Hash, Hasher};

use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::error::GhError;

type Result<T> = std::result::Result<T, GhError>;

// --- Public types ---

#[derive(Debug, Clone)]
pub struct Issue {
    pub number: u64,
    pub title: String,
    pub labels: Vec<String>,
    pub assignees: Vec<String>,
    pub updated_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub repo: String,
}

#[derive(Debug, Clone)]
pub struct IssueDetail {
    pub body: String,
    pub comments: Vec<Comment>,
}

#[derive(Debug, Clone)]
pub struct Comment {
    pub id: u64,
    pub author: String,
    pub body: String,
    pub created_at: DateTime<Utc>,
}

// --- Private deserialization types for gh CLI JSON ---

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
struct GhIssue {
    number: u64,
    title: String,
    labels: Vec<GhLabel>,
    assignees: Vec<GhUser>,
    updated_at: DateTime<Utc>,
    created_at: DateTime<Utc>,
}

#[derive(Deserialize)]
struct GhIssueDetail {
    body: String,
    comments: Vec<GhComment>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GhComment {
    id: String,
    author: GhUser,
    body: String,
    created_at: DateTime<Utc>,
}

// --- Implementations ---

impl Issue {
    /// Parse the JSON output of `gh issue list --json number,title,labels,assignees,updatedAt,createdAt`.
    pub fn parse_gh_list(json: &str, repo: &str) -> Result<Vec<Self>> {
        let gh_issues: Vec<GhIssue> = serde_json::from_str(json)?;
        let issues = gh_issues
            .into_iter()
            .map(|gh| Issue {
                number: gh.number,
                title: gh.title,
                labels: gh.labels.into_iter().map(|l| l.name).collect(),
                assignees: gh.assignees.into_iter().map(|a| a.login).collect(),
                updated_at: gh.updated_at,
                created_at: gh.created_at,
                repo: repo.to_string(),
            })
            .collect();
        Ok(issues)
    }

    /// Return labels from `required` that are missing on this issue.
    pub fn missing_labels(&self, required: &[String]) -> Vec<String> {
        required
            .iter()
            .filter(|r| !self.labels.contains(r))
            .cloned()
            .collect()
    }
}

impl IssueDetail {
    /// Parse the JSON output of `gh issue view --json body,comments`.
    pub fn parse_gh_view(json: &str) -> Result<Self> {
        let gh: GhIssueDetail = serde_json::from_str(json)?;
        Ok(IssueDetail {
            body: gh.body,
            comments: gh
                .comments
                .into_iter()
                .map(|c| Comment {
                    id: hash_node_id(&c.id),
                    author: c.author.login,
                    body: c.body,
                    created_at: c.created_at,
                })
                .collect(),
        })
    }
}

/// Convert a GitHub GraphQL node ID (string) to a stable u64 for use as a database key.
fn hash_node_id(node_id: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    node_id.hash(&mut hasher);
    hasher.finish()
}
