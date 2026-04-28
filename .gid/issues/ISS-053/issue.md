---
id: "ISS-053"
title: "Autopilot retry loop redesign — reflective escalation, no silent skips"
status: closed
priority: P0
created: 2026-04-27
closed: 2026-04-27
component: autopilot
related: []
---

# ISS-053 — Autopilot retry loop redesign

**Status:** open
**Severity:** high — autopilot is currently burning massive token budgets on doomed retries, and silently skipping tasks that need human attention
**Discovered:** 2026-04-27 — potato observed two tasks retried 60× each overnight before being skipped. Investigation revealed `max_turns_per_task: 60` (hardcoded in `src/channels/telegram.rs:769`), retry prompts that just escalate emotional pressure without supplying new information, and a false-positive completion check that lets agents claim "done" without updating the checkbox.

## Root causes (first-principles)

The current retry loop violates the **purpose of retry**. Retry exists to give the agent new information so it can take a different path. The current implementation breaks this in three ways:

1. **Wrong retry budget.** 60 attempts is not a retry — it's grinding. Each attempt already invokes `process_message`, which runs the agent's own tool loop (~25 iterations). 60 × 25 = 1500 tool calls per task. Token cost is catastrophic.

2. **Same prompt, harder voice.** Attempts 2 and 3 use prompts like "Your previous attempt didn't update the checkbox" and "FINAL ATTEMPT. You MUST complete this task NOW." This is emotional pressure, not new information. Same input → same output.

3. **No structural escalation.** When a task fails, the only outcome is `SKIPPED`. There is no path to:
   - Reflect on *why* it failed
   - Mark the task for **split** (too large)
   - Mark it as **blocked** (external dep missing)
   - Mark it for **human triage** (genuinely stuck)

4. **False-positive completion detection.** `src/autopilot.rs:376` accepts `response.contains("task completed")` as success even when the checkbox is still `[ ]`. The checkbox is the only ground truth.

5. **No cross-session memory.** When autopilot restarts, it has no record that task X failed 60× last night. It will retry the same way.

## Why this is rustclaw scope

Autopilot is a rustclaw-only feature (`src/autopilot.rs` + Telegram command surface in `src/channels/telegram.rs`). No other consumer.

## Solution — single-cut design (no V1/V2 phases)

### Change 1: Retry budget

`src/channels/telegram.rs:769` — change `max_turns_per_task: 60` to `max_turns_per_task: 3`.

Rationale: 3 attempts = original method + reflection-based retry + escalation to human. A fourth attempt cannot supply new information that wasn't available at attempt 3.

### Change 2: Reflective escalation prompts

In `src/autopilot.rs`, replace the current attempt 2 and attempt 3 prompts.

**Attempt 1** (unchanged): the original task description.

**Attempt 2** — supply new information by forcing self-reflection and offering structural escape hatches:

```
Your previous attempt did not complete the task (checkbox is still `[ ]`).

Before retrying, briefly self-reflect:
1. What did you actually do in the previous attempt?
2. What blocked you? (context exhaustion / unclear scope / failed sub-agent /
   wrong file path / build error / something else)
3. Is the task too large for one attempt?

Then choose ONE path:
(A) Retry with a different approach. State the change in approach BEFORE acting.
(B) If task is too large: append `⚠️ NEEDS_SPLIT: <reason; suggested sub-tasks>`
    to the task line in {file}, then stop. Do NOT mark `[x]`.
(C) If blocked by external (missing dep, unclear req, upstream task incomplete):
    append `⚠️ BLOCKED: <what's missing>` to the task line, then stop.

Task: {desc}
```

**Attempt 3** — final attempt, no further retries possible:

```
Final attempt failed twice. STOP retrying.

Append `⚠️ NEEDS_HUMAN_TRIAGE: <one-line: what you tried, why it failed,
suggested next step>` to the task line in {file}.
Do NOT mark `[x]`. Do NOT mark as `SKIPPED`.

