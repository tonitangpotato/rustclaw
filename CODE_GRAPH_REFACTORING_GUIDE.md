# GID-RS Code Graph Refactoring Implementation Guide

## Overview
This document provides the implementation details for fixing 5 interconnected design problems in the code_graph module.

## Problem 1: Call Site Position Data

**Fix**: Capture `node.start_position()` during tree-sitter extraction.

### Implementation Pattern (Rust)
```rust
// In extract_calls_rust() - when creating CodeEdge
let position = call_node.start_position();
let mut edge = CodeEdge::calls(caller_id, callee_id);
edge.call_site_line = Some(position.row as u32);
edge.call_site_column = Some(position.column as u32);
edge.confidence = 0.8; // tree-sitter confidence
```

### Implementation Pattern (TypeScript)
```rust
// In extract_calls_typescript() - when creating CodeEdge  
let position = call_node.start_position();
let mut edge = CodeEdge::calls(caller_id, callee_id);
edge.call_site_line = Some(position.row as u32);
edge.call_site_column = Some(position.column as u32);
edge.confidence = 0.8;
```

### Implementation Pattern (Python)
```rust
// In extract_calls_python() - when creating CodeEdge
let position = call_node.start_position();
let mut edge = CodeEdge::calls(caller_id, callee_id);
edge.call_site_line = Some(position.row as u32);
edge.call_site_column = Some(position.column as u32);
edge.confidence = 0.8;
```

### Verification in LSP Refinement
```rust
// In refine_with_lsp() - use the position data
if let (Some(line), Some(col)) = (edge.call_site_line, edge.call_site_column) {
    // Use exact position from tree-sitter
    if let Ok(Some(def)) = lsp.get_definition(file_path, line, col) {
        // Update edge target with LSP precision
    }
} else {
    // Fallback to text search (should be rare now)
}
```

## Problem 2: Unused LSP Methods

**Fix**: Wire `get_references()` and `get_implementations()` into LSP refinement.

### Implementation: Reference Discovery Phase
```rust
// Add after definition refinement in refine_with_lsp()
fn enrich_with_references(
    graph: &mut CodeGraph,
    lsp: &mut LspClient,
    language_id: &str,
) -> LspEnrichmentStats {
    let mut stats = LspEnrichmentStats::default();
    
    // Query references only for public symbols
    for node in &graph.nodes {
        if node.visibility != Visibility::Public {
            continue; // Skip private symbols
        }
        
        if let Some(line) = node.line {
            stats.nodes_queried += 1;
            
            match lsp.get_references(&node.file_path, line as u32, 0) {
                Ok(refs) => {
                    for ref_loc in refs {
                        // Find which function/method contains this reference
                        if let Some(caller) = find_containing_node(graph, &ref_loc) {
                            let edge_key = format!("{}→{}", caller, node.id);
                            if !edge_exists(graph, &caller, &node.id) {
                                graph.add_edge(CodeEdge::calls(&caller, &node.id));
                                stats.new_edges_added += 1;
                            } else {
                                stats.already_existed += 1;
                            }
                        }
                    }
                }
                Err(_) => stats.failed += 1,
            }
        }
    }
    
    stats
}
```

### Implementation: Implementation Discovery Phase
```rust
// Add after reference discovery in refine_with_lsp()
fn enrich_with_implementations(
    graph: &mut CodeGraph,
    lsp: &mut LspClient,
    language_id: &str,
) -> LspEnrichmentStats {
    let mut stats = LspEnrichmentStats::default();
    
    // Query implementations only for abstract/trait methods
    for node in &graph.nodes {
        if !node.is_abstract {
            continue; // Skip concrete methods
        }
        
        if let Some(line) = node.line {
            stats.nodes_queried += 1;
            
            match lsp.get_implementations(&node.file_path, line as u32, 0) {
                Ok(impls) => {
                    for impl_loc in impls {
                        // Create implementation edge
                        if let Some(impl_node_id) = find_node_at_location(graph, &impl_loc) {
                            let edge_key = format!("{}→{}", impl_node_id, node.id);
                            if !edge_exists(graph, &impl_node_id, &node.id) {
                                let mut edge = CodeEdge::new(&impl_node_id, &node.id, EdgeRelation::Implements);
                                edge.confidence = 1.0; // LSP is definitive
                                graph.add_edge(edge);
                                stats.new_edges_added += 1;
                            } else {
                                stats.already_existed += 1;
                            }
                        }
                    }
                }
                Err(_) => stats.failed += 1,
            }
        }
    }
    
    stats
}
```

