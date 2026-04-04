# Review: requirements-marketing-ritual.md (Round 2)

**Reviewer**: Claude Code (automated)
**Date**: 2026-04-04
**Document**: `.gid/requirements-marketing-ritual.md`
**Context**: Round 2 review. Round 1 review (7 findings) was applied on 2026-04-04.

---

## 🔴 Critical (blocks implementation)

*None found.*

---

## 🟡 Important (should fix before implementation)

### FINDING-1: State Machine Gap — Missing `schedule → publish` Transition ✅ Applied
**[Check #5 — State machine invariants]**

GOAL-7.1 defines 6 phases: `topic-selection → draft → review → schedule → publish → analyze`.

Sub-GOALs cover transitions:
- **7.1a**: topic-selection → draft ✅
- **7.1b**: draft → review ✅
- **7.1c**: review → schedule/publish ✅
- **7.1d**: publish → analyze ✅

**Missing**: There is no GOAL-7.1e for `schedule → publish`. When content is scheduled (not immediately published), what triggers the actual publish when the scheduled time arrives? Is this a cron job? Does gid-harness handle timed transitions? The document is silent.

This is a non-terminal state (`schedule`) with no explicitly specified outgoing transition — a potential deadlock per Check #5.

**Suggested fix:** Add GOAL-7.1e:
```markdown
- **GOAL-7.1e** [P0]: Phase 转换：schedule → publish。触发条件：到达预定发布时间（由 cron 或 gid-harness timer 触发）。输入：已审核内容 + 发布时间。输出：执行发布流程（GOAL-3.1），失败处理同 GOAL-7.7 *(ref: 定时发布完整性)*
```

---

### FINDING-2: Priority Inversion — GOAL-3.3 [P1] References P2-only Platforms ✅ Applied
**[Check #12 — Ordering sensitivity / Check #5 — Guard conditions]**

GOAL-3.3 [P1] says: *"同一内容自动适配不同平台格式——Twitter 版简短有力，Reddit 版详细有深度，HN 版技术聚焦"*

But GOAL-3.2 [P2] adds Reddit and HN in Phase 2. In Phase 1, only Twitter/X exists. A P1 goal whose primary examples reference P2-only platforms creates confusion:

- During Phase 1 implementation, GOAL-3.3 is trivially satisfied (only one platform → no adaptation needed)
- The real substance of GOAL-3.3 only materializes when GOAL-3.2 [P2] ships
- An implementer might waste effort building platform-adaptive infrastructure in Phase 1 when it's not yet needed

**Suggested fix:** Either:
(a) Reclassify GOAL-3.3 to [P2] (it's only meaningful with multi-platform), or
(b) Reword to clarify Phase 1 scope: *"Phase 1: content generation respects platform-specific constraints from `config/platforms.yaml` (GOAL-1.3a). Phase 2: same content auto-adapts to different platform formats (Reddit detailed, HN technical-focused)."*

---

## 🟢 Minor (can fix during implementation)

### FINDING-3: Module 8 Priority Labels Are Misleading ✅ Applied
**[Check #15 — Configuration vs hardcoding / Check #17 — Goals explicit]**

Line 160 has a warning box: Module 8 is blocked by gepa-core, all GOALs are effectively P2 regardless of label. GOAL-8.1 and GOAL-8.2 are both labeled [P1] but the warning says they're effectively P2.

However, the warning also says "GOAL-8.1 的 heuristic 分析是唯一可在 Phase 1 独立实现的部分" — so GOAL-8.1 [P1] *is* partly implementable. This creates a 3-tier situation:
- GOAL-8.1: Partly P1 (heuristic part), partly P2 (gepa integration)
- GOAL-8.2: Labeled P1, effectively P2
- GOAL-8.3/8.4: P2

The warning is helpful but the labels remain misleading for automated tooling or an implementer scanning priorities.

**Suggested fix:** Consider splitting GOAL-8.1 into 8.1a [P1] (heuristic analysis) and 8.1b [P2] (gepa-core integration), and relabel GOAL-8.2 to [P2] to match its effective priority.

---

### FINDING-4: GOAL-8.3 References "self-improvement GOAL-1.3" — Ambiguous Cross-Project Reference ✅ Applied
**[Check #2 — Every reference resolves]**

GOAL-8.3 says: *(ref: self-improvement GOAL-1.3 SkillAdapter, gepa-core, Hermes Agent DSPy+GEPA)*

"GOAL-1.3" in this context refers to a GOAL in the self-improvement system's requirements, not this document's GOAL-1.3 (content types). This is confusing — within this document, GOAL-1.3 means "support 3 content types."

**Suggested fix:** Clarify as: *(ref: self-improvement requirements GOAL-1.3 SkillAdapter, ...)* or use a fully qualified reference like `self-improvement/GOAL-1.3`.

---

## 📋 Path Traces (State Machine — GOAL-7.1)

### Happy Path (immediate publish):
topic-selection → [potato provides topic] → draft → [draft succeeds] → review → [potato approves, immediate] → publish → [publish succeeds] → analyze ✅

### Happy Path (scheduled publish):
topic-selection → draft → review → [potato approves + sets time] → schedule → ⚠️ **[no specified transition]** → publish → analyze ❌ (FINDING-1)

### Failure Path (draft fails):
topic-selection → draft → [LLM error] → retry (3x, GOAL-7.6) → [still fails] → blocked + notify potato ✅

### Failure Path (publish fails):
review → publish → [API error] → retry (3x, GOAL-7.7) → [still fails] → notify + save to queue ✅

### Rejection Path:
topic-selection → draft → review → [potato rejects] → ❓ (What happens? GOAL-7.2 says ❌ Reject = "丢弃", but is the ritual instance archived or deleted? Not critical — GOAL-7.8 implies `archived/` directory handles this.)

### Edit Path:
topic-selection → draft → review → [potato edits] → draft (regenerate) → review → ... (bounded? or infinite loop if potato keeps editing?) — GOAL-7.2 says ✏️ Edit = "修改后重新生成" but no bound on edit cycles. **Acceptable** — human-in-loop naturally bounds this.

### Skip Path (GOAL-7.5):
GOAL-7.5 [P2] allows skipping phases for certain content types. No detailed specification yet, acceptable for P2.

---

## ✅ Passed Checks

### Phase 1: Structural Completeness
- **Check #1: Types fully defined** ✅ — Content storage model (GOAL-7.8) has complete schema: YAML frontmatter fields listed, directory structure specified. Style Profile structure (GOAL-2.2) defined. xinfluencer discover output format specified (GOAL-6.1): `{url, author, score, topic, timestamp}`. Platform config (GOAL-1.3a): fields listed (max_length, supports_markdown, etc.).
- **Check #2: References resolve** ✅ — GOAL-0.1 references GOAL-2.6, GOAL-5.2, Module 8: all exist. GOAL-6.3 references GOAL-1.4: exists. GOAL-7.1c/d reference downstream GOALs correctly. GOAL-7.6 referenced by GOAL-1.1: exists. *Exception: FINDING-4 for ambiguous cross-project reference.*
- **Check #3: No dead definitions** ✅ — All 51 GOALs are referenced or part of the pipeline flow. All 7 GUARDs are applicable to at least one module. Metrics in Module 5 feed Module 8. Proactive Intake (Module 6) feeds Content Production (Module 1).
- **Check #4: Consistent naming** ✅ — "Style Profile" used consistently (never "style model" or "writing profile"). "Content Ritual" used consistently. "potato" lowercase throughout. Module names consistent between headers and references. Snake_case for file paths, CamelCase for type names. "intake" vs "social-intake" distinction is explicitly clarified in the terminology note at the top.

### Phase 2: Logic Correctness
- **Check #5: State machine** — Partial pass. See FINDING-1 for schedule→publish gap. All other transitions well-defined. No unreachable states. Terminal states: `analyze` (end of pipeline), `blocked` (failure terminal). Retry bounds specified (3x in GOAL-7.6 and GOAL-7.7).
- **Check #6: Data flow completeness** ✅ — Style Profile (Module 2) written → read by Content Production (Module 1). Content produced → stored (GOAL-7.8) → distributed (Module 3) → monitored (Module 4) → analyzed (Module 5) → fed back to optimization (Module 8). xinfluencer discover output → social-intake → engram → Content Production. No orphan data flows.
- **Check #7: Error handling** ✅ — GOAL-7.6: draft failures (3x retry + blocked). GOAL-7.7: publish failures (3x retry + queue). GOAL-5.1: API unavailability (exponential backoff, 24h timeout → `incomplete` marker). GUARD-1: auto-publish safety (fact-check gate). GOAL-0.2: startup precondition checks. No unbounded retry loops found.

### Phase 3: Type Safety & Edge Cases
- **Check #8: String operations** ✅ — No string slicing specified in requirements. GOAL-1.3 mentions "≤ 280 字符" for short-form — this is a constraint, not a slicing operation. Implementation should use char-aware counting (noted for design phase).
- **Check #9: Integer overflow** ✅ — Retry counters bounded at 3 (GOAL-7.6, 7.7). GUARD-1 auto-publish expiry at 30 days. GOAL-5.3: 30-day sliding window. GOAL-0.1: cold-start threshold of 10 posts. All bounds explicit.
- **Check #10: Option/None handling** ✅ — GOAL-2.6 handles cold-start (insufficient Twitter history → fallback to Telegram + seed samples). GOAL-5.1 handles missing metrics (`incomplete` marker). GOAL-0.1 handles insufficient data (< 10 posts → skip analysis).
- **Check #11: Match exhaustiveness** ✅ — GOAL-7.2 actions: Approve / Edit / Reject / Schedule — covers all review outcomes. GOAL-7.1c: review → schedule OR publish — covers both paths from review.
- **Check #12: Ordering sensitivity** — See FINDING-2 for priority ordering issue. Within state machine transitions, ordering is explicit and well-defined.

### Phase 4: Architecture Consistency
- **Check #13: Separation of concerns** ✅ — Clear separation: xinfluencer = platform I/O, engram = storage, social-intake = extraction, gid-harness = state machine, RustClaw skills = orchestration. Content Ritual is the coordinator, not a monolith. Each module has a single responsibility.
- **Check #14: Coupling** ✅ — Events carry observables (topic string, file path, potato's decision), not derived state. xinfluencer output schema is versioned (GOAL-6.1: "锁定 schema 版本"). Style Profile is a document, not embedded state.
- **Check #15: Configuration vs hardcoding** ✅ — Platform constraints in `config/platforms.yaml` (GOAL-1.3a). Publishing frequency configurable per-platform (GUARD-4). Scan strategy configurable (GOAL-6.4). "Good performance" threshold configurable (default 1.5x, GOAL-5.2). Token cost limit configurable (GUARD-7, default $0.50). Cold-start thresholds specified as defaults.
- **Check #16: API surface** ✅ — Requirements specify integration points (xinfluencer CLI/library API in GOAL-6.1, Telegram inline buttons in GOAL-7.2) without over-specifying internal APIs. Appropriate for a requirements doc.

### Phase 5: Design Doc Quality
- **Check #17: Goals and non-goals explicit** ✅ — 51 explicit GOALs. "Out of Scope" section lists 5 non-goals: no SEO, no paid ads, no multi-user SaaS, no video, no competitive monitoring. Non-goals are well-chosen and prevent scope creep.
- **Check #18: Trade-offs documented** ✅ — Risks section identifies 3 high-risk items with mitigation strategies. GUARD-5 vs GOAL-2.5 trade-off explicitly documented (rule-based fast check vs deep semantic detection). Phase 1 vs Phase 2 split is a deliberate scope trade-off.
- **Check #19: Cross-cutting concerns** ✅ — Security: GUARD-3 (credential encryption). Cost: GUARD-7 (per-content token budget). Compliance: GUARD-6 (rate limits, ToS). Observability: GOAL-7.4 (structured traces). Content integrity: GUARD-2 (no misinformation).
- **Check #20: Appropriate abstraction level** ✅ — Requirements specify *what* not *how* (mostly). Some specificity where needed: file path conventions (GOAL-7.8), API auth method (GOAL-3.1 OAuth 2.0 PKCE), config file format (GOAL-1.3a platforms.yaml). Good balance.

### Phase 6: Implementability
- **Check #21: Ambiguous prose** ✅ — Most GOALs are specific enough for unambiguous implementation. GOAL-2.1 has explicit 3-part verification criteria. GOAL-7.8 has explicit file structure. GOAL-5.1 has explicit collection cadence (1h, 6h, 24h, 7d).
- **Check #22: Missing helpers** ✅ — All referenced external systems exist (xinfluencer, engram, social-intake, gid-harness) or are explicitly marked as needed (xinfluencer publish module in Dependencies section).
- **Check #23: Dependency assumptions** ✅ — Dependencies section is thorough: 7 dependencies listed with status (existing vs new). xinfluencer publish explicitly marked as "待新增". gepa-core explicitly marked as blocking Module 8. Telegram inline buttons marked as needing implementation.
- **Check #24: Migration path** ✅ — No existing marketing ritual code in rustclaw codebase (verified by search). This is greenfield. Implementation Strategy section provides clear phased rollout.
- **Check #25: Testability** ✅ — GOAL-2.1 has 3 explicit test criteria (human review, LLM-as-judge match score > 0.8, distinction score < 0.4). State machine (gid-harness) is testable in isolation. Content storage (GOAL-7.8) is file-based, easily verifiable.

### Phase 7: Existing Code Alignment
- **Check #26: Existing functionality** ✅ — Verified: no duplicate marketing/content-generation code in rustclaw. Existing cron infrastructure (`src/cron.rs`) available for scheduled publishing. `gid_harness` referenced in `src/prompt/sections.rs`. social-intake skill exists.
- **Check #27: API compatibility** ✅ — New functionality. xinfluencer schema versioning (GOAL-6.1) prevents breaking changes.
- **Check #28: Feature flag / gradual rollout** ✅ — Implementation Strategy defines clear phases: Phase 1 (Twitter-only full loop) → Phase 2 (multi-platform). Within Phase 1, 9 steps with explicit dependency ordering. Cold-start handling (GOAL-0.1) enables graceful bootstrapping.

### Requirements-Specific Checks
- **GOALs verifiable?** ✅ — All P0 GOALs have clear success criteria. GOAL-2.1 has quantitative thresholds. GOAL-7.8 has structural specification. GOAL-0.2 has binary pass/fail preconditions.
- **GUARDs enforceable?** ✅ — Hard GUARDs (1-3) have clear enforcement mechanisms. GUARD-1 has explicit auto-publish exception with expiry. GUARD-5 has concrete examples of patterns to detect.
- **Priority distribution reasonable?** ✅ — 20 P0 (39%) / 21 P1 (41%) / 10 P2 (20%). P0s cover the critical path (style profile, content production, Twitter distribution, state machine, preconditions). P2s are genuinely deferrable (multi-platform, advanced optimization).
- **Duplicate/overlapping GOALs?** ✅ — GOAL-4.4 (discover posts to interact with) and GOAL-6.1 (discover high-value content) both use xinfluencer discover but for different purposes (engagement vs content production). Not a duplicate — distinct use cases.
- **Numbering consistency?** ✅ — Modules 0-8, all GOALs follow `GOAL-{module}.{number}` pattern. Sub-GOALs use letter suffixes (7.1a-d, 1.3a). No broken numbering from r1 edits.
- **r1 fix artifacts?** ✅ — No orphaned references, no broken cross-references from r1 edits. FINDING references in `(ref:)` annotations are from r1, all valid.

---

## Summary

| Severity | Count |
|----------|-------|
| 🔴 Critical | 0 |
| 🟡 Important | 2 |
| 🟢 Minor | 2 |
| **Total** | **4** |

### Findings Summary

| ID | Severity | Description |
|----|----------|-------------|
| FINDING-1 | 🟡 | State machine missing `schedule → publish` transition (GOAL-7.1e needed) |
| FINDING-2 | 🟡 | GOAL-3.3 [P1] references P2-only platforms (Reddit/HN) — priority inversion |
| FINDING-3 | 🟢 | Module 8 priority labels misleading (GOAL-8.1/8.2 labeled P1 but effectively P2) |
| FINDING-4 | 🟢 | GOAL-8.3 references "self-improvement GOAL-1.3" — ambiguous cross-project ref |

### Recommendation
**Ready to implement** with minor fixes. The document is well-structured, comprehensive, and shows clear improvement from r1. The two Important findings (FINDING-1 and FINDING-2) are straightforward to fix and don't require structural changes.

### Estimated Implementation Confidence
**High** — The requirements are specific, verifiable, and well-organized. Dependencies are clearly identified. The phased implementation strategy is sound.
