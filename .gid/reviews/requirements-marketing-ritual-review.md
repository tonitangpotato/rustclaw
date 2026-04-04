# Review: requirements-marketing-ritual.md

**Reviewed:** 2026-04-04
**Document:** 51 GOALs (20 P0 / 21 P1 / 10 P2) + 7 GUARDs (3 hard / 4 soft) across 9 modules (0-8)
**Findings Applied:** 2026-04-04

---

## 🔴 Critical (blocks implementation)

### FINDING-1 ✅ Applied [Check #17] GOAL-3.1 — xinfluencer publish module doesn't exist yet
GOAL-3.1 says "通过 xinfluencer 的 publish 模块" for Twitter posting. But xinfluencer's 13 modules are: autopilot, engage, discover, crawler, scoring, brand_audit, graph, monitor, cli, api, config, db, utils. There is no `publish` module. This requirement depends on a non-existent component.

**Suggested fix:** Either: (1) Add "xinfluencer publish module" as an explicit dependency with its own requirements/task, and mark GOAL-3.1 as blocked-by it, OR (2) Rewrite GOAL-3.1 to specify the publishing mechanism directly: "Support Twitter/X posting (tweet, thread, media tweet) via Twitter API v2. Authentication via OAuth 2.0 PKCE. Implement as a new `publish` module in xinfluencer or as a standalone publishing service."

**Applied:** GOAL-3.1 rewritten with Twitter API v2 / OAuth 2.0 PKCE spec + Blocked-by marker. Dependencies section updated to mark publish as 待新增.

### FINDING-2 ✅ Applied [Check #2] GOAL-2.1 — Style Profile extraction not testable
"从 potato 的多源历史内容中提取写作风格特征" — how do you verify this was done correctly? What makes a Style Profile "good"? There's no acceptance criteria for the profile itself. You can test that a profile *was generated*, but not that it accurately captures potato's style.

**Suggested fix:** Add testability criteria: "Style Profile is validated by: (1) potato reviews and approves the generated profile (human acceptance test), (2) LLM-as-judge scores 5 potato-written samples against the profile at > 0.8 similarity (calibration test), (3) LLM-as-judge scores 5 generic-LLM samples against the profile at < 0.4 similarity (discrimination test). Profile generation is complete only after passing all 3 validation steps."

**Applied:** Added 3-step validation criteria to GOAL-2.1 (human acceptance, calibration test > 0.8, discrimination test < 0.4).

### FINDING-3 ✅ Applied [Check #7] GOAL-1.1 — no error handling for content generation
What happens when the LLM generates garbage? Rate limited? Context too long? There's no error handling specified for the content production step itself — only GOAL-7.6 (draft generation failure) covers retries, but it's P1 and separated from the core production GOAL.

**Suggested fix:** Promote GOAL-7.6 to P0 and cross-reference from GOAL-1.1: "Draft generation failure handling: see GOAL-7.6. At minimum, LLM errors result in retry (up to 3x with exponential backoff), persistent failure notifies potato with partial output if available."

**Applied:** Promoted GOAL-7.6 from P1 to P0. Added rate limit + context overflow to error types. Added partial output preservation. GOAL-1.1 already cross-references GOAL-7.6.

---

## 🟡 Important (should fix before implementation)

### FINDING-4 ✅ Applied [Check #1] GOAL-5.1 — metrics partially vague
"impressions, engagement rate, reply depth, follower conversion, link CTR" — these are defined in the document header for Twitter but GOAL-5.1 doesn't specify: (a) data source for each metric (Twitter Analytics API? scraping? xinfluencer monitor?), (b) collection frequency, (c) what happens when metrics are unavailable (API changes, rate limits).

**Suggested fix:** Add to GOAL-5.1: "Data source: xinfluencer monitor module (which uses Twitter API v2 engagement endpoints). Collection frequency: 1h after publish, 6h, 24h, 7d (4 data points per post). If Twitter API is unavailable, retry with exponential backoff; if unavailable for >24h, mark metrics as `incomplete` and proceed with whatever data was collected."

**Applied:** Added data source (xinfluencer monitor / Twitter API v2), collection frequency (1h/6h/24h/7d), and API unavailability handling to GOAL-5.1.

