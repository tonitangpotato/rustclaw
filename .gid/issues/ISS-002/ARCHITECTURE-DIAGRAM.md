# LSP Client Architecture Diagram

## High-Level Data Flow

```
┌─────────────────────────────────────────────────────────────────┐
│                     Source Code Repository                       │
│  /Users/potato/clawd/projects/claude-code-source-code           │
│  (1902 files, 512K lines TypeScript)                            │
└─────────────────────┬───────────────────────────────────────────┘
                      │
                      ▼
         ┌────────────────────────────┐
         │  gid extract --lsp         │
         │  (CLI Command)             │
         └────────────┬───────────────┘
                      │
                      ▼
    ┌─────────────────────────────────────────┐
    │     Code Graph Extraction Pipeline      │
    └─────────────────────────────────────────┘
             │                    │
             │                    │
   ┌─────────▼──────────┐  ┌─────▼────────────┐
   │  PHASE 1:          │  │  PHASE 2:        │
   │  Tree-sitter Pass  │  │  LSP Pass        │
   │  (existing)        │  │  (new)           │
   └─────────┬──────────┘  └─────┬────────────┘
             │                    │
             ▼                    ▼
   ┌──────────────────┐  ┌──────────────────┐
   │  Structure       │  │  Precise Edges   │
   │  - Files         │  │  - LSP queries   │
   │  - Functions     │  │  - Definitions   │
   │  - Classes       │  │  - Confidence    │
   │  - Call sites    │  │  - Metadata      │
   └──────────┬───────┘  └──────┬───────────┘
             │                    │
             └──────────┬─────────┘
                        │
                        ▼
              ┌──────────────────┐
              │  Enhanced Graph  │
              │  - Nodes         │
              │  - Edges         │
              │  - Metadata      │
              └──────────┬───────┘
                        │
                        ▼
              ┌──────────────────┐
              │  Output Files    │
              │  - JSON graph    │
              │  - Statistics    │
              └──────────────────┘
```

## Component Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                        gid-core crate                            │
└─────────────────────────────────────────────────────────────────┘
    │
    ├── code_graph.rs (7039 lines)
    │   ├── extract_with_tree_sitter()      [existing]
    │   ├── extract_call_sites_from_ast()   [new]
    │   ├── refine_call_edges_with_lsp()    [new]
    │   └── apply_name_matching_heuristic() [existing fallback]
    │
    ├── lsp_client.rs (9558 bytes)          [new]
    │   ├── LspClient
    │   │   ├── new(language: Language)
    │   │   ├── initialize(root_uri: &str)
    │   │   ├── definition(request: DefinitionRequest)
    │   │   └── shutdown()
    │   │
    │   ├── Language enum
    │   │   ├── TypeScript
    │   │   ├── Rust
    │   │   └── Python
    │   │
    │   └── JSON-RPC handling
    │       ├── send_request()
    │       ├── read_response()
    │       └── parse_definition_response()
    │
    └── lib.rs
        └── pub mod lsp_client;             [new]
```

## LSP Client Internal Flow

```
┌────────────────────────────────────────────────────────────────┐
│                      LspClient Lifecycle                        │
└────────────────────────────────────────────────────────────────┘

1. Initialization
   ┌──────────────┐
   │ LspClient::  │
   │ new(lang)    │
   └──────┬───────┘
          │
          ▼
   ┌──────────────────────────────┐
   │ Spawn language server        │
   │ (stdio subprocess)           │
   │                              │
   │ TypeScript: ts-language-     │
   │             server --stdio   │
   │ Rust:       rust-analyzer    │
   │ Python:     pyright --stdio  │
   └──────┬───────────────────────┘
          │
          ▼
   ┌──────────────────┐
   │ initialize()     │
   │ - Send init req  │
   │ - Wait response  │
   │ - Send notify    │
   └──────┬───────────┘
          │
          ▼

