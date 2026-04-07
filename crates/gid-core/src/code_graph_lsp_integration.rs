// Example integration with code_graph.rs
// This shows how LSP client would be integrated into the existing code graph extraction pipeline
// NOTE: This is a design sketch, not production code

use crate::lsp_client::{LspClient, Language, DefinitionRequest, DefinitionResponse};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use anyhow::{Context, Result};

/// Source of a call edge
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EdgeSource {
    /// Extracted from tree-sitter AST
    TreeSitter,
    /// Resolved via LSP definition query
    Lsp,
    /// Heuristic name matching (fallback)
    NameMatch,
}

/// Metadata attached to each call edge
#[derive(Debug, Clone)]
pub struct CallEdgeMetadata {
    /// How this edge was detected
    pub source: EdgeSource,
    /// Confidence level (0.0 = uncertain, 1.0 = certain)
    pub confidence: f32,
    /// LSP server used, if applicable
    pub lsp_server: Option<String>,
    /// Time taken to query, in milliseconds
    pub query_time_ms: Option<u64>,
}

/// A call site extracted from tree-sitter
#[derive(Debug, Clone)]
pub struct CallSite {
    /// Source file containing the call
    pub file: PathBuf,
    /// Line number (0-indexed)
    pub line: u32,
    /// Column number (0-indexed)
    pub character: u32,
    /// Name of the called function/method
    pub callee_name: String,
    /// ID of the calling function in the graph
    pub caller_id: String,
}

/// A call edge in the code graph
#[derive(Debug, Clone)]
pub struct CallEdge {
    /// Caller function ID
    pub from: String,
    /// Callee function ID
    pub to: String,
    /// Edge metadata
    pub metadata: CallEdgeMetadata,
}

/// Statistics from LSP refinement
#[derive(Debug, Default)]
pub struct LspRefinementStats {
    /// Total call sites processed
    pub total_call_sites: usize,
    /// Call sites successfully resolved via LSP
    pub lsp_resolved: usize,
    /// Call sites that fell back to name matching
    pub name_matched: usize,
    /// Edges removed as false positives
    pub false_positives_removed: usize,
    /// Total time spent in LSP queries (ms)
    pub total_query_time_ms: u64,
}

/// Refine call edges using LSP definition queries
/// 
/// This function takes a code graph extracted via tree-sitter and refines the call edges
/// by querying an LSP server for precise definition locations.
/// 
/// # Process
/// 1. Extract all call sites from the graph (from tree-sitter AST)
/// 2. For each call site, send textDocument/definition request to LSP
/// 3. Map LSP response back to graph nodes
/// 4. Replace name-matched edges with LSP-verified edges
/// 5. Mark edges with confidence and source metadata
pub fn refine_call_edges_with_lsp(
    graph: &mut CodeGraph,
    lsp: &mut LspClient,
    call_sites: Vec<CallSite>,
) -> Result<LspRefinementStats> {
    let mut stats = LspRefinementStats::default();
    let mut new_edges = Vec::new();
    
    // Build mapping from file:line:char to graph node IDs
    let node_index = build_node_position_index(graph);

    for call_site in call_sites {
        stats.total_call_sites += 1;

        // Convert to LSP request
        let uri = file_to_uri(&call_site.file)?;
        let request = DefinitionRequest {
            uri,
            line: call_site.line,
            character: call_site.character,
        };

        // Query LSP server
        let start_time = std::time::Instant::now();
        let definitions = match lsp.definition(&request) {
            Ok(defs) => defs,
            Err(e) => {
                log::warn!("LSP query failed for {}:{}:{}: {}", 
                    call_site.file.display(), call_site.line, call_site.character, e);
                stats.name_matched += 1;
                continue;
            }
        };
        let query_time_ms = start_time.elapsed().as_millis() as u64;
        stats.total_query_time_ms += query_time_ms;

        if definitions.is_empty() {
            // No definition found, fall back to name matching
            stats.name_matched += 1;
            continue;
        }

        // Map LSP response to graph nodes
        for def in definitions {
            if let Some(target_node_id) = lookup_node_by_position(
                &node_index,
                &def.target_uri,
                def.target_line,
                def.target_character,
            ) {
                // Found matching node in graph
                new_edges.push(CallEdge {
                    from: call_site.caller_id.clone(),
                    to: target_node_id,
                    metadata: CallEdgeMetadata {
                        source: EdgeSource::Lsp,
                        confidence: 1.0,
                        lsp_server: Some("typescript-language-server".to_string()), // TODO: get from client
                        query_time_ms: Some(query_time_ms),
                    },
                });
                stats.lsp_resolved += 1;
            } else {
                log::warn!("LSP returned definition at {}:{}:{}, but no matching node in graph",
                    def.target_uri, def.target_line, def.target_character);
                stats.name_matched += 1;
            }
        }
    }

    // Replace name-matched edges with LSP edges
    let before_edge_count = graph.edges.len();
    merge_edges_into_graph(graph, new_edges)?;
    let after_edge_count = graph.edges.len();
    
    stats.false_positives_removed = before_edge_count.saturating_sub(after_edge_count);

    Ok(stats)
}

