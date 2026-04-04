# Review: requirements-self-improvement.md

**Reviewed:** 2026-04-04
**Document:** 53 GOALs (21 P0 / 24 P1 / 8 P2) + 9 GUARDs (5 hard / 4 soft) across 7 modules

---

## 🔴 Critical (blocks implementation)

### FINDING-1 ✅ Applied [Check #2, #3] GOAL-1.1 — miss-trigger and miss-fire rates not testable as specified
GOAL-1.1 defines 误触发率 and 漏触发率 but the denominators ("总触发数" and "总应触发数") are not directly observable. "总应触发数" requires knowing every message that *should have* triggered a skill — this is unknowable without ground truth labels. The two data sources listed ((a) user manually used skill functionality, (b) user corrects "你应该用 XX skill") are sparse and biased (only captures cases where the user bothers to correct).

**Suggested fix:** Redefine metrics with observable denominators: "误触发率 = user corrections after trigger / total triggers. 漏触发率 is estimated from: (a) user manually executes skill-like actions within 2 minutes of a trigger-miss, (b) explicit user corrections. Acknowledge this is a lower-bound estimate. Track `estimated_miss_rate` with a confidence qualifier (low/medium/high) based on sample size."

### FINDING-2 ✅ Applied [Check #7] GOAL-3.1 — LLM-as-judge for trace classification has no fallback
GOAL-3.1 relies on "LLM-as-judge 对每条 trace 标注类别 + 严重程度." But what if the LLM judge itself hallucinates or misclassifies? There's no validation mechanism for judge quality, no human-in-the-loop for classifications, and no calibration against ground truth. A bad judge will poison the entire behavioral learning pipeline.

**Suggested fix:** Add: "LLM judge classifications are stored with a `confidence` field (LLM self-assessed). Classifications with confidence < 0.7 are flagged as `needs_review`. potato can review flagged classifications via Telegram (batch: show 5 uncertain classifications, accept/reject each). Judge accuracy is tracked against potato's corrections; if accuracy drops below 80% over 20+ corrections, the judge prompt is flagged for optimization (feeds into GOAL-5.3)."

### FINDING-3 ✅ Applied [Check #17] GOAL-6.1 — synthetic test case generation depends on undefined data format
GOAL-6.1 generates test cases from (用户消息, agent 响应, 用户反馈) triples. But the document never defines:
1. Where "用户反馈" comes from — is it explicit (thumbs up/down) or inferred?
2. How "无用户纠正且任务完成" is determined — what defines "task completion"?
3. The schema of the execution history being read

**Suggested fix:** Add to GOAL-6.1: "用户反馈 inference rules: (1) explicit — user sends 👍/👎 or words like '不对'/'完美', (2) implicit positive — user proceeds to next topic without correction within 5 minutes, (3) implicit negative — user repeats the same request with different wording, or manually does what the agent was asked to do. 'Task completion' = user does not retry the same request. Source format: execution-log.jsonl entries with `message_type: user|assistant`, cross-referenced with engram session data."

### FINDING-4 ✅ Applied [Check #11] GUARD-1 vs GOAL-2.1 — contradiction potential
GUARD-1 says "SOUL.md 的 core identity 和 safety rules 绝对不可被优化系统修改." GOAL-2.1 says the system "识别 system prompt 中的可优化 sections" with AGENTS.md workflow rules being optimizable. But AGENTS.md contains safety rules too (the "Safety" section: "Don't exfiltrate private data. Never delete data files..."). GOAL-2.1 doesn't distinguish between AGENTS.md safety rules and AGENTS.md workflow rules.

**Suggested fix:** Make GOAL-2.1 explicit: "可优化 sections: AGENTS.md 的 Communication Style, Tool Usage patterns, Memory Recall instructions. 不可优化 sections: AGENTS.md 的 Safety section, External vs Internal rules, Group Chat rules. Both SOUL.md (全部) and AGENTS.md Safety section are immutable per GUARD-1." Also update GUARD-1 to mention AGENTS.md safety sections explicitly.

---

## 🟡 Important (should fix before implementation)

### FINDING-5 ✅ Applied [Check #4] GOAL-1.1 — compound requirement
GOAL-1.1 packs tracking events, trigger accuracy, output quality scoring, miss-trigger detection data sources, AND persistence format into one requirement. Should be split.

**Suggested fix:** Split into:
- GOAL-1.1a: Track skill usage events: trigger count, skill name, timestamp, user message that triggered it.
- GOAL-1.1b: Track trigger accuracy: 误触发 and 漏触发 rates (with definitions from FINDING-1 fix).
- GOAL-1.1c: Track output quality: user feedback scoring (correction=0, no feedback=0.5, positive=1).
- GOAL-1.1d: Persist all metrics to `.gid/skill-metrics/{skill-name}.jsonl`.

