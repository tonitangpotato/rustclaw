# ISS-007: Engram Memory Recall Quality — Three Bugs

**Status:** Open  
**Priority:** High  
**Components:** `src/memory.rs`, `src/engram_hooks.rs`, engramai `src/memory.rs`  
**Discovered:** 2026-04-10  
**Reporter:** potato + RustClaw (code-level investigation)

---

## Executive Summary

Three bugs in the Engram memory recall pipeline cause inaccurate confidence scoring and cross-session contamination. Together they degrade recall quality — memories appear with misleading confidence labels, unrelated memories from other conversations leak into the current session, and the cached path produces uniformly low (~0.2) confidence regardless of actual relevance.

---

## Bug 1: Dual Scoring — activation used as confidence

**Severity:** Medium  
**Location:** `src/memory.rs` lines 258-267, 344-351  
**Root cause:** RustClaw bridges `RecallResult.activation` (raw ACT-R value) to `RecalledMemory.confidence`, ignoring the properly computed `RecallResult.confidence` field.

### The Problem

engramai's `RecallResult` has two distinct fields:
- `activation` — raw ACT-R base-level activation (log-scale, unbounded, used for **sorting**)
- `confidence` — normalized 0.0-1.0 score combining embedding similarity, FTS match, entity overlap, and recency (used for **display/trust**)

RustClaw's mapping in both `recall()` and `session_recall()`:
```rust
RecalledMemory {
    content: r.record.content.clone(),
    memory_type: format!("{:?}", r.record.memory_type),
    confidence: r.activation,  // ← BUG: should be r.confidence
    source: Some(r.record.source.clone()),
    confidence_label: Some(r.confidence_label),  // ← correct (from r.confidence)
}
```

### Impact

- The `confidence_label` (e.g., "high", "medium") is derived from the correct `r.confidence` value inside engramai
- But `RecalledMemory.confidence` (the number) comes from `r.activation`
- Any downstream code using `confidence` numerically gets the wrong value
- The label and number don't correspond: a "medium" confidence label might show confidence=3.7 (raw ACT-R activation)

### Fix

Replace `r.activation` with `r.confidence` in both `recall()` and `session_recall()`.

---

## Bug 2: Global Singleton Working Memory (CRITICAL)

**Severity:** Critical  
**Location:** `src/memory.rs` line 52, 137-138, 332-354  
**Root cause:** `MemoryManager` maintains a single `SessionWorkingMemory` instance shared across ALL sessions (Telegram chats, Discord, CLI, heartbeat, sub-agents).

### The Problem

```rust
pub struct MemoryManager {
    // ...
    wm: Mutex<SessionWorkingMemory>,  // ← ONE instance, global
}
```

All calls to `session_recall()` share this single WM. There is no session isolation:

1. User A chats about "trading strategies" → WM caches trading memory IDs
2. User B (or same user in different chat) asks about "Rust compilation" 
3. The Jaccard overlap check may still match old WM entries → skips full recall
4. Result: User B gets stale trading memories mixed in, or misses relevant Rust memories

engramai provides `SessionRegistry` (a `HashMap<String, SessionWorkingMemory>`) for exactly this purpose, but RustClaw never uses it.

### Impact

- Cross-session memory pollution — memories from unrelated conversations contaminate each other
- The 0.6 Jaccard threshold makes it worse: even 2/5 overlapping memory IDs trigger the cached path
- Most severe with rapid context-switching (multiple Telegram chats, heartbeat interleaved with user messages)
- Sub-agents share the same WM as the main session

### Fix

Replace global `wm: Mutex<SessionWorkingMemory>` with `wm: Mutex<SessionRegistry>`. Thread `session_key` through from `HookContext` to `MemoryManager::session_recall()`.

---

## Bug 3: Broken Confidence in Cached WM Path (CRITICAL)

**Severity:** Critical  
**Location:** engramai `src/memory.rs` lines 1659-1680  
**Root cause:** When session_recall uses the cached WM path (topic continuous), `compute_query_confidence()` receives zero values for all meaningful signals.

### The Problem

In the cached WM path (`session_recall_ns`), for each active memory ID:
```rust
let confidence = compute_query_confidence(
    None,   // no embedding similarity
    false,  // not an FTS match
    0.0,    // no entity score
    age_hours,
);
```

With `compute_query_confidence`'s weights:
- Embedding (0.55 weight) → None → 0
- FTS (0.20 weight) → false → 0
- Entity (0.20 weight) → 0.0 → 0
- Recency (0.05 weight) → only non-zero signal

Result: `confidence ≈ 0.05 * recency / 0.45` ≈ 0.11-0.22 regardless of actual relevance.
All memories get `confidence_label: "very low"` or `"low"` in the cached path.

Additionally, a **redundant second probe** is executed purely to calculate `continuity_ratio` for metrics:
```rust
// After already returning cached results:
let probe = self.recall_from_namespace(query, 3, None, None, namespace)?;
```
This is a full recall (embedding + FTS + scoring) of 3 items — wasted computation.

### Impact

- All cached WM memories appear as "very low" or "low" confidence
- The `min_confidence` filter may exclude perfectly relevant memories
- The label "low" undermines trust in recalled memories ("You may have prior context" + "[low]" = agent ignores it)
- Redundant probe adds ~50ms latency per cached recall

### Fix (engramai side)

Two sub-fixes:
1. **Restore confidence for cached results**: Either re-compute embedding similarity for cached items, or carry the original confidence/similarity from the full recall that populated the WM in the first place.
2. **Eliminate redundant probe**: The continuity_ratio metric is informational only. Remove the second probe call, or compute ratio from the initial probe (already done before entering the cached path).

---

## Implementation Order

| Fix | Complexity | Risk | Dependency |
|-----|-----------|------|------------|
| Bug 1: Use r.confidence | Trivial (2 lines) | None | None |
| Bug 3a: Eliminate redundant probe | Low | None | None (engramai) |
| Bug 3b: Restore confidence in cached path | Medium | Low | Bug 1 done first |
| Bug 2: SessionRegistry | Medium | Low-Medium | Bug 1 done first |

Recommended order: Bug 1 → Bug 3a → Bug 3b → Bug 2

---

## Files to Modify

### RustClaw side
- `src/memory.rs` — Fix confidence mapping (Bug 1), replace global WM with SessionRegistry (Bug 2), thread session_key through API
- `src/engram_hooks.rs` — Pass session_key to memory.session_recall (Bug 2)

### engramai side  
- `src/memory.rs` — Fix cached WM confidence (Bug 3b), remove redundant probe (Bug 3a)
- `src/session_wm.rs` — Potentially extend SessionWorkingMemory to cache confidence scores

---

## Verification

### Bug 1
- Unit test: Verify `RecalledMemory.confidence` matches the label ranges (high: ≥0.8, medium: 0.5-0.79, low: 0.2-0.49)
- Before: confidence could be 3.7 with label "medium"
- After: confidence=0.65 with label "medium"

### Bug 2
- Integration test: Two sequential session_recall calls with different session_keys should have independent WM states
- Before: second call may return stale results from first session's WM
- After: each session gets its own WM via SessionRegistry

### Bug 3
- Unit test: Cached WM path returns confidence comparable to full recall path for the same memories
- Before: cached path confidence ≈ 0.2 always
- After: cached path confidence matches the score from original full recall
