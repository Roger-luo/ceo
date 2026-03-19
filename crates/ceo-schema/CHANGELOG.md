# Changelog

All notable changes to this project will be documented in this file.

### Added

- Include open PR additions/deletions in team line stats (2907020)
- Add commit_stats and email_to_github tables with DB operations (ce6c8a2)
- Add ceo-schema crate with versioned row types (eb2ffdf)

### Refactored

- Store raw email in commit_stats, resolve at query time (5c3b312)
