# Requirements: KC Auto-Recall

## Overview

Today, RustClaw's automatic memory recall path (`EngramRecallHook`) only queries raw `MemoryRecord` rows. Knowledge Compiler (KC) has already produced 173 compiled topic pages from ~11,200 raw memories (~65:1 compression), but these topics are only accessible through the explicit `knowledge_query` LLM tool — they never appear in the auto-recall injection. This feature connects KC topic pages into the automatic recall path so the agent sees both synthesized knowledge (topics) and raw episodic context in every turn. Additionally, the `knowledge_query` tool currently uses `KcApi::query()` (simple keyword match) instead of `KcApi::recall()` (blended scoring with quality / freshness / source count); this feature also upgrades the tool to use the richer ranking.

## Priority Levels

- **P0**: Core — required for the feature to function
- **P1**: Important — needed for production quality
- **P2**: Enhancement — improves observability or UX

## Guard Severity

- **hard**: Violation = system is broken, execution must stop
- **soft**: Violation = degraded quality, should warn but continue

---

## GOALs (Functional Requirements)

### GOAL-1: Automatic topic injection alongside raw memories

**P0** — When `EngramRecallHook` runs on an inbound message, it MUST query the KC topic store in addition to raw memory recall, and inject both results into the hook context metadata. The returned topics MUST be formatted as a distinct section in the system-prompt injection, clearly labeled so the agent can distinguish synthesized knowledge from raw episodic context.

**Verification**: In a session where the memory DB contains at least one topic whose content matches the user query, invoke `EngramRecallHook::execute`. Assert that `ctx.metadata["engram_recall"]` contains both a topic section and a raw-memory section, and that the formatted string contains two distinct headers matching the format declared in GOAL-3.

### GOAL-2: Topic recall uses blended scoring

**P0** — The auto-recall topic query MUST use `KcApi::recall()` (not `KcApi::query()`). The call MUST pass the user's inbound message as the query string, a top_k limit defined in GOAL-5, and a minimum quality threshold defined in GOAL-6.

**Verification**: Unit test that intercepts the KcApi call and asserts `recall` was invoked with `top_k ≤ max_topics` and `min_quality ≥ min_threshold`.

### GOAL-3: Prompt format for dual-section recall

**P0** — The injected prompt section MUST contain two subsections in this order when both have content:

```
## 🧠 Compiled Knowledge (auto)
- [quality 0.XX] **Topic: {title}**
  {summary or snippet, up to {snippet_char_limit} chars}

## 📜 Recalled Memories (auto) — You may have prior context on this topic. Review before answering.
- {existing format: [MM-DD HH:MM] [confidence] [type] content}
```

When only one section has content, only that section MUST appear. When both are empty, no injection MUST occur.

**Verification**: Golden-file test comparing the formatted output against a known input (3 topics + 5 raw memories, and edge cases: 0 topics + N raw, N topics + 0 raw, 0 + 0).

### GOAL-4: knowledge_query tool upgraded to blended recall

**P0** — The `knowledge_query` LLM tool (in `tools.rs`) MUST call `KcApi::recall()` instead of `KcApi::query()`. The tool's returned snippets and scores MUST reflect the blended ranking (text relevance + quality boost + freshness boost + source-count boost).

**Verification**: Tool-level integration test: call `knowledge_query` with a query that has one high-quality-but-keyword-weak topic and one low-quality-but-keyword-strong topic. Assert the high-quality topic ranks higher in the returned list.

### GOAL-5: Bounded topic count per recall

**P1** — Each auto-recall MUST return at most `max_topics` topics in the Compiled Knowledge section. The default MUST be 3. This value MUST be a named constant (not a magic number buried in code).

**Verification**: With 10 matching topics in the DB, invoke auto-recall and assert `results.topics.len() ≤ 3`.

### GOAL-6: Minimum quality threshold for auto-injection

**P1** — Topics with `quality_score < min_quality` MUST NOT be included in auto-recall injection. The default MUST be 0.3. This value MUST be a named constant.

**Verification**: Seed the DB with three topics at quality 0.1, 0.4, 0.9. Invoke auto-recall; assert only the 0.4 and 0.9 topics appear.

### GOAL-7: Snippet length cap per topic

**P1** — Each topic's content in the prompt MUST be truncated to at most `snippet_char_limit` characters (default 300) to control total prompt token usage. Truncation MUST preserve word boundaries (no cutting mid-word) and append an ellipsis when truncated.

**Verification**: Pass a topic with a 1000-char summary; assert the rendered snippet is ≤ 303 chars (300 + ellipsis) and ends at a word boundary.

### GOAL-8: Graceful degradation when KC is empty or fails

**P0** — If the KC topic store contains zero topics, or if `KcApi::recall()` returns an error, or if the recall exceeds a timeout `kc_recall_timeout_ms` (default 200ms), the hook MUST emit only the raw-memory section (preserving current behavior) without surfacing an error to the agent. The hook MUST log the degradation at `debug` level.

