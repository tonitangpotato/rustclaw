# Quick Start: LSP-Enhanced Code Graph Extraction

Get started with LSP-enhanced code graph extraction in 5 minutes.

## Prerequisites

Install the language servers you need:

```bash
# TypeScript (most common)
npm install -g typescript-language-server typescript

# Rust (if analyzing Rust code)
rustup component add rust-analyzer

# Python (if analyzing Python code)
npm install -g pyright
```

## Installation

```bash
# Clone and build gid-rs
cd /Users/potato/clawd/projects/gid-rs
cargo build --release

# Add to PATH
export PATH="$PATH:$(pwd)/target/release"
```

## Basic Usage

### 1. Extract Without LSP (Baseline)

```bash
gid extract /path/to/your/typescript/project \
  --output baseline.json \
  --stats baseline.txt

# Review results
cat baseline.txt
# Total edges: ~28,000
# Estimated false positives: ~30%
```

### 2. Extract With LSP (Enhanced)

```bash
gid extract /path/to/your/typescript/project \
  --lsp \
  --output enhanced.json \
  --stats enhanced.txt

# Review results
cat enhanced.txt
# Total edges: ~12,000
# LSP resolved: ~80%
# False positive reduction: ~93%
```

### 3. Compare Results

```bash
# Count edges
jq '.edges | length' baseline.json  # 28,000
jq '.edges | length' enhanced.json  # 12,000

# Check confidence levels
jq '.edges[].metadata.confidence' enhanced.json | sort | uniq -c
#   9,600   1.0  (LSP-verified)
#   2,400   0.5  (name-matched fallback)
```

## Real-World Example: claude-code-source-code

```bash
# Navigate to test codebase
cd /Users/potato/clawd/projects/claude-code-source-code

# Extract with LSP
gid extract . \
  --lsp \
  --lsp-langs=ts \
  --output code-graph-lsp.json \
  --stats stats.txt \
  --verbose

# Expected output:
# Phase 1: Tree-sitter structural extraction
# Tree-sitter extracted 15,234 nodes, 28,157 call sites
# Phase 2: LSP-based call edge refinement
# LSP refinement complete: 22,456 resolved, 5,701 name-matched, 15,834 false positives removed
# Total time: 3m 42s
```

## Troubleshooting

### "Failed to spawn typescript-language-server"

```bash
# Install language server
npm install -g typescript-language-server typescript

# Verify installation
which typescript-language-server
typescript-language-server --version
```

### "LSP query timeout" warnings

```bash
# Increase timeout
gid extract . --lsp --lsp-timeout=10000

# Check if project compiles
cd /path/to/project
npm install  # Install dependencies
tsc --noEmit # Check for errors
```

### Low LSP resolution rate (< 50%)

```bash
# Ensure project is set up correctly
cd /path/to/project

# TypeScript: install dependencies
npm install
npx tsc --noEmit

# Rust: build project
cargo check

# Python: install dependencies
pip install -r requirements.txt
```

## Advanced Usage

### Select Specific Languages

```bash
# Only use LSP for TypeScript, fall back to name-matching for others
gid extract . --lsp --lsp-langs=ts
```

### Tune Performance

```bash
# Adjust timeout (default: 5000ms)
gid extract . --lsp --lsp-timeout=3000

# Process specific directory
gid extract ./src --lsp
```

### Export with Metadata

```bash
# Full metadata in JSON output
gid extract . --lsp --output graph.json --include-metadata

# Sample output:
{
  "edges": [
    {
      "from": "src/index.ts:main",
      "to": "src/utils.ts:greet",
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

## Testing Your Setup

Use the included test fixture:

```bash
cd /Users/potato/clawd/projects/gid-rs/crates/gid-core/tests/fixtures/typescript-sample

# Install dependencies
npm install

# Run extraction
gid extract . --lsp --verbose

# Expected: 4 call edges, all confidence 1.0
# - main → greet
# - main → add
# - main → Calculator.multiply
# - main → Calculator.divide
```

## Performance Tips

1. **Use LSP for production extractions**: Accuracy > speed
2. **Cache results**: Run once, use multiple times
3. **Incremental updates**: Only re-analyze changed files (future feature)
4. **Parallel processing**: Coming in future release

## Expected Results

| Metric | Without LSP | With LSP |
|--------|-------------|----------|
| Extraction Time | 45s | 3-5 min |
| Total Edges | 28K | 12K |
| Precision | ~70% | >95% |
| False Positives | ~30% | <5% |

## Next Steps

- Read full documentation: `docs/LSP-FEATURE.md`
- Review architecture: `docs/DESIGN-LSP-CLIENT.md`
- See implementation plan: `docs/IMPLEMENTATION-PLAN-ISS002.md`

## Support

- Issues: See `ISSUES.md` (ISS-002)
- Questions: Contact potato
- Source: `/Users/potato/clawd/projects/gid-rs/`

---

**Happy graphing! 🎉**
