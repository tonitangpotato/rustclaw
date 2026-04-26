---
id: "ISS-021"
title: "Message context side-channel — Envelope refactor"
status: in_progress
priority: P1
created: 2026-04-22
component: "src/context.rs, src/memory.rs, engramai"
note: "Phase 1 done (P_before=0.767 baseline). Phases 2-5 ahead."
---
# ISS-021: Message Envelope — Side Channel, Not In-Band String

- **Status**: 🟢 Phase 5b complete — counterfactual measurement returned `delta = 0.000`; Phase 5c (wet migration) rejected on evidence. Architecturally ISS-021 is fully landed at Phase 4; Phase 5c stays `blocked` permanently barring an embedding-model change.
- **Created**: 2026-04-23
- **Updated**: 2026-04-23 (v2.6: Phase 5b controlled-experiment harness landed. Same corpus, query, and recall path; only variable is an 80-token Telegram header prepended (polluted) vs not (clean). 10/10 fixtures P_clean ≡ P_polluted. Validity double-checked: regime diagnostic confirms live Ollama+nomic-embed-text path; heavy-distractor sanity probe drops P@3 by 0.433, proving the harness IS content-sensitive. 315/315 tests pass.)
- **Priority**: High
- **Category**: Architecture / Memory Quality
- **Related**: ISS-018 (recall intent classification), engram extractor dimensional schema

## Phase 1 Execution Record (2026-04-23)

Landed commits: (pending user commit — working tree against HEAD)

**What was delivered** (matches spec):
- `Envelope` type in `src/context.rs` with `Serialize, Deserialize, Clone, Default` derives. `MessageContext` retained as `pub type` alias for the Phase 1 compatibility window (removed in Phase 4 per plan).
- `HookContext.envelope: Option<Envelope>` field added (`src/hooks.rs`). Populated by Phase 2+3, currently defaults to `None` at every call site.
- Recall quality baseline harness landed in `src/memory.rs` as `mod recall_quality_baseline { ... }`. 10 fixtures, Precision@3 metric, envelope-plumbing serde roundtrip smoke test.

**What was delivered beyond the original Phase 1 spec** (justified scope expansion):

1. **`MemoryManager::store` / `store_explicit` migrated from `engram.add(...)` to `engram.store_raw(content, StorageMeta { ... })`**, and both now accept `envelope: Option<&Envelope>` which is serialized into `StorageMeta::user_metadata` as `{"envelope": <json>}` when present.
   - **Why this belongs in Phase 1, not Phase 2+3**: the original spec said Phase 2+3 would "call new wrapper `memory.store_with_envelope(...)` which puts envelope in `user_metadata.envelope`". On implementation we realized the legacy `engram.add(content, type, importance)` API has **no** `user_metadata` channel — so landing envelope-aware storage requires the `store_raw` switch regardless. Deferring it to Phase 2+3 would have meant a half-implemented Phase 1 (envelope in HookContext but no way to persist it) followed by a mechanically-obvious API swap in Phase 2+3. Doing it in Phase 1 keeps each phase internally coherent.
   - **Zero-behavior-change proof**: baseline test `recall_baseline_precision_at_3_per_fixture` includes an explicit `storage_audit` block that asserts every fixture's 8 items land as `RawStoreOutcome::Stored(_)` — zero `Skipped`, zero `Quarantined`. If the migration had introduced any silent dedup/PII-filter behavior for our fixture shape, the test would trip.

2. **Fixture design upgraded from 1-relevant-per-fixture to 3-relevant-per-fixture** (corpus grew from 5 to 8 items per fixture, +1 near-topic distractor per fixture).
   - **Why**: the initial 1-relevant design capped `P@3` at `1/3 = 0.333` per fixture. Every fixture hit that ceiling in the first baseline run (mean = 0.333). **A saturated baseline cannot measure Phase 5 improvements** — any P_after ≥ 0.333 shows as 0.0 delta, and the evidence gate becomes meaningless. Expanding to 3 gold-relevant items makes P@3 range over `{0.0, 0.333, 0.667, 1.0}` and gives the measurement genuine dynamic range.
   - **New P_before = 0.767** (unsaturated). Remaining headroom to 1.0 is 0.233 — larger than the 0.15 Phase 5 significance threshold, so the gate remains informative.
   - **Near-topic distractors** (e.g. "TLS 1.3 handshake" in the OAuth fixture, "UDP is connectionless" in the TCP fixture) were added so recall must actually discriminate topic from adjacent topic — purely random distractors would let any baseline ace the test.

3. **`MemoryManager::for_testing(workspace_dir)` constructor** (gated `#[cfg(test)]`, private to crate) replacing the prior hand-constructed `MemoryManager { ... }` literal inside the baseline module.
   - **Why**: future additions to `MemoryManager` (e.g. envelope cache in Phase 2+3) would break every test harness that hand-constructs the struct. Centralizing construction in one place is the root fix. Test-only `#[cfg(test)]` gate means zero production surface.

**Baseline result** (`cargo test --bin rustclaw recall_quality_baseline -- --nocapture`):

```
=== ISS-021 Phase 1 Baseline: Precision@3 ===
(3 gold-relevant items per fixture of 8; P@3 range: 0.0 / 0.333 / 0.667 / 1.0)
  authentication-flow        P@3 = 0.667  (recalled 5)
  rust-ownership             P@3 = 1.000  (recalled 5)
  baking-sourdough           P@3 = 0.667  (recalled 5)
  tcp-networking             P@3 = 1.000  (recalled 5)
  espresso-extraction        P@3 = 0.667  (recalled 5)
  quantum-entanglement       P@3 = 0.667  (recalled 5)
  database-indexing          P@3 = 1.000  (recalled 5)
  mountain-climbing          P@3 = 0.667  (recalled 5)
  ml-gradient-descent        P@3 = 0.667  (recalled 5)
  culinary-fermentation      P@3 = 0.667  (recalled 5)
  ---
  mean P@3  = 0.767  (P_before)

=== Storage audit (store_raw outcome integrity) ===
  [every fixture: stored=8  quarantined=0]
```

**Test count**: 281 pass, 0 fail (net +2 from Phase 0: `recall_baseline_precision_at_3_per_fixture` + `envelope_plumbing_compiles`). Zero new rustclaw warnings.

**Deviation from v2.2 spec**: the fixture composition described in "Baseline Specification" below was a **synthetic-topic** design (technical / Chinese / code / person-name / conversation-recall). The harness actually landed uses **factual-topic fixtures** (OAuth, Rust ownership, sourdough, TCP, espresso, etc.) with synthetic factual distractors. The v2.2 design required a populated production DB with real conversations to label expected top-3 — a chicken-and-egg dependency since the DB isn't stable until after Phase 2+3. The factual-topic design is DB-independent, deterministic across runs, and sufficient for detecting header-pollution improvements (which is the dimension Phase 5 gates on). **The v2.2 fixture spec should be updated to reflect this**, or the Phase 5 protocol should explicitly say "re-run the factual harness plus a separate 10-query check against real DB content".

**Open question for Phase 5 reviewer**: whether the factual-topic harness alone is sufficient evidence to gate wet migration, or whether a second "real-content" fixture set is required. Tracked but not blocking Phase 2+3.

---

## Phase 2+3 Execution Record (2026-04-23)

Landed atomically (no Phase 2→3 bridge state). Merged per v2.1 audit finding FINDING-1.

**What was delivered** (matches merged Phase 2+3 spec):

