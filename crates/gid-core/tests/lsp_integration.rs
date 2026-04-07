// Integration test for LSP client
// Tests basic LSP protocol communication with typescript-language-server
// 
// Prerequisites:
// - typescript-language-server installed: npm install -g typescript-language-server typescript
// - Sample TypeScript project in tests/fixtures/typescript-sample/

use gid_core::lsp_client::{LspClient, Language, DefinitionRequest};
use std::path::PathBuf;

#[test]
#[ignore] // Run with: cargo test --test lsp_integration -- --ignored
fn test_typescript_definition_query() {
    // Setup test fixture path
    let fixture_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("typescript-sample");

    // Ensure fixture exists
    if !fixture_path.exists() {
        panic!(
            "Test fixture not found at {:?}. Create it with:\n\
             mkdir -p tests/fixtures/typescript-sample\n\
             cd tests/fixtures/typescript-sample\n\
             npm init -y\n\
             npm install typescript --save-dev\n\
             echo 'export function greet(name: string) {{ return `Hello, ${{name}}`; }}' > utils.ts\n\
             echo 'import {{ greet }} from \"./utils\"; console.log(greet(\"World\"));' > index.ts",
            fixture_path
        );
    }

    // Start LSP client
    let mut client = LspClient::new(Language::TypeScript)
        .expect("Failed to start typescript-language-server. Is it installed?");

    // Initialize with workspace root
    let root_uri = format!("file://{}", fixture_path.display());
    client.initialize(&root_uri)
        .expect("Failed to initialize language server");

    // Query definition: index.ts calling greet() should resolve to utils.ts
    let index_file = fixture_path.join("index.ts");
    let request = DefinitionRequest {
        uri: format!("file://{}", index_file.display()),
        line: 0, // Adjust based on actual line number
        character: 25, // Position of 'greet' in the import statement
    };

    let definitions = client.definition(&request)
        .expect("Definition query failed");

    // Verify response
    assert!(!definitions.is_empty(), "Expected at least one definition");
    
    let def = &definitions[0];
    assert!(def.target_uri.contains("utils.ts"), 
        "Expected definition in utils.ts, got: {}", def.target_uri);
    assert_eq!(def.confidence, 1.0, "LSP results should have confidence 1.0");

    // Cleanup
    client.shutdown().expect("Failed to shutdown LSP client");
}

#[test]
#[ignore]
fn test_lsp_client_lifecycle() {
    // Test basic initialization and shutdown
    let mut client = LspClient::new(Language::TypeScript)
        .expect("Failed to create LSP client");

    client.initialize("file:///tmp/test-workspace")
        .expect("Failed to initialize");

    client.shutdown()
        .expect("Failed to shutdown");
}

#[test]
#[ignore]
fn test_invalid_definition_request() {
    let mut client = LspClient::new(Language::TypeScript)
        .expect("Failed to create LSP client");

    client.initialize("file:///tmp/test-workspace")
        .expect("Failed to initialize");

    // Query non-existent file
    let request = DefinitionRequest {
        uri: "file:///tmp/non-existent.ts".to_string(),
        line: 0,
        character: 0,
    };

    let result = client.definition(&request);
    // Should either return empty vec or error, depending on implementation
    // Both are acceptable - document the behavior

    client.shutdown().expect("Failed to shutdown");
}

#[test]
fn test_language_enum() {
    // Test language detection and server commands
    assert_eq!(Language::TypeScript.server_command().0, "typescript-language-server");
    assert_eq!(Language::Rust.server_command().0, "rust-analyzer");
    assert_eq!(Language::Python.server_command().0, "pyright-langserver");
}
