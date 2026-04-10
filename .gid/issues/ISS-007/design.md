# ISS-007: Design — Engram Recall Quality Fixes

**Issue:** ISS-007  
**Scope:** 4 fixes across 2 crates (RustClaw + engramai)  

---

## §1 Overview

Four targeted fixes to restore correct memory recall behavior. No new features, no architecture changes — pure bug fixes with surgical precision.

**Non-goals:**
- No new recall algorithms
- No API changes beyond threading session_key
- No performance optimization beyond removing the redundant probe

---

## §2 Fix 1: Confidence Mapping (RustClaw)

**File:** `src/memory.rs`  
**Lines:** ~260 and ~347  
**Change:** 2-line replacement

### Current (broken)
```rust
// In recall()
RecalledMemory {
    confidence: r.activation,  // raw ACT-R, unbounded
    // ...
}

// In session_recall()
RecalledMemory {
    confidence: r.activation,  // same bug
    // ...
}
```

### After (fixed)
```rust
// In recall()
RecalledMemory {
    confidence: r.confidence,  // normalized 0.0-1.0
    // ...
}

// In session_recall()
RecalledMemory {
    confidence: r.confidence,  // same fix
    // ...
}
```

**Also fix in:** `recall_associated()` — same pattern at ~375.

### Verification
```rust
#[test]
fn test_recalled_memory_confidence_in_range() {
    // After recall, every RecalledMemory.confidence must be 0.0..=1.0
    // and must correspond to confidence_label thresholds:
    //   high:     >= 0.8
    //   medium:   0.5..0.8
    //   low:      0.2..0.5
    //   very low: < 0.2
}
```

---

## §3 Fix 2: Session-Isolated Working Memory (RustClaw)

**Files:** `src/memory.rs`, `src/engram_hooks.rs`, `src/hooks.rs`

### §3.1 Replace global WM with SessionRegistry

**`src/memory.rs`:**

```rust
// Before:
pub struct MemoryManager {
    engram: Mutex<Memory>,
    wm: Mutex<SessionWorkingMemory>,  // single global instance
    // ...
}

// After:
use engramai::SessionRegistry;

pub struct MemoryManager {
    engram: Mutex<Memory>,
    wm_registry: Mutex<SessionRegistry>,  // per-session instances
    // ...
}
```

**Constructor change:**
```rust
// Before:
let wm = SessionWorkingMemory::new(15, WORKING_MEMORY_DECAY_SECS);

// After:
let wm_registry = SessionRegistry::with_defaults(15, WORKING_MEMORY_DECAY_SECS);
```

**Field change:**
```rust
// Before:
wm: Mutex::new(wm),

// After:
wm_registry: Mutex::new(wm_registry),
```

### §3.2 Thread session_key through session_recall

**`src/memory.rs` — session_recall signature change:**
```rust
// Before:
pub fn session_recall(&self, query: &str) -> anyhow::Result<(Vec<RecalledMemory>, bool)>

// After:
pub fn session_recall(&self, query: &str, session_key: &str) -> anyhow::Result<(Vec<RecalledMemory>, bool)>
```

**Body change:**
```rust
// Before:
let mut wm = self.wm.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;

// After:
let mut registry = self.wm_registry.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
let wm = registry.get_session(session_key);
```

The rest of the function body is unchanged — `wm` is still a `&mut SessionWorkingMemory`.

### §3.3 Pass session_key from hooks

**`src/engram_hooks.rs` — EngramRecallHook::execute:**
```rust
// Before:
match self.memory.session_recall(&ctx.content) {

// After:
match self.memory.session_recall(&ctx.content, &ctx.session_key) {
```

No changes to HookContext needed — `session_key` is already populated by the framework.

### §3.4 Session cleanup

Add a method to MemoryManager for periodic cleanup (called from heartbeat):
```rust
/// Remove empty sessions from the registry.
pub fn prune_sessions(&self) -> anyhow::Result<usize> {
    let mut registry = self.wm_registry.lock()
        .map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
    registry.prune_all();
    Ok(registry.remove_empty_sessions())
}
```