1. **`MessageContext::format_prefix()` renamed to `Envelope::render_for_prompt()`** (`src/context.rs`). Same rendering logic, but now explicitly scoped to system-prompt construction — not in-band content prefixing. Returns the same `## Message Context` markdown block.

2. **System prompt construction consumes envelope structurally**. `build_system_prompt_full()` (`src/system_prompt.rs`) accepts `envelope: Option<&Envelope>` and injects `envelope.render_for_prompt()` into the `## Message Context` section of the system prompt. The LLM sees sender/time/chat-type via system prompt only; **user content stays clean**.

3. **`HookContext.envelope` populated before hooks execute**. `run_agent_loop()` and `process_message_events()` (`src/agent.rs`) accept an optional `envelope: Option<Envelope>` parameter. The envelope is cloned into `HookContext` before `EngramRecallHook` and `EngramStoreHook` run, so recall queries and store calls can route via envelope without depending on in-band content prefixes.

4. **Telegram channel stops prefixing content**. `channels/telegram.rs` now:
   - Builds `Envelope` from Telegram update metadata (sender, chat type, reply-to, timestamp) — unchanged logic.
   - Passes **raw message text** (`msg.text().unwrap_or("")`) as content to `agent.process_message_with_envelope(...)`.
   - Removes the `format!("{}\n\n{}", envelope.format_prefix(), content)` concatenation at the channel entry point.
   - Net effect: every Telegram message enters the agent loop with clean content and a structural envelope beside it.

5. **Recall + store paths routed through envelope-aware wrappers**:
   - `EngramRecallHook` reads `ctx.envelope.as_ref()` and forwards it to `session_recall` (currently a no-op on the envelope side — reserved for Phase 5 dimensional filtering; the path is wired so Phase 5 needs no plumbing changes).
   - `EngramStoreHook` reads `ctx.envelope.as_ref()` and forwards to `MemoryManager::store_explicit(content, type, importance, envelope)`, which serializes the envelope into `user_metadata.envelope` via the Phase 1 `store_raw` migration.
   - Result: new engram records land with `user_metadata.envelope` populated and `content` clean (no `[TELEGRAM ...]` headers).

6. **New test: `envelope_renders_into_system_prompt_not_content`** verifies the core invariant — given a raw content string `"what time is it?"` and an envelope with Telegram metadata, the assembled system prompt contains the envelope's rendered block AND the raw content string contains no `[TELEGRAM ...]` prefix. This test gates regression: any future channel that accidentally re-introduces prefixing breaks this test.

**Zero behavior change for callers without envelope**: `process_message_with_envelope(content, opts, None)` produces byte-identical output to the old `process_message_with_options(content, opts)` path. All 281 pre-existing tests continue to pass unchanged.

**What is NOT yet changed** (intentionally deferred):
- `Envelope::strip_from_content` helper is defined but has zero production call sites. Reserved for Phase 5 historical migration CLI.
- Discord, Slack, Signal, Matrix, WhatsApp channels still use the legacy `format_prefix` path. **This is intentional** — only Telegram was in the active-use path, and migrating the other channels is mechanical (same pattern). Filed as follow-up. Phase 4 will remove `MessageContext` type alias once all channels migrate.
- `MessageContext` type alias retained. Phase 4 removes it.

**Test count**: 284 pass, 0 fail (net +3 from Phase 1: `envelope_renders_into_system_prompt_not_content` + 2 smaller integration tests covering `process_message_with_envelope` serialization of `None` vs `Some(env)` paths). Zero new warnings.

**Recall quality baseline re-run**: `P@3 = 0.767` (unchanged from Phase 1).
- **This is expected and not a failure.** The baseline harness fixtures are synthetic and don't depend on production channel traffic, so Phase 2+3 code changes don't affect the fixture-based score. More importantly, the DB is still ~100% old records with dirty headers — any realistic real-content measurement at this moment would show <0.15 delta because the pool hasn't turned over.
- **Per "Baseline Timing" protocol (below)**: do NOT judge Phase 5 gating from this immediate measurement. Phase 5 evidence-gating requires waiting until new clean records are a majority of the active recall pool, OR running a separate cohort-split harness that ingests a known volume of clean records against the existing dirty baseline.

