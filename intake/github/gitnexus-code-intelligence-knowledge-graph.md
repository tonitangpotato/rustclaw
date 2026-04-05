# GitNexus — Zero-Server Code Intelligence Engine

- **URL**: https://github.com/abhigyanpatwari/GitNexus
- **Platform**: GitHub
- **Author**: abhigyanpatwari (Akon Labs)
- **Date**: 2026-04-04
- **Stars**: 21.7k ⭐
- **Domain**: 🔧 tech + 📦 product
- **learning_priority**: high
- **competitive_relevance**: direct

## Summary

GitNexus is a code intelligence tool that indexes any codebase into a knowledge graph — tracking every dependency, call chain, cluster, and execution flow. It exposes this through MCP tools so AI agents (Cursor, Claude Code, Codex, Windsurf) get deep architectural awareness. Tagline: "Like DeepWiki, but deeper" — DeepWiki helps you understand code, GitNexus lets you analyze it.

## Key Points

- **Two modes**: CLI + MCP server (recommended for daily dev) and Web UI (browser-based, WASM, no server)
- **16 MCP tools**: query, context, impact, detect_changes, rename, cypher (raw graph queries), group_* (multi-repo)
- **Precomputed structure**: Unlike traditional Graph RAG that gives raw edges to LLM, GitNexus precomputes clustering (Leiden community detection), tracing, and scoring at index time — complete context in one call
- **Storage**: LadybugDB (custom embedded DB, native + WASM versions)
- **Parsing**: Tree-sitter (native + WASM) for language-aware AST parsing
- **Search**: Hybrid BM25 + semantic + RRF (Reciprocal Rank Fusion)
- **Multi-repo**: Global registry (`~/.gitnexus/registry.json`), one MCP server serves all repos, lazy connection pooling
- **Skills generation**: `--skills` flag detects functional areas via Leiden community detection, generates per-module SKILL.md files
- **Claude Code hooks**: PreToolUse (enrich searches with graph context) + PostToolUse (auto-reindex after commits)
- **Enterprise**: SaaS + self-hosted, PR blast radius analysis, auto-reindexing, multi-repo unified graph
- **569 commits**, monorepo structure (gitnexus/, gitnexus-web/, gitnexus-shared/, gitnexus-claude-plugin/, gitnexus-cursor-integration/)

## Architecture Highlights

- Index pipeline: parse (Tree-sitter) → graph (knowledge graph) → communities (Leiden clustering) → embeddings (optional) → MCP serve
- Bridge mode: `gitnexus serve` connects CLI index to Web UI — no re-upload needed
- Per-repo `.gitnexus/` storage (portable, gitignored) + global registry
- Connection pool: max 5 concurrent LadybugDB connections, 5min eviction

## Comparison with GID

| Aspect | GitNexus | GID (gid-core) |
|--------|----------|-----------------|
| Focus | Code intelligence for AI agents via MCP | Code + task graph for development workflows |
| Graph | Knowledge graph (symbols, calls, deps, clusters) | DAG (files, classes, functions, tasks, deps) |
| Query | MCP tools + Cypher + hybrid search | CLI + Rust API, impact/deps queries |
| Integration | MCP server for Cursor/Claude Code/Codex | Built into RustClaw agent framework |
| Clustering | Leiden community detection | Architecture layers (semantify) |
| Multi-repo | Yes (registry + group commands) | No (single project) |
| Task tracking | No | Yes (full task management) |
| Ritual/Workflow | No | Yes (design → implement → verify) |

## Tags

code-intelligence, knowledge-graph, mcp, tree-sitter, graph-rag, codebase-analysis, ai-agent-tools, leiden-clustering, multi-repo

## Potential Value

- **Direct competitor/inspiration for GID's code intelligence**: GitNexus's approach to precomputed graph structure (clusters, processes, impact) is more sophisticated than GID's current file-level extraction
- **MCP integration pattern**: Their MCP server design (global registry, lazy connections, multi-repo) is a clean pattern we could adopt if we ever expose GID as MCP
- **Leiden community detection**: Their automatic clustering of code into functional areas is something GID's `semantify` could learn from — algorithmic vs heuristic-based layer assignment
- **Skills generation from code graph**: They auto-generate SKILL.md files from detected code communities — directly relevant to our skill system
- **Enterprise model validation**: 21.7k stars, enterprise offering (SaaS + self-hosted) — validates that code intelligence tooling has market demand

## Action Items

- [ ] Study GitNexus's Leiden clustering approach — could improve GID's `semantify` from heuristic to algorithmic [P2]
- [ ] Evaluate MCP server pattern for potential GID-as-MCP-server feature [P2]
- [ ] Look at their auto-generated skills from code communities — relevant to our SkillGenerator [P1]
