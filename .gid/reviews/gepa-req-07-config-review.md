# Review: requirements-07-config.md (GEPA Configuration)

> Reviewed: 2026-04-04 | Reviewer: Claude Code (subagent)
> Document: `.gid/features/gepa-core/requirements-07-config.md`
> Context: `.gid/features/gepa-core/requirements-master.md` (GUARDs, Out of Scope)
> Cross-refs checked: requirements-01, 02, 03, 04, 06, 08

---

## 🔴 Critical (blocks implementation)

### FINDING-1
**[Check #4] GOAL-7.1: Massive compound requirement — not atomic**
GOAL-7.1 is a single requirement that enumerates ~14 distinct configuration parameters with defaults, cross-references 4 other GOALs, and defines the "core set" concept. This is not one requirement — it's a table of contents for the entire config struct. Two engineers would likely implement the same struct, but testability and traceability suffer: a test for "GOAL-7.1" would need to check 14+ fields simultaneously.

Suggested fix: GOAL-7.1 should remain as an **inventory/overview** requirement stating "GEPAConfig includes AT MINIMUM the parameters listed in GOAL-7.2 through GOAL-7.7", and each parameter cluster should be defined within its respective GOAL. The current re-listing of retry params (already in GOAL-7.5), re-eval params (already cross-referenced from GOAL-8.5), and min_shared_examples (already in GOAL-2.1) creates duplication. Trim GOAL-7.1 to:
```
GOAL-7.1 [P0]: `GEPAConfig` includes all parameters defined in GOAL-7.2 through GOAL-7.7, 
plus: optional RNG seed (Option<u64>, default: random — per GUARD-9), 
min_shared_examples (default: minibatch_size — per GOAL-2.1), and 
max_re_eval_per_iteration (default: pareto_max_size × minibatch_size / 2 — per GOAL-1.7c).
The full parameter list is the union of all fields specified across GOAL-7.1 through GOAL-7.7.
```

### FINDING-2
**[Check #9] GOAL-7.3: Incomplete boundary condition list — no validation for retry/backoff/time-budget/re-eval params**
GOAL-7.3 defines invalid conditions only for: `minibatch_size=0`, `max_iterations=0`, `stagnation_limit > max_iterations`, `pareto_max_size < 1`, `min_shared_examples=0`. But the config also includes:
- `retry_max` (GOAL-7.5): what if 0? Is that "no retries" (valid) or invalid?
- `base_delay` (GOAL-7.5): what if 0 or negative? What if Duration::MAX?
- `max_consecutive_skips` (GOAL-7.5): what if 0? That means halt on the very first skip — is that intended as valid?
- `re_eval_interval` (GOAL-7.1/8.5): what if 0? Every iteration? That's expensive but is it invalid?
- `re_eval_sample_size` (GOAL-7.1/8.5): what if 0?
- `max_re_eval_per_iteration` (GOAL-7.1/1.7c): what if 0? That disables re-evaluation — is that valid?
- `checkpoint_interval` (GOAL-7.2): what if 0?
- `merge_interval` (GOAL-7.7): what if 0?
- `time_budget` (GOAL-7.6): what if Duration::ZERO?

Without explicit valid ranges, each implementer/tester will invent different boundaries.

Suggested fix: Add to GOAL-7.3:
```
Additional invalid conditions: checkpoint_interval=0, retry_max with base_delay=0 
when strategy=exponential (exponential backoff of 0 is always 0), 
re_eval_interval=0, re_eval_sample_size=0, max_re_eval_per_iteration=0 
(use None/disabled pattern instead), merge_interval=0 when merge enabled. 
Valid edge cases (explicitly allowed): retry_max=0 (no retries, immediate skip/halt), 
max_consecutive_skips=0 (halt on first skip), time_budget of Duration::ZERO 
(immediately terminate — treated as "run at most 1 iteration").
```

---

## 🟡 Important (should fix before implementation)

### FINDING-3
**[Check #5] GOAL-7.6: Missing actor/trigger detail — when exactly is time checked?**
GOAL-7.6 says "checks elapsed time at the start of each iteration." But what is the epoch? When does the clock start — at `engine.run()` call? At construction? At first iteration start? Also: if an iteration starts under budget but finishes over budget, does it still count? The answer matters for correctness.

Suggested fix: Clarify: "Wall-clock timer starts when `engine.run()` begins (after any resumption setup). The check occurs at the start of each iteration BEFORE the Select step. An iteration that started within budget runs to completion even if it exceeds the budget during execution."

### FINDING-4
**[Check #7] GOAL-7.5: Retry policy missing interaction with RateLimited errors**
GOAL-7.5 defines retry with fixed/exponential backoff. GOAL-3.7 defines `RateLimited { retry_after: Option<Duration> }`. But GOAL-7.5 doesn't specify how `retry_after` from the adapter interacts with the configured backoff. Does `retry_after` override the backoff? Use max(retry_after, backoff)? What if `retry_after` is much longer than the configured backoff?

Suggested fix: Add to GOAL-7.5: "When the adapter returns `RateLimited { retry_after: Some(d) }`, the engine uses `max(d, computed_backoff)` as the delay before the next retry. When `retry_after` is None, the engine uses the configured backoff strategy."

### FINDING-5
**[Check #1] GOAL-7.7: "Complementary vs random" merge selection strategy is underspecified**
GOAL-7.7 says merge selection strategy is "complementary vs random" but doesn't define "complementary" here. GOAL-4.4 defines it precisely (maximize |A_better ∪ B_better|). GOAL-7.7 should at least reference GOAL-4.4 for the definition, or the config requirement stands alone as vague.

Suggested fix: Change GOAL-7.7 to: "...merge selection strategy (complementary per GOAL-4.4, or random, default: complementary)."

### FINDING-6
**[Check #11] GOAL-7.1 & GOAL-7.5: Duplicated retry parameter definitions**
GOAL-7.1 lists "retry max (default: 3), backoff strategy (fixed/exponential, default: exponential), base retry delay (default: 1s)" AND GOAL-7.5 lists the exact same parameters with the same defaults. If someone updates one and not the other, they diverge. Single source of truth needed.

Suggested fix: GOAL-7.1 should reference GOAL-7.5 for retry parameters rather than repeating them: "retry policy (see GOAL-7.5)." Same for `max_consecutive_skips` and `error_policy`.

### FINDING-7
**[Check #8] Missing non-functional requirements for config itself**
The config document has no requirements about:
- **Performance**: config construction/validation time (probably trivial, but not stated)
- **Observability**: should config values be logged at engine start? (Useful for debugging, especially with GUARD-9 reproducibility)
- **Security**: are there any config values that should be treated as sensitive? (RNG seed could be security-relevant in adversarial settings)

These are likely intentionally trivial for a config struct, but should be explicitly stated as out-of-scope or covered by a blanket statement.

Suggested fix: Add a brief note: "Non-functional: Config construction and validation are synchronous, infallible except for validation errors, and O(1). Config values are logged at `info` level at engine start for reproducibility diagnostics."

### FINDING-8
**[Check #16] GOAL-7.5: Backoff strategy technology assumption — no max delay cap**
Exponential backoff without a maximum delay cap can grow unbounded. After 10 retries with base 1s, exponential backoff would be 1024 seconds (~17 minutes). While `retry_max` defaults to 3 (max 8s), there's no explicit cap for users who set `retry_max` higher.

Suggested fix: Add to GOAL-7.5 or GOAL-7.1: "max_retry_delay (default: 60s) — caps the computed backoff delay. Effective delay = min(computed_backoff, max_retry_delay)."

### FINDING-9
**[Check #15] GUARD-9 vs GOAL-7.7: Merge proposer determinism not addressed**
GUARD-9 requires determinism given same RNG seed. GOAL-7.7's merge proposer selects based on "complementary performance profiles." If two pairs have identical complementarity scores, how is the tie broken deterministically? GOAL-4.4 mentions tie-breaking by "highest combined average score" but if that also ties, the RNG must break it consistently. This isn't stated.

Suggested fix: Add to GOAL-7.7 or GOAL-4.4: "All merge selection ties are broken using the seeded RNG per GUARD-9."

---

## 🟢 Minor (can fix during implementation)

### FINDING-10
**[Check #12] Terminology: "error policy" vs "halt policy" vs "skip policy"**
GOAL-7.5 calls it "error policy (skip vs halt)". Other documents might call it differently. Within this document it's consistent ("error policy"), but the naming "skip vs halt" reads like two separate concepts. Consider a more precise name.

Suggested fix: Consider naming the enum `ErrorPolicy::Skip` and `ErrorPolicy::Halt` explicitly in the requirement, which GOAL-7.5 already mostly does. This is minor — current wording is workable.

### FINDING-11
**[Check #21] Numbering: Gap between GOAL-7.3 and GOAL-7.4 is fine, but no GOAL for the config builder pattern**
The config has 7 GOALs (7.1-7.7) with no gaps. However, there's no explicit requirement for HOW the config is constructed (builder pattern? struct literal? `new()` + setters?). GOAL-7.2 says `GEPAConfig::default()` exists, and GOAL-7.3 says "rejected at construction time", implying a validating constructor, but the API shape is unspecified.

Suggested fix: Add to GOAL-7.2 or as GOAL-7.2b: "GEPAConfig is constructed via `GEPAConfig::builder()` (builder pattern) or `GEPAConfig::default()`. The builder validates all fields on `.build()` per GOAL-7.3. Direct struct construction is prevented (fields are private)."

### FINDING-12
**[Check #22] Grouping: Parameters scattered across GOAL-7.1 and their "owner" GOALs**
Re-evaluation params are defined in GOAL-7.1 but owned by GOAL-8.5. Min_shared_examples is defined in GOAL-7.1 but owned by GOAL-2.1. This scattering makes it hard to know the authoritative source for a default value. 

Suggested fix: See FINDING-6. Making GOAL-7.1 reference other GOALs as the authoritative source for parameter definitions would fix this.

### FINDING-13
**[Check #25] User perspective: Config requirements are appropriately system-internal**
Config is an API-level concern, so system-internal language is appropriate. However, GOAL-7.2's "a user can construct GEPAConfig::default() and run the engine" is good user-perspective framing. All 7 GOALs pass this check — config docs should be developer-facing.

---

## 📊 Coverage Matrix

| Category | Covered | Missing |
|---|---|---|
| Happy path (construct config → use) | GOAL-7.1, 7.2 | No requirement for config immutability after construction (can it be modified mid-run?) |
| Error handling (invalid config) | GOAL-7.3, 7.5 | Boundary validation incomplete (FINDING-2); RateLimited interaction (FINDING-4) |
| Default values | GOAL-7.2 | ✅ All core defaults specified |
| Serialization | GOAL-7.4 | ✅ serde support for checkpoints |
| Retry/error policy | GOAL-7.5 | No max delay cap (FINDING-8); no RateLimited interaction (FINDING-4) |
| Time budget | GOAL-7.6 | Timer start/iteration semantics unclear (FINDING-3) |
| Merge proposer config | GOAL-7.7 | Strategy definition references unclear (FINDING-5) |
| Performance | - | ⚠️ Not specified (likely trivial — see FINDING-7) |
| Security | - | ⚠️ Not specified (likely N/A — see FINDING-7) |
| Observability | - | ⚠️ No config logging requirement (FINDING-7) |
| Determinism (GUARD-9) | GOAL-7.1 (RNG seed) | Merge proposer tie-breaking (FINDING-9) |
| Boundary conditions | GOAL-7.3 (partial) | Many params missing validation rules (FINDING-2) |

---

## ✅ Passed Checks

- **Check #0: Document size** ✅ — 7 GOALs, well under the 15-GOAL limit.
- **Check #1: Specificity** ✅ with exceptions — 6/7 GOALs are specific enough for implementation. GOAL-7.7 "complementary vs random" flagged (FINDING-5).
- **Check #2: Testability** ✅ — 7/7 GOALs have clear pass/fail conditions. GOAL-7.1: check struct has all fields. GOAL-7.2: `GEPAConfig::default()` compiles and all defaults match. GOAL-7.3: construct with invalid values → error. GOAL-7.4: serialize/deserialize round-trip. GOAL-7.5: inject failing adapter, verify retry count and backoff timing. GOAL-7.6: set short time budget, verify early termination. GOAL-7.7: check merge config fields exist and defaults.
- **Check #3: Measurability** ✅ — All numeric defaults are concrete: max_iterations=100, minibatch_size=16, stagnation_limit=20, checkpoint_interval=1, pareto_max_size=50, retry_max=3, base_delay=1s, max_consecutive_skips=5, re_eval_interval=5, merge_interval=10. No vague "fast" or "large" language.
- **Check #5: Completeness** ✅ with exception — 6/7 GOALs specify actor (user/engine), behavior, and outcome. GOAL-7.6 missing timer epoch detail (FINDING-3).
- **Check #6: Happy path** ✅ — User constructs config → optionally customizes → engine validates → engine uses config throughout run. Covered by GOAL-7.1 (structure), GOAL-7.2 (defaults), GOAL-7.3 (validation).
- **Check #10: State transitions** ✅ — Config is constructed once and used immutably. No state machine within config itself (though this immutability should be stated — see Coverage Matrix).
- **Check #11: Internal consistency** ✅ with exception — 6/7 pairs consistent. GOAL-7.1 vs GOAL-7.5 duplicate retry params (FINDING-6).
- **Check #12: Terminology** ✅ with minor note — Terms are consistent within the document: "error policy", "retry policy", "backoff strategy" used consistently. Minor note on naming (FINDING-10).
- **Check #13: Priority consistency** ✅ — P0 GOALs (7.1, 7.2, 7.3) are foundational. P1 GOALs (7.4, 7.5) depend on P0 config structure. P2 GOALs (7.6, 7.7) add optional features. No inversions: nothing P0 depends on P1/P2.
- **Check #14: Numbering/cross-references** ✅ — All cross-references verified:
  - GOAL-7.1 → GUARD-9 ✅, GOAL-7.5 ✅, GOAL-8.5 ✅, GOAL-2.1 ✅, GOAL-1.7c ✅
  - GOAL-7.5 → GOAL-1.2b ✅, GOAL-6.5 ✅
  - GOAL-7.6 → GOAL-1.2a ✅
  - GOAL-7.7 → (implicit GOAL-4.4) ✅
  - Cross-references section lists GOAL-1.2a-d ✅, GOAL-2.1 ✅, GOAL-8.5 ✅, GUARD-9 ✅
- **Check #15: GUARDs vs GOALs** ✅ with exception — No GUARD contradicts any GOAL. GUARD-9 (determinism) is supported by GOAL-7.1's RNG seed. GUARD-8 (Debug/Error) is satisfied by GOAL-7.3's "descriptive error message." GUARD-5 (no network) is N/A for config. GOAL-7.7 merge determinism flagged (FINDING-9).
- **Check #17: External dependencies** ✅ — Config has no external dependencies. Serde (GOAL-7.4) is an allowed dependency per master doc.
- **Check #18: Data requirements** ✅ — Config is a pure value type. No data loading, no storage beyond checkpoint serialization (GOAL-7.4).
- **Check #19: Migration/compatibility** ✅ — No existing system to migrate from. Config is new.
- **Check #20: Scope boundaries** ✅ — Master doc defines clear out-of-scope items. Config doc doesn't add explicit non-goals but this is appropriate for a focused feature doc. Implicit: config doesn't handle runtime reconfiguration, config files (TOML/YAML), or environment variable binding.
- **Check #21: Unique identifiers** ✅ — 7 GOALs: 7.1 through 7.7, sequential, no gaps, no duplicates.
- **Check #22: Grouping** ✅ with minor note — GOALs are organized logically: structure (7.1), defaults (7.2), validation (7.3), serialization (7.4), retry (7.5), time budget (7.6), merge config (7.7). Parameter scattering noted (FINDING-12).
- **Check #23: Dependency graph** ✅ — Implicit but clear: GOAL-7.2 depends on GOAL-7.1 (can't set defaults without fields), GOAL-7.3 depends on GOAL-7.1 (can't validate without fields), GOAL-7.4 depends on GOAL-7.1 (can't serialize without fields). No circular dependencies.
- **Check #24: Acceptance criteria** ✅ — Each GOAL has implicit acceptance criteria (struct has fields, defaults work, validation catches errors, serde round-trips, retry behavior matches spec, time budget terminates, merge config toggles). Could be more explicit but is adequate.
- **Check #26: Success metrics** ✅ — Config success is binary: valid config → engine runs, invalid config → clear error. No production metrics needed beyond "engine starts successfully with default config."
- **Check #27: Risk identification** ✅ — Config is low-risk. No novel algorithms, no uncertain requirements. The master doc correctly doesn't list any config GOALs as high-risk. The only subtlety is retry/backoff interaction with rate-limited errors (FINDING-4), which is well-understood.

---

## Summary

- **Total requirements:** 7 GOALs (3 P0, 2 P1, 2 P2), 0 local GUARDs (relies on 9 master GUARDs)
- **Critical:** 2 (FINDING-1, FINDING-2)
- **Important:** 7 (FINDING-3 through FINDING-9)
- **Minor:** 4 (FINDING-10 through FINDING-13)
- **Total findings:** 13

### Coverage gaps
- Boundary validation for retry/backoff/merge/time params (FINDING-2)
- RateLimited ↔ backoff interaction (FINDING-4)
- Exponential backoff max delay cap (FINDING-8)
- Config observability/logging (FINDING-7)
- Config immutability after construction (coverage matrix note)

### Recommendation
**Needs fixes first** — FINDING-1 (atomicity of GOAL-7.1) and FINDING-2 (boundary validation completeness) are critical for implementability. An engineer starting on GOAL-7.3 today would not know whether `retry_max=0` or `checkpoint_interval=0` should be accepted or rejected. Fix these two, then the document is ready for design.

### Estimated implementation clarity: **Medium-High**
The config is well-structured overall with concrete defaults and clear cross-references. The critical gaps are boundary validation and parameter duplication, not fundamental design ambiguity. An experienced Rust developer could start implementing within ~1 hour of reviewing the fixed requirements.
