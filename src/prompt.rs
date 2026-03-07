pub trait Prompt {
    fn render(&self) -> String;
}

pub struct WeeklySummaryPrompt {
    pub repo: String,
    pub issue_summaries: String,
}

impl Prompt for WeeklySummaryPrompt {
    fn render(&self) -> String {
        format!(
            "Summarize the past week's progress for repo {}. \
             Here are the issues updated this week:\n\
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

impl Prompt for IssueTriagePrompt {
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
