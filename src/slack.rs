use anyhow::{bail, Context, Result};
use log::debug;

use crate::config::SlackConfig;

const ENV_WEBHOOK: &str = "CEO_SLACK_WEBHOOK";

/// Resolve the Slack webhook URL: $CEO_SLACK_WEBHOOK env var takes precedence,
/// then falls back to the config file value.
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
    bail!("Slack webhook URL not configured. Set $CEO_SLACK_WEBHOOK or add webhook_url under [slack] in config.toml")
}

/// Send a markdown report to Slack via an incoming webhook.
pub async fn send_report(markdown: &str, slack_config: Option<&SlackConfig>) -> Result<()> {
    let url = resolve_webhook_url(slack_config)?;
    let channel = slack_config.and_then(|c| c.channel.as_deref());
    let mrkdwn = markdown_to_mrkdwn(markdown);

    // Slack has a 3000-char limit per text block, so we chunk into sections.
    let blocks = build_blocks(&mrkdwn, channel);
    let payload = serde_json::json!({ "blocks": blocks });

    debug!("Sending Slack payload ({} blocks)", blocks.as_array().map_or(0, |a| a.len()));

    let client = reqwest::Client::new();
    let resp = client
        .post(&url)
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

/// Build Slack Block Kit blocks from mrkdwn text, respecting the 3000-char section limit.
fn build_blocks(mrkdwn: &str, _channel: Option<&str>) -> serde_json::Value {
    const MAX_SECTION: usize = 3000;

    let mut blocks = Vec::new();
    let mut current = String::new();

    for line in mrkdwn.lines() {
        // If adding this line would exceed the limit, flush
        if !current.is_empty() && current.len() + line.len() + 1 > MAX_SECTION {
            blocks.push(section_block(&current));
            current.clear();
        }
        if !current.is_empty() {
            current.push('\n');
        }
        current.push_str(line);
    }
    if !current.is_empty() {
        blocks.push(section_block(&current));
    }

    serde_json::Value::Array(blocks)
}

fn section_block(text: &str) -> serde_json::Value {
    serde_json::json!({
        "type": "section",
        "text": {
            "type": "mrkdwn",
            "text": text
        }
    })
}

/// Convert GitHub-flavored markdown to Slack mrkdwn.
///
/// Key differences:
/// - `# Heading` → `*Heading*` (bold)
/// - `**bold**` → `*bold*`
/// - `[text](url)` → `<url|text>`
/// - `---` → divider (we just keep it as-is, Slack ignores it)
/// - Tables are kept as monospace
fn markdown_to_mrkdwn(md: &str) -> String {
    let mut out = String::with_capacity(md.len());
    let mut in_table = false;

    for line in md.lines() {
        let trimmed = line.trim();

        // Headings: # Title → *Title*
        if let Some(rest) = trimmed.strip_prefix("### ") {
            flush_table(&mut out, &mut in_table);
            out.push_str(&format!("*{}*\n", rest.trim()));
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("## ") {
            flush_table(&mut out, &mut in_table);
            out.push('\n');
            out.push_str(&format!("*{}*\n", rest.trim()));
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("# ") {
            flush_table(&mut out, &mut in_table);
            out.push('\n');
            out.push_str(&format!("*{}*\n", rest.trim()));
            continue;
        }

        // Horizontal rules → divider-like spacing
        if trimmed == "---" {
            flush_table(&mut out, &mut in_table);
            out.push_str("───────────────────────\n");
            continue;
        }

        // Table rows: keep as-is but wrap in code-style if first table row
        if trimmed.starts_with('|') && trimmed.ends_with('|') {
            // Skip separator rows (|---|---|)
            if trimmed.contains("---") {
                continue;
            }
            if !in_table {
                in_table = true;
                out.push_str("```\n");
            }
            out.push_str(trimmed);
            out.push('\n');
            continue;
        }

        flush_table(&mut out, &mut in_table);

        // Convert inline markdown
        let converted = convert_inline(trimmed);
        out.push_str(&converted);
        out.push('\n');
    }

    flush_table(&mut out, &mut in_table);
    out
}

fn flush_table(out: &mut String, in_table: &mut bool) {
    if *in_table {
        out.push_str("```\n");
        *in_table = false;
    }
}

/// Convert inline markdown to Slack mrkdwn:
/// - `**bold**` → `*bold*`
/// - `[text](url)` → `<url|text>`
fn convert_inline(line: &str) -> String {
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
        let Some(bracket_start) = text.find('[') else { break };
        let search_from = bracket_start + 1;
        let Some(bracket_end_offset) = text[search_from..].find("](") else { break };
        let bracket_end = search_from + bracket_end_offset;
        let url_start = bracket_end + 2;
        let Some(paren_end_offset) = text[url_start..].find(')') else { break };
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

    #[test]
    fn test_heading_conversion() {
        let md = "# Project Report — 2026-03-19\n\n## org/repo\n";
        let result = markdown_to_mrkdwn(md);
        assert!(result.contains("*Project Report — 2026-03-19*"));
        assert!(result.contains("*org/repo*"));
    }

    #[test]
    fn test_bold_conversion() {
        let md = "**Done:** Fixed the bug\n";
        let result = markdown_to_mrkdwn(md);
        assert!(result.contains("*Done:* Fixed the bug"));
    }

    #[test]
    fn test_link_conversion() {
        let md = "[@user](https://github.com/user) opened [#42](https://github.com/org/repo/issues/42)\n";
        let result = markdown_to_mrkdwn(md);
        assert!(result.contains("<https://github.com/user|@user>"));
        assert!(result.contains("<https://github.com/org/repo/issues/42|#42>"));
    }

    #[test]
    fn test_table_wrapped_in_code() {
        let md = "| Person | Active |\n|--------|--------|\n| Alice | 3 |\n";
        let result = markdown_to_mrkdwn(md);
        assert!(result.contains("```\n| Person | Active |"));
        assert!(!result.contains("---"));
    }

    #[test]
    fn test_hr_conversion() {
        let md = "---\n";
        let result = markdown_to_mrkdwn(md);
        assert!(result.contains("───"));
    }
}