### FINDING-6 ✅ Applied [Check #8] Module 4 (Memory Optimization) — all P1, no P0
Every GOAL in Module 4 is P1. This means memory optimization is entirely skippable in MVP. But GOAL-4.1 (recall precision tracking) is foundational — if you don't track it, you can't improve it. And memory is core to RustClaw's identity.

**Suggested fix:** Promote GOAL-4.1 to P0. Recall precision tracking is the minimum viable observation needed for any memory optimization.

### FINDING-7 ✅ Applied [Check #7] GOAL-7.3 — rollback trigger definition ambiguous
GOAL-7.3 says "表现低于基线版本" triggers rollback, but "表现" is measured how? Using the primary metric from GOAL-6.8? Across how many uses? On what data? And "基线版本" is "最近一次被 potato approve 的版本" — but what if potato approved multiple versions in sequence (A → B → C) and C is bad? Does it rollback to B or A?

**Suggested fix:** Rewrite: "If the deployed version's primary metric (GOAL-6.8) is < baseline version's metric by > 10% (configurable) over M uses (configurable, default 10), auto-rollback to the immediate previous approved version (one step back, not to origin). If that version was also rolled back previously, halt optimization for this dimension and notify potato."

### FINDING-8 ✅ Applied [Check #6] Missing happy path — end-to-end optimization cycle
No single GOAL describes the complete flow from "weak skill detected" → "GEPA runs" → "candidate produced" → "potato approves" → "deployed" → "monitored" → "either kept or rolled back." The flow is implied across Modules 1, 6, 7 but never stated as a single traceable path.

**Suggested fix:** Add GOAL-7.0 [P0]: "The end-to-end optimization cycle for any dimension is: (1) observation — metric tracking identifies degradation or optimization opportunity, (2) data assembly — evaluation test cases generated from history + golden set, (3) optimization — GEPA or heuristic runs during idle time, (4) approval — result sent to potato via Telegram, (5) deployment — approved version replaces current, (6) monitoring — M subsequent uses tracked against baseline, (7) verdict — kept if better, auto-rolled-back if worse. Each step emits a trace event to the audit log (GOAL-7.6)."