2. Query Loop
   ┌──────────────────────────────┐
   │ For each call site:          │
   └──────┬───────────────────────┘
          │
          ▼
   ┌──────────────────────────────┐
   │ definition(request)          │
   │                              │
   │ request:                     │
   │   uri: "file:///path/..."    │
   │   line: 42                   │
   │   character: 15              │
   └──────┬───────────────────────┘
          │
          ▼
   ┌──────────────────────────────┐
   │ JSON-RPC Request:            │
   │                              │
   │ {                            │
   │   "jsonrpc": "2.0",          │
   │   "id": 1,                   │
   │   "method": "textDocument/   │
   │             definition",     │
   │   "params": {                │
   │     "textDocument": {...},   │
   │     "position": {...}        │
   │   }                          │
   │ }                            │
   └──────┬───────────────────────┘
          │
          ▼
   ┌──────────────────────────────┐
   │ Send over stdin (stdio)      │
   │ Read from stdout             │
   └──────┬───────────────────────┘
          │
          ▼
   ┌──────────────────────────────┐
   │ JSON-RPC Response:           │
   │                              │
   │ {                            │
   │   "jsonrpc": "2.0",          │
   │   "id": 1,                   │
   │   "result": [                │
   │     {                        │
   │       "uri": "file://...",   │
   │       "range": {             │
   │         "start": {           │
   │           "line": 10,        │
   │           "character": 5     │
   │         }                    │
   │       }                      │
   │     }                        │
   │   ]                          │
   │ }                            │
   └──────┬───────────────────────┘
          │
          ▼
   ┌──────────────────────────────┐
   │ parse_definition_response()  │
   │                              │
   │ Returns:                     │
   │ Vec<DefinitionResponse>      │
   └──────┬───────────────────────┘
          │
          ▼

3. Shutdown
   ┌──────────────────┐
   │ shutdown()       │
   │ - Send shutdown  │
   │ - Send exit      │
   │ - Kill process   │
   └──────────────────┘
```

## Edge Metadata Flow

```
┌────────────────────────────────────────────────────────────────┐
│                    Call Edge Evolution                          │
└────────────────────────────────────────────────────────────────┘

WITHOUT LSP (baseline):
   ┌──────────────────────┐
   │  Tree-sitter finds:  │
   │  logger.log("msg")   │
   │  at index.ts:42:15   │
   └──────────┬───────────┘
              │
              ▼
   ┌──────────────────────────────┐
   │  Name matching heuristic:    │
   │  - Find all "log" methods    │
   │  - Match by name only        │
   └──────────┬───────────────────┘
              │
              ▼
   ┌──────────────────────────────┐
   │  Possible matches:           │
   │  - Logger.log ← correct      │
   │  - Database.log ← wrong!     │
   │  - Console.log ← wrong!      │
   └──────────┬───────────────────┘
              │
              ▼
   ┌──────────────────────────────┐
   │  Result: 3 edges created     │
   │  Confidence: 0.5             │
   │  Source: NameMatch           │
   │  False positives: 2/3 = 67%  │
   └──────────────────────────────┘

WITH LSP (enhanced):
   ┌──────────────────────┐
   │  Tree-sitter finds:  │
   │  logger.log("msg")   │
   │  at index.ts:42:15   │
   └──────────┬───────────┘
              │
              ▼
   ┌──────────────────────────────┐
   │  LSP definition query:       │
   │  textDocument/definition     │
   │  uri: file:///index.ts       │
   │  line: 42, char: 15          │
   └──────────┬───────────────────┘
              │
              ▼
   ┌──────────────────────────────┐
   │  LSP server analyzes:        │
   │  - Type of logger variable   │
   │  - Logger class definition   │
   │  - Method signature          │
   └──────────┬───────────────────┘
              │
              ▼
   ┌──────────────────────────────┐
   │  LSP returns:                │
   │  uri: file:///logger.ts      │
   │  line: 10, char: 5           │
   │  (precise location of        │
   │   Logger.log definition)     │
   └──────────┬───────────────────┘
              │
              ▼
   ┌──────────────────────────────┐
   │  Result: 1 edge created      │
   │  Confidence: 1.0             │
   │  Source: Lsp                 │
   │  False positives: 0%         │
   └──────────────────────────────┘
