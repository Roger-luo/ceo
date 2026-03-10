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

    fn run(&self, ctx: &mut PipelineContext) -> Result<()> {
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
                TeamStats {
                    name: member.name.clone(),
                    github: member.github.clone(),
                    active,
                    closed_this_week,
                }
            })
            .collect();

        Ok(())
    }
}
