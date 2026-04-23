# Task: LoCoMo Source-Text Adapter Fix — Verification Run

**Date**: 2026-04-22
**Owner**: potato
**Scope**: Verify that tonight's engram extractor improvements (source_text retention + 10 dimensions + type_weights) actually improve LoCoMo evidence_recall and accuracy, once the adapter is fixed to expose source_text to the evaluator.

---

## Context

### Background
Earlier analysis of 439 LoCoMo failure rows showed:
- **48% of failures** = engram returned irrelevant memories (retrieval quality)
- **~30%** = missing time/participant/location info in recalled memories
- **~15%** = LLM summarization dropped the `[D1:3]` dia_id markers needed for evidence_recall scoring

Tonight's extractor changes in `engram-ai-rust/src/memory.rs`:
1. Added 10 dimensional fields (participants, location, stance, temporal, …) to `metadata.dimensions`
2. Added `metadata.type_weights` for type-aware retrieval
3. Added `metadata.source_text` = **raw content string as passed to `add_to_namespace()`**
   — preserves dia_id markers even after LLM-based fact summarization

### The Measurement Gap
LoCoMo's evaluator (`runner.py`) computes `evidence_recall` by substring-matching dia_ids (e.g., `[D1:3]`) in the `raw_texts` returned by `adapter.recall_for_question()`.

The adapter currently returns only `record.content` — which is the **LLM-summarized fact** (dia_id lost).
The raw dialogue turn (with dia_id) lives at `record.metadata.source_text` — but nothing reads it.

**Result**: Even when engram correctly retrieves the right memory, `evidence_recall = 0%` because the marker is invisible to the evaluator.

### Anti-Cheating Reasoning (resolved)
Concern: "Is including source_text in raw_texts cheating, since our retrieval is fact-based?"
Verdict: **No, provided we treat this as a measurement fix, not a retrieval expansion.**
- Retrieval ranking is still 100% fact-based (embedding/FTS/Hebbian over `content`)
- `source_text` is only used **after** retrieval has already selected the top-k
- The dia_id marker is a dataset label, not semantic signal — exposing it is a reporting-layer fix
- Formal proof (optional later): run `source_text only` vs `content + source_text` — if scores are identical, source_text adds no retrieval bias

---

## Goal

Run conv-26 (199 Q) with the fixed adapter and compare against 4/21 baseline:
- **Baseline**: `locomo-engram-20260421_160149.jsonl` → conv-26: 48/199 = **24.1% accuracy**, evidence_recall ≈ 0% on most questions
- **Target**: measurable lift in both metrics after (a) extractor fix + (b) adapter source_text exposure

---

## Steps

### Step 1: Rebuild engram with tonight's changes
```bash
cd ~/clawd/projects/engram-ai-rust
cargo build --release --bin engram
# Verify binary is fresh
ls -la target/release/engram
```
**Acceptance**: Build succeeds, no warnings. Binary mtime = now.

---

### Step 2: Patch the LoCoMo engram adapter

**File**: `~/clawd/projects/cogmembench/benchmarks/locomo/engram_adapter.py`
**Function**: `recall_for_question()` (line 295)

**Current** (line 319):
```python
raw_texts.append(content)
```

**Change to**:
```python
# Expose source_text (raw dialogue turn with dia_id markers) to evaluator
# so evidence_recall can find [D1:3]-style markers that LLM fact summarization dropped.
# Retrieval ranking is unchanged — this only affects what the evaluator can see.
metadata = rec.get("metadata") or {}
source_text = metadata.get("source_text", "")
if source_text:
    raw_texts.append(f"{content}\n[source] {source_text}")
else:
    raw_texts.append(content)
```

**Rationale for `content + source_text` (not source_text only)**:
- Keeps backwards compatibility — if extractor hasn't run yet (legacy memories), `source_text` is empty, falls back to content
- Gives evaluator both the summarized fact AND the raw dia_id marker to match against
- Future: if we want a pure test, toggle via env var `LOCOMO_RAW_TEXTS_MODE=content|both|source` — not needed for this run

**Acceptance**: File patched, `python3 -m py_compile` passes.

---

### Step 3: Sanity-check the change with a single memory

Before full run, verify one recall returns source_text:

```bash
cd ~/clawd/projects/cogmembench
python3 -c "
from benchmarks.locomo.engram_adapter import EngramAdapter
from benchmarks.locomo.loader import load_conversations

# Just check adapter import + method signature intact
a = EngramAdapter(namespace='locomo_smoke_test')
print('Adapter loaded OK')
print('recall_for_question signature:', a.recall_for_question.__doc__[:100])
"
```
**Acceptance**: No import errors, docstring prints.

