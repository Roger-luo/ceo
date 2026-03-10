pub trait Prompt {
    fn render(&self) -> String;
    fn kind(&self) -> &str;
    /// Tools the agent must have access to for this prompt.
    fn required_tools(&self) -> &[&str] { &[] }
}

/// Summarize what an issue/PR is about (title, body, labels — no discussion).
/// Generated once and cached.
pub struct IssueDescriptionPrompt {
    pub repo: String,
    pub number: u64,
    pub title: String,
    pub kind: String,
    pub labels: String,
    pub assignees: String,
    pub body: String,
}

impl Prompt for IssueDescriptionPrompt {
    fn kind(&self) -> &str { "summary" }

    fn render(&self) -> String {
        format!(
            "Summarize what this GitHub {} is about in 1-2 sentences. \
             Focus on the purpose and scope — do NOT summarize any discussion or comments.\n\
             All information you need is provided below — do NOT attempt to fetch data \
             from GitHub or any external source.\n\
             When mentioning GitHub users, preserve the [@handle](https://github.com/handle) \
             link format used in the data below.\n\n\
             Repo: {}\n\
             {} #{}: {}\n\
             Labels: {}\n\
             Assignees: {}\n\n\
             Description:\n{}",
            self.kind, self.repo,
            if self.kind == "pr" { "PR" } else { "Issue" },
            self.number, self.title,
            self.labels, self.assignees,
            self.body
        )
    }
}

/// Summarize the discussion on an issue/PR.
/// When previous_summary is provided, the agent updates it incrementally.
pub struct DiscussionSummaryPrompt {
    pub repo: String,
    pub number: u64,
    pub title: String,
    pub comments: String,
    pub previous_summary: Option<String>,
}

impl Prompt for DiscussionSummaryPrompt {
    fn kind(&self) -> &str { "summary" }

    fn render(&self) -> String {
        let previous_section = match &self.previous_summary {
            Some(prev) => format!(
                "\n\nHere is the previous discussion summary. Update it with any new \
                 information from the comments below. Only modify what has changed — \
                 preserve existing content that is still accurate:\n{prev}"
            ),
            None => String::new(),
        };
        format!(
            "Summarize the discussion and recent activity on this GitHub issue/PR in 2-3 sentences.\n\
             All information you need is provided below — do NOT attempt to fetch data \
             from GitHub or any external source.\n\
             When mentioning GitHub users, preserve the [@handle](https://github.com/handle) \
             link format used in the data below.\n\n\
             Repo: {}\n\
             #{}: {}\n\n\
             Comments:\n{}{}\n\n\
             Focus on decisions made, blockers raised, and current status of the discussion.",
            self.repo, self.number, self.title,
            self.comments, previous_section
        )
    }
}

/// Aggregate per-issue summaries into a repo-level summary report.
pub struct WeeklySummaryPrompt {
    pub repo: String,
    pub issue_summaries: String,
    pub commit_log: String,
    pub previous_summary: Option<String>,
    pub initiatives: String,
}

impl Prompt for WeeklySummaryPrompt {
    fn kind(&self) -> &str { "summary" }

    fn render(&self) -> String {
        let commits_section = if self.commit_log.is_empty() {
            String::new()
        } else {
            format!(
                "\n\nRecent commits (may span multiple branches):\n{}",
                self.commit_log
            )
        };
        let previous_section = match &self.previous_summary {
            Some(prev) => format!(
                "\n\nHere is the previous report for this repo (use it as a baseline — \
                 update it with new information, or return similar content if nothing \
                 has meaningfully changed):\n{prev}"
            ),
            None => String::new(),
        };
        let initiatives_section = if self.initiatives.is_empty() {
            String::new()
        } else {
            format!(
                "\n\nThis repo is part of the following initiatives — frame your summary \
                 in terms of these goals where relevant:\n{}",
                self.initiatives
            )
        };
        format!(
            "Write a concise summary for repo {}.\n\
             All information you need is provided below — do NOT fetch external data.\n\
             Preserve [@handle](https://github.com/handle) link format.\n\n\
             Issue/PR summaries:\n{}{}{}{}\n\n\
             Respond ONLY with XML tags — no other text. Use these tags:\n\
             <done>1-2 sentences: what got completed or merged</done>\n\
             <in_progress>1-2 sentences: active work, open blockers</in_progress>\n\
             <next>1 sentence: what's planned (only if clear from data)</next>\n\n\
             Omit any tag entirely if there's nothing for that category.\n\
             Be direct — no filler words, no hedging.",
            self.repo, self.issue_summaries, commits_section, previous_section, initiatives_section
        )
    }
}

pub struct IssueTriagePrompt {
    pub title: String,
    pub body: String,
    pub comments: String,
}

impl Prompt for IssueTriagePrompt {
    fn kind(&self) -> &str { "triage" }

    fn render(&self) -> String {
        format!(
            "Analyze this GitHub issue. It lacks proper labels/status. \
             Summarize what the issue is about in 2-3 sentences and suggest \
             appropriate priority and status labels.\n\
             All information you need is provided below — do NOT attempt to \
             fetch data from GitHub or any external source.\n\n\
             Issue: {}\n\n\
             {}\n\n\
             Comments:\n{}",
            self.title, self.body, self.comments
        )
    }
}
