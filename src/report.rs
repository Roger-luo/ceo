use std::collections::{HashMap, HashSet};
use std::fmt::Write;

/// Format a GitHub handle as a clickable markdown link.
pub fn github_link(handle: &str) -> String {
    format!("[@{handle}](https://github.com/{handle})")
}

/// Expand short GitHub reference tags in LLM output to full markdown links.
///
/// Tags:
/// - `<gh>handle</gh>` → `[@handle](https://github.com/handle)`
/// - `<issue>64</issue>` → `[#64](https://github.com/{repo}/issues/64)`
/// - `<issue>org/repo#64</issue>` → `[org/repo#64](https://github.com/org/repo/issues/64)`
/// - `<pr>32</pr>` → `[#32](https://github.com/{repo}/pull/32)`
/// - `<pr>org/repo#32</pr>` → `[org/repo#32](https://github.com/org/repo/pull/32)`
///
/// Qualified references (`owner/repo#N`) override the `repo` parameter.
pub fn expand_github_tags(text: &str, repo: &str) -> String {
    let mut result = text.to_string();
    // Process each tag type by scanning for open/close pairs
    for (open_tag, close_tag, fmt_fn) in [
        ("<gh>", "</gh>", Box::new(|inner: &str, _repo: &str| {
            format!("[@{inner}](https://github.com/{inner})")
        }) as Box<dyn Fn(&str, &str) -> String>),
        ("<issue>", "</issue>", Box::new(|inner: &str, repo: &str| {
            format_issue_or_pr_link(inner, repo, "issues")
        }) as Box<dyn Fn(&str, &str) -> String>),
        ("<pr>", "</pr>", Box::new(|inner: &str, repo: &str| {
            format_issue_or_pr_link(inner, repo, "pull")
        }) as Box<dyn Fn(&str, &str) -> String>),
    ] {
        loop {
            let Some(start) = result.find(open_tag) else { break };
            let after_open = start + open_tag.len();
            let Some(end_offset) = result[after_open..].find(close_tag) else { break };
            let inner = result[after_open..after_open + end_offset].trim();
            let replacement = fmt_fn(inner, repo);
            result.replace_range(start..after_open + end_offset + close_tag.len(), &replacement);
        }
    }
    result
}

/// Format a qualified or unqualified issue/PR reference as a markdown link.
///
/// - `"64"` + repo `"org/repo"` → `[#64](https://github.com/org/repo/issues/64)`
/// - `"org/repo#64"` (any repo) → `[org/repo#64](https://github.com/org/repo/issues/64)`
fn format_issue_or_pr_link(inner: &str, fallback_repo: &str, path: &str) -> String {
    if let Some((qualified_repo, number)) = inner.split_once('#') {
        if !qualified_repo.is_empty() && qualified_repo.contains('/') {
            return format!("[{inner}](https://github.com/{qualified_repo}/{path}/{number})");
        }
    }
    // Unqualified: just a number
    if fallback_repo.is_empty() {
        format!("#{inner}")
    } else {
        format!("[#{inner}](https://github.com/{fallback_repo}/{path}/{inner})")
    }
}

/// Extract all `<summary id="N">...</summary>` tags from a batch response.
/// Returns a vec of (issue_number, summary_text) tuples.
pub fn extract_all_summary_tags(text: &str) -> Vec<(u64, String)> {
    let mut results = Vec::new();
    let mut search_from = 0;
    let open_prefix = "<summary id=\"";

    while let Some(tag_start) = text[search_from..].find(open_prefix) {
        let abs_start = search_from + tag_start;
        let after_prefix = abs_start + open_prefix.len();

        let Some(quote_end) = text[after_prefix..].find('"') else { break };
        let id_str = &text[after_prefix..after_prefix + quote_end];
        let Ok(id) = id_str.parse::<u64>() else {
            search_from = after_prefix;
            continue;
        };

        let after_id = after_prefix + quote_end + 1;
        let Some(gt_offset) = text[after_id..].find('>') else { break };
        let content_start = after_id + gt_offset + 1;

        let close_tag = "</summary>";
        let Some(close_offset) = text[content_start..].find(close_tag) else { break };
        let content = text[content_start..content_start + close_offset].trim();
        if !content.is_empty() {
            results.push((id, content.to_string()));
        }

        search_from = content_start + close_offset + close_tag.len();
    }

    results
}

