# ISS-008: KC compilation pipeline never populates kc_compilation_sources table

## Summary

The Knowledge Compiler's three compilation code paths (`compile_new()`, `recompile()`, `compile_all()`) store source memory IDs in the `source_memory_ids` JSON field of `kc_topic_pages`, but **never call `save_source_refs()`** to populate the `kc_compilation_sources` relational table. This table is empty for all 167 topics.

## Impact

- **All 167 topics marked stale**: `DecayEngine::evaluate_topic()` calls `get_source_refs()` → gets empty vec → freshness defaults to 0 → everything is stale
- **10,719 broken links**: `HealthReport` link audit calls `get_source_refs()` → empty → all source links report as broken
- **4,919 conflicts**: Secondary issue — similarity threshold too low in conflict detection (not part of this fix)

## Root Cause

Three code paths in engramai that persist compiled topics:

1. **`CompilationPipeline::compile_new()`** (`src/compiler/compilation.rs:500`)
   - Sets `source_memory_ids` in `TopicMetadata` ✅
   - Calls `create_topic_page()` ✅
   - Calls `save_compilation_record()` ✅
   - **Never calls `save_source_refs()`** ❌

2. **`CompilationPipeline::recompile()`** (`src/compiler/compilation.rs:566`)
   - Sets `source_memory_ids` in `TopicMetadata` ✅
   - Calls `update_topic_page()` ✅
   - Calls `save_compilation_record()` ✅
   - **Never calls `save_source_refs()`** ❌

3. **`KnowledgeCompiler::compile_all()`** (`src/compiler/api.rs:588`)
   - Sets `source_memory_ids` in `TopicMetadata` ✅
   - Calls `create_topic_page()` ✅
   - Calls `save_compilation_record()` ✅
   - **Never calls `save_source_refs()`** ❌

## Fix

After each `create_topic_page()`/`update_topic_page()` + `save_compilation_record()` call, add:

```rust
let source_refs: Vec<SourceMemoryRef> = memories.iter().map(|m| SourceMemoryRef {
    memory_id: m.id.clone(),
    relevance_score: m.importance,
    added_at: now,
}).collect();
self.store.save_source_refs(&topic_id, &source_refs)?;
```

## Affected Files

- `/Users/potato/clawd/projects/engram-ai-rust/src/compiler/compilation.rs` (2 sites)
- `/Users/potato/clawd/projects/engram-ai-rust/src/compiler/api.rs` (1 site)

## Post-Fix

After deploying the fix, run `knowledge_compile` to recompile all topics — this will populate the `kc_compilation_sources` table for existing topics. Then `knowledge_health` should show clean results.