Task: {desc}
```

### Change 3: New task states + parser support

Extend `Task` struct in `src/autopilot.rs` with new state flags. Treat all of
them as "not pending" so `next_task` skips them:

- `SKIPPED` — legacy, kept for back-compat (autopilot auto-gave-up; should
  rarely appear under new logic)
- `NEEDS_HUMAN_TRIAGE` — autopilot reflected and gave up; explicit human review
  required
- `NEEDS_SPLIT` — task is too large; should be broken into sub-tasks before retry
- `BLOCKED` — external blocker (dep, missing req, upstream incomplete)

Update `parse_tasks` to recognize these markers. Update `next_task` filter
logic. Update unit tests.

Each non-pending state must include the human-readable reason on the same line
so a human reviewer (and future autopilot runs) can understand context without
opening logs.

### Change 4: Remove false-positive completion detection

`src/autopilot.rs:376` — delete the `response.contains("task completed")` /
`response.contains("all tasks completed")` shortcut. The **only** source of
truth that a task is done is: the checkbox in the task file changed from `[ ]`
to `[x]`. Agent prose is unreliable.

### Change 5: Cross-session failure memory

When a task transitions to `NEEDS_HUMAN_TRIAGE` or `BLOCKED` or hits max
attempts, store an engram memory:

```
type: factual
importance: 0.7
content: "autopilot_failure: task=\"{desc}\" file={path} reason=\"{r}\" \
         attempts={n} date={YYYY-MM-DD}"
```

At autopilot startup, before the loop:

1. `engram_recall("autopilot_failure")` (limit 20)
2. For each pending task in the file, check if its description matches a prior
   failure record (substring match on first 60 chars)
3. If yes, prepend a context note to the attempt-1 prompt:

```
⚠️ Prior failure on this task: {prior_reason} (attempts: {n}, date: {date}).
Consider this before starting. If the prior failure mode still applies,
go straight to NEEDS_SPLIT or NEEDS_HUMAN_TRIAGE without retrying.
```

This breaks the "fresh autopilot session re-tries the same dead task" cycle.

### Change 6: Logging + Telegram notifications

When a task is marked `NEEDS_HUMAN_TRIAGE` / `NEEDS_SPLIT` / `BLOCKED`, the
notify message should make the state and reason explicit:

```
⚠️ Task needs human review: <desc>
   State: NEEDS_HUMAN_TRIAGE
   Reason: <r>
   File: <path>:<line>