/// Extract the text content of an XML tag from a string.
/// Returns `None` if the tag is not found.
pub fn extract_xml_tag(text: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = text.find(&open).map(|i| i + open.len())?;
    let end = text[start..].find(&close).map(|i| i + start)?;
    let content = text[start..end].trim();
    if content.is_empty() { None } else { Some(content.to_string()) }
}

pub struct Report {
    pub date: String,
    pub executive_summary: Option<String>,
    pub repos: Vec<RepoSection>,
    pub team_stats: Vec<TeamStats>,
    /// Lookup table for linkifying #N and @user references in text.
    pub refs: RefLookup,
}

/// Lookup table for resolving bare `#N`, `repo#N`, `org/repo#N`, and `@user`
/// references into full GitHub links.
#[derive(Default)]
pub struct RefLookup {
    /// (full_repo, number) → "issue" or "pr"
    pub issues: HashMap<(String, u64), String>,
    /// Set of known GitHub handles (lowercase)
    pub users: HashSet<String>,
    /// All full repo names (e.g. "QuEraComputing/bloqade-lanes")
    pub repos: Vec<String>,
    /// Short repo name → full repo name (e.g. "bloqade-lanes" → "QuEraComputing/bloqade-lanes")
    pub short_to_full: HashMap<String, String>,
}

impl RefLookup {
    /// Build from issue rows returned by the pipeline.
    pub fn from_issue_rows(rows: &HashMap<String, Vec<crate::db::IssueRow>>) -> Self {
        let mut issues = HashMap::new();
        let mut users = HashSet::new();
        let repos: Vec<String> = rows.keys().cloned().collect();
        let mut short_to_full = HashMap::new();

        for (repo, repo_rows) in rows {
            // Map short name → full name (e.g. "bloqade-lanes" → "QuEraComputing/bloqade-lanes")
            if let Some(short) = repo.split('/').last() {
                short_to_full.insert(short.to_string(), repo.clone());
            }
            for row in repo_rows {
                issues.insert((repo.clone(), row.number), row.kind.clone());
                if let Some(author) = &row.author {
                    users.insert(author.to_lowercase());
                }
                // Parse assignees JSON array
                if let Ok(assignees) = serde_json::from_str::<Vec<String>>(&row.assignees) {
                    for a in assignees {
                        users.insert(a.to_lowercase());
                    }
                }
            }
        }

        Self { issues, users, repos, short_to_full }
    }

    /// Resolve a repo reference (short or full) to the full "org/repo" name.
    fn resolve_repo(&self, name: &str) -> Option<&str> {
        // Already a full name?
        if self.repos.iter().any(|r| r == name) {
            return Some(self.repos.iter().find(|r| r.as_str() == name).unwrap());
        }
        // Try short name
        self.short_to_full.get(name).map(|s| s.as_str())
    }
}

