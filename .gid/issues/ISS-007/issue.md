---
id: "ISS-007"
title: "Engram Recall Quality Fixes"
status: blocked
priority: P2
created: 2026-04-19
component: "src/memory.rs"
depends_on: ["engram:ISS-032"]
note: "2 of 3 bugs fixed in rustclaw; bug 3 migrated to engram ISS-032."
---
# ISS-007: Engram Memory Recall Quality — Three Bugs

**Status:** 🟡 Blocked on engram ISS-032 — Bug 1 ✅ fixed (rustclaw), Bug 2 ✅ fixed (rustclaw), Bug 3 → migrated to engram ISS-032
**Priority:** Medium (RustClaw-side work complete; awaiting engramai release)
**Components:** `src/memory.rs`, `src/engram_hooks.rs` (RustClaw side complete)
**Discovered:** 2026-04-10
**Reporter:** potato + RustClaw (code-level investigation)
**Last verified:** 2026-04-25

---

## Executive Summary

Three bugs in the Engram memory recall pipeline were originally reported here. Bugs 1 and 2 lived on the RustClaw side and have been fixed. Bug 3 lives entirely inside the engramai crate and was migrated to the engram monorepo on 2026-04-25 as **engram ISS-032** (`/Users/potato/clawd/projects/engram/.gid/issues/ISS-032-cached-recall-confidence.md`). This issue stays open in RustClaw until engram ships the fix and RustClaw bumps the engramai dependency.

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

## Bug 3: Broken Confidence in Cached WM Path → Migrated

**Status:** Migrated to **engram ISS-032** on 2026-04-25.

This bug lives entirely inside the engramai crate (`crates/engramai/src/memory.rs` lines 4164, 4314, 4685, plus a redundant probe in the cached path). RustClaw is a downstream consumer — there is no fix to apply here.

See `/Users/potato/clawd/projects/engram/.gid/issues/ISS-032-cached-recall-confidence.md` for the full report (problem, root cause, three sub-fixes, verification plan).

**RustClaw's role going forward:**
1. Wait for engram ISS-032 to land and engramai to publish a new version.
2. Bump `engramai = "x.y.z"` in `Cargo.toml`.
3. Run RustClaw's recall regression tests against the new version.
4. Close ISS-007.

---

## Implementation Order

| Fix | Status | Where |
|-----|--------|-------|
| Bug 1: Use r.confidence | ✅ Done | rustclaw `src/memory.rs` |
| Bug 2: SessionRegistry | ✅ Done | rustclaw `src/memory.rs`, `src/engram_hooks.rs` |
| Bug 3: Cached path confidence | 🔄 Migrated | engram ISS-032 |

---

## Files Modified (RustClaw side)

- `src/memory.rs` — confidence mapping fix (Bug 1, 4 call sites); `wm: Mutex<SessionWorkingMemory>` → `wm_registry: Mutex<SessionRegistry>` (Bug 2); `session_recall(query, session_key)` API
- `src/engram_hooks.rs` — threads `session_key` into `memory.session_recall` (Bug 2)

---

## Verification

### Bug 1 ✅
- All 4 `RecalledMemory` construction sites bind `confidence: r.confidence` (line 425, 538, 567, 663)
- `confidence_label` and numeric `confidence` now correspond
- Before: confidence could be 3.7 with label "medium". After: confidence=0.65 with label "medium".

### Bug 2 ✅
- `MemoryManager` now holds `wm_registry: Mutex<SessionRegistry>` (line 136, 181, 286)
- Test fixtures at lines 1632, 1798, 2000, 2020 confirm session-scoped behavior
- Sub-agents and different chats no longer share a single WM

### Bug 3 → engram ISS-032
- Verification plan lives in engram ISS-032
- RustClaw verification on dependency bump: existing recall regression tests must pass; sample `engram_recall` traces should no longer collapse to `[low]` labels in steady-state conversation

---

## Implementation Record

### 2026-04-25 — Bugs 1 & 2 verified fixed; Bug 3 migrated

While auditing open issues, code inspection of `src/memory.rs` showed Bug 1 and
Bug 2 had already been fixed in earlier work but the issue header was never
updated.

**Bug 1 — Confidence mapping** ✅
- All 4 `RecalledMemory` construction sites in `src/memory.rs` (lines 425, 538,
  567, 663) now bind `confidence: r.confidence` rather than `r.activation`.

**Bug 2 — Session isolation** ✅
- `MemoryManager` now holds `wm_registry: Mutex<SessionRegistry>` instead of
  the old global `wm: Mutex<SessionWorkingMemory>`.
- `session_recall(query, session_key)` accepts a `session_key` and routes
  through the registry.

**Bug 3 — Cached WM confidence** → migrated to **engram ISS-032**
- The fix lives entirely inside engramai. Filing it under rustclaw was a
  category error; the report has been moved to
  `/Users/potato/clawd/projects/engram/.gid/issues/ISS-032-cached-recall-confidence.md`
  with the full problem statement, three sub-fixes (3a redundant probe,
  3b carry-forward confidence, 3c call-site audit), and verification plan.
- ISS-007 stays open in RustClaw, status "Blocked on engram ISS-032",
  until engramai ships the fix and RustClaw bumps the dep.

## Next Step

Run a `start_ritual` against the engram project for ISS-032. Once engramai
publishes a new version with the fix, bump the dep in RustClaw's `Cargo.toml`,
re-run recall regression tests, and close this issue.
