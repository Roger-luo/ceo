# Changelog

All notable changes to this project will be documented in this file.

### Added

- Add context-sensitive status bar with keyboard hints (077b1ee)
- Render slash command completions as vertical menu with descriptions (8e44132)
- Refactor interactive TUI with keyboard nav, slash commands, and tests (8cd24be)
- Support short repo names and XML tag backward compat in linkify (061ce20)
- Auto-linkify #N and @user references via database lookup (aa34c77)
- Add --slack-dry-run flag to preview Slack JSON payload (eaeee09)
- Support qualified repo references in executive summary links (44c69fa)
- Reverse Slack message order and add repo sorting (7ba6148)
- Improve Slack repo report formatting with visual structure (070b0b8)
- Improve Slack formatting with Block Kit and add threaded reports (dc7598b)
- Add Slack webhook integration for sending reports (7002b1a)

### Fixed

- Reserve block budget for summary so it's never truncated (46022d2)
- Add visible Summary label to executive summary section in Slack (a13be8b)
- Convert markdown headings and bullets to Slack mrkdwn (38f7d3c)

### Refactored

- Remove team overview from Slack report (f06cd93)
- Make config TUI schema-driven via ui_tabs() (d51c4b6)

### Testing

- Add insta snapshot test for Slack webhook blocks (c4cc3fe)
