# ISS-002: STATUS — ⚰️ Superseded / Abandoned

**Final status (2026-04-25):** Superseded. Will not be implemented as designed.

## Original Goal

ISS-002 proposed adding an LSP (Language Server Protocol) client to gid-core's
`code_graph` module to replace name-matching heuristics with compiler-precise
call-edge detection.

Phase 1 deliverables (design docs, architecture diagrams, implementation plan)
were authored inside this directory but never integrated into gid-core.

## Why Superseded

Between Phase 1 design and any implementation, **gid-rs (the active fork) chose
a different, lighter-weight path**: tree-sitter–based code extraction in
`crates/gid-core/src/code_graph/`. The tree-sitter approach:

- Has no LSP daemon dependency (zero-runtime-cost startup)
- Works uniformly across Rust / Python / TypeScript without rust-analyzer / pyright / tsserver
- Already shipped in `gid-rs` v0.2.x and is in production use
- Resolved ISS-004 (the underlying refactor goal of ISS-002)

LSP would still give us *more precise* call edges than tree-sitter (real
type-resolved calls vs. name matching), but the cost (LSP daemon lifecycle,
per-language adapters, async streaming protocol) is no longer justified given
how well tree-sitter + the rest of the graph layer perform in practice.

## What Lives On

The Phase 1 documents (DESIGN-LSP-CLIENT.md, ARCHITECTURE-DIAGRAM.md,
IMPLEMENTATION-PLAN-ISS002.md, etc.) are **kept in this directory as a
historical record** — if we ever revisit precise call-edge detection, this is a
solid starting point. They are not active TODOs.

## Verification

- `gid-rs` source: no LSP code present (`grep -r lsp /Users/potato/clawd/projects/gid-rs/src/` → empty)
- Active code-graph implementation: tree-sitter (see `crates/gid-core/src/code_graph/`)
- ISS-004 (the broader code_graph refactor) shipped 2026-04-xx, marked complete

## Closing Action

This issue is closed without implementation. No further work planned.
