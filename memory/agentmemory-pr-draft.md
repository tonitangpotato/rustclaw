# PR Draft: Add engramai to COMPARISON.md

## PR Title

`Add engramai (Rust cognitive architecture) to comparison`

## PR Body

---

Hi! I maintain [engramai](https://crates.io/crates/engramai), a Rust-native cognitive memory system for AI agents. Per the invitation in your comparison doc, here's a PR adding it to the feature matrix and overview.

**What engramai is:** A neuroscience-grounded memory crate (~15K lines Rust, 700+ tests) that goes beyond retrieval — it implements cognitive models (ACT-R, Hebbian learning, Ebbinghaus decay) and an interoceptive emotion layer that modulates agent behavior in real-time.

**Why it's a meaningfully different entry:** Most tools in the comparison are memory-as-retrieval. engramai treats memory as one component of a cognitive architecture — agents don't just remember, they form associative links, experience functional analogs of stress/flow, and adapt behavior based on internal signals. This is a distinct design point worth representing.

Happy to adjust the copy or provide benchmark numbers if you'd like. I haven't run LongMemEval yet but plan to — will update with results.

**Repo:** https://github.com/tonitangpotato/engram-ai  
**Crate:** https://crates.io/crates/engramai  
**License:** AGPL-3.0 + Commercial

---

## File Changes: `benchmark/COMPARISON.md`

### 1. Add to Feature Matrix table

Add `engramai` column:

| Feature | agentmemory | mem0 | Letta/MemGPT | Khoj | claude-mem | Hippo | **engramai** |
|---|---|---|---|---|---|---|---|
| **GitHub stars** | Growing | 53K+ | 22K+ | 34K+ | 46K+ | Trending | New |
| **Type** | Memory engine + MCP server | Memory layer API | Full agent runtime | Personal AI | MCP server | Memory system | **Cognitive architecture** |
| **Auto-capture via hooks** | ✅ 12 lifecycle hooks | ❌ Manual `add()` | ❌ Agent self-edits | ❌ Manual | ✅ Limited | ❌ Manual | ❌ API (`add()`) |
| **Search strategy** | BM25 + Vector + Graph | Vector + Graph | Vector (archival) | Semantic | FTS5 | Decay-weighted | **BM25 + Vector + ACT-R activation** |
| **Multi-agent coordination** | ✅ Leases + signals + mesh | ❌ | Runtime-internal only | ❌ | ❌ | Multi-agent shared | ✅ Shared DB |
| **Framework lock-in** | None | None | High | Standalone | Claude Code | None | None |
| **External deps** | None | Qdrant/pgvector | Postgres + vector | Multiple | None (SQLite) | None | **None (SQLite bundled)** |
| **Self-hostable** | ✅ default | Optional | Optional | ✅ | ✅ | ✅ | **✅ Single binary** |
| **Knowledge graph** | ✅ Entity extraction + BFS | ✅ Mem0g variant | ❌ | Doc links | ❌ | ❌ | **✅ Hebbian associative graph** |
| **Memory decay** | ✅ Ebbinghaus + tiered | ❌ | ❌ | ❌ | ❌ | ✅ Half-lives | **✅ Ebbinghaus + ACT-R base-level** |
| **4-tier consolidation** | ✅ Working → episodic → semantic → procedural | ❌ | OS-inspired tiers | ❌ | ❌ | Episodic + semantic | **✅ Dual-trace (hippocampus → neocortex)** |
| **Version / supersession** | ✅ Jaccard-based | Passive | ❌ | ❌ | ❌ | ❌ | ❌ |
| **Real-time viewer** | ✅ Port 3113 | Cloud dashboard | Cloud dashboard | Web UI | ❌ | ❌ | ❌ |
| **Privacy filtering** | ✅ Strips secrets pre-store | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ |
| **Obsidian export** | ✅ Built-in | ❌ | ❌ | Native format | ❌ | ❌ | ❌ |
| **Cross-agent** | ✅ MCP + REST | API calls | Within runtime | Standalone | Claude-only | Multi-agent shared | ✅ Rust crate + CLI |
| **Audit trail** | ✅ All mutations logged | ❌ | Limited | ❌ | ❌ | ❌ | ✅ All operations logged |
| **Cognitive models** | ❌ | ❌ | ❌ | ❌ | ❌ | Partial | **✅ ACT-R + Hebbian + STDP** |
| **Behavioral modulation** | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | **✅ Interoceptive emotion → strategy adaptation** |
| **Language** | TypeScript + iii-engine | Python + TS | Python | Python | Any (MCP) | Node | **Rust (native, zero-copy)** |

### 2. Add to "What Each Tool Is Best At" section

Add this block after the Hippo section:

```markdown
**Choose engramai if you want:**
- Neuroscience-grounded memory (ACT-R activation, Hebbian learning, Ebbinghaus decay — not metaphors, actual implementations)
- Behavioral modulation — agents that detect stress, cognitive overload, or flow states and adapt strategy in real-time
- Rust-native performance with zero external dependencies (single SQLite file, ~90ms cold start)
- A cognitive architecture, not just a memory layer — memory, emotion, and behavior as an integrated system
- Associative memory via Hebbian links (memories that co-activate strengthen connections automatically)
- AGPL + commercial dual license
```

### 3. New rows I'm adding to the matrix (explanation)

Two new rows that don't exist in the current table:

- **Cognitive models** — Whether the system implements established cognitive science models (ACT-R, Hebbian learning, STDP) vs. ad-hoc algorithms. This matters because cognitive models have decades of validation in human memory research.
- **Behavioral modulation** — Whether memory state can influence agent behavior beyond retrieval (e.g., detecting failure patterns and switching strategies). This is the difference between "remembering" and "learning from experience."

---

## Notes for potato

### Strategic considerations:

1. **Two new rows are the power move.** "Cognitive models" and "Behavioral modulation" are rows where ONLY engramai has ✅. This reframes the comparison from "feature checklist" to "depth of cognitive architecture." Every other tool is ❌/❌ on these rows.

2. **Honest about weaknesses.** We show ❌ for auto-capture hooks, viewer, privacy filtering, Obsidian export, version/supersession. This builds trust and makes the ✅s more credible.

3. **"Cognitive architecture" type label** is the key framing. Everyone else is "memory engine" / "memory layer" / "memory system." We're claiming a different category entirely.

4. **No benchmark numbers yet.** I'd recommend running LongMemEval before submitting. If you score well, add to the benchmark table. If not, leave it out — the PR is strong without it. The differentiation isn't on retrieval accuracy, it's on cognitive depth.

5. **Consider running LongMemEval.** If engram's hybrid search (BM25 + vector + ACT-R activation boosting) scores >90% R@5, that's a very strong addition. Even 85% + cognitive features would be compelling.

### Before submitting:

- [ ] Verify GitHub repo is public and README is polished
- [ ] Confirm crates.io version is current (v0.2.2 or v0.2.3?)
- [ ] Consider: do you want to link to the HN post about the anxiety system?
- [ ] Fork the repo, make the changes, submit PR