**Phase 5 gating decision**: deferred. Current evidence is insufficient (pool hasn't turned over; fixtures are synthetic). Revisit in 2–4 weeks with production traffic accumulated, OR design a separate cohort harness as part of Phase 5 prep.

**Follow-up work filed** (not blocking):
- Discord/Slack/Signal/Matrix/WhatsApp channel migration to `process_message_with_envelope` — mechanical, ~10 LOC per channel.
- Phase 4: remove `MessageContext` type alias and `Envelope::format_prefix` deprecation shim.
- Phase 5 prep: design cohort-split recall harness OR wait for DB turnover.

---

## Phase 4 Execution Record (2026-04-23)

**What was delivered**:

1. **`MessageContext` type alias removed** from `src/context.rs`. The `pub type MessageContext = Envelope;` line is deleted. Module-doc comments updated to describe the removal (single-sentence historical note retained for anyone searching the codebase for `MessageContext`).

2. **All call sites renamed to `Envelope` directly** across 5 files:
   - `src/context.rs` — 8 test-fixture struct literals
   - `src/channels/telegram.rs` — 2 channel-entry build sites
   - `src/channels/discord.rs` — use import + `build_message_context` return type + struct literal
   - `src/channels/slack.rs` — use import + `build_message_context` return type + struct literal
   - `src/agent.rs` — function signature parameter type
   - `src/memory.rs` — `envelope_plumbing_compiles` test (alias-roundtrip assertion removed; serde roundtrip + construction smoke test retained)

3. **`process_message_with_context` renamed to `process_message_with_envelope`** in `src/agent.rs`. This matches the issue spec: `_with_envelope` is now the canonical channel entry point. All 3 channel callers (telegram.rs line 2158, discord.rs line 326, slack.rs line 408) updated to call the new name.

4. **Parameter name inside `process_message_with_envelope` renamed**: `msg_ctx: &Envelope` → `envelope: &Envelope`, for consistency with the function name. Local variables in channel code (`let msg_ctx = …`) retained — channel-internal naming is not part of the public surface.

**Verification**:

- `grep -rn "MessageContext" src/ --include="*.rs"` returns exactly one hit: a historical doc-comment in `src/context.rs` describing that the alias was removed. Zero code references.
- `grep -rn "process_message_with_context" src/ --include="*.rs"` returns zero hits.
- `cargo check` passes with zero new warnings (4 pre-existing warnings in `gid-core` upstream unchanged).
- `cargo test --quiet` passes **284/284 tests** — same count as after Phase 2+3, confirming pure textual refactor with no behaviour change.

**What this does NOT change**: no runtime behaviour. The envelope was already flowing structurally after Phase 2+3; Phase 4 is purely a naming-cleanup commit so there is exactly one canonical term (`Envelope`) and one canonical entry point (`process_message_with_envelope`) in the codebase.

**Follow-up** (Phase 5):
- Implement `rustclaw memory migrate-envelope --dry-run` CLI (ISS-021-12 in graph).
- Counterfactual baseline measurement: `P_clean` vs `P_polluted` via controlled-experiment harness (ISS-021-13).
- Wet migration (ISS-021-14) is **gated** on `P_clean - P_polluted ≥ 0.15`; marked `blocked` in graph until Phase 5b evidence.

### Phase 5b Design Decision (2026-04-23)

The literal Phase 5 protocol ("copy prod DB → apply migration → re-run baseline fixtures") has a fatal flaw: the synthetic fixture corpus does not exist in the production DB. Running the fixtures against prod data yields `P@3 ≡ 0` because `relevant_ixs` indexes into strings that are not present in prod. The numbers would be produced but signal-free.

**Revised Phase 5b protocol — controlled experiment on synthetic corpus:**

For each of the 10 baseline fixtures, run two variants:

1. **`polluted`** — each corpus item is prepended with a realistic 80-token Telegram header before `store()`:
   `[TELEGRAM potato (@potatosoupup) id:7539582820 Thu 2026-04-23 12:00 -04:00]\n\n{original_content}`
2. **`clean`** — corpus stored as-is (current post-Phase-2+3 behavior).

All other variables (embedding model, recall path, session_key scheme, query) are held identical. `delta = P_clean_mean - P_polluted_mean` directly isolates header pollution as a variable.

**Adversarial strengthening:** the polluted variant uses the *same* header across all items in a fixture (same sender, same timestamp-prefix). This matches prod (most polluted rows share `id:7539582820`), and if the embedding model is header-sensitive it will pull polluted items into a tight cluster away from the query.

**Why this is the right measurement:** the hypothesis under test is *"the embedding model is contaminated by in-content channel headers"*. That is a **model property**, not a data-distribution property. A controlled experiment on synthetic corpus measures the model property directly and cleanly.

**Gating:** if `delta ≥ 0.15`, Phase 5c wet migration is justified. If `delta < 0.15`, pollution is not dominant — open a new issue for the real recall bottleneck (embedding choice, extractor dimensions, session_recall strategy) and leave Phase 5c blocked.

## Phase 5b Execution Record (2026-04-23)

### Result

```
Recall regime: EMBEDDING (Ollama/nomic-embed-text live)

fixture                       P_clean P_polluted      delta
authentication-flow             0.667      0.667      0.000
rust-ownership                  1.000      1.000      0.000
baking-sourdough                0.667      0.667      0.000
tcp-networking                  1.000      1.000      0.000
espresso-extraction             0.667      0.667      0.000
quantum-entanglement            0.667      0.667      0.000
database-indexing               1.000      1.000      0.000
mountain-climbing               0.667      0.667      0.000
ml-gradient-descent             0.667      0.667      0.000
culinary-fermentation           0.667      0.667      0.000
  ---
  mean P_clean    = 0.767
  mean P_polluted = 0.767
  DELTA           = +0.000
  gate threshold  = 0.150
  decision        = REJECT — pollution not dominant
```

**10/10 fixtures: clean P@3 ≡ polluted P@3.** `delta = 0.000 ≪ 0.15`.

### Validity checks (both passed)

1. **Recall regime diagnostic**: test self-reports `EMBEDDING (Ollama/nomic-embed-text live)` — confirms the measurement exercises the real embedding + cosine-similarity path, NOT the FTS fallback that would trivially ignore a fixed header token set.
2. **Harness sanity probe** (`recall_harness_sanity_reacts_to_large_content_changes`): seeding corpus items with a LARGE off-topic prefix (≈150 tokens, 11 unrelated facts) drops P@3 from 0.767 → 0.333, a **−0.433 delta**. The harness is content-sensitive; a real effect would be detected.
3. **Storage audit**: all 10×2=20 runs stored exactly 8 items, 0 quarantined. The two modes operate on identical populations; delta is not confounded by differential quarantine.

### Interpretation

`nomic-embed-text` is robust to the 80-token Telegram channel header prefix. Mechanistically this matches the retrieval literature: for mean-pooled transformer embeddings, a fixed prefix shared by every item in the corpus adds a common-mode vector component that cancels out of pairwise cosine similarities. Ranking is preserved.

The hypothesis ("header pollution measurably degrades recall") is **falsified under the current embedding model.**

### Gating decision

- **Phase 5c wet migration: REJECTED.** No evidence that rewriting historical rows would improve recall quality. Scheduling that work would be cargo-culting on an untested assumption.
- **ISS-021-14 remains `blocked`** in the graph, permanently, unless a future change of embedding model re-opens the hypothesis.

### What IS still worth doing (independent of Phase 5c)

The architectural wins from Phase 2+3 still stand on their own merits, none of which required a recall-quality delta to justify:

1. **Single source of truth** — envelope metadata lives in one place (`user_metadata.envelope`), not duplicated in `content`.
2. **Clean content for downstream extractors** — the engramai entity extractor, KC compiler, and any future LLM-based consumers see real message text, not telegram-preambled noise.
3. **No round-trip parsing** — no regex-based strip path between channels and agents.
4. **Debuggability** — querying "which messages came from group X" becomes a structured filter, not a substring search.

### Next step: open a follow-up issue

If recall quality on prod ever feels subjectively worse than expected, the bottleneck is **NOT** header pollution. A new issue should investigate:

- Embedding model choice (nomic-embed-text is small + general; maybe swap for a higher-dimensional or domain-tuned model)
- Extractor dimension quality (entity extraction relevance to Chinese mixed content, cross-language alignment)
- `session_recall` strategy (weighting between embedding / FTS / entity channels; working-memory decay tuning)
- Namespace partitioning (is default namespace overloaded?)

### Verification artifacts

- `src/memory.rs::recall_quality_baseline::recall_counterfactual_header_pollution_phase_5b` — the measurement test (deterministic, idempotent, runs under `cargo test`).
- `src/memory.rs::recall_quality_baseline::recall_harness_sanity_reacts_to_large_content_changes` — the harness validity guard. If recall ever stops reacting to content changes, THIS test fails first, before Phase 5b produces a meaningless zero.
- All 315 tests pass, zero new warnings.

---

## TL;DR

Channel transport metadata (sender, time, chat type, reply-to) is currently stringified via `MessageContext::format_prefix()` and **prepended in-band to the user message text** before being passed to hooks, engram, and the LLM. This pollutes embeddings, intent classification, and stored memories with ~80-token structural headers that have nothing to do with the message's semantic content.

**Fix**:

1. Rename `MessageContext` → `Envelope`. Single source of truth.
2. Make `Envelope` `Serialize + Deserialize` — JSON mapping is automatic via serde, no hand-written projection layer.
3. Add `HookContext.envelope: Option<Envelope>` — hooks get the struct directly.
4. Move LLM prompt rendering from in-band prefix to `Envelope::render_for_prompt()`, called at prompt-construction time.
5. In engram records, store envelope under `user_metadata.envelope` (engramai's existing caller-supplied JSON column). **No engramai changes required** — `StorageMeta.user_metadata: serde_json::Value` already exists and is passed through. The `dimensions` side is a separate, already-typed `EnrichedMemory.dimensions` column (core_fact, participants, temporal, domain, sentiment, stance, valence, outcome, tags, type_weights, confidence) produced by the engramai extractor. Two independent storage slots with clean provenance: `user_metadata.envelope` = fact from channel; `dimensions` (typed, separate column) = inference from LLM.

**Key principle**: Fact vs inference separation. `envelope` is zero-cost truth and lives in `user_metadata`; `dimensions` is LLM-extracted, typed, and may be wrong/re-extracted. Extractor does **not** read `user_metadata` (verified: engramai's `extractor.extract(content)` only sees content). They are architecturally independent.

**Audit note (2026-04-23)**: Earlier drafts described "`metadata.envelope` + `metadata.dimensions` namespaces in one JSON column". That was wrong — engramai stores `dimensions` as a typed struct field, not as a JSON sub-key. The storage model is: `EnrichedMemory { content, dimensions: Dimensions, user_metadata: Value, ... }`. This issue adds data **only to `user_metadata`**. No changes to engramai.

## Problem

### Symptom

Engram recall quality is degraded. `recall-trace.jsonl` shows queries like:

```json
{"query": "[TELEGRAM potato (@potatosoupup) id:7539582820 Thu 2026-04-23 01:05 -04:00]\n\ntelegram header这种东西..."}
```

The embedding is computed over this whole blob. The `[TELEGRAM ... id:...]` prefix is ~80 tokens of structural metadata — deterministic across every message from the same user — which **dominates the embedding direction** and drowns out the 15-token semantic payload.

### Root cause chain

1. **`MessageContext::format_prefix()`** renders structured fields into a string prefix: `[TELEGRAM name (@user) id:N timestamp]\n\nReplying to X:\n> quoted\n\n`
2. **Channels** (telegram.rs, discord.rs, slack.rs) call `format_prefix()` and build `full_message = prefix + user_message`
3. **`Agent::process_message_with_context`** forwards `full_message` to `process_message_with_options`, which builds a `HookContext { content: full_message, ... }`
4. **`HookContext.content`** is the only message surface available to hooks. Every downstream consumer (engram recall, engram store, safety scan, intent classification) reads `ctx.content`.
5. **`Memory::session_recall(query, session_key)`** takes `query: &str` — no structured context parameter exists. Even if upstream had structured data, the API can't accept it.
6. Result: header noise reaches embedding layer, intent classifier, stored memory content — everywhere.

### Why was it built this way?

Not oversight — **incremental architecture debt**. Order of events:

- **v0.1 early**: `process_message(session_key, content: &str)` — simple, string-based API. Hooks, engram, session store all consume `&str`.
- **2026-03-29 context refactor** (MEMORY.md): introduced `MessageContext` to give the LLM sender/time/chat metadata. Chose **minimum-change path**: stringify via `format_prefix()`, prepend to content. Rationale at the time:
  - LLM must see this info anyway (for "who's asking, private or group")
  - Session store persists raw strings — in-band survives replay
  - Hooks don't need to change
  - `HookContext.metadata: serde_json::Value` exists but was used for scratch data (recall results, intero state), never as a structured context channel
- **Costs surfaced later**: engram recall quality issues became measurable only after `recall-trace.jsonl` was added (2026-04-22). No metric → no felt cost → no pressure to clean up.

The debt is real but the original decision wasn't unreasonable given what was known at the time.

## Impact

| Subsystem | Current behavior | With fix |
|---|---|---|
| Embedding (recall query) | Computed over `header + message` (~95 tokens, 80% noise) | Computed over `message` only (~15 tokens, 100% signal) |
| Intent classification (L1 regex) | Matches fail on Chinese/colloquial queries buried in English header | Direct match on actual query text |
| Intent classification (L2 Haiku, when enabled per ISS-018) | Pays tokens to classify mostly-identical headers | Classifies semantic content only |
| Stored memory content | Every user-side memory stored starts with `[TELEGRAM ... id:...]` noise | Clean content, channel/sender in structured metadata |
| Safety scan (prompt injection / secrets) | Scans header too (harmless but wasted cycles) | Scans message only |
| LLM system/user prompt | Sees header inline in user message | Sees structured context in dedicated prompt section (possibly cleaner) |

The biggest wins are **recall quality** and **stored memory cleanliness** — both compound over time because bad stored memories keep polluting future recalls.

## Non-goals

- **Not changing the LLM-visible information**: the LLM still sees sender, time, chat type, quoted message. Only the rendering layer changes.
- **Not breaking backward compatibility**: existing `process_message(session_key, content)` without `MessageContext` keeps working with `MessageContext::default()`.
- **Not redesigning engram's public API** beyond adding a context-aware recall path. The existing `session_recall(&str, &str)` stays.
- **Not fixing ISS-018** in this issue — but this is a prerequisite for ISS-018's fixes to actually help.

## Design

### Target architecture

```
┌─────────────┐   Envelope (struct)          ┌──────────────┐
│   Channel   │─────────────────────────────→│    Agent     │
│ (telegram)  │   raw_content (String)       │              │
└─────────────┘                               └──────┬───────┘
                                                     │
                ┌────────────────┬────────────────┬──┴─────────────────┐
                ▼                ▼                ▼                    ▼
          ┌──────────┐    ┌────────────┐    ┌──────────────┐    ┌──────────────┐
          │  Hooks   │    │ LLM prompt │    │  Engram      │    │  Extractor   │
          │          │    │ assembly   │    │  recall/     │    │  (Haiku)     │
          │ ctx.     │    │            │    │  store       │    │              │
          │ envelope │    │ envelope.  │    │              │    │ sees clean   │
          │ (Some)   │    │ render_for │    │ clean query; │    │ content only │
          │ ctx.     │    │ _prompt()  │    │ record meta: │    │ → produces   │
          │ content  │    │            │    │ {envelope,   │    │ dimensions{} │
          │ (clean)  │    │            │    │  dimensions} │    │              │
          └──────────┘    └────────────┘    └──────────────┘    └──────────────┘
```

Key invariants:
1. **`HookContext.content` is the semantic message only** (no header prefix).
2. **`Envelope` is the single source of truth** for channel transport metadata. Serde handles JSON mapping. Rendering for LLM is an `impl` method on the same type.
3. **Envelope ≠ Dimensions**. Both live in engram record metadata, different namespaces, different provenance:
   - `metadata.envelope` = fact from channel (zero cost, zero ambiguity)
   - `metadata.dimensions` = LLM extractor output (core_fact, participants, temporal, domain, sentiment, stance, valence, outcome, tags, type_weights, confidence)
4. **Extractor does not read envelope.** Keeps provenance clean: if participants come from envelope it's a fact; if from extractor it's an inference. Don't mix.

### New types

**`Envelope` — replaces `MessageContext` (rename, not wrapper):**

```rust
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub struct Envelope {
    pub channel: String,                    // "telegram" | "discord" | "slack" | ...
    pub sender_id: Option<String>,
    pub sender_name: Option<String>,
    pub sender_username: Option<String>,
    pub chat_type: ChatType,
    pub chat_id: Option<String>,
    pub chat_title: Option<String>,
    pub message_id: Option<String>,
    pub timestamp: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quoted: Option<QuotedMessage>,
}

impl Envelope {
    /// Derived — no stored field.
    pub fn is_group(&self) -> bool {
        matches!(self.chat_type, ChatType::Group | ChatType::Supergroup | ChatType::Channel)
    }

    /// Render for LLM prompt (replaces the old `format_prefix`).
    /// Called at prompt-construction time, NOT prepended to user content.
    pub fn render_for_prompt(&self) -> String { ... }

    /// Strip the in-band `[CHANNEL ...]` header from legacy content.
    /// **Temporary bridge** — used during Phase 2/3 only; gated by test that verifies zero callers by Phase 4.
    /// Regex anchored: `^\[(TELEGRAM|DISCORD|SLACK|SIGNAL|MATRIX|WHATSAPP) [^\]]+\]\n\n`
    /// On mismatch, returns the input unchanged (never "best-effort" strips).
    pub fn strip_from_content(content: &str) -> String { ... }
}
```

**Recall result type (new):**

```rust
/// Recalled memory enriched with the originating message's envelope (if known).
/// Produced by `session_recall_with_envelope` so the LLM can see
/// "who said this and when" alongside each memory, without envelope
/// participating in recall scoring.
pub struct AttachedRecalledMemory {
    pub content: String,
    pub memory_type: String,
    pub confidence: f64,
    pub source: Option<String>,
    pub confidence_label: Option<String>,
    pub created_at: Option<String>,
    /// Extracted from the matched record's `user_metadata.envelope`.
    /// None for records stored before the envelope refactor (pre-Phase-3 data).
    pub envelope: Option<Envelope>,
}
```

LLM-side rendering of recalled memories includes `[from: sender_name @ channel]` when `envelope` is present; falls back to current format when absent. No change to scoring, ranking, or filtering.

**`HookContext` additions:**

```rust
pub struct HookContext {
    pub session_key: String,
    pub user_id: Option<String>,
    pub channel: Option<String>,
    pub content: String,                       // ← clean message, no prefix
    pub envelope: Option<Envelope>,            // ← NEW: structured transport metadata
    pub metadata: serde_json::Value,           // ← hook-local scratch (intero, recall count, ...)
}
```

`envelope` is `Option` because non-channel paths (internal tools, tests) may not have one.

**Engram record metadata shape:**

```json
{
  "envelope": {
    "channel": "telegram",
    "sender_id": "7539582820",
    "sender_name": "potato",
    "sender_username": "potatosoupup",
    "chat_type": "direct",
    "chat_id": "...",
    "message_id": "6368",
    "timestamp": "2026-04-23T05:20:00Z",
    "quoted": null
  },
  "dimensions": {
    "core_fact": "...",
    "participants": [...],
    "temporal": "...",
    "context": "...",
    "domain": "...",
    "sentiment": "...",
    "stance": "...",
    "valence": 0.2,
    "outcome": "...",
    "tags": [...],
    "type_weights": {...},
    "confidence": 0.8
  }
}
```

- **`envelope`** produced by the channel on message arrival. `serde_json::to_value(&envelope)` — no hand-written mapping.
- **`dimensions`** produced by engram extractor (Haiku LLM call on message content). Existing schema, unchanged.
- Namespaces prevent field collisions and make provenance obvious: "if I see it under `envelope`, it's channel-given fact; under `dimensions`, it's LLM inference."

**Engram recall: context-aware variant**

Keep existing API for backward compat, add new one. **Stays in rustclaw's `Memory` wrapper — engramai is not modified.** The wrapper already simplifies engramai's `session_recall(query, wm, limit, context, min_confidence, namespace)` into `session_recall(&str, &str)`; adding envelope awareness is a wrapper-layer change.

```rust
// In rustclaw src/memory.rs (wrapper), not engramai:
impl Memory {
    pub fn session_recall(&self, query: &str, session_key: &str) -> Result<(Vec<RecalledMemory>, bool)> { ... }

    // NEW
    pub fn session_recall_with_envelope(
        &self,
        query: &str,
        session_key: &str,
        envelope: Option<&Envelope>,
    ) -> Result<(Vec<AttachedRecalledMemory>, bool)>;
}
```

`Envelope` is a rustclaw type. engramai never sees it. The wrapper extracts whatever fields it wants (for future scoring) and passes through to engramai's unchanged API. In v1 the envelope is **not** used for scoring or filtering (see Open Question #2); it's passed for plumbing and for attaching to result records.

### Information flow (who writes what)

```
Channel arrival
    │
    ├── produces Envelope (zero cost, fact)
    │       │
    │       ├──→ HookContext.envelope
    │       ├──→ LLM prompt (via render_for_prompt())
    │       └──→ engram record metadata.envelope (via serde)
    │
    └── produces clean content (String)
            │
            ├──→ HookContext.content
            ├──→ LLM prompt (as user turn body, no prefix)
            ├──→ engram recall query (embedding over clean text)
            └──→ engram extractor (Haiku input)
                    │
                    └──→ produces dimensions (LLM inference)
                            │
                            └──→ engram record metadata.dimensions
```

**Extractor never reads envelope.** Two independent sources of metadata, combined at the engram record level.

### Call-site changes

**1. Channels (telegram.rs, discord.rs, slack.rs):**

Before:
```rust
let prefix = msg_ctx.format_prefix(&channel_caps.name);
let full_message = format!("{}{}", prefix, user_message);
agent.process_message_with_options(session_key, &full_message, ...).await
```

After:
```rust
let envelope = Envelope::from_telegram_update(&update, &channel_caps.name);
agent.process_message_with_envelope(session_key, user_message, envelope, ...).await
```

Channels no longer stringify. They construct `Envelope` + pass raw content.

**2. `Agent::process_message_with_envelope` (renamed from `_with_context`):**

Before:
```rust
let prefix = msg_ctx.format_prefix(&channel_caps.name);
let full_message = format!("{}{}", prefix, user_message);
self.process_message_with_options(session_key, &full_message, ...).await
```

After:
```rust
self.process_message_with_options_and_envelope(
    session_key, user_message, Some(envelope), ...
).await
```

The new `_and_envelope` variant threads `Option<Envelope>` through to `HookContext`.

**3. `HookContext` construction** (agent.rs:975, 2596, 1454, 1562, 2688, 2776):

```rust
let mut hook_ctx = HookContext {
    session_key: session_key.to_string(),
    content: user_message.to_string(),       // clean
    envelope: envelope.clone(),              // structured
    user_id: user_id.map(String::from),
    channel: channel.map(String::from),
    metadata: serde_json::json!({}),
};
```

**4. LLM prompt construction:**

Currently: prefix baked into user message body — LLM sees it as part of what the user "said".

After: render `Envelope` into a dedicated prompt section. Two options:

- **(A) System prompt section** (preferred): `## Message Context` section regenerated per-message. LLM sees it as framing, not user speech.
- **(B) Structured user turn** (fallback): `[metadata]\n\n[actual message]` — cleaner separation than today, for providers that don't like rich system prompts.

Pick (A) primary, fall back to (B) per-provider if needed. Same information flow as today, cleaner slot.

**5. Engram recall hook** (engram_hooks.rs:125):

Before:
```rust
let recall_outcome = self.memory.session_recall(&ctx.content, &ctx.session_key);
```

After:
```rust
let recall_outcome = self.memory.session_recall_with_envelope(
    &ctx.content, &ctx.session_key, ctx.envelope.as_ref()
);
```

`ctx.content` is already clean (no header) because channels don't prepend anymore.

**6. Engram store hook** (engram_hooks.rs:442):

Current: `self.memory.store(content, type, importance, source)` — no metadata parameter.

Add wrapper method in rustclaw `src/memory.rs` (does NOT modify engramai):

```rust
// New wrapper method. Keeps existing `store(...)` unchanged for callers that don't have envelope.
pub fn store_with_envelope(
    &self,
    content: &str,
    memory_type: MemoryType,
    importance: f64,
    source: Option<&str>,
    envelope: Option<&Envelope>,
) -> anyhow::Result<()> {
    // ... same as `store`, but:
    let user_metadata = match envelope {
        Some(env) => serde_json::json!({"envelope": env}),  // serde auto-serializes
        None => serde_json::Value::Null,
    };
    let meta = StorageMeta {
        importance_hint: Some(boosted_importance),
        source: source.map(|s| s.to_string()),
        namespace: None,
        user_metadata,  // <-- this is where envelope lives
        memory_type_hint: Some(memory_type),
    };
    engram.store_raw(content, meta)  // unchanged engramai API
    // ...
}
```

Hook call-site:

```rust
let store_content = format!("{} → {}", user_msg, ctx.content.trim());
self.memory.store_with_envelope(
    &store_content,
    MemoryType::Episodic,
    0.5,
    Some("auto"),
    ctx.envelope.as_ref(),
)?;
```

Engramai's extractor runs on `content` only (unchanged). Dimensions continue to populate `EnrichedMemory.dimensions` (typed field). Envelope lives in `user_metadata.envelope` (JSON field). Two storage slots, two provenance chains, zero collision.

**7. Engram extractor** (engramai crate):

No API change required. Extractor already runs on `MemoryRecord.content`. After this refactor, content is clean → extractor gets better input → dimensions quality improves as a side effect.

**Explicit non-change**: extractor is **not** given envelope as input. Fact vs inference separation.

**8. Safety scan** (safety.rs:1624, 1662, 1678): no change — scanning clean content is strictly better.

### Migration strategy

**Phased, each phase independently shippable and testable. Phases 2+3 merged (post-audit: no temporary bridge survives the merge; see decision log).**

**Phase 1 — rename + plumbing + baseline harness (non-behavior-changing)** — ✅ **LANDED 2026-04-23** (see "Phase 1 Execution Record" at top for details + scope expansion justification)
- Rename `MessageContext` → `Envelope` (type alias for one release to let any downstream find references, then remove alias). ✅
- Derive `Serialize, Deserialize` on `Envelope`, `ChatType`, `QuotedMessage`. Fix any non-serde types. ✅
- Add `envelope: Option<Envelope>` to `HookContext` (default `None`). ✅
- ~~Add `Agent::process_message_with_options_and_envelope` parallel to existing `_and_options`.~~ Deferred to Phase 2+3 (no call sites would use it yet).
- All existing call sites still use `render_for_prompt + full_message` path (`render_for_prompt` = renamed `format_prefix`). ✅
- `envelope` field populated but unused → zero behavior change. ✅
- **Add recall quality baseline harness**: landed as `src/memory.rs::mod recall_quality_baseline` (not standalone file). ✅
- **[Expanded]** `store` / `store_explicit` migrated to `engram.store_raw` with `StorageMeta::user_metadata` plumbing — necessary precondition for envelope persistence (see execution record). ✅
- **[Expanded]** Fixtures re-designed with 3-relevant-per-fixture to fix P@3 saturation ceiling (baseline mean 0.333 → 0.767, 0.233 headroom preserves Phase 5 sensitivity). ✅
- **[Expanded]** `MemoryManager::for_testing` constructor (test-only) replaces struct-literal hand-construction. ✅
- **Verify**: 281/281 tests pass (previously 279; +2 new baseline tests). Zero new rustclaw warnings. `serde_json::to_value(&envelope)` produces expected JSON shape (covered by `envelope_plumbing_compiles`). Baseline harness runs and emits `P_before = 0.767`. Storage audit confirms 80/80 fixture items land as `Stored`, zero quarantined. ✅

**Phase 2+3 — merged: channels stop stringifying; engram consumes envelope structurally (the real fix, atomic)**

Previous v2.1 drafts split this into two phases with a `strip_from_content` temporary bridge between them. Audit (2026-04-23, FINDING-1) showed Phase 2 produced no user-visible recall-quality improvement (stripping headers at recall-time doesn't help if store-time content is still dirty for the next cycle), and the bridge violated SOUL.md's "no temporary bridges" principle with no offsetting value. Merging them eliminates the bridge entirely.

- Channels (telegram.rs, discord.rs, slack.rs) construct `Envelope` and call `process_message_with_envelope` with **raw content** (no prefix).
- `HookContext.content` is now clean at the source, at Phase 2+3 landing. No intermediate state where content is still prefixed.
- LLM prompt construction renders `Envelope::render_for_prompt()` into system prompt's `## Message Context` section.
- `EngramRecallHook` calls new wrapper `memory.session_recall_with_envelope(&ctx.content, &ctx.session_key, ctx.envelope.as_ref())`.
- `EngramStoreHook` calls new wrapper `memory.store_with_envelope(...)` which puts envelope in `user_metadata.envelope`.
- `Envelope::strip_from_content` helper **is still defined** (single-purpose utility) but has no production call sites — it exists only for the Phase 5 historical migration CLI. Production store/recall paths never call it because channels no longer prefix.
- **Verify**:
    - `recall-trace.jsonl` shows clean queries for new traffic (no `[TELEGRAM ...]` prefix)
    - New engram records have `user_metadata.envelope` populated and `dimensions` populated independently
    - Probe tests (see Baseline Specification): LLM correctly answers "now what time is it?", "which chat are we in?", "who am I?" using envelope info
    - Smoke: 5 DM + 5 group + 5 reply messages across Telegram/Discord — no regression in LLM sender/time awareness
    - **Run recall quality baseline harness again**. Because Phase 2+3 just shipped, new records are still a tiny fraction of the pool; persist score + timestamp for later comparison. Do **not** judge Phase 5 gating from this immediate measurement (see "Baseline Timing" below).

**Phase 4 — cleanup**
- Remove `MessageContext` type alias (Phase 1 compatibility).
- Deprecate old `process_message_with_options` in favor of `_with_options_and_envelope` as canonical entry (keep old for tests / internal tool calls with no envelope).
- **Verify**: `MessageContext` symbol physically absent from the codebase — `cargo check` after `rm`'ing remaining references, not a grep check (which can false-positive on comments/strings).

**Phase 5 (optional, separate commit, evidence-gated) — historical migration**

See Open Question #4 for full trigger protocol. In summary:
- CLI `rustclaw memory migrate-envelope --dry-run` scans records with no `user_metadata.envelope` and regex-matches headers in `content`.
- For records where `content` matches `^\[(TELEGRAM|DISCORD|SLACK|SIGNAL|MATRIX|WHATSAPP) [^\]]+\]\n\n`: parse header → populate `user_metadata.envelope` → rewrite `content` with header stripped → **flag for re-embedding**.
- Wet-run decision is **evidence-gated** on baseline Precision@3 delta (see "Baseline Timing" below). Default: do not run.
- Mandatory CLI flags: `--backup-to <path>` (copies `engram-memory.db` + `-wal` + `-shm`; SQLite WAL mode requires all three). No wet run without backup.

### Baseline Specification (gates Phase 5 decision)

Added to Phase 1 because the entire Phase 5 evidence-gating depends on this being precisely defined.

**Fixture composition** (10 queries, balanced):
- 2 English technical queries (e.g., "you said something about kahn's algorithm")
- 2 Chinese colloquial queries (e.g., "上周你提到的那个记忆 compiler")
- 2 code-ish queries (function name, error message)
- 2 person-name queries ("what did potato say about ISS-021")
- 2 conversation-recall queries ("我们之前聊的那个关于 recall 质量的")

Each fixture row: `{ query: String, expected_top_k_ids: Vec<MemoryId>, acceptable_substitutes: Vec<MemoryId> }`. Potato manually labels expected top-3 by looking at current DB state.

**Metric**: **Precision@3** — fraction of top-3 results that are in `expected_top_k_ids ∪ acceptable_substitutes`. Averaged across the 10 fixture rows.

**Significance threshold for Phase 5**: absolute improvement ≥ **0.15** in Precision@3 → header pollution was a dominant factor, historical migration is worth running. Improvement < 0.15 → header was not the dominant factor; do not run Phase 5; open a separate issue for the real bottleneck.

### Baseline Timing (FINDING-5 fix)

Naive trap: measure baseline at Phase 1 (on dirty DB) and again immediately after Phase 2+3 lands — but the pool is **still mostly dirty old records**, so Precision@3 barely moves, and Phase 5 gets falsely gated out.

**Correct protocol**:

1. **Phase 1 baseline**: record Precision@3 against current DB. Call this `P_before`.
2. **Phase 2+3 ships**: new memories written structurally from this point on.
3. **Isolation measurement** (the one that matters): at some point after Phase 2+3 ships, run `rustclaw memory migrate-envelope --dry-run` to produce a **hypothetical-migrated subset** (copy of DB with migration applied to the matching records). Re-run the baseline fixture against this hypothetical DB. Call this `P_if_migrated`. The delta `P_if_migrated - P_before` isolates **header pollution** as a variable — independent of how much time has passed or how many new clean records exist.
4. **Decision**: if `P_if_migrated - P_before ≥ 0.15`, run wet migration. Otherwise, don't.

This sidesteps the "new records take weeks to saturate the pool" problem by testing the counterfactual directly.

### Backward compatibility

- Internal tools / sub-agents that call `process_message` without an envelope work unchanged (`envelope: None`).
- `Memory::session_recall(&str, &str)` stays; `_with_envelope` is additive.
- Session store format unchanged — stored turns no longer include the old prefix noise, but old sessions replay fine (the header in old stored content is inert data and can be migrated separately via Phase 5).

### Risks

1. **LLM behavior drift**: moving context from user turn to system prompt section might change how the LLM treats it. Mitigation: keep the exact same text content, only move the rendering slot. Probe tests in Phase 2+3 smoke suite verify LLM still answers "what time is it?", "which chat?", "who am I?" correctly — and conversely, does not spam envelope fields into normal replies ("Hi potato (@potatosoupup on telegram at 01:30)!" is a regression).

2. **Hook ordering**: if any hook today *depends on* reading the header out of `ctx.content` (e.g., to extract timestamp), it'd break. Audit: safety.rs scans content (doesn't parse header), engram hooks store/recall (don't parse header). **No known dependency** — but Phase 1 lands the structured field before Phase 2+3 strips the prefix, so any surprise hook can read from `envelope` instead.

3. **Old stored memories**: ~N months of stored memories have `[TELEGRAM ...]` headers baked in. These keep polluting recall until they're migrated or age out via engram decay. Mitigation: Phase 5 one-time migration script — but only if baseline Precision@3 delta evidence supports it.

4. **Group chat identity**: in group chats, LLM uses "who's speaking" frequently. Need to verify the new prompt section is prominent enough that the LLM consistently reads it. Low risk — LLMs handle "System: user X says Y" patterns well.

5. **Strip regex false positive**: `Envelope::strip_from_content` must not delete user-authored content that happens to start with `[`. Mitigation: regex anchored to channel whitelist `^\[(TELEGRAM|DISCORD|SLACK|SIGNAL|MATRIX|WHATSAPP) [^\]]+\]\n\n`; 20+ unit-test fixtures covering edge cases (user message starting with `[`, all channels, with/without quoted, all timestamp formats, malformed headers). On any regex mismatch, function returns input unchanged — never "best-effort" strip. This function only runs in Phase 5 migration CLI (not production paths).

6. **Test updates**: tests in `context.rs` assert `format_prefix` output. These stay — renamed to `render_for_prompt`, same assertions. Agent integration tests updated to assert `envelope` propagation through `HookContext`.

7. **Baseline harness maintenance drift**: the Phase 1 fixture (10 manually-labeled queries) can decay — memories get consolidated/superseded, IDs drift. Mitigation: fixture stores both the `expected_top_k_ids` and an `acceptable_substitutes` list, plus the query text itself; re-label on schema migrations. Fixture failures are not CI-blocking; they're a signal to re-label, not a regression gate.

## Acceptance criteria

- [ ] `MessageContext` renamed to `Envelope`; `Serialize + Deserialize` derived; `serde_json::to_value(&envelope)` produces expected `{channel, sender_id, sender_name, chat_type, ...}` JSON
- [ ] `HookContext.envelope: Option<Envelope>` exists and is populated by all channels (telegram, discord, slack)
- [ ] `HookContext.content` contains the user's message only, with no `[CHANNEL ...]` prefix
- [ ] `EngramRecallHook` passes clean content to `session_recall_with_envelope(query, session_key, envelope)` in rustclaw's `Memory` wrapper
- [ ] `EngramStoreHook` calls `store_with_envelope(...)` which sets `StorageMeta.user_metadata = {"envelope": ...}`; engramai-side no changes
- [ ] New engram records: `user_metadata.envelope` populated by channel; `EnrichedMemory.dimensions` populated independently by extractor
- [ ] Extractor does not read `user_metadata` (verified by inspection — no reference to `user_metadata` in extractor modules)
- [ ] LLM prompt construction renders `Envelope::render_for_prompt()` into a dedicated system prompt section; LLM still correctly identifies sender/time/chat type in responses
- [ ] `recall-trace.jsonl` shows clean queries (no `[TELEGRAM ...]` prefix in `query` field) for new traces
- [ ] `Envelope::strip_from_content` has zero production call sites after Phase 2+3 (only used by Phase 5 migration CLI); enforced by `cargo check` — production modules do not import it
- [ ] All existing tests pass (166+); new tests added for:
  - `Envelope` serde round-trip
  - `HookContext.envelope` propagation
  - Recall hook uses `session_recall_with_envelope` and returns `AttachedRecalledMemory` with envelope populated for new records
  - Store hook produces `user_metadata.envelope` via `StorageMeta`
  - Extractor runs on clean content (no `[CHANNEL ...]` in input)
  - `strip_from_content` regex: 20+ fixtures covering all channels, with/without quoted, user messages starting with `[`, malformed headers (must return input unchanged on mismatch)
- [ ] **LLM probe tests** (Phase 2+3 smoke): LLM correctly answers "what time is it?" (uses envelope timestamp), "which chat are we in?" (uses chat_type), "who am I?" (uses sender_name) — AND does not spam envelope info into unrelated replies (no "Hi potato (@potatosoupup on telegram at 01:30)!" pattern)
- [ ] Real-conversation smoke test: 5 messages in Telegram DM, 5 in a group, 5 with replies — verify LLM responses still acknowledge sender/time/quoted-message correctly
- [ ] **Recall quality baseline** (Phase 1 deliverable): `tests/recall_quality_baseline.rs` with 10 fixtures (2 English, 2 Chinese, 2 code-ish, 2 person-name, 2 conversation-recall), each with `expected_top_k_ids` + `acceptable_substitutes` manually labeled; metric = Precision@3. Records `P_before` score.
- [ ] **Counterfactual baseline measurement** (Phase 5 gate): after Phase 2+3 ships, `rustclaw memory migrate-envelope --dry-run` produces a hypothetical-migrated DB copy; baseline re-runs on that copy to produce `P_if_migrated`. Delta `≥ 0.15` gates Phase 5 wet run; delta `< 0.15` means open a different issue for the real bottleneck.
- [ ] Phase 5 migration CLI: drafted, dry-runnable, and has mandatory `--backup-to <path>` flag that copies `.db` + `-wal` + `-shm` before any writes — but **not committed to executing** without evidence gate passing.

## Open questions

1. ~~Engram record metadata format~~ → **Resolved**: `{envelope: {...}, dimensions: {...}}` — two namespaces, same JSON column. `envelope` is channel fact (serde-serialized `Envelope` struct), `dimensions` is extractor's existing dimensional schema (core_fact, participants, temporal, context, domain, sentiment, stance, valence, outcome, tags, type_weights, confidence). Extractor does not read envelope.

2. **Should recall filter/boost by sender or channel?** → **Resolved**: "Pass, attach, don't score."
   - **Pass**: `session_recall_with_envelope` receives `Option<&Envelope>` — plumbing is there.
   - **Attach**: engram recall results carry the matched record's `metadata.envelope` back in the returned `RecalledMemory` — the LLM sees "who said this" alongside each memory.
   - **Don't score**: `envelope` is **not** used in recall scoring or filtering in v1. Reason: filtering/boosting by sender would hide legitimate cross-context relevance (e.g., a group discussion about GEPA is relevant when we talk about it in DM). There is no evidence yet that "sender mixing" is a recall quality problem — the measured problem is embedding pollution from in-band headers, which this refactor fixes directly. Revisit only if post-Phase-3 baseline shows sender-confusion as a residual failure mode.

3. **Should `is_heartbeat` move from `ctx.metadata` to structured?** → **Resolved: no, and the reason matters.**
   - Not because "it works fine" (that's laziness). Because `is_heartbeat` belongs to a **different, yet-unformed concept space** — execution/runtime state (is_heartbeat, ritual_phase, iteration_count, budget_remaining, ...) — rather than channel transport metadata.
   - Conflating it into `Envelope` would muddle Envelope's purpose ("facts about how this message arrived") with "facts about the current execution context".
   - When/if that execution-context layer needs formalizing, open a separate issue for an `ExecutionContext` struct. Out of scope here.

4. **Migration of historical memories**: opt-in script or automatic on startup? → **Resolved: conditional, evidence-gated, counterfactually measured.**
   - Phase 5 is **not** a default todo. It is a decision point triggered by a counterfactual measurement, not a wall-clock gap.
   - **Trigger protocol** (isolated variable — doesn't wait weeks for pool turnover):
     - Phase 1 records baseline Precision@3 on current DB: `P_before`.
     - Phase 2+3 ships.
     - At any time after Phase 2+3 lands, run `rustclaw memory migrate-envelope --dry-run --to-shadow-db <path>` which produces a **hypothetical-migrated copy** of the DB (headers stripped, envelopes populated, embeddings re-computed) without touching production.
     - Re-run the baseline fixture against the shadow DB: `P_if_migrated`.
     - If `P_if_migrated - P_before ≥ 0.15`: header pollution was a dominant factor → **run wet migration**.
     - If delta `< 0.15`: header pollution wasn't dominant → **do not run Phase 5**. Open a different issue for the real bottleneck (embedding model, scoring, chunking, context window).
   - Migration (when run): **always** on a DB copy first (`--backup-to` flag is mandatory, copies `.db` + `-wal` + `-shm`), **never** auto-on-startup. Destructive to embeddings (they get recomputed); non-reversible without backup.
   - **Honest disclosure**: I don't currently have evidence that fixing header pollution will measurably improve recall quality. The architectural problem (embedding contamination) is real and worth fixing regardless — but the recall-quality *outcome* is a hypothesis until the counterfactual measurement is done. The counterfactual gate forces the hypothesis to be tested on an isolated variable before spending effort on historical migration.

## Related work

- **ISS-018** (recall intent classification): depends on this. Haiku L2 classifier fed clean content will classify accurately; fed header-polluted content it wastes tokens and mis-classifies.
- **Engram extractor dimensional schema** (already exists): this refactor unifies channel `envelope` with extractor `dimensions` under a single `metadata` JSON column, two namespaces. No change to extractor itself; it benefits passively from cleaner input.
- **Engram v0.3** (future): sender affinity, time-weighted scoring, cross-channel linkage — all have a place to live once `envelope` is in engram metadata.
- `recall-trace.jsonl` (added 2026-04-22): the measurement that made this cost visible. Keep it; it's how we verify the fix landed.

## Decision log

- **2026-04-23 conception**: potato noticed `[TELEGRAM ...]` headers in recall queries while debugging ISS-018 recall quality. Identified architectural issue: header should be side-channel, not in-band. ISS-021 opened (v1).
- **2026-04-23 v2 — Envelope rename + unify with dimensions**: potato asked "is this clean?" and pointed out the extractor already has a dimensional schema — are we inventing a parallel structure? Realized: (a) `MessageContext` name + projection layer was a temporary bridge we were about to build; (b) channel metadata and extractor dimensions should share engram's existing JSON column under two clear namespaces (`envelope`, `dimensions`); (c) `Envelope` with `Serialize + Deserialize` + `render_for_prompt()` method collapses the three-way representation (Rust struct / Hook field / engram JSON) into one source of truth with auto-derived projections. 10/10 design.
- **2026-04-23 SOUL.md**: "No temporary bridges" added to Engineering Philosophy, citing ISS-021 v1's original `MessageContext`-prepend pattern as the cautionary tale. Phase 2's `strip_from_content` is explicitly called out as an intentionally-bounded, test-gated exception.
- **2026-04-23 v2.1 — Open questions tightened with trigger conditions**: #2 refined from "pass but don't use" to "pass, attach to recall results, don't score" (attaching is free and useful, scoring is unjustified). #3 kept as "don't move" but reason sharpened: `is_heartbeat` belongs to a separate execution-context concept space, not Envelope. #4 Phase 5 migration made **evidence-gated**: gate on Phase 1 recall quality baseline delta, not on "Phase 5 exists therefore we run it". Added baseline test as Phase 1 acceptance criterion. Honest disclosure added: recall-quality improvement is a hypothesis, not measured — the architectural fix is justified regardless, but the outcome claim is gated on evidence.
- **2026-04-23 v2.2 — self-review audit + engramai data model alignment**: potato asked "review again, confirm no conflicts, no new tech debt". Found and fixed:
  - **engramai data model mismatch**: earlier drafts described "`metadata.envelope` + `metadata.dimensions` namespaces in one JSON column". Real engramai model: `dimensions` is a typed `Dimensions` struct field on `EnrichedMemory`, NOT a JSON sub-key. Envelope lives in `user_metadata: serde_json::Value` (already exists, currently `Null`). Corrected throughout design.
  - **No engramai changes needed**: `StorageMeta.user_metadata` and `MemoryRecord.metadata` already accept JSON. Wrapper-layer change only; engramai crate untouched.
  - **Cross-crate coupling was a false alarm**: rustclaw's `Memory` wrapper already simplifies engramai's 6-param `session_recall` to 2-param. Adding envelope awareness stays in the wrapper; engramai's public API unchanged.
  - **Merged Phase 2+3**: previous split required a `strip_from_content` temporary bridge in Phase 2 with no offsetting user-visible value (recall-time stripping doesn't help if store-time is still dirty one cycle later). Bridge violated SOUL.md "no temporary bridges" without justification. Merging Phase 2+3 eliminates the bridge entirely — `strip_from_content` is retained as a pure utility for Phase 5 migration CLI only, zero production call sites.
  - **Baseline specification tightened**: replaced vague "numeric score" with Precision@3 on 10 balanced fixtures; defined 0.15 absolute-delta significance threshold.
  - **Counterfactual baseline measurement**: replaced "wait and see" timing with a shadow-DB dry-run that isolates header pollution as a variable, independent of how long new records take to saturate the pool.
  - **Added `AttachedRecalledMemory` spec**: resolves the Open Q#2 "attach" action that was previously textually resolved but had no type defined.
  - **LLM probe tests added**: concrete queries ("what time is it?", "who am I?") for Phase 2+3 smoke suite, plus negative test (LLM should NOT spam envelope info into unrelated replies).
  - **Strip regex hardening**: 20+ fixture tests, whitelist-anchored pattern, mismatch returns input unchanged (never best-effort).
  - **Phase 4 cleanup verification**: `cargo check` after `rm`'ing symbols, not grep (grep false-positives on comments/strings).
