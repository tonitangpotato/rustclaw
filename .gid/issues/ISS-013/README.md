# ISS-013: gid_extract tool missing incremental extract + LSP refinement

## Summary

RustClaw's `gid_extract` tool calls `CodeGraph::extract_from_dir()` (full rebuild, no LSP), while the CLI `gid extract` correctly calls `extract_incremental()` + `refine_with_lsp()`. This means:

1. All edges get default confidence 1.0 (tree-sitter guesses look identical to LSP-verified facts)
2. ISS-012 confidence weighting fixes have zero practical effect in RustClaw workflows
3. Every extract is a full rebuild — no incremental benefit

## Root Cause

`GidExtractTool::execute()` in `src/tools.rs:4170` calls:
```rust
let code_graph = CodeGraph::extract_from_dir(dir_path);
```

Should match CLI behavior:
```rust
let (mut code_graph, report) = CodeGraph::extract_incremental(&dir, &gid_dir, &meta_path, force)?;
code_graph.refine_with_lsp(&dir);
```

## Affected Callers

| Caller | incremental? | LSP? |
|--------|---|---|
| gid-cli `cmd_extract` | ✅ | ✅ |
| gid-core `watch.rs` | ✅ | ✅ |
| **RustClaw `gid_extract`** | ❌ | ❌ |
| harness `scheduler.rs` | ❌ | ❌ |
| ritual `executor.rs` | ❌ | ❌ |
| infer `mod.rs` | ❌ | ❌ |

## Fix

Replace `extract_from_dir` with `extract_incremental` + `refine_with_lsp` in RustClaw's `gid_extract` tool. The gid-core internal callers (harness, ritual, infer) are out of scope — they should be fixed in gid-core separately.

## Fix Applied

In `src/tools.rs` `GidExtractTool::execute()`:
1. Replaced `CodeGraph::extract_from_dir()` → `CodeGraph::extract_incremental()` with fallback
2. Added `code_graph.refine_with_lsp()` call after extraction
3. LSP refinement stats included in output message
4. Both external and internal code paths now use the same refined graph

Build: clean, 0 warnings.

## Priority: P1
## Status: done