/// Configuration for code graph extraction
#[derive(Debug, Clone)]
pub struct ExtractionConfig {
    /// Root directory to analyze
    pub root: PathBuf,
    /// Enable LSP-enhanced extraction
    pub lsp_enabled: bool,
    /// Languages to use LSP for
    pub lsp_languages: Vec<Language>,
    /// Timeout for LSP queries (ms)
    pub lsp_timeout_ms: u64,
}

impl Default for ExtractionConfig {
    fn default() -> Self {
        Self {
            root: PathBuf::from("."),
            lsp_enabled: false,
            lsp_languages: vec![],
            lsp_timeout_ms: 5000,
        }
    }
}

/// Main extraction function with optional LSP enhancement
pub fn extract_code_graph(config: ExtractionConfig) -> Result<CodeGraph> {
    log::info!("Starting code graph extraction for {:?}", config.root);

    // Phase 1: Tree-sitter extraction (existing logic)
    log::info!("Phase 1: Tree-sitter structural extraction");
    let mut graph = extract_with_tree_sitter(&config.root)?;
    let call_sites = extract_call_sites_from_ast(&graph)?;
    
    log::info!("Tree-sitter extracted {} nodes, {} call sites", 
        graph.nodes.len(), call_sites.len());

    // Phase 2: LSP refinement (new)
    if config.lsp_enabled {
        log::info!("Phase 2: LSP-based call edge refinement");
        
        let language = detect_project_language(&config.root)?;
        if !config.lsp_languages.contains(&language) {
            log::warn!("LSP enabled but language {:?} not in configured languages", language);
            return Ok(graph);
        }

        let mut lsp = LspClient::new(language)
            .context("Failed to start LSP server")?;

        let root_uri = format!("file://{}", config.root.display());
        lsp.initialize(&root_uri)
            .context("Failed to initialize LSP server")?;

        let stats = refine_call_edges_with_lsp(&mut graph, &mut lsp, call_sites)?;
        
        log::info!("LSP refinement complete: {} resolved, {} name-matched, {} false positives removed, avg query time: {}ms",
            stats.lsp_resolved,
            stats.name_matched,
            stats.false_positives_removed,
            if stats.total_call_sites > 0 { 
                stats.total_query_time_ms / stats.total_call_sites as u64 
            } else { 0 }
        );

        lsp.shutdown()?;
    } else {
        log::info!("LSP disabled, using name-matching for call edges");
        apply_name_matching_heuristic(&mut graph)?;
    }

    Ok(graph)
}

// Helper functions (stubs - would be implemented in actual code_graph.rs)

struct CodeGraph {
    nodes: Vec<GraphNode>,
    edges: Vec<CallEdge>,
}

struct GraphNode {
    id: String,
    file: PathBuf,
    line: u32,
    character: u32,
    name: String,
}

fn extract_with_tree_sitter(_root: &Path) -> Result<CodeGraph> {
    // Existing tree-sitter extraction logic
    todo!()
}

fn extract_call_sites_from_ast(_graph: &CodeGraph) -> Result<Vec<CallSite>> {
    // Extract call sites with position information from tree-sitter AST
    todo!()
}

fn detect_project_language(_root: &Path) -> Result<Language> {
    // Auto-detect language from project files (package.json, Cargo.toml, etc.)
    todo!()
}

fn build_node_position_index(_graph: &CodeGraph) -> HashMap<String, Vec<String>> {
    // Build index: "file:line:char" -> [node_ids]
    todo!()
}

fn lookup_node_by_position(
    _index: &HashMap<String, Vec<String>>,
    _uri: &str,
    _line: u32,
    _character: u32,
) -> Option<String> {
    // Look up graph node by position
    todo!()
}

fn merge_edges_into_graph(_graph: &mut CodeGraph, _new_edges: Vec<CallEdge>) -> Result<()> {
    // Replace name-matched edges with LSP edges
    // Keep name-matched edges for call sites that LSP couldn't resolve
    todo!()
}

fn apply_name_matching_heuristic(_graph: &mut CodeGraph) -> Result<()> {
    // Existing name-matching logic
    todo!()
}

fn file_to_uri(path: &Path) -> Result<String> {
    Ok(format!("file://{}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extraction_config_default() {
        let config = ExtractionConfig::default();
        assert!(!config.lsp_enabled);
        assert_eq!(config.lsp_timeout_ms, 5000);
    }

    #[test]
    fn test_edge_metadata() {
        let metadata = CallEdgeMetadata {
            source: EdgeSource::Lsp,
            confidence: 1.0,
            lsp_server: Some("typescript-language-server".to_string()),
            query_time_ms: Some(42),
        };

        assert_eq!(metadata.source, EdgeSource::Lsp);
        assert_eq!(metadata.confidence, 1.0);
    }
}