### Verification
```rust
#[test]
fn test_session_isolation() {
    // Two calls with different session_keys should use independent WMs.
    // 1. session_recall("trading", "session-A") → populates session-A WM
    // 2. session_recall("rust compiling", "session-B") → populates session-B WM
    // 3. session_recall("more trading", "session-A") → hits session-A WM (not B)
    // Verify session-B never sees trading memories from cached path.
}
```

---

## §4 Fix 3a: Remove Redundant Probe (engramai)

**File:** engramai `src/memory.rs`, in `session_recall_ns()`, cached path  
**Lines:** ~1690-1694

### Current (wasteful)
```rust
// After building cached_results, ANOTHER probe for metrics only:
let probe = self.recall_from_namespace(query, 3, None, None, namespace)?;
let probe_ids: Vec<String> = probe.iter().map(|r| r.record.id.clone()).collect();
let (_, ratio) = session_wm.overlap(&probe_ids);
```

### After (fixed)
```rust
// Reuse the continuity ratio from the initial probe that triggered the cached path.
// The probe was already done in the need_full_recall check above.
// We can't easily pass it down, so just use 1.0 (we already know topic is continuous).
let ratio = 1.0;  // We're in the cached path because continuity was already confirmed
```

More precisely: restructure the code to capture the initial probe's overlap ratio and pass it to the result, rather than probing twice.

### Detailed restructure

```rust
// Before the need_full_recall decision:
let (initial_ratio, need_full_recall) = if active_ids.is_empty() {
    (0.0, true)
} else {
    let probe = self.recall_from_namespace(query, 3, context.clone(), min_confidence, namespace)?;
    let probe_ids: Vec<String> = probe.iter().map(|r| r.record.id.clone()).collect();
    let (_, ratio) = session_wm.overlap(&probe_ids);
    if ratio >= CONTINUITY_THRESHOLD {
        (ratio, false)  // continuous
    } else {
        (ratio, true)   // topic changed
    }
};

// In the cached path, use initial_ratio directly:
Ok(SessionRecallResult {
    results: cached_results,
    full_recall: false,
    wm_size: session_wm.len(),
    continuity_ratio: initial_ratio,  // ← no second probe needed
})
```

### Verification
- Ensure `session_recall` only calls `recall_from_namespace` once in the cached path (was calling it twice).
- Performance test: cached path should be ~50ms faster.

---

## §5 Fix 3b: Restore Confidence in Cached WM Path (engramai)

**File:** engramai `src/memory.rs`, in `session_recall_ns()`, cached path  
**Lines:** ~1659-1680

### The Problem

Cached results call `compute_query_confidence(None, false, 0.0, age_hours)` — all meaningful signals are zero. Only recency (weight 0.05 out of 0.45 max) contributes, yielding ~0.15 confidence.

### Design Decision: Cache original scores

The cleanest fix is to store the confidence and embedding similarity from the original full recall in the `SessionWorkingMemory`, so the cached path can reuse them.

### §5.1 Extend SessionWorkingMemory

**`engramai/src/session_wm.rs`:**
```rust
pub struct SessionWorkingMemory {
    capacity: usize,
    decay_duration: Duration,
    items: HashMap<String, Instant>,
    /// Cached scores from original full recall
    scores: HashMap<String, CachedScore>,  // ← NEW
    last_query: Option<String>,
}

/// Scores cached from the full recall that populated this item.
#[derive(Debug, Clone)]
pub struct CachedScore {
    pub confidence: f64,
    pub activation: f64,
}
```

**New method on SessionWorkingMemory:**
```rust
/// Activate memory IDs with their scores for cached recall.
pub fn activate_with_scores(&mut self, entries: &[(String, f64, f64)]) {
    let now = Instant::now();
    for (id, confidence, activation) in entries {
        self.items.insert(id.clone(), now);
        self.scores.insert(id.clone(), CachedScore {
            confidence: *confidence,
            activation: *activation,
        });
    }
    self.prune();
}

/// Get cached score for a memory ID.
pub fn get_score(&self, id: &str) -> Option<&CachedScore> {
    self.scores.get(id)
}
```

