use anyhow::{bail, Context, Result};
use log::debug;
use serde_json::{json, Value};

use crate::config::SlackConfig;
use crate::report::{expand_github_tags, Report, TeamStats};

const ENV_WEBHOOK: &str = "CEO_SLACK_WEBHOOK";
const ENV_TOKEN: &str = "CEO_SLACK_TOKEN";

/// Resolve the Slack webhook URL: env var takes precedence over config.
fn resolve_webhook_url(config: Option<&SlackConfig>) -> Result<String> {
    if let Ok(url) = std::env::var(ENV_WEBHOOK) {
        if !url.is_empty() {
            return Ok(url);
        }
    }
    if let Some(url) = config.and_then(|c| c.webhook_url.as_deref()) {
        if !url.is_empty() {
            return Ok(url.to_string());
        }
    }
    bail!("Slack webhook URL not configured. Set ${ENV_WEBHOOK} or add webhook_url under [slack] in config.toml")
}

/// Resolve the Slack bot token (optional): env var takes precedence over config.
fn resolve_bot_token(config: Option<&SlackConfig>) -> Option<String> {
    if let Ok(tok) = std::env::var(ENV_TOKEN) {
        if !tok.is_empty() {
            return Some(tok);
        }
    }
    config.and_then(|c| c.bot_token.as_deref())
        .filter(|t| !t.is_empty())
        .map(|t| t.to_string())
}

/// Send a report to Slack.
///
/// - With a bot token: posts a summary message, then uploads the full markdown
///   report as a file in a thread reply.
/// - Without a bot token (webhook only): posts a single well-formatted message
///   using Block Kit.
pub async fn send_report(
    report: &Report,
    markdown: &str,
    slack_config: Option<&SlackConfig>,
) -> Result<()> {
    let client = reqwest::Client::new();
    let channel = slack_config.and_then(|c| c.channel.as_deref());

    if let Some(token) = resolve_bot_token(slack_config) {
        send_threaded(&client, &token, channel, report, markdown).await
    } else {
        let url = resolve_webhook_url(slack_config)?;
        send_webhook(&client, &url, report).await
    }
}

// ============================================================================
// Webhook path: single well-formatted message
// ============================================================================

async fn send_webhook(client: &reqwest::Client, url: &str, report: &Report) -> Result<()> {
    let blocks = build_report_blocks(report);
    let payload = json!({ "blocks": blocks });

    debug!(
        "Sending Slack webhook ({} blocks)",
        blocks.len()
    );

    let resp = client
        .post(url)
        .json(&payload)
        .send()
        .await
        .context("Failed to send Slack webhook")?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        bail!("Slack webhook returned {status}: {body}");
    }
    Ok(())
}

// ============================================================================
// Bot token path: summary + threaded file upload
// ============================================================================