/// Replace bare `#N`, `org/repo#N`, and `@user` patterns in text with GitHub
/// markdown links, using the lookup table to determine whether references are
/// issues or PRs.
///
/// When `repo_context` is non-empty, bare `#N` resolves against that repo.
/// When empty, all configured repos are searched (first match wins).
pub fn linkify(text: &str, refs: &RefLookup, repo_context: &str) -> String {
    // Pre-pass: strip XML tags from old cached summaries into natural syntax.
    // <gh>handle</gh> → @handle, <issue>N</issue> → #N, <pr>N</pr> → #N
    let text = strip_xml_tags(text);

    let mut result = String::with_capacity(text.len());
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        // Skip anything inside existing markdown links [text](url) or <url|text>
        if chars[i] == '[' {
            if let Some(end) = find_markdown_link_end(&chars, i) {
                for &c in &chars[i..=end] {
                    result.push(c);
                }
                i = end + 1;
                continue;
            }
        }
        if chars[i] == '<' {
            if let Some(end) = find_angle_link_end(&chars, i) {
                for &c in &chars[i..=end] {
                    result.push(c);
                }
                i = end + 1;
                continue;
            }
        }

        // Try qualified ref: org/repo#N
        if chars[i].is_alphanumeric() || chars[i] == '-' || chars[i] == '_' {
            if let Some((link, advance)) = try_qualified_ref(&chars, i, refs) {
                result.push_str(&link);
                i += advance;
                continue;
            }
        }

        // Try bare #N (only if preceded by space, start of line, or punctuation)
        if chars[i] == '#' && (i == 0 || !chars[i - 1].is_alphanumeric()) {
            if let Some((link, advance)) = try_bare_ref(&chars, i, refs, repo_context) {
                result.push_str(&link);
                i += advance;
                continue;
            }
        }

        // Try @user (only if preceded by space, start of line, or punctuation)
        if chars[i] == '@' && (i == 0 || !chars[i - 1].is_alphanumeric()) {
            if let Some((link, advance)) = try_user_ref(&chars, i, refs) {
                result.push_str(&link);
                i += advance;
                continue;
            }
        }

        result.push(chars[i]);
        i += 1;
    }

    result
}

/// Strip XML reference tags from old cached summaries into natural syntax.
/// `<gh>handle</gh>` → `@handle`, `<issue>N</issue>` → `#N`, `<pr>N</pr>` → `#N`
fn strip_xml_tags(text: &str) -> String {
    let mut result = text.to_string();
    for (open, close, prefix) in [
        ("<gh>", "</gh>", "@"),
        ("<issue>", "</issue>", "#"),
        ("<pr>", "</pr>", "#"),
    ] {
        loop {
            let Some(start) = result.find(open) else { break };
            let after_open = start + open.len();
            let Some(end_offset) = result[after_open..].find(close) else { break };
            let inner = result[after_open..after_open + end_offset].trim().to_string();
            // If inner already has # (like "org/repo#42"), keep as-is; otherwise add prefix
            let replacement = if inner.contains('#') || prefix == "@" {
                format!("{prefix}{inner}")
            } else {
                format!("{prefix}{inner}")
            };
            result.replace_range(start..after_open + end_offset + close.len(), &replacement);
        }
    }
    result
}

/// Find the end of a markdown link `[text](url)` starting at `[`.
fn find_markdown_link_end(chars: &[char], start: usize) -> Option<usize> {
    let mut j = start + 1;
    while j < chars.len() && chars[j] != ']' {
        j += 1;
    }
    if j + 1 >= chars.len() || chars[j + 1] != '(' {
        return None;
    }
    j += 2;
    while j < chars.len() && chars[j] != ')' {
        j += 1;
    }
    if j < chars.len() { Some(j) } else { None }
}

/// Find the end of an angle-bracket link `<url|text>` starting at `<`.
fn find_angle_link_end(chars: &[char], start: usize) -> Option<usize> {
    // Must contain a pipe or "http" to be a link, not an XML tag
    let mut j = start + 1;
    let mut has_pipe = false;
    let mut has_http = false;
    while j < chars.len() && chars[j] != '>' {
        if chars[j] == '|' { has_pipe = true; }
        if chars[j] == 'h' && j + 4 < chars.len() {
            let s: String = chars[j..j + 4].iter().collect();
            if s == "http" { has_http = true; }
        }
        j += 1;
    }
    if j < chars.len() && (has_pipe || has_http) { Some(j) } else { None }
}

