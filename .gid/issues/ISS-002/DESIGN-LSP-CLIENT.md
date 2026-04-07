# LSP Client for Precise Call-Edge Detection (ISS-002)

## Overview

This document describes the architecture for adding LSP (Language Server Protocol) client support to gid-core's code_graph module to replace name-matching heuristics with compiler-precise call-edge detection.

## Problem Statement

Current `code_graph.rs` (7039 lines) uses:
- **Tree-sitter** for structure extraction (files, classes, functions, imports)
- **Name-matching heuristics** for call edges

This produces ~28K call edges with many false positives:
- Different types with same method name get incorrectly linked
- No type information to disambiguate overloaded methods
- No cross-module resolution accuracy

## Solution Architecture

### Component Design

#### 1. New Module: `crates/gid-core/src/lsp_client.rs`

Lightweight LSP client that:
- Starts language servers via stdio (typescript-language-server, rust-analyzer, pyright)
- Manages LSP lifecycle: initialize → definition queries → shutdown
- Handles textDocument/definition requests for call sites
- Returns precise definition locations with confidence 1.0

**Key Structures:**
```rust
pub struct LspClient {
    process: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStout>,
    next_id: i32,
}

pub struct DefinitionRequest {
    pub uri: Url,
    pub line: u32,
    pub character: u32,
}

pub struct DefinitionResponse {
    pub target_uri: Url,
    pub target_range: Range,
    pub confidence: f32,  // Always 1.0 for LSP results
}
```

**Core Methods:**
- `LspClient::new(language: Language) -> Result<Self>` - Spawns appropriate language server
- `initialize(&mut self, root_uri: Url) -> Result<()>` - LSP initialize handshake
- `definition(&mut self, request: DefinitionRequest) -> Result<Vec<DefinitionResponse>>` - Query definition
- `shutdown(&mut self) -> Result<()>` - Clean shutdown

#### 2. Modified: `crates/gid-core/src/code_graph.rs`

Enhanced extraction pipeline:
```
1. Tree-sitter pass (existing)
   → Extract structure: files, symbols, imports
   → Extract call sites with positions

2. LSP pass (new, optional)
   → For each call site from tree-sitter:
      - Send textDocument/definition request
      - Map response to call edge
      - Replace name-matched edge with LSP edge
   → Mark edges with confidence=1.0

3. Fallback strategy
   → Keep name-matched edges where LSP returns no results
   → Mark with confidence=0.5 (heuristic)
```

**Integration Points:**
- Add `lsp_enabled: bool` parameter to extraction config
- New function: `refine_call_edges_with_lsp(graph: &mut Graph, lsp: &mut LspClient) -> Result<()>`
- Update edge metadata to include `confidence: f32` and `source: EdgeSource` (TreeSitter | Lsp | NameMatch)

#### 3. CLI Enhancement

Add flag to `gid extract` command:
```bash
gid extract --lsp          # Enable LSP-enhanced extraction
gid extract --lsp=ts,rust  # Enable for specific languages
```

### Language Server Support

**Phase 1: TypeScript**
- Server: `typescript-language-server --stdio`
- Test target: claude-code-source-code (1902 files, 512K lines)
- Package manager: auto-detect package.json

**Phase 2: Rust**
- Server: `rust-analyzer`
- Build system: Cargo.toml detection

**Phase 3: Python**
- Server: `pyright --stdio`
- Venv detection and configuration

### Data Flow

```
┌─────────────────┐
│  Tree-sitter    │
│  Parse Source   │
└────────┬────────┘
         │ AST + Call Sites
         ▼
┌─────────────────┐
│  Name Matching  │◄─── Without --lsp flag
│  (heuristic)    │
└────────┬────────┘
         │ Confidence=0.5
         ▼
┌─────────────────┐
│  LSP Client     │◄─── With --lsp flag
│  Definition     │
│  Queries        │
└────────┬────────┘
         │ Confidence=1.0
         ▼
┌─────────────────┐
│  Unified Graph  │
│  + Metadata     │
└─────────────────┘
```

### Error Handling & Fallbacks

1. **LSP server not installed**: Warn and fall back to name-matching
2. **LSP timeout**: 5-second timeout per definition query, skip edge on timeout
3. **Parse errors**: Continue with partial results
4. **Type resolution failure**: Keep name-matched edge with lower confidence

### Performance Considerations

- **Incremental processing**: Process files in batches, reuse LSP server
- **Caching**: Cache definition results per file to avoid duplicate queries
- **Parallelization**: Multiple LSP clients for large codebases (future)
- **Expected overhead**: ~2-5x slower than pure tree-sitter, but far more accurate

### Metadata & Observability

Add to call edge metadata:
```rust
pub struct CallEdgeMetadata {
    pub source: EdgeSource,        // TreeSitter | Lsp | NameMatch
    pub confidence: f32,            // 0.0-1.0
    pub lsp_server: Option<String>, // "typescript-language-server@4.0.0"
    pub query_time_ms: Option<u64>,
}
```

Statistics to track:
- Total edges: before/after LSP
- False positive reduction: estimated via confidence scores
- Query performance: p50, p95, p99 latencies

## Implementation Plan

### Phase 1: LSP Client Foundation
- [ ] Create `lsp_client.rs` with LSP protocol basics (initialize, shutdown)
- [ ] Implement JSON-RPC message framing over stdio
- [ ] Add textDocument/definition request/response handling
- [ ] Test with typescript-language-server on small TS project

### Phase 2: Integration with code_graph
- [ ] Add `confidence` and `source` fields to call edges
- [ ] Implement `refine_call_edges_with_lsp()` function
- [ ] Add `--lsp` CLI flag
- [ ] Test on claude-code-source-code (1902 files)

### Phase 3: Multi-Language Support
- [ ] Add rust-analyzer support
- [ ] Add pyright support
- [ ] Language detection and server selection logic

### Phase 4: Optimization & Observability
- [ ] Add caching layer for definition queries
- [ ] Implement batch processing
- [ ] Add metrics and logging
- [ ] Document accuracy improvements in METRICS.md

## Testing Strategy

1. **Unit tests**: LSP client JSON-RPC protocol handling
2. **Integration tests**: End-to-end on small known codebases with verified call graphs
3. **Benchmark test**: claude-code-source-code (1902 files) - measure precision/recall
4. **Comparison test**: Compare edge counts and false positive rates before/after LSP

## Success Metrics

- **Precision**: >95% of LSP-detected edges are correct (vs ~70% with name-matching)
- **Coverage**: >80% of call sites resolved via LSP (remaining use fallback)
- **Performance**: <5 minutes for claude-code-source-code on M1 Mac
- **Edge reduction**: Reduce false positive edges by >50%

## References

- LSP Specification: https://microsoft.github.io/language-server-protocol/
- Tree-sitter: https://tree-sitter.github.io/tree-sitter/
- Target test codebase: `/Users/potato/clawd/projects/claude-code-source-code`
- Project source: `/Users/potato/clawd/projects/gid-rs/`
