# Code Graph Refactoring Migration Checklist

## Overview

This checklist helps migrate existing code to use the new code_graph design. Follow in order to avoid compilation errors.

## Phase 1: Data Structure Updates ✅

### CodeNode Changes
- [x] Add `visibility: Visibility` field
- [x] Add `is_abstract: bool` field
- [x] Update `CodeNode::new_*()` constructors to set default values:
  ```rust
  visibility: Visibility::Private,  // Default to most restrictive
  is_abstract: false,               // Default to concrete
  ```

### CodeEdge Changes  
- [x] Add `call_site_line: Option<u32>` field
- [x] Add `call_site_column: Option<u32>` field
- [x] Update `CodeEdge::calls()` constructor to set:
  ```rust
  call_site_line: None,    // Will be populated by tree-sitter
  call_site_column: None,  // Will be populated by tree-sitter
  ```

### ResolutionContext Creation
- [x] Define `ResolutionContext` struct with 8 fields
- [x] Add lifetimes: `ResolutionContext<'a>`
- [x] Implement builder pattern or simple struct literal construction

## Phase 2: Tree-sitter Extraction Updates

### Call Site Position Capture

#### Rust Extraction (`extract_calls_rust`)
- [ ] Locate all `CodeEdge::calls()` call sites
- [ ] Add position capture before each edge creation:
  ```rust
  let position = call_node.start_position();
  let mut edge = CodeEdge::calls(caller_id, callee_id);
  edge.call_site_line = Some(position.row as u32);
  edge.call_site_column = Some(position.column as u32);
  ```

#### TypeScript Extraction (`extract_calls_typescript`)
- [ ] Same pattern as Rust above
- [ ] Apply to all call expression nodes

#### Python Extraction (`extract_calls_python`)
- [ ] Same pattern as Rust above
- [ ] Apply to all call nodes

### Visibility Extraction

#### Rust Visibility
- [ ] Create `extract_visibility_rust(node, source) -> Visibility`
- [ ] Check for `visibility_modifier` child nodes
- [ ] Parse "pub", "pub(crate)", etc.
- [ ] Apply when creating function/struct/impl nodes:
  ```rust
  let mut func_node = CodeNode::new_function(...);
  func_node.visibility = extract_visibility_rust(&node, source);
  ```

#### TypeScript Visibility
- [ ] Create `extract_visibility_typescript(node, source) -> Visibility`
- [ ] Check for "export", "public", "private", "protected" modifiers
- [ ] Default to Public if no modifier (TypeScript convention)
- [ ] Apply when creating function/class/method nodes

#### Python Visibility
- [ ] Create `extract_visibility_python(name) -> Visibility`
- [ ] Check for `_` or `__` prefix in name
- [ ] Default to Public for normal names
- [ ] Apply when creating function/class/method nodes

### Abstract Method Detection

#### Rust Abstract Detection
- [ ] Create `is_trait_item(node) -> bool`
- [ ] Walk parent chain checking for `trait_item` kind
- [ ] Apply when creating method nodes:
  ```rust
  let mut method_node = CodeNode::new_function(...);
  method_node.is_abstract = is_trait_item(&node);
  ```

#### TypeScript Abstract Detection
- [ ] Create `is_abstract_typescript(node, source) -> bool`
- [ ] Check for "abstract" modifier
- [ ] Check if parent is `interface_declaration`
- [ ] Apply when creating method nodes

#### Python Abstract Detection
- [ ] Create `is_abstract_python(node, source, class_parents) -> bool`
- [ ] Check for `@abstractmethod` decorator
- [ ] Check if class inherits from ABC
- [ ] Apply when creating method nodes

## Phase 3: Function Signature Refactoring

### Identify 12-Parameter Functions
- [ ] Search for functions with bloated signatures:
  ```bash
  rg "fn \w+\([^)]{200,}" crates/gid-core/src/code_graph.rs
  ```
- [ ] Common culprits:
  - `resolve_call_edge()`
  - `extract_calls_rust()`
  - `extract_calls_typescript()`
  - `extract_calls_python()`
  - Any resolution helper functions

### Refactor to ResolutionContext
- [ ] For each function, replace 8 map parameters with `ctx: &ResolutionContext`
- [ ] Before:
  ```rust
  fn resolve(
      caller: &str,
      class_map: &HashMap<...>,
      func_map: &HashMap<...>,
      module_map: &HashMap<...>,
      // ... 9 more parameters
  ) -> Option<String>
  ```
- [ ] After:
  ```rust
  fn resolve(
      caller: &str,
      ctx: &ResolutionContext,
  ) -> Option<String>
  ```

### Update Call Sites
- [ ] Build ResolutionContext once at top of extraction:
  ```rust
  let ctx = ResolutionContext {
      class_map: &class_map,
      func_map: &func_map,
      module_map: &module_map,
      method_to_class: &method_to_class,
      class_methods: &class_methods,
      class_parents: &class_parents,
      file_imported_names: &file_imported_names,
      all_struct_field_types: &all_struct_field_types,
  };
  ```
- [ ] Pass to all extraction functions:
  ```rust
  extract_calls_rust(&tree, source, &ctx, file_path);
  ```

## Phase 4: LSP Integration

### Reference Discovery (NEW)
- [ ] Add `enrich_with_references()` function
- [ ] Filter to `visibility == Public` nodes only
- [ ] Query `lsp.get_references()` for each
- [ ] Create incoming call edges for discovered references
- [ ] Track stats (queries, new edges, failures)