### FINDING-5 ✅ Applied [Check #11] GUARD-1 auto-publish exception vs GUARD-2 contradiction risk
GUARD-1 allows auto-publish for specific content types (e.g., quote tweets) with potato's explicit opt-in. But GUARD-2 says content must not contain unverified claims. If auto-publish is enabled, who verifies GUARD-2 compliance? The system generates content → checks GUARD-5 (pattern blacklist) → auto-publishes. But GUARD-5 only catches AI-pattern text, not factual accuracy.

**Suggested fix:** Add to GUARD-1 auto-publish exception: "Auto-publish content must also pass GUARD-2 verification: a lightweight fact-check step (LLM self-review: 'does this contain claims I cannot verify?') before auto-publishing. If the self-review flags any uncertain claims, the content is routed to manual review instead of auto-published."

**Applied:** Added LLM self-review fact-check step to GUARD-1's auto-publish exception clause. Uncertain claims trigger manual review fallback.

### FINDING-6 ✅ Applied [Check #4] GOAL-7.1 — compound requirement
GOAL-7.1 packs the entire Content Ritual state machine (6 phases) into one requirement: "intake → draft → review → schedule → publish → analyze." Each phase has different inputs, outputs, and transition conditions. This should be one GOAL per phase or at minimum one GOAL for the state machine + separate GOALs for phase transitions.

**Suggested fix:** Split GOAL-7.1 into:
- GOAL-7.1: Content Ritual state machine with 6 phases. Each content item progresses through phases sequentially. Phase transitions require explicit conditions (defined per-phase below).
- GOAL-7.1a: intake→draft: triggered by potato providing topic/intake reference. Input: topic string or file path. Output: draft content + metadata.
- GOAL-7.1b: draft→review: automatic after draft generation. Input: draft. Output: potato's decision (approve/edit/reject/schedule).
- GOAL-7.1c: review→schedule/publish: on potato approve. Input: approved content. Output: scheduled time or immediate publish.
- GOAL-7.1d: publish→analyze: automatic after publish. Input: published content ID. Output: metrics collection initiated.

**Applied:** Split GOAL-7.1 into GOAL-7.1 (state machine overview) + GOAL-7.1a through GOAL-7.1d (phase transitions with I/O specs). Used "topic-selection" as first phase name per FINDING-8.

### FINDING-7 ✅ Applied [Check #8] Missing — content storage/persistence model
Where do drafts, approved content, published content, and their metadata live? There's no GOAL specifying data persistence. Is it files in a directory? SQLite? GID graph nodes? Without this, every other GOAL's implementation is ambiguous.

**Suggested fix:** Add GOAL-7.8 [P0]: "Content items are persisted as structured files in `content/{status}/{YYYY-MM-DD}-{slug}.md` with YAML frontmatter (content_id, status, platform, created_at, published_at, metrics). Status directories: drafts/, approved/, published/, archived/. Content metadata is also tracked as GID graph nodes for dependency and impact analysis."

**Applied:** Added GOAL-7.8 [P0] with file-based persistence model, YAML frontmatter spec, status directories, and GID graph node tracking.

### FINDING-8 ✅ Applied [Check #12] Terminology — "intake" overloaded
"Intake" means two different things: (1) the existing social-intake skill (consuming external content), and (2) the first phase of the Content Ritual (receiving a topic/素材 to write about). In GOAL-6.2 "发现的高价值内容自动进入 social-intake 提取管道" — this is intake-as-consumption. In GOAL-7.1 "intake → draft" — this is intake-as-ritual-phase.

**Suggested fix:** Rename the Content Ritual's first phase to "Source" or "Topic Selection" to distinguish from social-intake: "source → draft → review → schedule → publish → analyze." Or explicitly define: "Content Ritual 'intake' phase = selecting/receiving the topic or source material for content production. This is distinct from 'social-intake' skill which extracts and stores external content."

**Applied:** Overview already had 术語說明 section. GOAL-7.1 state machine now uses "topic-selection" as first phase. Implementation Strategy updated to match.

