# Implementation Plan: LSP Client Integration (ISS-002)

## Overview
This document provides a step-by-step implementation plan for integrating the LSP client into gid-core's code_graph module.

## Current Status
- ✅ Design document created: `docs/DESIGN-LSP-CLIENT.md`
- ✅ LSP client skeleton implemented: `crates/gid-core/src/lsp_client.rs`
- ✅ ISSUES.md updated with ISS-002 details
- ⏳ Integration with code_graph.rs (next steps below)

## Phase 1: LSP Client Completion

### 1.1 Add Dependencies
Add to `crates/gid-core/Cargo.toml`:
```toml
[dependencies]
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
anyhow = "1.0"
```

### 1.2 Register Module
Update `crates/gid-core/src/lib.rs`:
```rust
pub mod lsp_client;
```

### 1.3 Test LSP Client
Create `crates/gid-core/tests/lsp_integration.rs`:
- Test initialization with typescript-language-server
- Test definition queries on known TypeScript code
- Test error handling (server not installed, timeouts)
- Verify JSON-RPC protocol compliance

## Phase 2: Code Graph Integration

### 2.1 Update Edge Metadata
Modify `code_graph.rs` to add edge metadata:
```rust
#[derive(Debug, Clone)]
pub enum EdgeSource {
    TreeSitter,
    Lsp,
    NameMatch,
}

#[derive(Debug, Clone)]
pub struct CallEdgeMetadata {
    pub source: EdgeSource,
    pub confidence: f32,
    pub lsp_server: Option<String>,
    pub query_time_ms: Option<u64>,
}
```

### 2.2 Add LSP Refinement Function
New function in `code_graph.rs`:
```rust
/// Refine call edges using LSP definition queries
pub fn refine_call_edges_with_lsp(
    graph: &mut Graph,
    lsp: &mut LspClient,
    file_paths: &[PathBuf],
) -> Result<LspRefinementStats> {
    // 1. Extract all call sites from graph (position info from tree-sitter)
    // 2. For each call site:
    //    - Convert to LSP DefinitionRequest
    //    - Query definition location
    //    - Map response to target function in graph
    //    - Replace/augment edge with LSP result
    // 3. Return statistics (edges refined, false positives removed, etc.)
}
```

### 2.3 Update Extraction Pipeline
Modify main extraction function to support LSP:
```rust
pub struct ExtractionConfig {
    pub lsp_enabled: bool,
    pub lsp_languages: Vec<Language>,
    // ... existing config
}

pub fn extract_code_graph(config: ExtractionConfig) -> Result<Graph> {
    // 1. Tree-sitter pass (existing)
    let mut graph = extract_with_tree_sitter(&config)?;
    
    // 2. LSP pass (new)
    if config.lsp_enabled {
        let mut lsp = LspClient::new(detect_language(&config)?)?;
        lsp.initialize(&config.root_uri)?;
        
        let stats = refine_call_edges_with_lsp(&mut graph, &mut lsp, &config.files)?;
        log::info!("LSP refinement: {:?}", stats);
        
        lsp.shutdown()?;
    }
    
    Ok(graph)
}
```

## Phase 3: CLI Integration

### 3.1 Add CLI Flag
Update CLI parser (in main crate or bin):
```rust
#[derive(Parser)]
struct ExtractArgs {
    /// Enable LSP-enhanced extraction
    #[arg(long)]
    lsp: bool,
    
    /// Languages to use LSP for (comma-separated: ts,rust,python)
    #[arg(long, value_delimiter = ',')]
    lsp_langs: Option<Vec<String>>,
    
    // ... existing args
}
```

### 3.2 Wire to Extraction Config
```rust
let config = ExtractionConfig {
    lsp_enabled: args.lsp,
    lsp_languages: parse_languages(&args.lsp_langs)?,
    // ... other config
};
```

## Phase 4: Testing & Validation

### 4.1 Unit Tests
- LSP client JSON-RPC protocol
- Edge metadata handling
- Language detection

