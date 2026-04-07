#!/bin/bash
# Script to audit code_graph.rs for the refactoring implementation

FILE="crates/gid-core/src/code_graph.rs"

echo "=== Checking for call site position capture ==="
grep -n "call_site_line" "$FILE" | head -20

echo ""
echo "=== Checking for start_position usage ==="
grep -n "start_position" "$FILE" | head -20

echo ""
echo "=== Checking for visibility assignment ==="
grep -n "visibility:" "$FILE" | head -20

echo ""
echo "=== Checking for is_abstract assignment ==="
grep -n "is_abstract:" "$FILE" | head -20

echo ""
echo "=== Checking for get_references calls ==="
grep -n "get_references" "$FILE" | head -20

echo ""
echo "=== Checking for get_implementations calls ==="
grep -n "get_implementations" "$FILE" | head -20

echo ""
echo "=== Checking for ResolutionContext usage ==="
grep -n "ResolutionContext" "$FILE" | head -20

echo ""
echo "=== Checking extract_calls function signatures ==="
grep -n "fn extract_calls" "$FILE" | head -20