### FINDING-9 ✅ Applied [Check #18] GOAL-6.1 — execution-log.jsonl format dependency not specified
GOAL-6.1 and GOAL-3.1 both depend on `execution-log.jsonl` but neither specifies its schema or guarantees about its content. This is an external dependency (RustClaw's existing log format) that could change.

**Suggested fix:** Add to Dependencies section: "execution-log.jsonl — RustClaw's execution trace log. Required fields per entry: timestamp, session_id, message_type (user|assistant|tool_call|tool_result), content, tool_name (if tool_call), success (bool). Schema version must be checked at startup; if incompatible, emit warning and degrade gracefully (skip entries with unknown fields rather than crashing)."

### FINDING-10 ✅ Applied [Check #8] Module 5 (Ritual Optimization) — missing error handling
GOAL-5.1 tracks ritual metrics, GOAL-5.2 identifies bottlenecks, but there's no GOAL for what happens when ritual optimization itself fails. If `RitualAdapter` (GOAL-5.3) returns errors, or the optimization produces a worse ritual template, what's the recovery path?

**Suggested fix:** Add GOAL-5.7 [P1]: "RitualAdapter failures (GEPA errors, evaluation failures) are logged and the optimization attempt is abandoned without modifying any ritual configuration. The existing ritual behavior is preserved (safe default). Consecutive failures (> 3) disable ritual optimization until manually re-enabled."

### FINDING-11 ✅ Applied [Check #15] GUARD-7 vs GOAL-6.8 — interaction unclear
GUARD-7 says "数据不足时，系统降级到简单模式（before/after 比较，keep/discard）而非尝试不可靠的统计。" GOAL-6.8 defines fixed evaluation budgets and primary metrics. What's the threshold for "数据不足"? How does the system know it's in degraded mode? Does GOAL-6.8's primary metric still apply in degraded mode?

**Suggested fix:** Add to GUARD-7: "数据不足 threshold: < 10 test cases for the target dimension. In degraded mode: (1) skip GEPA, use single before/after comparison on available test cases, (2) primary metric (GOAL-6.8) is still used but with `low_confidence` flag, (3) approval notification (GOAL-7.2) shows 'LOW DATA: N test cases only' warning, (4) auto-rollback threshold (GOAL-7.3) is tightened to M/2 uses in degraded mode."

### FINDING-12 ✅ Applied [Check #12] Terminology — "维度" vs "dimension" vs "module"
The document uses 维度, dimension, module, and 模块 somewhat interchangeably. Module 1-7 are code modules. "5个优化维度" in the Overview lists: Skill / System Prompt / 行为学习 / 记忆优化 / Ritual. These map to Modules 1-5. But Module 6 (Evaluation) and Module 7 (Orchestration) are infrastructure, not optimization dimensions. This is mostly clear from context but "按 dimension 排序" in GOAL-7.5 could confuse.

**Suggested fix:** Add a terminology note in Overview: "本文档中，'优化维度' (dimension) 指 5 个可优化的目标领域（Module 1-5）。Module 6 (Evaluation) 和 Module 7 (Orchestration) 是基础设施层，不是优化维度。"

### FINDING-13 ✅ Applied [Check #9] GOAL-3.3 — injection threshold vs engram threshold inconsistency note is good but incomplete
GOAL-3.3 notes that the pattern matching threshold (0.6) is higher than engram recall (0.3) and explains why. Good. But it doesn't specify: what embedding model is used? Is it the same as engram's? If different, thresholds aren't comparable.

**Suggested fix:** Add to GOAL-3.3: "Pattern matching uses the same embedding model as engram (currently OpenAI text-embedding-3-small). If the embedding model changes, the threshold must be recalibrated."

---

## 🟢 Minor (can fix during implementation)

### FINDING-14 ✅ Applied [Check #22] Module 4 (Memory Optimization) organizational choice
All of Module 4 is "heuristic" (not GEPA-based), making it architecturally different from Modules 1-2 which use GEPAAdapter. Consider splitting the doc into "GEPA-based optimization" and "Heuristic optimization" categories rather than by domain.

**Suggested fix:** Keep current organization (by domain is more intuitive) but add a note in each module header: "[GEPA-based]" or "[Heuristic-based]" to clarify the optimization approach used.

### FINDING-15 ✅ Applied [Check #20] Out of Scope — missing "no human eval marketplace"
The system generates synthetic evaluations and uses LLM-as-judge. It's worth explicitly stating that human evaluation marketplace integration (like Scale AI, Surge AI) is out of scope to prevent scope creep.

**Suggested fix:** Add to Out of Scope: "人工评估服务集成 — 不对接 Scale AI / Surge AI 等人工评估平台。所有评估使用自动化方式（LLM-as-judge + synthetic test cases + golden set）。"

### FINDING-16 ✅ Applied [Check #26] Success metrics — no high-level success criteria
What does "self-improvement is working" look like? No GOAL defines overall system success. Is it "average skill quality improves 10% over 30 days"? "Error rate decreases week-over-week"?

**Suggested fix:** Add GOAL-7.9 [P2]: "系统级成功指标：(1) 至少 1 个维度的 primary metric 在 30 天内有统计显著的提升（p < 0.05 on paired test, or > 5% absolute improvement with > 20 data points），(2) potato 的 approve rate > 50%（优化结果不总是被 reject），(3) 自动回滚率 < 30%（部署的优化大多数站得住）。"

---

## 📊 Coverage Matrix

| Category | Covered | Missing |
|---|---|---|
| Happy path | Modules 1-7 cover each domain's flow | End-to-end cycle (FINDING-8) |
| Error handling | GOAL-7.3 (rollback), 7.4 (token budget), GUARD-7 (degraded) | Ritual opt failure (FINDING-10), judge quality (FINDING-2) |
| Performance | GOAL-7.1 (idle scheduling), 7.4 (token budget) | ✅ Adequate for batch system |
| Security | GUARD-1-4 (immutable SOUL, human approval, no deletion, data protection) | AGENTS.md safety sections (FINDING-4) |
| Observability | GOAL-7.6 (audit log), 7.7 (Telegram status) | ✅ Good |
| Edge cases | GUARD-7 (low data degradation), GUARD-9 (cold start) | Degradation threshold (FINDING-11) |
| Data dependencies | Listed in Dependencies | execution-log.jsonl schema (FINDING-9) |

## ✅ Passed Checks

- Check #5: Completeness ✅ — Each GOAL specifies actor, behavior, and outcome
- Check #10: State transitions ✅ — Skill lifecycle (active → needs_optimization → candidate → approved → deployed → monitored) well defined
- Check #13: Priority consistency ✅ — P0 items don't depend on P2 items
- Check #14: Numbering ✅ — All cross-references resolve, both internal and to gepa-core
- Check #15: GUARD vs GOAL alignment ✅ (with FINDING-4 exception)
- Check #16: Technology assumptions ✅ — gepa-core, engram, gid-core all explicitly named and justified
- Check #19: Migration ✅ — N/A (new system)
- Check #20: Scope boundaries ✅ — Clear Out of Scope section
- Check #23: Dependency graph ✅ — Clear: gepa-core → adapters → evaluation → orchestration
- Check #24: Acceptance criteria ✅ — Primary metrics + golden sets serve as acceptance criteria

## Summary

- **Total requirements:** 53 GOALs + 9 GUARDs
- **Critical:** 4 (FINDING-1 through 4) — all ✅ Applied
- **Important:** 9 (FINDING-5 through 13) — all ✅ Applied
- **Minor:** 3 (FINDING-14 through 16) — all ✅ Applied
- **Coverage gaps:** End-to-end happy path, LLM judge calibration, execution-log schema, degradation thresholds
- **Recommendation:** All findings applied — document ready for implementation
- **Estimated implementation clarity:** HIGH — core design is solid, all implicit assumptions now made explicit