```

Not "skipping task" (which sounds dismissable).

## Acceptance criteria

- `max_turns_per_task` is 3 in default config (telegram.rs)
- `parse_tasks` correctly identifies `NEEDS_HUMAN_TRIAGE`, `NEEDS_SPLIT`,
  `BLOCKED`, `SKIPPED` as non-pending
- `next_task` skips all four states
- Attempt-2 prompt presents A/B/C branches; attempt-3 prompt mandates triage
  marker
- Removing the prose-based completion check does not break existing tests
- Engram store on failure is observable (recall finds the entry by query
  `autopilot_failure`)
- Engram recall on autopilot start injects prior-failure context for matching
  pending tasks
- Telegram notifications use `⚠️ Task needs human review` for triage states,
  not "Skipping task"
- Unit tests:
  - `parse_tasks` recognizes all four non-pending markers
  - `next_task` skips all four
  - `find_task_by_description` still works after marker is appended
  - new helper to write `NEEDS_HUMAN_TRIAGE` / `NEEDS_SPLIT` / `BLOCKED`
    appends correctly without duplicating

## Out of scope

- Auto-splitting tasks (autopilot only *flags* `NEEDS_SPLIT`; the actual
  re-decomposition happens in a separate human-or-agent pass)
- Detecting context exhaustion programmatically (relying on agent
  self-reflection in attempt 2 is good enough; programmatic detection is a
  separate concern)
- Re-running skipped tasks automatically (skipped/triage states stay
  non-pending until a human edits the marker out)

## Files to modify

- `src/channels/telegram.rs` — change retry budget constant (line 769)
- `src/autopilot.rs` — Task struct, parse_tasks, next_task, retry prompt
  builder, completion detector, engram store/recall integration, notify
  messages, unit tests
- (no test fixture files needed — unit tests use inline content)

## Notes for implementer

- The current attempt-2 / attempt-3 prompt block is at roughly
  `src/autopilot.rs:280-330`. Match the existing style (multi-line `format!`)
- `runner` already has access to engram via `runner.memory()` — see
  `src/orchestrator.rs` for the pattern
- Do NOT change `mark_task_skipped` signature; add new helpers
  (`mark_task_needs_triage`, `mark_task_needs_split`, `mark_task_blocked`)
  and have the loop call the right one based on agent's response parsing
- For attempt-2 path detection: after the call, re-read the file and check
  whether the task line now contains `NEEDS_SPLIT` or `BLOCKED`. If yes, treat
  as resolved-non-completion (don't retry, don't mark skipped, just move on).
- Keep `SKIPPED` legacy state for back-compat with old task files

## Implementation — closed 2026-04-27

All six changes landed in a single coherent edit. Single-cut as design specified, no V1/V2 phases.

**Files changed:**
- `src/autopilot.rs` — Task struct gains `terminal: Option<TaskTerminal>` (replaces bool `skipped`); new enum `TaskTerminal { Skipped, NeedsSplit, Blocked, NeedsHumanTriage }` with detect/marker helpers; `parse_tasks` recognizes all four; `next_task` filters all four; `mark_task_terminal` (private, idempotent — never double-marks already-terminal lines), `mark_task_skipped` (legacy back-compat), `mark_task_needs_triage`. Reflective attempt-2 (A/B/C branches) and attempt-3 (mandate triage marker) prompts. False-positive `response.contains("task completed")` check **deleted**. Engram store_explicit on failure (`autopilot_failure: task=... reason=... attempts=N date=...`). Engram recall_explicit at startup (limit 20) parsed back into `Vec<PriorFailure>` with helpers `parse_failure_record` / `extract_quoted` / `extract_unquoted` / `match_prior_failure` (60-char prefix match, picks most recent by date). Attempt-1 prompt prepends prior-failure note when matched. Mid-attempt detection of agent-written terminal marker (B/C path) via re-parsing the file after each turn — treated as resolved-non-completion, no double-marking. Notify wording changed from "Skipping task" to "⚠️ Task needs human review" with State/Reason/File lines.
- `src/channels/telegram.rs:769` — `max_turns_per_task: 60` → `3`.
- Added 7 new unit tests (12 total in `autopilot::tests`); 355 tests passing (was 348).

**Acceptance criteria (all met):**
- ✅ `max_turns_per_task` is 3 in the telegram autopilot config
- ✅ `parse_tasks` recognizes all four non-pending markers
- ✅ `next_task` skips all four states (`test_next_task_skips_all_terminal_states`)
- ✅ Attempt-2 prompt presents A/B/C branches; attempt-3 mandates triage marker
- ✅ False-positive prose check removed; existing tests still pass
- ✅ Engram store on failure (`engramai::MemoryType::Factual`, importance 0.7)
- ✅ Engram recall at startup; matched prior failures injected into attempt-1
- ✅ Telegram notifications use `⚠️ Task needs human review`, not "Skipping task"
- ✅ Unit tests cover: all four markers parsed, `next_task` filters them, `find_task_by_description` works after marker append, `mark_*` idempotent (no double-marking)
- ✅ `parse_failure_record` round-trip verified; `match_prior_failure` picks most recent

**Deviations from issue.md spec:**
- `mark_task_needs_split` and `mark_task_blocked` not added as standalone helpers — the agent self-marks via attempt-2 prompts, and the loop's mid-attempt re-parse detects the marker. The fallback path (`!task_completed` after retry exhaustion) only ever needs `mark_task_needs_triage`. Adding unused helpers would be debt; keep them as `mark_task_terminal(_, _, NeedsSplit, _)` callable when needed.
- `agent_tool` source on engram failure store inherited from `store_explicit`'s built-in metadata — no separate source wiring needed.

**Verification:** `cargo test` → 355 passed, 0 failed; `cargo build` → 0 warnings.