## Problem 3: Missing Visibility Info

**Fix**: Populate `visibility` field during tree-sitter extraction.

### Implementation Pattern (Rust)
```rust
// In extract_rust() - when creating CodeNode for functions
fn extract_visibility_rust(node: &tree_sitter::Node, source: &str) -> Visibility {
    // Look for 'pub' keyword in modifiers
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "visibility_modifier" {
            let text = child.utf8_text(source.as_bytes()).unwrap_or("");
            return match text {
                "pub" => Visibility::Public,
                "pub(crate)" => Visibility::Crate,
                _ => Visibility::Private,
            };
        }
    }
    Visibility::Private
}

// Apply when creating nodes
let mut func_node = CodeNode::new_function(file_path, name, line, is_method);
func_node.visibility = extract_visibility_rust(&function_node, source);
```

### Implementation Pattern (TypeScript)
```rust
// In extract_typescript() - when creating CodeNode
fn extract_visibility_typescript(node: &tree_sitter::Node, source: &str) -> Visibility {
    // Check for 'export' keyword or access modifiers
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "export" => return Visibility::Public,
            "public" => return Visibility::Public,
            "private" => return Visibility::Private,
            "protected" => return Visibility::Protected,
            _ => {}
        }
    }
    
    // TypeScript defaults to public if no modifier
    Visibility::Public
}
```

### Implementation Pattern (Python)
```rust
// In extract_python() - when creating CodeNode
fn extract_visibility_python(name: &str) -> Visibility {
    // Python convention: leading underscore = private
    if name.starts_with("__") {
        Visibility::Private
    } else if name.starts_with("_") {
        Visibility::Protected
    } else {
        Visibility::Public
    }
}

// Apply when creating nodes
let mut func_node = CodeNode::new_function(file_path, name, line, is_method);
func_node.visibility = extract_visibility_python(name);
```

## Problem 4: No Trait vs Concrete Distinction

**Fix**: Populate `is_abstract` field during tree-sitter extraction.

### Implementation Pattern (Rust)
```rust
// In extract_rust() - when processing trait items
fn is_trait_item(node: &tree_sitter::Node) -> bool {
    // Walk up parent chain to find if inside trait_item
    let mut current = node.parent();
    while let Some(parent) = current {
        if parent.kind() == "trait_item" {
            return true;
        }
        current = parent.parent();
    }
    false
}

// When creating function nodes in trait context
let mut method_node = CodeNode::new_function(file_path, name, line, true);
method_node.is_abstract = is_trait_item(&function_node);
```

### Implementation Pattern (TypeScript)
```rust
// In extract_typescript() - check for 'abstract' modifier
fn is_abstract_method(node: &tree_sitter::Node, source: &str) -> bool {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "abstract" {
            return true;
        }
    }
    false
}

// Also check interfaces (all interface methods are abstract)
fn is_in_interface(node: &tree_sitter::Node) -> bool {
    let mut current = node.parent();
    while let Some(parent) = current {
        if parent.kind() == "interface_declaration" {
            return true;
        }
        current = parent.parent();
    }
    false
}

let mut method_node = CodeNode::new_function(file_path, name, line, true);
method_node.is_abstract = is_abstract_method(&method_node_ts, source) || is_in_interface(&method_node_ts);
```

