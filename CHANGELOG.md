# Changelog

All notable changes to this project will be documented in this file.

### Added

- Reverse Slack message order and add repo sorting (7ba6148)
- Improve Slack repo report formatting with visual structure (070b0b8)
- Improve Slack formatting with Block Kit and add threaded reports (dc7598b)
- Add Slack webhook integration for sending reports (7002b1a)

### Fixed

- Reserve block budget for summary so it's never truncated (46022d2)
- Add visible Summary label to executive summary section in Slack (a13be8b)
- Convert markdown headings and bullets to Slack mrkdwn (38f7d3c)

### Refactored

- Make config TUI schema-driven via ui_tabs() (d51c4b6)
