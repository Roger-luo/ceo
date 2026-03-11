use std::future::Future;
use std::pin::Pin;

use crate::report::TeamStats;

use super::{PipelineContext, Result, Task};

pub struct TeamStatsTask;

impl Task for TeamStatsTask {
    fn name(&self) -> &str {
        "Team stats"
    }

    fn description(&self) -> &str {
        "Compute per-member active and closed issue counts"
    }

    fn step_count(&self, ctx: &PipelineContext) -> usize {
        ctx.config.team.len()
    }

    fn should_skip(&self, ctx: &PipelineContext) -> bool {
        ctx.config.team.is_empty()
    }

    fn run<'a, 'ctx>(&'a self, ctx: &'a mut PipelineContext<'ctx>) -> Pin<Box<dyn Future<Output = Result<()>> + 'a>>
    where 'ctx: 'a {
        Box::pin(async move {
        ctx.team_stats = ctx
            .config
            .team
            .iter()
            .map(|member| {
                let (active, closed_this_week) = ctx
                    .all_recent_issues
                    .iter()
                    .filter(|i| i.assignees.contains(&member.github))
                    .fold((0, 0), |(open, closed), i| {
                        if i.state.eq_ignore_ascii_case("OPEN") {
                            (open + 1, closed)
                        } else if i.state.eq_ignore_ascii_case("CLOSED") {
                            (open, closed + 1)
                        } else {
                            (open, closed)
                        }
                    });

                // Aggregate additions/deletions from merged commits
                let (mut additions, mut deletions) = ctx
                    .contributor_stats
                    .values()
                    .flat_map(|rows| rows.iter())
                    .filter(|row| row.author.eq_ignore_ascii_case(&member.github))
                    .fold((0i64, 0i64), |(a, d), row| {
                        (a + row.additions, d + row.deletions)
                    });

                // Add lines from open (unmerged) PRs authored by this member
                for issue in &ctx.all_recent_issues {
                    if issue.kind == "pr"
                        && issue.state.eq_ignore_ascii_case("OPEN")
                        && issue.author.as_deref() == Some(&member.github)
                    {
                        additions += issue.pr_additions.unwrap_or(0);
                        deletions += issue.pr_deletions.unwrap_or(0);
                    }
                }

                TeamStats {
                    name: member.name.clone(),
                    github: member.github.clone(),
                    active,
                    closed_this_week,
                    additions,
                    deletions,
                }
            })
            .collect();

        Ok(())
        })
    }
}