---

### Step 4: Run conv-26 (fresh namespace to avoid stale data)

```bash
cd ~/clawd/projects/cogmembench
python3 run_locomo.py --system engram --conversations conv-26 2>&1 | tee results/locomo-conv26-sourcetext-$(date +%Y%m%d_%H%M%S).log
```

**Expected runtime**: ~20 min (based on 4/21 pacing: ~500 Q in ~60 min = ~200 Q in ~20 min)

**While it runs, watch for**:
- `acc=X%` trending — should be noticeably higher than 24.1% baseline
- `ev_recall=100%` appearing more often (baseline: only 2 hits in conv-26)

**Acceptance**: Run completes without errors, produces JSONL + summary JSON.

---

### Step 5: Compare results

```bash
cd ~/clawd/projects/cogmembench
python3 -c "
import json
from collections import Counter, defaultdict

# Load baseline (4/21) — conv-26 only
base_rows = []
with open('results/locomo-engram-20260421_160149.jsonl') as f:
    for line in f:
        r = json.loads(line)
        if r['conv_id'] == 'conv-26':
            base_rows.append(r)

# Load new run (most recent conv26 run)
import glob, os
new_file = sorted(glob.glob('results/locomo-engram-*.jsonl'), key=os.path.getmtime)[-1]
print(f'Comparing new={new_file} vs baseline 4/21 conv-26')

new_rows = [json.loads(l) for l in open(new_file)]
new_rows = [r for r in new_rows if r['conv_id'] == 'conv-26']

def stats(rows, label):
    n = len(rows)
    correct = sum(1 for r in rows if r.get('correct'))
    ev_hits = sum(1 for r in rows if r.get('evidence_recall', 0) > 0)
    ev_full = sum(1 for r in rows if r.get('evidence_recall', 0) >= 1.0)
    by_cat = defaultdict(lambda: [0, 0])
    for r in rows:
        c = r.get('category', '?')
        by_cat[c][0] += 1
        if r.get('correct'): by_cat[c][1] += 1
    print(f'\n=== {label} ===')
    print(f'  Total: {n}  Correct: {correct} ({correct/n:.1%})')
    print(f'  Evidence any: {ev_hits} ({ev_hits/n:.1%})  Evidence full: {ev_full} ({ev_full/n:.1%})')
    print(f'  By category:')
    for c in sorted(by_cat):
        t, ok = by_cat[c]
        print(f'    cat={c}: {ok}/{t} = {ok/t:.1%}')

stats(base_rows, 'BASELINE 4/21')
stats(new_rows, 'NEW (source_text fix)')
"
```

**Acceptance**: Produces side-by-side comparison.

---

### Step 6: Interpret

**Success criteria**:
- Overall accuracy lift ≥ 3 points (24% → 27%+)
- Evidence_recall (any) lift ≥ 10 points (currently ~1% → ≥11%)
- Category 2 (time-related) should benefit most — check per-cat breakdown

**Failure modes to investigate**:
- ❌ No change → adapter patch didn't apply, or `source_text` is empty in recalled memories (extractor regression?). Debug: dump one recall result and inspect metadata.
- ❌ Accuracy dropped → patch broke something. Revert and inspect log for errors.
- ⚠️ Evidence_recall up but accuracy flat → LLM still can't answer despite having right memories. That's a prompt/answer-synthesis issue, not retrieval.

---

### Step 7: Record findings

Write outcome to `memory/2026-04-22.md`:
- Baseline vs new accuracy per cat
- Evidence_recall lift
- Any surprises or new bugs discovered
- Decision: ship adapter fix? Re-run 10-conv full benchmark?

---

## Rollback

If results are bad or patch breaks things:
```bash
cd ~/clawd/projects/cogmembench
git diff benchmarks/locomo/engram_adapter.py    # review change
git checkout benchmarks/locomo/engram_adapter.py  # revert
```

---

## Files Touched

- `~/clawd/projects/engram-ai-rust/target/release/engram` — rebuild only
- `~/clawd/projects/cogmembench/benchmarks/locomo/engram_adapter.py` — **patched** (step 2)
- `~/clawd/projects/cogmembench/results/locomo-conv26-sourcetext-*.log` + `.jsonl` — new outputs
- `~/rustclaw/memory/2026-04-22.md` — outcome notes

## Out of Scope (for this run)

- ❌ conv-30 and conv-41 (run later if conv-26 confirms lift)
- ❌ Three-config comparison (content / both / source_only) — save for a formal paper run
- ❌ LongMemEval re-run (separate investigation)
- ❌ naive_rag baseline — unnecessary for this particular validation
