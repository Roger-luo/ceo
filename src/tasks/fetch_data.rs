use chrono::Utc;
use log::{debug, info};

use crate::db::{self, IssueRow};
use crate::github::Issue;

use super::{PipelineContext, Result, Task};

fn row_to_issue(row: &IssueRow) -> Issue {
    let labels: Vec<String> = serde_json::from_str(&row.labels).unwrap_or_default();
    let assignees: Vec<String> = serde_json::from_str(&row.assignees).unwrap_or_default();
    Issue {
        number: row.number,
        title: row.title.clone(),
        kind: row.kind.clone(),
        state: row.state.clone().unwrap_or_else(|| "OPEN".to_string()),
        labels,
        assignees,
        updated_at: chrono::DateTime::parse_from_rfc3339(&row.updated_at)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now()),
        created_at: chrono::DateTime::parse_from_rfc3339(&row.created_at)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now()),
        repo: row.repo.clone(),
    }
}

pub struct FetchDataTask;

impl Task for FetchDataTask {
    fn name(&self) -> &str {
        "fetch_data"
    }

    fn description(&self) -> &str {
        "Fetch issues, comments, and commits from the database"
    }

    fn step_count(&self, ctx: &PipelineContext) -> usize {
        ctx.config.repos.len()
    }

    fn should_skip(&self, _ctx: &PipelineContext) -> bool {
        false
    }

    fn run(&self, ctx: &mut PipelineContext) -> Result<()> {
        for repo_config in &ctx.config.repos {
            info!("Processing repo: {}", repo_config.name);
            let repo_names = vec![repo_config.name.clone()];
            let issue_rows = db::query_recent_issues(ctx.conn, &repo_names, &ctx.since)?;
            let issues: Vec<Issue> = issue_rows.iter().map(row_to_issue).collect();
            debug!("Found {} recent issues (since {})", issues.len(), ctx.since);

            // Collect issue numbers for comment lookup
            let issue_numbers: Vec<u64> = issue_rows.iter().map(|r| r.number).collect();
            let comment_rows =
                db::query_comments_for_issues(ctx.conn, &repo_config.name, &issue_numbers)?;

            // Build a map of (repo, issue_number) -> comment rows
            let mut comments_by_issue: std::collections::HashMap<u64, Vec<db::CommentRow>> =
                std::collections::HashMap::new();
            for c in comment_rows {
                comments_by_issue.entry(c.issue_number).or_default().push(c);
            }
            for (issue_number, rows) in comments_by_issue {
                ctx.comments
                    .insert((repo_config.name.clone(), issue_number), rows);
            }

            // Build a map of (repo, issue_number) -> body from the rows
            for r in &issue_rows {
                ctx.issue_bodies.insert(
                    (repo_config.name.clone(), r.number),
                    r.body.clone().unwrap_or_default(),
                );
            }

            // Accumulate all recent issues
            for issue in &issues {
                ctx.all_recent_issues.push(issue.clone());
            }

            // Query recent commits for this repo
            let repo_names_for_commits = vec![repo_config.name.clone()];
            let commit_rows =
                db::query_recent_commits(ctx.conn, &repo_names_for_commits, &ctx.since)?;
            debug!(
                "Found {} recent commits for {}",
                commit_rows.len(),
                repo_config.name
            );

            ctx.commit_rows
                .insert(repo_config.name.clone(), commit_rows);
            ctx.issues.insert(repo_config.name.clone(), issues);
            ctx.issue_rows.insert(repo_config.name.clone(), issue_rows);
        }

        Ok(())
    }
}
