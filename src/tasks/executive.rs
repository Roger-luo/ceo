use std::fmt::Write;

use crate::error::AgentError;
use crate::prompt::ExecutiveSummaryPrompt;

use super::{PipelineContext, PipelineError, Result, Task};

pub struct ExecutiveSummaryTask;

impl Task for ExecutiveSummaryTask {
    fn name(&self) -> &str {
        "Executive Summary"
    }

    fn description(&self) -> &str {
        "Generate a cross-repo executive summary from a template"
    }

    fn step_count(&self, _ctx: &PipelineContext) -> usize {
        1
    }

    fn should_skip(&self, ctx: &PipelineContext) -> bool {
        ctx.template.is_none()
    }

    fn run(&self, ctx: &mut PipelineContext) -> Result<()> {
        let template_name = match &ctx.template {
            Some(name) => name.clone(),
            None => return Ok(()),
        };

        let template_text = crate::prompt::resolve_template(&template_name).ok_or_else(|| {
            PipelineError::Agent(AgentError::ExitError(format!(
                "Unknown template: {template_name}"
            )))
        })?;

        let mut repo_text = String::new();
        for section in &ctx.repo_sections {
            writeln!(repo_text, "## {}", section.name).unwrap();
            if let Some(done) = &section.done {
                writeln!(repo_text, "Done: {done}").unwrap();
            }
            if let Some(ip) = &section.in_progress {
                writeln!(repo_text, "In Progress: {ip}").unwrap();
            }
            if let Some(next) = &section.next {
                writeln!(repo_text, "Next: {next}").unwrap();
            }
            writeln!(repo_text).unwrap();
        }

        let prompt = ExecutiveSummaryPrompt {
            repo_summaries: repo_text,
            template: template_text,
        };
        let summary = ctx.agent.invoke(&prompt).map_err(|e| {
            eprintln!("  Error generating executive summary: {e}");
            e
        })?;

        ctx.executive_summary = Some(summary);

        Ok(())
    }
}