**Update prune() to also clean scores:**
```rust
pub fn prune(&mut self) {
    let now = Instant::now();
    self.items.retain(|_, activated_at| {
        now.duration_since(*activated_at) < self.decay_duration
    });
    // Clean scores for pruned items
    self.scores.retain(|id, _| self.items.contains_key(id));
    // capacity pruning...
}
```

### §5.2 Store scores during full recall path

**In `session_recall_ns()`, full recall branch:**
```rust
// Before:
let result_ids: Vec<String> = results.iter().map(|r| r.record.id.clone()).collect();
session_wm.activate(&result_ids);

// After:
let entries: Vec<(String, f64, f64)> = results.iter()
    .map(|r| (r.record.id.clone(), r.confidence, r.activation))
    .collect();
session_wm.activate_with_scores(&entries);
```

### §5.3 Use cached scores in cached path

**In `session_recall_ns()`, cached branch:**
```rust
// Before:
let confidence = compute_query_confidence(None, false, 0.0, age_hours);

// After:
let confidence = if let Some(cached) = session_wm.get_score(&id) {
    cached.confidence  // reuse original full-recall confidence
} else {
    // Fallback: memory was activated by ID only (legacy path)
    compute_query_confidence(None, false, 0.0, age_hours)
};

let activation = if let Some(cached) = session_wm.get_score(&id) {
    cached.activation
} else {
    retrieval_activation(/* ... existing code ... */)
};
```

### Verification
```rust
#[test]
fn test_cached_path_preserves_confidence() {
    // 1. Full recall: get results with confidence C
    // 2. Cached recall (same topic): get same results
    // 3. Assert cached confidence == original confidence C
    // Before fix: cached confidence ≈ 0.15 regardless
    // After fix: cached confidence == C
}
```

---

## §6 Summary of All Changes

### RustClaw (`src/memory.rs`)
| Change | Lines affected | Risk |
|--------|---------------|------|
| `r.activation` → `r.confidence` (3 places) | ~3 lines | None |
| `wm: Mutex<SessionWorkingMemory>` → `wm_registry: Mutex<SessionRegistry>` | ~10 lines | Low |
| `session_recall()` signature + session_key threading | ~5 lines | Low |
| Add `prune_sessions()` method | ~8 lines new | None |

### RustClaw (`src/engram_hooks.rs`)
| Change | Lines affected | Risk |
|--------|---------------|------|
| Pass `&ctx.session_key` to `session_recall` | 1 line | None |

### engramai (`src/memory.rs`)
| Change | Lines affected | Risk |
|--------|---------------|------|
| Remove redundant probe in cached path | ~5 lines removed | None |
| Capture initial probe ratio | ~10 lines restructured | Low |
| Use cached scores instead of zero-signals | ~10 lines changed | Low |
| Store scores in full recall path | ~5 lines changed | None |

### engramai (`src/session_wm.rs`)
| Change | Lines affected | Risk |
|--------|---------------|------|
| Add `scores: HashMap` field | ~3 lines | None |
| Add `CachedScore` struct | ~6 lines new | None |
| Add `activate_with_scores()` | ~12 lines new | None |
| Add `get_score()` | ~4 lines new | None |
| Update `prune()` to clean scores | ~2 lines | Low |

**Total: ~85 lines changed/added across 4 files. No API breaking changes.**

---

## §7 Backward Compatibility

- `activate()` method remains unchanged — `activate_with_scores()` is additive
- `session_recall()` gains a `session_key` parameter — all callers in RustClaw are internal (2 call sites), easily updated
- No changes to `RecallResult` struct in engramai
- `SessionWorkingMemory::new()` still works (scores map starts empty)
- Old WM items without cached scores gracefully fall back to current behavior