/// Try to parse `repo#N` or `org/repo#N` at position `i`.
fn try_qualified_ref(chars: &[char], i: usize, refs: &RefLookup) -> Option<(String, usize)> {
    // Scan for pattern: word#digits or word/word#digits
    let mut j = i;
    // First word part (could be owner or short repo name)
    while j < chars.len() && (chars[j].is_alphanumeric() || chars[j] == '-' || chars[j] == '_' || chars[j] == '.') {
        j += 1;
    }
    if j >= chars.len() { return None; }

    let hash;
    if chars[j] == '/' {
        // org/repo#N pattern
        j += 1;
        while j < chars.len() && (chars[j].is_alphanumeric() || chars[j] == '-' || chars[j] == '_' || chars[j] == '.') {
            j += 1;
        }
        if j >= chars.len() || chars[j] != '#' { return None; }
        hash = j;
    } else if chars[j] == '#' {
        // repo#N pattern (short name)
        hash = j;
    } else {
        return None;
    }

    j = hash + 1;
    // number part
    let num_start = j;
    while j < chars.len() && chars[j].is_ascii_digit() {
        j += 1;
    }
    if j == num_start { return None; }

    let ref_text: String = chars[i..hash].iter().collect();
    let num_str: String = chars[num_start..j].iter().collect();
    let number: u64 = num_str.parse().ok()?;

    // Resolve the repo name — could be "org/repo" or just "repo"
    let full_repo = refs.resolve_repo(&ref_text)?;

    let kind = refs.issues.get(&(full_repo.to_string(), number))?;
    let path = if kind == "pr" { "pull" } else { "issues" };
    let display: String = chars[i..j].iter().collect();
    Some((format!("[{display}](https://github.com/{full_repo}/{path}/{number})"), j - i))
}

/// Try to parse bare `#N` at position `i`.
fn try_bare_ref(chars: &[char], i: usize, refs: &RefLookup, repo_context: &str) -> Option<(String, usize)> {
    if chars[i] != '#' { return None; }
    let mut j = i + 1;
    let num_start = j;
    while j < chars.len() && chars[j].is_ascii_digit() {
        j += 1;
    }
    if j == num_start { return None; }
    let num_str: String = chars[num_start..j].iter().collect();
    let number: u64 = num_str.parse().ok()?;

    // Try repo context first
    if !repo_context.is_empty() {
        if let Some(kind) = refs.issues.get(&(repo_context.to_string(), number)) {
            let path = if kind == "pr" { "pull" } else { "issues" };
            return Some((format!("[#{number}](https://github.com/{repo_context}/{path}/{number})"), j - i));
        }
    }

    // Try all repos
    for repo in &refs.repos {
        if let Some(kind) = refs.issues.get(&(repo.clone(), number)) {
            let path = if kind == "pr" { "pull" } else { "issues" };
            return Some((format!("[{repo}#{number}](https://github.com/{repo}/{path}/{number})"), j - i));
        }
    }

    None
}

/// Try to parse `@username` at position `i`.
fn try_user_ref(chars: &[char], i: usize, refs: &RefLookup) -> Option<(String, usize)> {
    if chars[i] != '@' { return None; }
    let mut j = i + 1;
    while j < chars.len() && (chars[j].is_alphanumeric() || chars[j] == '-' || chars[j] == '_') {
        j += 1;
    }
    if j == i + 1 { return None; }
    let handle: String = chars[i + 1..j].iter().collect();

    if refs.users.contains(&handle.to_lowercase()) {
        Some((format!("[@{handle}](https://github.com/{handle})"), j - i))
    } else {
        None
    }
}

pub struct RepoSection {
    pub name: String,
    pub done: Option<String>,
    pub in_progress: Option<String>,
    pub next: Option<String>,
    pub flagged_issues: Vec<FlaggedIssue>,
}

impl RepoSection {
    pub fn has_activity(&self) -> bool {
        self.done.is_some() || self.in_progress.is_some() || self.next.is_some()
    }
}

pub struct FlaggedIssue {
    pub number: u64,
    pub title: String,
    pub missing_labels: Vec<String>,
    pub summary: String,
}

pub struct TeamStats {
    pub name: String,
    pub github: String,
    pub active: usize,
    pub closed_this_week: usize,
    pub additions: i64,
    pub deletions: i64,
}