### FINDING-9 ✅ Applied [Check #6] Missing happy path — cold start scenario
GOAL-2.6 mentions cold start for Style Profile (< 50 tweets), but there's no cold start requirement for the overall pipeline. When potato has zero published content, zero analytics data, and a fresh Style Profile: what's the expected first-run experience? GOAL-5.3 requires a 30-day baseline that doesn't exist yet.

**Suggested fix:** Add GOAL-0.1 [P0] or expand GOAL-2.6: "Cold start phase (first 30 days): (1) Style Profile built from Telegram + seed samples (GOAL-2.6), (2) analytics baselines accumulate — no comparative analysis until 10+ posts published, (3) GOAL-5.2 weekly reports start after week 2, (4) self-optimization (Module 8) disabled until 30-day baseline established. System explicitly tells potato 'bootstrapping phase, N more posts needed for analytics.'"

**Applied:** Added GOAL-0.1 [P0] in new Module 0 (Setup & Bootstrap) with full cold start flow and explicit bootstrapping messaging.

### FINDING-10 ✅ Applied [Check #18] GOAL-6.1 — xinfluencer discover module dependency unclear
GOAL-6.1 says "通过 xinfluencer discover 模块" but doesn't specify: what's the interface? How does marketing-ritual invoke discover? Is it a CLI command, a library API, or a Telegram command? If discover's API changes, what breaks?

**Suggested fix:** Add to Dependencies: "xinfluencer discover — invoked via CLI (`xinfluencer discover --topics <topics> --output json`) or library API (`xinfluencer::discover::find_content(topics, config) -> Vec<DiscoveredContent>`). Marketing-ritual depends on the output format: `{url, author, score, topic, timestamp}`. Version compatibility: marketing-ritual pins to xinfluencer's output schema version."

**Applied:** Added CLI/library API interface spec and output format to GOAL-6.1. Updated Dependencies section with discover output format and schema version pinning.

### FINDING-11 ✅ Applied [Check #9] GOAL-1.3 — platform length limits underspecified
"≤ 280 字符或平台允许的最大长度" — Twitter's limit is 280 chars for free, 25,000 for Premium. X's thread individual tweet limit varies. Reddit has 40,000 char limit. HN has 2,000 for comments. These limits change. Where are platform limits maintained?

**Suggested fix:** Add GOAL-1.3a [P1]: "Platform-specific constraints (character limits, formatting rules, media requirements) are maintained in a configuration file (`config/platforms.yaml`), not hardcoded. The content generation step reads platform constraints from this config. Config includes: max_length, supports_markdown, supports_media, supports_threads, rate_limit."

**Applied:** GOAL-1.3a already present in document with full config/platforms.yaml spec.

---

## 🟢 Minor (can fix during implementation)

### FINDING-12 ✅ Applied [Check #21] GOAL numbering — Module 0 missing
Modules start at 1. If FINDING-9's cold start GOAL is added as GOAL-0.1, a Module 0 (Setup / Bootstrap) should be created. Otherwise, renumber.

**Suggested fix:** If adding cold start requirements, create Module 0: "Setup & Bootstrap" with GOAL-0.1 (cold start) and potentially GOAL-0.2 (prerequisites check: xinfluencer installed, API keys configured, etc.)

**Applied:** Created Module 0 "启动与引导（Setup & Bootstrap）" with GOAL-0.1 (cold start) and GOAL-0.2 (prerequisites check).

### FINDING-13 ✅ Applied [Check #22] Module 8 (Self-Improvement) is thin and mostly P2
Module 8 has 4 GOALs: 1 P1 (heuristic), 1 P1 (suggestions), 2 P2 (GEPA + LLM judge). It's explicitly noted as "依赖 gepa-core 和 self-improvement system 就绪后才能执行" — meaning it's entirely blocked by another project. Consider moving to a separate future requirements doc or marking all as P2.

**Suggested fix:** Either: (1) move Module 8 to "Phase 2 Requirements" appendix, or (2) keep as-is but add a note: "Module 8 is blocked by: gepa-core (crate) + self-improvement system. Not implementable in Phase 1. All GOALs are effective P2 regardless of stated priority."

**Applied:** Added ⚠️ blocking note to Module 8 header: blocked by gepa-core + self-improvement, Phase 1 不可实现, GOAL-8.1 heuristic is only independent part.

