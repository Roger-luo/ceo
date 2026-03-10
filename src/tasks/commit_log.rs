use std::collections::BTreeMap;

use crate::report::github_link;

use super::{PipelineContext, Result, Task};

pub struct BuildCommitLogTask;

impl Task for BuildCommitLogTask {
    fn name(&self) -> &str {
        "Build commit logs"
    }

    fn description(&self) -> &str {
        "Building per-repo commit logs grouped by branch"
    }

    fn step_count(&self, ctx: &PipelineContext) -> usize {
        ctx.config.repos.len()
    }

    fn should_skip(&self, _ctx: &PipelineContext) -> bool {
        false
    }

    fn run(&self, ctx: &mut PipelineContext) -> Result<()> {
        for repo_config in &ctx.config.repos {
            let commit_rows = ctx.commit_rows.get(&repo_config.name);
            let commit_log = match commit_rows {
                Some(rows) if !rows.is_empty() => {
                    let mut by_branch: BTreeMap<&str, Vec<String>> = BTreeMap::new();
                    for c in rows {
                        let short_sha = &c.sha[..c.sha.len().min(7)];
                        let first_line = c.message.lines().next().unwrap_or("");
                        let line = format!(
                            "- {} {}: {}",
                            short_sha,
                            github_link(&c.author),
                            first_line
                        );
                        let branch_key = if c.branch.is_empty() {
                            "default"
                        } else {
                            &c.branch
                        };
                        by_branch.entry(branch_key).or_default().push(line);
                    }
                    if by_branch.len() <= 1 {
                        // Single branch (or no commits): flat list, no headers
                        by_branch
                            .into_values()
                            .flatten()
                            .collect::<Vec<_>>()
                            .join("\n")
                    } else {
                        // Multiple branches: add headers
                        by_branch
                            .into_iter()
                            .map(|(branch, lines)| {
                                format!("[{branch}]\n{}", lines.join("\n"))
                            })
                            .collect::<Vec<_>>()
                            .join("\n\n")
                    }
                }
                _ => String::new(),
            };

            ctx.commit_logs
                .insert(repo_config.name.clone(), commit_log);
        }

        Ok(())
    }
}