async fn send_threaded(
    client: &reqwest::Client,
    token: &str,
    channel: Option<&str>,
    report: &Report,
    markdown: &str,
) -> Result<()> {
    let channel = channel
        .ok_or_else(|| anyhow::anyhow!("slack.channel is required when using a bot token"))?;

    // 1. Post summary message
    let blocks = build_summary_blocks(report);
    let msg_payload = json!({
        "channel": channel,
        "blocks": blocks,
        "text": format!("Project Report — {}", report.date), // fallback for notifications
    });

    debug!("Posting summary to {channel} ({} blocks)", blocks.len());

    let resp = client
        .post("https://slack.com/api/chat.postMessage")
        .bearer_auth(token)
        .json(&msg_payload)
        .send()
        .await
        .context("Failed to post Slack message")?;

    let body: Value = resp.json().await.context("Failed to parse Slack response")?;
    if body["ok"].as_bool() != Some(true) {
        bail!("Slack chat.postMessage failed: {}", body["error"].as_str().unwrap_or("unknown"));
    }

    let thread_ts = body["ts"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Slack response missing ts field"))?;

    // 2. Upload full report as a .md snippet in the thread
    upload_file(client, token, channel, thread_ts, markdown, report).await?;

    Ok(())
}

async fn upload_file(
    client: &reqwest::Client,
    token: &str,
    channel: &str,
    thread_ts: &str,
    markdown: &str,
    report: &Report,
) -> Result<()> {
    let filename = format!("report-{}.md", report.date);

    // Step 1: Get upload URL
    let resp = client
        .get("https://slack.com/api/files.getUploadURLExternal")
        .bearer_auth(token)
        .query(&[
            ("filename", filename.as_str()),
            ("length", &markdown.len().to_string()),
        ])
        .send()
        .await
        .context("Failed to get Slack upload URL")?;

    let body: Value = resp.json().await?;
    if body["ok"].as_bool() != Some(true) {
        bail!("files.getUploadURLExternal failed: {}", body["error"].as_str().unwrap_or("unknown"));
    }

    let upload_url = body["upload_url"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing upload_url in response"))?;
    let file_id = body["file_id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing file_id in response"))?;

    // Step 2: Upload content
    client
        .post(upload_url)
        .body(markdown.to_string())
        .send()
        .await
        .context("Failed to upload file content")?;

    // Step 3: Complete upload, attach to thread
    let complete_payload = json!({
        "files": [{ "id": file_id, "title": format!("Full Report — {}", report.date) }],
        "channel_id": channel,
        "thread_ts": thread_ts,
    });

    let resp = client
        .post("https://slack.com/api/files.completeUploadExternal")
        .bearer_auth(token)
        .json(&complete_payload)
        .send()
        .await
        .context("Failed to complete file upload")?;

    let body: Value = resp.json().await?;
    if body["ok"].as_bool() != Some(true) {
        bail!("files.completeUploadExternal failed: {}", body["error"].as_str().unwrap_or("unknown"));
    }

    debug!("Uploaded report as {filename} in thread");
    Ok(())
}

// ============================================================================
// Block Kit builders
// ============================================================================

/// Build a complete Block Kit message from a Report (for webhook, single message).
fn build_report_blocks(report: &Report) -> Vec<Value> {
    let mut blocks: Vec<Value> = Vec::new();

    // Header
    blocks.push(header_block(&format!("Project Report — {}", report.date)));

    // Executive summary
    if let Some(summary) = &report.executive_summary {
        let text = convert_markdown(&expand_github_tags(summary, ""));
        // Chunk if needed (3000 char limit per section)
        for chunk in chunk_text(&text, 3000) {
            blocks.push(section_block(&chunk));
        }
        blocks.push(divider());
    }

    // Active repos
    let active: Vec<_> = report.repos.iter().filter(|r| r.has_activity()).collect();
    let inactive: Vec<_> = report.repos.iter().filter(|r| !r.has_activity()).collect();

    for repo in &active {
        blocks.push(section_block(&format!("*{}*", repo.name)));
        let mut details = String::new();
        if let Some(done) = &repo.done {
            details.push_str("*Done:*\n");
            details.push_str(&convert_markdown(&expand_github_tags(done, &repo.name)));
        }
        if let Some(ip) = &repo.in_progress {
            details.push_str("*In Progress:*\n");
            details.push_str(&convert_markdown(&expand_github_tags(ip, &repo.name)));
        }
        if let Some(next) = &repo.next {
            details.push_str("*Next:*\n");
            details.push_str(&convert_markdown(&expand_github_tags(next, &repo.name)));
        }
        if !repo.flagged_issues.is_empty() {
            details.push_str("*Needs Attention:*\n");
            for issue in &repo.flagged_issues {
                let missing = issue.missing_labels.join(", ");
                details.push_str(&format!(
                    "• *#{}* \"{}\" — missing {} label. _{}_\n",
                    issue.number,
                    issue.title,
                    missing,
                    convert_inline_spans(&expand_github_tags(&issue.summary, &repo.name))
                ));
            }
        }
        if !details.is_empty() {
            for chunk in chunk_text(&details, 3000) {
                blocks.push(section_block(&chunk));
            }
        }
    }

    // Inactive repos
    if !inactive.is_empty() {
        let names: Vec<_> = inactive.iter().map(|r| r.name.as_str()).collect();
        blocks.push(context_block(&format!(
            "No recent activity: {}",
            names.join(", ")
        )));
    }

    // Team stats
    if !report.team_stats.is_empty() {
        blocks.push(divider());
        blocks.push(section_block("*Team Overview*"));
        build_team_blocks(&mut blocks, &report.team_stats);
    }

    // Footer
    blocks.push(context_block(&format!(
        "Generated by ceo-cli on {}",
        report.date
    )));

    // Slack max is 50 blocks
    blocks.truncate(50);
    blocks
}

/// Build a summary-only Block Kit message (for bot token path — parent message).
fn build_summary_blocks(report: &Report) -> Vec<Value> {
    let mut blocks: Vec<Value> = Vec::new();

    blocks.push(header_block(&format!("Project Report — {}", report.date)));

    if let Some(summary) = &report.executive_summary {
        let text = convert_markdown(&expand_github_tags(summary, ""));
        for chunk in chunk_text(&text, 3000) {
            blocks.push(section_block(&chunk));
        }
    } else {
        // No executive summary — build a quick overview
        let active_count = report.repos.iter().filter(|r| r.has_activity()).count();
        let total = report.repos.len();
        blocks.push(section_block(&format!(
            "{active_count} of {total} repos had activity this period."
        )));
    }

    // Compact team stats
    if !report.team_stats.is_empty() {
        blocks.push(divider());
        blocks.push(section_block("*Team Overview*"));
        build_team_blocks(&mut blocks, &report.team_stats);
    }

    blocks.push(context_block("_Full report attached in thread_ :thread:"));

    blocks.truncate(50);
    blocks
}

/// Render team stats using Block Kit fields (2-column layout).
fn build_team_blocks(blocks: &mut Vec<Value>, stats: &[TeamStats]) {
    let active: Vec<_> = stats
        .iter()
        .filter(|m| m.active > 0 || m.closed_this_week > 0 || m.additions > 0 || m.deletions > 0)
        .collect();
    let inactive: Vec<_> = stats
        .iter()
        .filter(|m| m.active == 0 && m.closed_this_week == 0 && m.additions == 0 && m.deletions == 0)
        .collect();

    if !active.is_empty() {
        // Build a code-block table for clean alignment
        let mut table = String::from("```\n");
        table.push_str(&format!(
            "{:<20} {:>6} {:>6} {:>12}\n",
            "Person", "Active", "Closed", "Lines"
        ));
        table.push_str(&format!("{}\n", "─".repeat(48)));
        for m in &active {
            table.push_str(&format!(
                "{:<20} {:>6} {:>6}  +{:<5} -{:<5}\n",
                format!("{} @{}", m.name, m.github),
                m.active,
                m.closed_this_week,
                m.additions,
                m.deletions
            ));
        }
        table.push_str("```");

        for chunk in chunk_text(&table, 3000) {
            blocks.push(section_block(&chunk));
        }
    }

    if !inactive.is_empty() {
        let names: Vec<_> = inactive
            .iter()
            .map(|m| format!("{} (@{})", m.name, m.github))
            .collect();
        blocks.push(context_block(&format!("No activity: {}", names.join(", "))));
    }
}

// ============================================================================
// Block Kit primitives
// ============================================================================

fn header_block(text: &str) -> Value {
    json!({
        "type": "header",
        "text": { "type": "plain_text", "text": &text[..text.len().min(150)] }
    })
}

fn section_block(text: &str) -> Value {
    json!({
        "type": "section",
        "text": { "type": "mrkdwn", "text": text }
    })
}

fn divider() -> Value {
    json!({ "type": "divider" })
}

fn context_block(text: &str) -> Value {
    json!({
        "type": "context",
        "elements": [{ "type": "mrkdwn", "text": text }]
    })
}

/// Split text into chunks of at most `max` characters, breaking at newlines.
fn chunk_text(text: &str, max: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();

    for line in text.lines() {
        if !current.is_empty() && current.len() + line.len() + 1 > max {
            chunks.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push('\n');
        }
        current.push_str(line);
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    if chunks.is_empty() {
        chunks.push(String::new());
    }
    chunks
}

// ============================================================================
// Markdown → Slack mrkdwn conversion
// ============================================================================

/// Convert a block of GitHub-flavored markdown to Slack mrkdwn.
///
/// Handles:
/// - `# / ## / ###` headings → `*bold*`
/// - `**bold**` → `*bold*`
/// - `- item` / `* item` → `• item`
/// - `[text](url)` → `<url|text>`
/// - `---` → divider line
fn convert_markdown(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        let trimmed = line.trim();
        // Count leading spaces on original line for nesting detection
        let indent = line.len() - line.trim_start().len();

        // Headings
        if let Some(rest) = trimmed.strip_prefix("### ") {
            out.push_str(&format!("*{}*\n", convert_inline_spans(rest.trim())));
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("## ") {
            out.push_str(&format!("*{}*\n", convert_inline_spans(rest.trim())));
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("# ") {
            out.push_str(&format!("*{}*\n", convert_inline_spans(rest.trim())));
            continue;
        }

        // Horizontal rules
        if trimmed == "---" || trimmed == "***" || trimmed == "___" {
            out.push_str("───────────────────────\n");
            continue;
        }

        // List items: detect nesting by indentation
        if let Some(rest) = trimmed.strip_prefix("- ").or_else(|| trimmed.strip_prefix("* ")) {
            if indent >= 2 {
                out.push_str(&format!("    ◦ {}\n", convert_inline_spans(rest)));
            } else {
                out.push_str(&format!("• {}\n", convert_inline_spans(rest)));
            }
            continue;
        }

        // Regular line — convert inline spans
        out.push_str(&convert_inline_spans(trimmed));
        out.push('\n');
    }
    out
}

/// Convert inline markdown spans to Slack mrkdwn:
/// - `**bold**` → `*bold*`
/// - `[text](url)` → `<url|text>`
fn convert_inline_spans(line: &str) -> String {
    let mut result = line.to_string();

    // Bold: **text** → *text*
    while let Some(start) = result.find("**") {
        let after = start + 2;
        if let Some(end) = result[after..].find("**") {
            let inner = result[after..after + end].to_string();
            result = format!("{}*{}*{}", &result[..start], inner, &result[after + end + 2..]);
        } else {
            break;
        }
    }

    // Links: [text](url) → <url|text>
    convert_links(&mut result);

    result
}

fn convert_links(text: &mut String) {
    loop {
        let Some(bracket_start) = text.find('[') else {
            break;
        };
        let search_from = bracket_start + 1;
        let Some(bracket_end_offset) = text[search_from..].find("](") else {
            break;
        };
        let bracket_end = search_from + bracket_end_offset;
        let url_start = bracket_end + 2;
        let Some(paren_end_offset) = text[url_start..].find(')') else {
            break;
        };
        let paren_end = url_start + paren_end_offset;

        let link_text = text[search_from..bracket_end].to_string();
        let url = text[url_start..paren_end].to_string();

        let replacement = format!("<{url}|{link_text}>");
        text.replace_range(bracket_start..paren_end + 1, &replacement);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::{FlaggedIssue, RepoSection};

    fn sample_report() -> Report {
        Report {
            date: "2026-03-19".to_string(),
            executive_summary: Some("All systems operational. Key highlights this week.".to_string()),
            repos: vec![
                RepoSection {
                    name: "org/frontend".to_string(),
                    done: Some("Shipped new dashboard".to_string()),
                    in_progress: Some("Working on auth flow".to_string()),
                    next: None,
                    flagged_issues: vec![],
                },
                RepoSection {
                    name: "org/backend".to_string(),
                    done: None,
                    in_progress: None,
                    next: None,
                    flagged_issues: vec![],
                },
            ],
            team_stats: vec![
                TeamStats {
                    name: "Alice".to_string(),
                    github: "alice".to_string(),
                    active: 3,
                    closed_this_week: 2,
                    additions: 150,
                    deletions: 40,
                },
            ],
        }
    }

    #[test]
    fn test_webhook_blocks_have_header() {
        let report = sample_report();
        let blocks = build_report_blocks(&report);
        assert_eq!(blocks[0]["type"], "header");
        assert!(blocks[0]["text"]["text"]
            .as_str()
            .unwrap()
            .contains("2026-03-19"));
    }

    #[test]
    fn test_webhook_blocks_have_divider_after_summary() {
        let report = sample_report();
        let blocks = build_report_blocks(&report);
        // Find divider after executive summary
        let has_divider = blocks.iter().any(|b| b["type"] == "divider");
        assert!(has_divider);
    }

    #[test]
    fn test_webhook_blocks_include_team_code_block() {
        let report = sample_report();
        let blocks = build_report_blocks(&report);
        let team_block = blocks.iter().find(|b| {
            b["text"]["text"]
                .as_str()
                .map(|t| t.contains("```") && t.contains("Alice"))
                .unwrap_or(false)
        });
        assert!(team_block.is_some(), "Expected team stats code block");
    }

    #[test]
    fn test_webhook_blocks_max_50() {
        let mut report = sample_report();
        // Add many repos to push block count
        for i in 0..60 {
            report.repos.push(RepoSection {
                name: format!("org/repo-{i}"),
                done: Some(format!("Done item {i}")),
                in_progress: Some(format!("In progress {i}")),
                next: Some(format!("Next {i}")),
                flagged_issues: vec![],
            });
        }
        let blocks = build_report_blocks(&report);
        assert!(blocks.len() <= 50, "Blocks should be truncated to 50");
    }

    #[test]
    fn test_summary_blocks_have_thread_hint() {
        let report = sample_report();
        let blocks = build_summary_blocks(&report);
        let has_thread_hint = blocks.iter().any(|b| {
            b["elements"]
                .as_array()
                .and_then(|elems| elems.first())
                .and_then(|e| e["text"].as_str())
                .map(|t| t.contains("thread"))
                .unwrap_or(false)
        });
        assert!(has_thread_hint, "Summary should mention thread");
    }

    #[test]
    fn test_inactive_repos_in_context() {
        let report = sample_report();
        let blocks = build_report_blocks(&report);
        let context = blocks.iter().find(|b| {
            b["type"] == "context"
                && b["elements"]
                    .as_array()
                    .and_then(|e| e.first())
                    .and_then(|e| e["text"].as_str())
                    .map(|t| t.contains("org/backend"))
                    .unwrap_or(false)
        });
        assert!(context.is_some(), "Inactive repos should appear in context block");
    }

    #[test]
    fn test_bold_conversion() {
        assert_eq!(convert_inline_spans("**Done:** hello"), "*Done:* hello");
    }

    #[test]
    fn test_link_conversion() {
        let result = convert_inline_spans("[click](https://example.com)");
        assert_eq!(result, "<https://example.com|click>");
    }

    #[test]
    fn test_heading_conversion() {
        let text = "## Highlights\n- Item one\n- Item two\n";
        let result = convert_markdown(text);
        assert!(result.contains("*Highlights*"), "## should become bold");
        assert!(result.contains("• Item one"), "- should become •");
        assert!(result.contains("• Item two"));
    }

    #[test]
    fn test_nested_bullet_conversion() {
        let text = "- Top\n  - Nested\n";
        let result = convert_markdown(text);
        assert!(result.contains("• Top"));
        assert!(result.contains("◦ Nested"));
    }

    #[test]
    fn test_hr_conversion() {
        let result = convert_markdown("---\n");
        assert!(result.contains("───"));
    }

    #[test]
    fn test_mixed_markdown() {
        let text = "# Summary\n\n**Key wins:**\n- Shipped [feature](https://example.com)\n- Fixed **critical** bug\n";
        let result = convert_markdown(text);
        assert!(result.contains("*Summary*"));
        assert!(result.contains("*Key wins:*"));
        assert!(result.contains("• Shipped <https://example.com|feature>"));
        assert!(result.contains("• Fixed *critical* bug"));
    }

    #[test]
    fn test_chunk_text() {
        let text = "line1\nline2\nline3";
        let chunks = chunk_text(text, 12);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0], "line1\nline2");
        assert_eq!(chunks[1], "line3");
    }

    #[test]
    fn test_flagged_issues_in_blocks() {
        let report = Report {
            date: "2026-03-19".to_string(),
            executive_summary: None,
            repos: vec![RepoSection {
                name: "org/repo".to_string(),
                done: Some("stuff".to_string()),
                in_progress: None,
                next: None,
                flagged_issues: vec![FlaggedIssue {
                    number: 42,
                    title: "Fix login".to_string(),
                    missing_labels: vec!["priority".to_string()],
                    summary: "Login is broken".to_string(),
                }],
            }],
            team_stats: vec![],
        };
        let blocks = build_report_blocks(&report);
        let has_flagged = blocks.iter().any(|b| {
            b["text"]["text"]
                .as_str()
                .map(|t| t.contains("#42") && t.contains("priority"))
                .unwrap_or(false)
        });
        assert!(has_flagged, "Flagged issues should appear in blocks");
    }
}