### FINDING-14 ✅ Applied [Check #25] User perspective — potato's workflow not described
Requirements are system-centric. What does potato's day look like with this system? A user journey GOAL would help: "potato wakes up → checks Telegram → sees 3 content suggestions and 2 engagement recommendations → approves 1 draft, edits 1, rejects 1 → scheduled posts go out → evening: checks daily summary."

**Suggested fix:** Add to Overview or as GOAL-7.0 [P1]: "potato 的典型工作流：收到 Telegram 通知 → 审核内容草稿（approve/edit/reject） → 审核互动建议 → 查看周报。potato 的时间投入目标：< 15 min/day for content management."

**Applied:** Added GOAL-7.0 [P1] with potato's daily workflow description and < 15 min/day time investment target.

### FINDING-15 ✅ Applied [Check #27] No risk identification
Several GOALs are high-risk: Style Profile accuracy (subjective), Twitter API stability (external dependency), anti-AI detection (adversarial). None flagged.

**Suggested fix:** Add Risk section: "High-risk items requiring spike/prototype: (1) Style Profile quality — validate with potato before building pipeline around it, (2) Twitter API publish — test with a throwaway account first, (3) anti-AI detection (GOAL-2.5) — accuracy of LLM detecting LLM output is an open research problem."

**Applied:** Added new "## Risks" section before Out of Scope with 3 high-risk items: Style Profile quality, Twitter API publish, anti-AI detection.

---

## 📊 Coverage Matrix

| Category | Covered | Missing |
|---|---|---|
| Happy path | Modules 0-7 (bootstrap → production → distribution → analytics) | ✅ Fixed (FINDING-9, 12) |
| Error handling | GOAL-7.6 [P0] (draft retry), 7.7 (publish retry) | ✅ Fixed (FINDING-3) |
| Performance | GUARD-7 (cost per content) | No throughput requirements (posts/day capacity) |
| Security | GUARD-3 (credential encryption) | ✅ Adequate |
| Observability | GOAL-5.1-5.5 (analytics), 7.4 (trace) | ✅ Good |
| Edge cases | GOAL-2.6 (cold start profile), GOAL-0.1 (cold start pipeline) | ✅ Fixed (FINDING-9) |
| Data persistence | GOAL-7.8 (content storage model) | ✅ Fixed (FINDING-7) |
| External dependencies | Listed in Dependencies with interfaces | ✅ Fixed (FINDING-1, 10) |

## ✅ Passed Checks

- Check #3: Measurability ✅ — Analytics metrics are concrete with formulas
- Check #5: Completeness ✅ — Most GOALs specify trigger, behavior, outcome
- Check #10: State transitions ✅ — Content Ritual phases defined (GOAL-7.1 + 7.1a-d)
- Check #13: Priority consistency ✅ — P0 items are independent of P2
- Check #14: Numbering ✅ — Cross-references valid, xinfluencer module references accurate
- Check #15: GUARD vs GOAL alignment ✅ (FINDING-5 resolved)
- Check #16: Technology assumptions ✅ — xinfluencer, social-intake, engram all justified
- Check #19: Migration ✅ — N/A (new system)
- Check #20: Scope boundaries ✅ — Clear Out of Scope section (no SEO, no ads, no SaaS, no video)
- Check #23: Dependency graph ✅ — Phased approach (Phase 1: Twitter only → Phase 2: multi-platform)
- Check #24: Acceptance criteria ✅ — Analytics metrics serve as acceptance criteria

## Summary

- **Total requirements:** 51 GOALs + 7 GUARDs (was 42 GOALs before review)
- **Critical:** 3/3 applied (FINDING-1, 2, 3)
- **Important:** 8/8 applied (FINDING-4 through 11)
- **Minor:** 4/4 applied (FINDING-12 through 15)
- **All 15 findings applied ✅**
- **New GOALs added:** GOAL-0.1, GOAL-0.2, GOAL-1.3a, GOAL-7.0, GOAL-7.1a-d, GOAL-7.8 (9 new)
- **Promoted:** GOAL-7.6 P1→P0
- **New sections:** Module 0 (Setup & Bootstrap), Risks