**Verification**: Three tests — (a) empty topic store returns only raw section; (b) mocked KcApi returning `Err` returns only raw section; (c) mocked KcApi sleeping past the timeout returns only raw section.

### GOAL-9: Metadata observability

**P1** — The hook context metadata MUST include a `kc_recall` field alongside the existing `engram_recall` field, containing: topic count, elapsed milliseconds, and a boolean `degraded` flag indicating whether fallback was triggered. This is consumed by downstream observability (logs, tests).

**Verification**: Assert `ctx.metadata["kc_recall"]` exists after a successful auto-recall and contains the three specified sub-fields.

### GOAL-10: No duplication at the data layer; LLM disambiguates

**P1** — The hook MUST NOT attempt to remove raw memories whose IDs appear in a returned topic's `source_memory_ids`. Topic and raw sections MUST be shown in parallel, allowing the agent to treat the topic as synthesis and the raw entries as underlying evidence.

**Verification**: Seed the DB such that topic T's `source_memory_ids` includes memory M's ID, and both T and M match the query. Invoke auto-recall; assert the output contains T in the Compiled Knowledge section AND M in the Recalled Memories section.

### GOAL-11: Skip short queries

**P0** — The existing short-query skip (currently: message length < 10 chars) MUST continue to apply to both topic recall and raw recall. Neither path runs for ignored messages.

**Verification**: Call `execute` with a 5-char message; assert neither `KcApi::recall` nor `MemoryManager::session_recall` is invoked.

---

## GUARDs (System Invariants)

### GUARD-1 [hard]: No breakage to existing raw-recall behavior

The output for the existing `engram_recall` metadata field MUST remain unchanged for all inputs that previously produced output. Adding KC does NOT alter what raw-recall returns, injects, or logs.

**Verification**: Run the existing test suite for `EngramRecallHook`; all pre-existing tests MUST pass unmodified.

### GUARD-2 [hard]: No data mutation during recall

Neither topic recall nor raw recall MUST write to the memory DB during the recall path. Recall is read-only.

**Verification**: Wrap the DB connection in a write-detector during recall tests; assert no writes occur.

### GUARD-3 [soft]: Total injection size bounded

The combined prompt injection (topics + raw memories + headers) MUST NOT exceed `max_injection_chars` (default 4000 chars, roughly 1000 tokens). If the raw section alone already exceeds the limit, the topic section MUST be dropped rather than truncating raw memories; raw-recall's existing sizing behavior is preserved unchanged. If topic + raw exceeds the limit, topics MUST be reduced first (one at a time, lowest-scored first) until the total fits.

**Verification**: Construct a scenario with 3 high-quality topics and a raw section already near the limit. Assert the final injection stays within `max_injection_chars` and topics are dropped in the documented order.

### GUARD-4 [hard]: Timeout does not block request

`KcApi::recall()` calls MUST be wrapped in a timeout (`kc_recall_timeout_ms`, default 200ms). Exceeding the timeout MUST NOT block, panic, or surface an error to the agent — it MUST fall back to raw-only recall as specified in GOAL-8.

**Verification**: Mocked KcApi with a 500ms artificial delay; assert hook completes in < 300ms (200ms timeout + 100ms slack) and returns only the raw section.

### GUARD-5 [hard]: Config keys live in one place

All tunables (`max_topics`, `min_quality`, `snippet_char_limit`, `kc_recall_timeout_ms`, `max_injection_chars`) MUST be defined as named constants in a single module (`engram_hooks::kc_config` or similar). No duplicated literals in multiple files.

**Verification**: grep for each default value across the RustClaw source tree; assert each value appears only in its constant definition and the tests that verify it.

### GUARD-6 [hard]: Telemetry is debug-level, not user-visible

Logging about KC recall (timings, degradation, counts) MUST use `tracing::debug!` or lower. KC-related output MUST NOT appear in user-facing channels (Telegram, CLI stdout) under normal operation.

**Verification**: Review all new tracing calls in the diff; assert none use `info!`, `warn!`, or `error!` except for genuine error conditions.

---

## Out of Scope

- **Storing interoceptive state on memories** (valence, stress). Separate future feature (discussed as P3).
- **Writing regulation suggestions back to SOUL.md / IDENTITY.md**. Separate future feature (discussed as P2).
- **Introducing triple extraction into KC clustering**. Mentioned as a future extension; not part of this feature.
- **Changing KC compilation behavior** (when/how topics are created). This feature only consumes existing topics.
- **New tools for KC health/conflict detection**. Existing tools stay as-is.
- **Cross-session topic caching**. Each recall re-queries; no per-session topic cache.
- **UI/dashboard changes**. No visualization work.

---

## Dependencies

- Requires `engramai::compiler::api::KcApi::recall` (already exists in the engram crate; no engram-side changes).
- Requires 1+ compiled topic pages in the DB for the feature to produce visible effect (but MUST degrade gracefully when zero, per GOAL-8).
- Consumed by: `EngramRecallHook` (auto-recall path), `knowledge_query` tool.
- No schema migrations required.