pub fn render_markdown(report: &Report) -> String {
    let mut out = String::new();
    writeln!(out, "# Project Report — {}\n", report.date).unwrap();

    // Executive summary (already linkified by pipeline)
    if let Some(summary) = &report.executive_summary {
        writeln!(out, "{}\n", summary).unwrap();
        writeln!(out, "---\n").unwrap();
    }

    // Split repos into active and inactive
    let active: Vec<&RepoSection> = report.repos.iter()
        .filter(|r| r.has_activity())
        .collect();
    let inactive: Vec<&RepoSection> = report.repos.iter()
        .filter(|r| !r.has_activity())
        .collect();

    for repo in &active {
        writeln!(out, "## {}\n", repo.name).unwrap();
        if let Some(done) = &repo.done {
            writeln!(out, "**Done:** {done}\n").unwrap();
        }
        if let Some(ip) = &repo.in_progress {
            writeln!(out, "**In Progress:** {ip}\n").unwrap();
        }
        if let Some(next) = &repo.next {
            writeln!(out, "**Next:** {next}\n").unwrap();
        }

        if !repo.flagged_issues.is_empty() {
            writeln!(out, "**Needs Attention:**\n").unwrap();
            for issue in &repo.flagged_issues {
                let missing = issue.missing_labels.join(", ");
                writeln!(
                    out,
                    "- **#{}** \"{}\" — missing {} label. *{}*",
                    issue.number, issue.title, missing, issue.summary
                ).unwrap();
            }
            writeln!(out).unwrap();
        }
    }

    // Inactive repos as a compact list
    if !inactive.is_empty() {
        writeln!(out, "## No Recent Activity\n").unwrap();
        for repo in &inactive {
            writeln!(out, "- {}", repo.name).unwrap();
        }
        writeln!(out).unwrap();
    }

    if !report.team_stats.is_empty() {
        // Filter out team members with zero activity
        let active_members: Vec<&TeamStats> = report.team_stats.iter()
            .filter(|m| m.active > 0 || m.closed_this_week > 0 || m.additions > 0 || m.deletions > 0)
            .collect();
        let inactive_members: Vec<&TeamStats> = report.team_stats.iter()
            .filter(|m| m.active == 0 && m.closed_this_week == 0 && m.additions == 0 && m.deletions == 0)
            .collect();

        writeln!(out, "## Team Overview\n").unwrap();
        writeln!(out, "| Person | Active | Closed | Lines |").unwrap();
        writeln!(out, "|--------|--------|--------|-------|").unwrap();
        for member in &active_members {
            writeln!(
                out,
                "| {} ({}) | {} | {} | +{} / -{} |",
                member.name, github_link(&member.github),
                member.active, member.closed_this_week,
                member.additions, member.deletions
            ).unwrap();
        }
        if !inactive_members.is_empty() {
            let names: Vec<String> = inactive_members.iter()
                .map(|m| format!("{} ({})", m.name, github_link(&m.github)))
                .collect();
            writeln!(out, "\n*No activity:* {}", names.join(", ")).unwrap();
        }
        writeln!(out).unwrap();
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_refs() -> RefLookup {
        let mut refs = RefLookup::default();
        refs.issues.insert(("org/repo".into(), 42), "pr".into());
        refs.issues.insert(("org/repo".into(), 55), "issue".into());
        refs.issues.insert(("acme/frontend".into(), 15), "issue".into());
        refs.issues.insert(("acme/backend".into(), 50), "pr".into());
        refs.users.insert("alice".into());
        refs.users.insert("bob".into());
        refs.repos = vec!["org/repo".into(), "acme/frontend".into(), "acme/backend".into()];
        refs.short_to_full.insert("repo".into(), "org/repo".into());
        refs.short_to_full.insert("frontend".into(), "acme/frontend".into());
        refs.short_to_full.insert("backend".into(), "acme/backend".into());
        refs
    }

    #[test]
    fn linkify_bare_issue_with_repo_context() {
        let refs = test_refs();
        let result = linkify("Fixed #55 today", &refs, "org/repo");
        assert_eq!(result, "Fixed [#55](https://github.com/org/repo/issues/55) today");
    }

    #[test]
    fn linkify_bare_pr_with_repo_context() {
        let refs = test_refs();
        let result = linkify("Merged #42", &refs, "org/repo");
        assert_eq!(result, "Merged [#42](https://github.com/org/repo/pull/42)");
    }

    #[test]
    fn linkify_qualified_ref() {
        let refs = test_refs();
        let result = linkify("See acme/backend#50 for details", &refs, "");
        assert_eq!(result, "See [acme/backend#50](https://github.com/acme/backend/pull/50) for details");
    }

    #[test]
    fn linkify_user_ref() {
        let refs = test_refs();
        let result = linkify("Thanks @alice!", &refs, "");
        assert_eq!(result, "Thanks [@alice](https://github.com/alice)!");
    }

    #[test]
    fn linkify_unknown_user_left_alone() {
        let refs = test_refs();
        let result = linkify("@unknown did stuff", &refs, "");
        assert_eq!(result, "@unknown did stuff");
    }

    #[test]
    fn linkify_unknown_number_left_alone() {
        let refs = test_refs();
        let result = linkify("See #999", &refs, "org/repo");
        assert_eq!(result, "See #999");
    }

    #[test]
    fn linkify_bare_ref_no_context_searches_all_repos() {
        let refs = test_refs();
        // #42 exists in org/repo — should find it even without context
        let result = linkify("PR #42 landed", &refs, "");
        assert!(result.contains("[org/repo#42](https://github.com/org/repo/pull/42)"));
    }

    #[test]
    fn linkify_skips_existing_markdown_links() {
        let refs = test_refs();
        let result = linkify("[#42](https://example.com) and #55", &refs, "org/repo");
        assert!(result.starts_with("[#42](https://example.com)"));
        assert!(result.contains("[#55](https://github.com/org/repo/issues/55)"));
    }

    #[test]
    fn linkify_mixed() {
        let refs = test_refs();
        let text = "@alice fixed org/repo#42 and #55 is next";
        let result = linkify(text, &refs, "org/repo");
        assert!(result.contains("[@alice](https://github.com/alice)"));
        assert!(result.contains("[org/repo#42](https://github.com/org/repo/pull/42)"));
        assert!(result.contains("[#55](https://github.com/org/repo/issues/55)"));
    }

    #[test]
    fn linkify_short_repo_name() {
        let refs = test_refs();
        let result = linkify("See backend#50 for details", &refs, "");
        assert_eq!(result, "See [backend#50](https://github.com/acme/backend/pull/50) for details");
    }

    #[test]
    fn linkify_short_repo_in_sentence() {
        let refs = test_refs();
        let result = linkify("frontend#15 needs triage, backend#50 is done", &refs, "");
        assert!(result.contains("[frontend#15](https://github.com/acme/frontend/issues/15)"));
        assert!(result.contains("[backend#50](https://github.com/acme/backend/pull/50)"));
    }

    #[test]
    fn linkify_xml_tags_backward_compat() {
        let refs = test_refs();
        let result = linkify("<gh>alice</gh> fixed <pr>42</pr> and <issue>55</issue>", &refs, "org/repo");
        assert!(result.contains("[@alice](https://github.com/alice)"));
        assert!(result.contains("[#42](https://github.com/org/repo/pull/42)"));
        assert!(result.contains("[#55](https://github.com/org/repo/issues/55)"));
    }

    #[test]
    fn linkify_xml_tags_with_qualified_refs() {
        let refs = test_refs();
        let result = linkify("<pr>acme/backend#50</pr>", &refs, "");
        // XML stripped to #acme/backend#50, then qualified ref resolves it
        assert!(result.contains("[acme/backend#50](https://github.com/acme/backend/pull/50)"));
    }

    // Keep expand_github_tags tests for backward compat with cached summaries
    #[test]
    fn expand_tags_still_works() {
        let result = expand_github_tags("<gh>alice</gh> fixed <pr>42</pr>", "org/repo");
        assert!(result.contains("[@alice](https://github.com/alice)"));
        assert!(result.contains("[#42](https://github.com/org/repo/pull/42)"));
    }
}