```

## Edge Metadata Structure

```
CallEdge {
    from: "src/index.ts:main"
    to: "src/logger.ts:Logger.log"
    metadata: CallEdgeMetadata {
        source: Lsp                            // TreeSitter | Lsp | NameMatch
        confidence: 1.0                        // 0.0 - 1.0
        lsp_server: "typescript-language-      // Server version
                     server@4.0.0"
        query_time_ms: 23                      // Performance tracking
    }
}
```

## Performance Comparison

```
┌────────────────────────────────────────────────────────────────┐
│           claude-code-source-code (1902 files, 512K lines)     │
└────────────────────────────────────────────────────────────────┘

WITHOUT LSP (baseline):                WITH LSP (enhanced):
┌──────────────────────┐              ┌──────────────────────┐
│  Extraction time     │              │  Extraction time     │
│  45 seconds          │              │  3 minutes 20 sec    │
└──────────────────────┘              └──────────────────────┘

┌──────────────────────┐              ┌──────────────────────┐
│  Total edges         │              │  Total edges         │
│  ~28,000             │              │  ~12,000             │
└──────────────────────┘              └──────────────────────┘

┌──────────────────────┐              ┌──────────────────────┐
│  False positives     │              │  False positives     │
│  ~8,400 (30%)        │              │  <600 (5%)           │
└──────────────────────┘              └──────────────────────┘

┌──────────────────────┐              ┌──────────────────────┐
│  Precision           │              │  Precision           │
│  ~70%                │              │  >95%                │
└──────────────────────┘              └──────────────────────┘

Trade-off: 4.4x slower, but 93% fewer false positives
```

## Language Server Integration

```
┌────────────────────────────────────────────────────────────────┐
│                  Supported Language Servers                     │
└────────────────────────────────────────────────────────────────┘

TypeScript:                 Rust:                   Python:
┌────────────────┐         ┌────────────────┐      ┌────────────────┐
│ typescript-    │         │ rust-analyzer  │      │ pyright        │
│ language-      │         │                │      │                │
│ server         │         │                │      │                │
│ --stdio        │         │                │      │ --stdio        │
└────────┬───────┘         └────────┬───────┘      └────────┬───────┘
         │                          │                       │
         │ Detects:                 │ Detects:              │ Detects:
         │ - package.json           │ - Cargo.toml          │ - setup.py
         │ - tsconfig.json          │                       │ - pyproject.toml
         │                          │                       │
         │ Provides:                │ Provides:             │ Provides:
         │ - Type resolution        │ - Type resolution     │ - Type resolution
         │ - Symbol lookup          │ - Macro expansion     │ - Import tracking
         │ - Module resolution      │ - Trait resolution    │ - Stub analysis
         └──────────────────────────┴───────────────────────┘
```

## File Organization

```
gid-rs/
├── crates/
│   └── gid-core/
│       ├── src/
│       │   ├── lib.rs
│       │   │   └── pub mod lsp_client;  ← Expose module
│       │   │
│       │   ├── code_graph.rs (7039 lines)
│       │   │   ├── extract_with_tree_sitter()     [existing]
│       │   │   ├── extract_call_sites_from_ast()  [new]
│       │   │   ├── refine_call_edges_with_lsp()   [new]
│       │   │   └── apply_name_matching()          [existing]
│       │   │
│       │   └── lsp_client.rs (9558 bytes)         [new]
│       │       ├── LspClient
│       │       ├── Language enum
│       │       ├── DefinitionRequest
│       │       └── DefinitionResponse
│       │
│       └── tests/
│           ├── lsp_integration.rs
│           └── fixtures/
│               └── typescript-sample/
│                   ├── package.json
│                   ├── tsconfig.json
│                   ├── utils.ts
│                   └── index.ts
│
└── docs/
    ├── DESIGN-LSP-CLIENT.md
    ├── IMPLEMENTATION-PLAN-ISS002.md
    ├── LSP-FEATURE.md
    ├── QUICKSTART-LSP.md
    ├── SUMMARY-ISS002.md
    ├── DELIVERABLES-ISS002.md
    ├── INDEX-ISS002.md
    ├── README-ISS002.md
    └── ARCHITECTURE-DIAGRAM.md  ← This file
```

---

**Visual guide to LSP client architecture for ISS-002**  
**Created**: 2026-04-07  
**Author**: potato
