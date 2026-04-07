# Code Graph Refactoring Audit

## Current State Analysis

### ✅ COMPLETED: Data Structure Changes

1. **Call Site Position Fields** ✅
   - `CodeEdge` has `call_site_line: Option<u32>` 
   - `CodeEdge` has `call_site_column: Option<u32>`
   - Location: `crates/gid-core/src/code_graph.rs` lines ~160-165

2. **Visibility Field** ✅
   - `CodeNode` has `visibility: Visibility` enum
   - Enum variants: Public, Private, Crate, Protected
   - Location: `crates/gid-core/src/code_graph.rs` lines ~40-65

3. **Abstract/Trait Field** ✅
   - `CodeNode` has `is_abstract: bool`
   - Location: `crates/gid-core/src/code_graph.rs` line ~75

4. **ResolutionContext Struct** ✅
   - Created with 8 fields replacing 12-parameter signatures
   - Location: `crates/gid-core/src/code_graph.rs` lines ~250-260

5. **LSP Methods** ✅
   - `get_references()` implemented in `lsp_client.rs`
   - `get_implementations()` implemented in `lsp_client.rs`
   - `LspRefinementStats` tracks references and implementations

### 🔍 NEEDS VERIFICATION: Implementation

Need to verify these are actually used:

1. **Call site position capture during tree-sitter extraction**
   - Do extract_calls_* functions capture node.start_position()?
   - Are call_site_line and call_site_column populated?

2. **LSP method integration**
   - Is get_references() called in refine_with_lsp?
   - Is get_implementations() called in refine_with_lsp?
   - Are public symbols filtered for reference queries?
   - Are abstract methods filtered for implementation queries?

3. **Visibility population**
   - Do tree-sitter extractors set visibility based on modifiers?
   - Rust: pub keyword
   - TypeScript: export keyword
   - Python: _ prefix

4. **is_abstract population**
   - Rust: trait items marked as abstract?
   - TypeScript: abstract methods marked?
   - Python: ABC methods marked?

5. **ResolutionContext usage**
   - Are the 12-parameter functions refactored to use ResolutionContext?

## Next Steps

1. Search for actual tree-sitter extraction implementation
2. Verify call site position capture
3. Verify LSP method integration in refinement
4. Verify visibility and is_abstract population