### Implementation Discovery (NEW)
- [ ] Add `enrich_with_implementations()` function
- [ ] Filter to `is_abstract == true` nodes only
- [ ] Query `lsp.get_implementations()` for each
- [ ] Create implementation edges for discovered impls
- [ ] Track stats (queries, new edges, failures)

### Integrate into refine_with_lsp()
- [ ] Keep existing definition verification (Phase 1)
- [ ] Add reference discovery (Phase 2)
- [ ] Add implementation discovery (Phase 3)
- [ ] Log stats for each phase

### Use Call Site Positions
- [ ] Replace text-based fallback searches
- [ ] Use `edge.call_site_line` and `edge.call_site_column` directly
- [ ] Handle `None` case gracefully (shouldn't happen with new extraction)

## Phase 5: Testing & Validation

### Unit Tests
- [ ] Test `extract_visibility_*()` functions with fixtures
- [ ] Test `is_abstract_*()` functions with trait/interface examples
- [ ] Test `ResolutionContext` construction
- [ ] Test call site position capture with sample AST nodes

### Integration Tests
- [ ] Test full extraction pipeline on small codebase
- [ ] Verify all edges have `call_site_line` and `call_site_column`
- [ ] Verify all nodes have `visibility` populated
- [ ] Verify trait/abstract methods have `is_abstract = true`

### LSP Tests
- [ ] Test reference discovery finds incoming calls
- [ ] Test implementation discovery finds trait impls
- [ ] Test visibility filtering (private symbols skipped)
- [ ] Test abstract filtering (concrete methods skipped)

### Performance Tests
- [ ] Measure LSP query count before/after visibility filtering
- [ ] Measure edge discovery rate (should increase 20-40%)
- [ ] Verify LSP query success rate (should be 100%)

## Phase 6: Documentation

- [x] Update DESIGN.md with ISS-004 section
- [x] Create CODE_GRAPH_REFACTORING_GUIDE.md with patterns
- [x] Create REFACTORING_COMPLETE.md with summary
- [x] Create CODE_GRAPH_QUICK_REF.md for developers
- [x] Create this MIGRATION_CHECKLIST.md

## Verification Commands

```bash
# Verify data structures are updated
rg "visibility: Visibility" crates/gid-core/src/code_graph.rs
rg "is_abstract: bool" crates/gid-core/src/code_graph.rs
rg "call_site_line: Option" crates/gid-core/src/code_graph.rs

# Verify position capture
rg "start_position\(\)" crates/gid-core/src/code_graph.rs
rg "call_site_line = Some" crates/gid-core/src/code_graph.rs

# Verify visibility extraction
rg "extract_visibility" crates/gid-core/src/code_graph.rs
rg "visibility.*Public|Private|Crate|Protected" crates/gid-core/src/code_graph.rs

# Verify abstract detection
rg "is_abstract.*true" crates/gid-core/src/code_graph.rs
rg "is_trait_item|is_abstract_" crates/gid-core/src/code_graph.rs

# Verify ResolutionContext usage
rg "ResolutionContext" crates/gid-core/src/code_graph.rs
rg "ctx\." crates/gid-core/src/code_graph.rs | wc -l  # Should be high

# Verify LSP integration
rg "get_references" crates/gid-core/src/code_graph.rs
rg "get_implementations" crates/gid-core/src/code_graph.rs

# Run tests
cd crates/gid-core
cargo test code_graph::tests --nocapture
cargo test lsp_client::tests --nocapture
```

## Rollback Plan

If issues arise:

1. **Data structure changes**: These are backward compatible (new fields have defaults)
2. **Position capture**: Can be disabled by not calling `start_position()`
3. **LSP phases**: Can be disabled by commenting out Phase 2 and 3
4. **ResolutionContext**: Most complex change, may need gradual rollout

Keep git commits atomic:
- Commit 1: Data structure changes
- Commit 2: Position capture
- Commit 3: Visibility extraction
- Commit 4: Abstract detection
- Commit 5: ResolutionContext refactor
- Commit 6: LSP phase 2 (references)
- Commit 7: LSP phase 3 (implementations)

## Completion Criteria

- [ ] All checkboxes in this file are marked ✅
- [ ] `cargo test` passes without errors
- [ ] `cargo clippy` has no warnings
- [ ] All extraction functions capture call site positions
- [ ] All nodes have visibility populated
- [ ] Trait/abstract methods are marked
- [ ] LSP refinement has three phases
- [ ] Performance metrics meet targets (see REFACTORING_COMPLETE.md)

## Support Resources

1. **Implementation patterns**: CODE_GRAPH_REFACTORING_GUIDE.md
2. **Quick reference**: CODE_GRAPH_QUICK_REF.md
3. **Architecture decisions**: DESIGN.md (ISS-004 section)
4. **Completion status**: REFACTORING_COMPLETE.md

## Sign-off

Once all phases complete, mark status:

- [ ] Phase 1: Data Structures ✅
- [ ] Phase 2: Tree-sitter Extraction ⏳
- [ ] Phase 3: Function Signatures ⏳
- [ ] Phase 4: LSP Integration ⏳
- [ ] Phase 5: Testing ⏳
- [ ] Phase 6: Documentation ✅

**Overall Status**: 🚧 IN PROGRESS

(Update this section as you complete each phase)