### 4.2 Integration Tests
Create test project: `crates/gid-core/tests/fixtures/typescript-sample/`
- Small TypeScript project with known call graph
- Verify LSP produces correct edges
- Compare with name-matching baseline

### 4.3 Benchmark Test
Run on claude-code-source-code:
```bash
# Baseline (tree-sitter + name-matching)
gid extract /Users/potato/clawd/projects/claude-code-source-code \
  --output baseline.json \
  --stats baseline-stats.txt

# LSP-enhanced
gid extract /Users/potato/clawd/projects/claude-code-source-code \
  --lsp \
  --output lsp.json \
  --stats lsp-stats.txt

# Compare
diff baseline-stats.txt lsp-stats.txt
# Measure: edge count, confidence distribution, false positive reduction
```

### 4.4 Validation Metrics
Manually verify sample of edges:
- Select 100 random call edges from baseline
- Check if LSP version is correct
- Calculate precision improvement
- Document in METRICS.md

## Phase 5: Multi-Language Support

### 5.1 Rust Support
- Add rust-analyzer configuration
- Handle Cargo.toml workspaces
- Test on gid-rs itself

### 5.2 Python Support
- Add pyright configuration
- Handle virtual environments
- Test on Python codebase

### 5.3 Language Auto-Detection
```rust
fn detect_language(root: &Path) -> Result<Language> {
    if root.join("package.json").exists() {
        Ok(Language::TypeScript)
    } else if root.join("Cargo.toml").exists() {
        Ok(Language::Rust)
    } else if root.join("setup.py").exists() || root.join("pyproject.toml").exists() {
        Ok(Language::Python)
    } else {
        anyhow::bail!("Unable to detect project language")
    }
}
```

## Phase 6: Optimization

### 6.1 Caching
- Cache definition query results per file
- Persist cache between runs (optional)

### 6.2 Batch Processing
- Process files in chunks
- Reuse LSP server instance

### 6.3 Parallelization
- Multiple LSP clients for different file sets (future)

### 6.4 Timeout Handling
- 5-second timeout per definition query
- Graceful degradation on timeout

## Phase 7: Documentation & Metrics

### 7.1 Update Documentation
- User guide: how to use --lsp flag
- Installation: ensure language servers are installed
- Troubleshooting: common issues

### 7.2 Metrics Collection
Create `docs/METRICS.md`:
- Edge count comparison (baseline vs LSP)
- Precision/recall measurements
- Performance benchmarks
- False positive reduction percentage

### 7.3 Example Output
Document example extraction output showing metadata:
```json
{
  "edges": [
    {
      "from": "src/index.ts:processFile",
      "to": "src/utils.ts:readFile",
      "metadata": {
        "source": "Lsp",
        "confidence": 1.0,
        "lsp_server": "typescript-language-server@4.0.0",
        "query_time_ms": 23
      }
    }
  ]
}
```

## Dependencies & Prerequisites

### Required Tools
- typescript-language-server (npm install -g typescript-language-server)
- rust-analyzer (rustup component add rust-analyzer)
- pyright (npm install -g pyright)

### Development Environment
- Rust 1.70+
- Test codebase: claude-code-source-code cloned at `/Users/potato/clawd/projects/claude-code-source-code`

## Timeline Estimate
- Phase 1 (LSP Client): 2-3 hours
- Phase 2 (Integration): 4-6 hours
- Phase 3 (CLI): 1-2 hours
- Phase 4 (Testing): 3-4 hours
- Phase 5 (Multi-lang): 4-5 hours
- Phase 6 (Optimization): 3-4 hours
- Phase 7 (Docs): 2-3 hours

**Total: 19-27 hours** (2.5-3.5 days of focused work)

## Success Criteria
- [x] LSP client successfully queries typescript-language-server
- [ ] Integration test passes on typescript-sample fixture
- [ ] Benchmark on claude-code-source-code completes in <5 minutes
- [ ] Precision >95% on manually verified edge sample
- [ ] False positive reduction >50% vs baseline
- [ ] Documentation complete with examples
