#!/bin/bash
# Verification script for ISS-006 section in DESIGN.md

if grep -q "## Incremental Updates for gid extract (ISS-006)" DESIGN.md; then
    echo "✓ ISS-006 section header found"
else
    echo "✗ ISS-006 section header NOT found"
    exit 1
fi

if grep -q "Incremental Extraction Flow" DESIGN.md; then
    echo "✓ Architecture diagram found"
else
    echo "✗ Architecture diagram NOT found"
    exit 1
fi

if grep -q "CachedCodeGraph" DESIGN.md; then
    echo "✓ CachedCodeGraph type found"
else
    echo "✗ CachedCodeGraph type NOT found"
    exit 1
fi

if grep -q "extract_incremental" DESIGN.md; then
    echo "✓ extract_incremental method found"
else
    echo "✗ extract_incremental method NOT found"
    exit 1
fi

if grep -q "Integration with LSP Daemon" DESIGN.md; then
    echo "✓ LSP Daemon integration section found"
else
    echo "✗ LSP Daemon integration section NOT found"
    exit 1
fi

echo ""
echo "All ISS-006 sections verified successfully!"
