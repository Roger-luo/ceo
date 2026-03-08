use log::debug;
use std::process::Command;

use crate::error::GhError;
use crate::github::{Issue, IssueDetail};

type Result<T> = std::result::Result<T, GhError>;

pub trait GhRunner {
    fn run_gh(&self, args: &[&str]) -> Result<String>;
}

pub struct RealGhRunner;

impl GhRunner for RealGhRunner {
    fn run_gh(&self, args: &[&str]) -> Result<String> {
        debug!("Running: gh {}", args.join(" "));
        let output = Command::new("gh")
            .args(args)
            .output()
            .map_err(GhError::NotInstalled)?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if stderr.contains("auth login") || stderr.contains("not logged") {
                return Err(GhError::NotAuthenticated);
            }
            return Err(GhError::CommandFailed(stderr.to_string()));
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }
}

pub fn fetch_issues(runner: &dyn GhRunner, repo: &str) -> Result<Vec<Issue>> {
    let json = runner.run_gh(&[
        "issue", "list",
        "--repo", repo,
        "--state", "open",
        "--json", "number,title,labels,assignees,updatedAt,createdAt",
        "--limit", "200",
    ])?;
    Issue::parse_gh_list(&json, repo)
}

pub fn fetch_issue_detail(runner: &dyn GhRunner, repo: &str, number: u64) -> Result<IssueDetail> {
    let json = runner.run_gh(&[
        "issue", "view",
        &number.to_string(),
        "--repo", repo,
        "--json", "body,comments",
    ])?;
    IssueDetail::parse_gh_view(&json)
}
