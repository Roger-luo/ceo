# Batch & Parallel Summarization Design

## Problem

On cold runs or when many issues have updated discussions, the pipeline makes up to 2N
sequential LLM subprocess calls (N issue descriptions + N discussion summaries). With
30 issues, that's ~60 serial blocking calls.

## Solution: Two Phases

### Phase 1: Batch Issue Description Prompting

Combine multiple issue descriptions into a single LLM call, reducing N individual calls
to N/batch_size calls.

**New prompt type:** `BatchIssueDescriptionPrompt` sends up to `batch_size` issues (default
10, configurable in config.toml) in one prompt. Each issue is wrapped in `<issue id="N">`
tags. The LLM returns `<summary id="N">...</summary>` tags, one per issue.

**Cache interaction:**
- Filter out issues with existing cached `issue_summary` before batching
- Parse response and save each summary to the issue cache individually
- Discussion summaries remain individual calls (unchanged)
- If a summary is missing from a batch response, fall back to individual call for that issue
- If batch parsing fails entirely, fall back to individual calls for the whole batch

**Why only descriptions:** Issue descriptions are small (~500 chars each), making them
ideal for batching. Discussion summaries include full comment threads (variable, potentially
large), making them poor batch candidates due to context window pressure.

### Phase 2: Full Async with Tokio

Convert the pipeline to async to run multiple agent subprocess calls concurrently.

**Dependencies:** `tokio` with `rt-multi-thread` feature.

**Agent trait change:** `async fn invoke(&self, prompt: &dyn Prompt) -> Result<String>`.
Uses `tokio::process::Command` instead of `std::process::Command`.

**Concurrency control:** New `concurrency` config setting (default: 4). Implemented as
a `tokio::sync::Semaphore` limiting concurrent agent subprocess calls.

**What gets parallelized:**
- `SummarizeIssuesTask`: batch calls + discussion calls run concurrently
- `RepoSummaryTask`: per-repo calls run concurrently
- `TriageTask`: per-issue calls run concurrently

**PipelineContext and DB access:** `rusqlite::Connection` is not `Send`. Agent calls run
on spawned tasks (returning results). DB reads/writes happen on the main task before/after
the concurrent fan-out. Each task: collect inputs -> fan out agent calls -> collect results
-> write to DB serially.

## Expected Impact

**Phase 1 (batching):** Reduces cold-run description calls from N to ceil(N/10). For 30
issues: 30 calls -> 3 calls. Discussion calls unchanged (still up to N).

**Phase 2 (parallelism):** With concurrency=4, remaining serial calls run ~4x faster.
Combined: a 30-issue cold run goes from ~60 serial calls to ~3 batch + ~30 discussion
calls running 4-wide = ~11 sequential call-slots instead of 60.