### Implementation Pattern (Python)
```rust
// In extract_python() - check for ABC or NotImplementedError
fn is_abstract_python(node: &tree_sitter::Node, source: &str, class_parents: &[String]) -> bool {
    // Check if class inherits from ABC
    let has_abc_parent = class_parents.iter().any(|p| p == "ABC" || p.contains("abc.ABC"));
    
    if has_abc_parent {
        // Check for @abstractmethod decorator
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "decorator" {
                let text = child.utf8_text(source.as_bytes()).unwrap_or("");
                if text.contains("abstractmethod") {
                    return true;
                }
            }
        }
    }
    
    // Also check method body for NotImplementedError or pass
    // (common pattern for abstract methods)
    false
}
```

## Problem 5: Bloated Function Signatures

**Fix**: Use `ResolutionContext` struct instead of 12 parameters.

### Before (12 parameters)
```rust
fn resolve_call_edge(
    caller_id: &str,
    callee_name: &str,
    class_map: &HashMap<String, String>,
    func_map: &HashMap<String, Vec<String>>,
    module_map: &HashMap<String, String>,
    method_to_class: &HashMap<String, String>,
    class_methods: &HashMap<String, Vec<String>>,
    class_parents: &HashMap<String, Vec<String>>,
    file_imported_names: &HashMap<String, HashSet<String>>,
    all_struct_field_types: &HashMap<String, HashMap<String, String>>,
    current_class: Option<&str>,
    current_file: &str,
) -> Option<String> {
    // ... resolution logic
}
```

### After (1 parameter)
```rust
fn resolve_call_edge(
    caller_id: &str,
    callee_name: &str,
    ctx: &ResolutionContext,
    current_class: Option<&str>,
    current_file: &str,
) -> Option<String> {
    // Use ctx.class_map, ctx.func_map, etc.
    // ... resolution logic
}
```

### Usage
```rust
// Build context once
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

// Pass to all extraction functions
extract_calls_rust(&tree, source, &ctx, file_path);
extract_calls_typescript(&tree, source, &ctx, file_path);
extract_calls_python(&tree, source, &ctx, file_path);
```

## Integration: Three-Phase LSP Session

### Phase 1: Definition Verification (Existing)
```rust
// Already implemented - use call site positions
for edge in &mut graph.edges {
    if edge.relation == EdgeRelation::Calls {
        if let (Some(line), Some(col)) = (edge.call_site_line, edge.call_site_column) {
            // Query LSP with exact position
        }
    }
}
```

### Phase 2: Reference Discovery (NEW)
```rust
// After definition verification
let ref_stats = enrich_with_references(&mut graph, &mut lsp, language_id);
tracing::info!("Reference discovery: {} new edges from {} queries", 
    ref_stats.new_edges_added, ref_stats.nodes_queried);
```

### Phase 3: Implementation Discovery (NEW)
```rust
// After reference discovery
let impl_stats = enrich_with_implementations(&mut graph, &mut lsp, language_id);
tracing::info!("Implementation discovery: {} new edges from {} queries",
    impl_stats.new_edges_added, impl_stats.nodes_queried);
```

## Expected Outcomes

1. **Call Site Positions**: 100% LSP query success rate (0% fallback)
2. **Reference Discovery**: 20-40% more incoming call edges discovered
3. **Implementation Discovery**: 100% trait/interface implementation coverage
4. **Visibility Filtering**: 50-70% reduction in LSP queries
5. **Code Clarity**: Function signatures reduced from 12 params to 1-3 params

## Testing

Run the test suite to verify:
```bash
cd crates/gid-core
cargo test code_graph::tests --nocapture
```

Check for:
- All call edges have call_site_line and call_site_column
- Public symbols have reference edges
- Trait methods have implementation edges
- Visibility field populated for all nodes
- is_abstract field populated for trait/interface methods
