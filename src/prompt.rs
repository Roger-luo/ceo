pub trait Prompt {
    fn render(&self) -> String;
    fn kind(&self) -> &str;
    /// Tools the agent must have access to for this prompt.
    fn required_tools(&self) -> &[&str] { &[] }
}

/// Summarize a single issue's recent activity.
pub struct IssueSummaryPrompt {
    pub repo: String,
    pub number: u64,
    pub title: String,
    pub labels: String,
    pub assignees: String,
    pub body: String,
    pub comments: String,
}

impl Prompt for IssueSummaryPrompt {
    fn kind(&self) -> &str { "summary" }

    fn render(&self) -> String {
        format!(
            "Summarize the recent activity on this GitHub issue in 2-3 sentences.\n\n\
             Repo: {}\n\
             Issue #{}: {}\n\
             Labels: {}\n\
             Assignees: {}\n\n\
             Description:\n{}\n\n\
             Recent comments:\n{}",
            self.repo, self.number, self.title,
            self.labels, self.assignees,
            self.body, self.comments
        )
    }
}

/// Aggregate per-issue summaries into a repo-level weekly report.
pub struct WeeklySummaryPrompt {
    pub repo: String,
    pub issue_summaries: String,
}

impl Prompt for WeeklySummaryPrompt {
    fn kind(&self) -> &str { "summary" }

    fn render(&self) -> String {
        format!(
            "Summarize the past week's progress for repo {}. \
             Here are summaries of each active issue:\n\
             {}\n\n\
             Provide:\n\
             1) Key progress and completed work\n\
             2) Big updates or decisions\n\
             3) What people are planning to work on next",
            self.repo, self.issue_summaries
        )
    }
}

pub struct IssueTriagePrompt {
    pub title: String,
    pub body: String,
    pub comments: String,
}

const TRIAGE_TOOLS: &[&str] = &["Bash(gh:*)"];

impl Prompt for IssueTriagePrompt {
    fn kind(&self) -> &str { "triage" }
    fn required_tools(&self) -> &[&str] { TRIAGE_TOOLS }

    fn render(&self) -> String {
        format!(
            "Analyze this GitHub issue. It lacks proper labels/status. \
             Summarize what the issue is about in 2-3 sentences and suggest \
             appropriate priority and status labels.\n\n\
             Issue: {}\n\n\
             {}\n\n\
             Comments:\n{}",
            self.title, self.body, self.comments
        )
    }
}
