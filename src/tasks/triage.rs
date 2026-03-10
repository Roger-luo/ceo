use std::future::Future;
use std::pin::Pin;

use log::debug;

use crate::filter;
use crate::prompt::IssueTriagePrompt;
use crate::report::{github_link, FlaggedIssue};

use super::{PipelineContext, Result, Task};

pub struct TriageTask;

impl Task for TriageTask {
    fn name(&self) -> &str {
        "Triage flagged issues"
    }

    fn description(&self) -> &str {
        "Triaging issues missing required labels"
    }

    fn step_count(&self, ctx: &PipelineContext) -> usize {
        ctx.config
            .repos
            .iter()
            .map(|repo_config| {
                let issues = match ctx.issues.get(&repo_config.name) {
                    Some(issues) => issues,
                    None => return 0,
                };
                let issue_refs: Vec<_> = issues.iter().collect();
                filter::find_flagged_issues(&issue_refs, &repo_config.labels_required).len()
            })
            .sum()
    }

    fn should_skip(&self, ctx: &PipelineContext) -> bool {
        ctx.config
            .repos
            .iter()
            .all(|repo_config| repo_config.labels_required.is_empty())
    }

    fn run<'a, 'ctx>(&'a self, ctx: &'a mut PipelineContext<'ctx>) -> Pin<Box<dyn Future<Output = Result<()>> + 'a>>
    where 'ctx: 'a {
        Box::pin(async move {
        for (section_idx, repo_config) in ctx.config.repos.iter().enumerate() {
            let repo_name = &repo_config.name;
            let issues = match ctx.issues.get(repo_name) {
                Some(issues) => issues.clone(),
                None => continue,
            };

            let issue_refs: Vec<_> = issues.iter().collect();
            let flagged_refs =
                filter::find_flagged_issues(&issue_refs, &repo_config.labels_required);
            debug!(
                "Found {} flagged issues in {}",
                flagged_refs.len(),
                repo_name
            );

            let mut flagged_issues = Vec::new();

            for issue in &flagged_refs {
                debug!("Triaging issue #{}: {}", issue.number, issue.title);

                let body = ctx
                    .issue_bodies
                    .get(&(repo_name.clone(), issue.number))
                    .cloned()
                    .unwrap_or_default();

                let comments_text = ctx
                    .comments
                    .get(&(repo_name.clone(), issue.number))
                    .map(|v| {
                        v.iter()
                            .map(|c| format!("{}: {}", github_link(&c.author), c.body))
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
                    .unwrap_or_default();

                let triage_prompt = IssueTriagePrompt {
                    title: issue.title.clone(),
                    body,
                    comments: comments_text,
                };
                let triage_summary = ctx.agent.invoke(&triage_prompt).await.map_err(|e| {
                    eprintln!("  Error triaging #{}: {e}", issue.number);
                    e
                })?;

                flagged_issues.push(FlaggedIssue {
                    number: issue.number,
                    title: issue.title.clone(),
                    missing_labels: issue.missing_labels(&repo_config.labels_required),
                    summary: triage_summary,
                });
            }

            if let Some(section) = ctx.repo_sections.get_mut(section_idx) {
                section.flagged_issues = flagged_issues;
            }
        }

        Ok(())
        })
    }
}
