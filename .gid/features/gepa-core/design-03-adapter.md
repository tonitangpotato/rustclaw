# Design: GEPA Adapter Trait

## 1. Overview

The `GEPAAdapter` trait is the sole integration boundary between `gepa-core` and external LLM providers. The crate never makes network calls or imports LLM SDKs (GUARD-5) — all LLM interaction is delegated through this async trait. Implementers provide four required methods (`execute`, `reflect`, `mutate`, `evaluate`) and one optional method (`merge`).

The design uses `async_trait` for ergonomic async trait methods with `Send` bounds. All methods receive immutable references to candidates (GUARD-2) and return `Result<T, GEPAError>` for uniform error handling. The trait requires `Send + Sync + 'static` to support multi-threaded async runtimes (GUARD-11). The engine calls adapter methods sequentially — never concurrently — so implementations need not be reentrant, though they may use internal concurrency.

**Satisfies:** GOAL-3.1 through GOAL-3.8. **Applicable GUARDs:** GUARD-3, GUARD-5, GUARD-8, GUARD-10, GUARD-11.

## 2. Components

### 2.1 GEPAAdapter Trait Definition

**Responsibility:** Define the four required and one optional async methods that bridge the core engine to user-provided LLM logic.

**Interface:**

```rust
#[async_trait]
pub trait GEPAAdapter: Send + Sync + 'static {
    async fn execute(
        &self,
        candidate: &Candidate,
        examples: &[Example],
    ) -> Result<Vec<ExecutionTrace>, GEPAError>;

    async fn reflect(
        &self,
        candidate: &Candidate,
        traces: &[ExecutionTrace],
    ) -> Result<Reflection, GEPAError>;

    async fn mutate(
        &self,
        parent: &Candidate,
        reflection: &Reflection,
        ancestor_lessons: &[String],
    ) -> Result<Candidate, GEPAError>;

    async fn evaluate(
        &self,
        candidate: &Candidate,
        examples: &[Example],
    ) -> Result<Vec<f64>, GEPAError>;

    async fn merge(
        &self,
        _parent_a: &Candidate,
        _parent_b: &Candidate,
    ) -> Result<Candidate, GEPAError> {
        Err(GEPAError::AdapterError {
            source: "merge not implemented".to_string(),
            retryable: false,
        })
    }
}
```

**Key Details:**

- `#[async_trait]` from the `async-trait` crate desugars to `fn method(...) -> Pin<Box<dyn Future<Output = ...> + Send + '_>>`. This ensures all returned futures are `Send` (GUARD-11), making `engine.run()` future also `Send`.
- All `&self` — adapter is shared immutably. Implementations needing interior mutability use `Arc<Mutex<_>>` or similar.
- All candidate references are `&Candidate` — never `&mut`. The engine never passes mutable candidate references to the adapter (GUARD-2).
- `merge` has a default implementation returning `Err` with `retryable: false`, so the engine immediately skips merge iterations for adapters that don't support it (GOAL-3.6). The engine treats this specific error as "merge unsupported" and disables merge for the remainder of the run.
- Generic bounds: `Send + Sync + 'static` enables the adapter to be stored in the engine struct and used across `.await` points in the tokio runtime (GUARD-11).

**Satisfies:** GOAL-3.1, GOAL-3.6, GOAL-3.8

### 2.2 Method Contracts

**Responsibility:** Define the exact input/output contracts for each adapter method so implementations are correct.

**`execute` contract:**

- **Input:** `&Candidate` (immutable, with text parameters in `candidate.params: HashMap<String, String>`), `&[Example]` (the minibatch, length = `config.minibatch_size` or less per GOAL-8.3).
- **Output:** `Vec<ExecutionTrace>` with **exactly one entry per input example, in the same order** (GOAL-3.2). Length mismatch is a caller-side bug in the adapter — the engine validates `traces.len() == examples.len()` and returns `GEPAError::AdapterError` if violated.
- **Partial failure:** If execution fails for a specific example, the adapter sets that trace's `output` to `""`, `score` to `None`, and may include diagnostic info in `asi` (GOAL-3.2). Whole-batch failure returns `Err(GEPAError::AdapterError { source, retryable })`.

**`reflect` contract:**

- **Input:** `&Candidate` (the parent that produced the traces), `&[ExecutionTrace]` (from the preceding `execute` call).
- **Output:** `Reflection { diagnosis: String, directions: Vec<String> }` (GOAL-3.3). The `diagnosis` is free-form natural language. `directions` are suggested improvement axes — the engine passes these to `mutate`.
- **Error:** `Err(GEPAError::AdapterError { .. })` on LLM failure. Retried per GOAL-7.5.

**`mutate` contract:**

- **Input:** `&Candidate` (parent), `&Reflection` (from reflect), `&[String]` (ancestor lessons — reflections from ancestors up to `config.max_lesson_depth`, GOAL-1.5). For seed candidates with no lineage, this slice is empty (GOAL-3.4).
- **Output:** A new `Candidate` with `parent_id = Some(parent.id)`, `generation = parent.generation + 1`, and potentially modified `params` HashMap. The returned candidate must have the same parameter keys as the parent. The engine validates key consistency and assigns the candidate ID (GOAL-5.4) — the adapter need not set the ID.
- **Error:** `Err(GEPAError::AdapterError { .. })` on LLM failure.

**`evaluate` contract:**

- **Input:** `&Candidate`, `&[Example]`. Used for acceptance testing, re-evaluation backfill, seed evaluation, and validation scoring.
- **Output:** `Vec<f64>` with **exactly one score per example, same order** (GOAL-3.5). Scores follow GUARD-10 semantics: higher is better, finite f64. The engine sanitizes post-return (NaN → None in cache, ±Inf → clamped).
- **Error:** `Err(GEPAError::AdapterError { .. })` on failure. `Err(GEPAError::RateLimited { retry_after })` if the LLM provider rate-limits. `Err(GEPAError::Timeout)` on timeout.

**`merge` contract:**

- **Input:** Two `&Candidate` references — parents selected from different Pareto front regions (GOAL-1.10).
- **Output:** A new `Candidate` combining strengths of both. The engine assigns ID and sets metadata.
- **Default:** Returns `Err(GEPAError::AdapterError { source: "merge not implemented", retryable: false })` (§2.5).

**Satisfies:** GOAL-3.2, GOAL-3.3, GOAL-3.4, GOAL-3.5

### 2.3 Trace Capture

**Responsibility:** Define the `ExecutionTrace` struct that captures rich diagnostic data flowing from `execute` into `reflect`.

**Interface:**

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionTrace {
    pub example_id: String,
    pub input: serde_json::Value,
    pub output: String,
    pub score: Option<f64>,
    pub asi: Option<String>,
}
```

**Key Details:**

- `example_id` matches the `Example::id` for the corresponding input. The engine uses this to correlate traces with examples.
- `input` is the full example input payload (copied from `Example::input`), included so the reflect step has complete context without needing the original example list.
- `output` is the LLM's generated response. Empty string `""` for failed examples (GOAL-3.2).
- `score` is an optional per-example score computed during execution. `None` means the example was not successfully scored (execution failure or score not applicable at execution time). This is a convenience — the authoritative scores come from `evaluate` (GOAL-3.5). The `reflect` method can use these scores to focus on low-scoring examples.
- `asi` (Actionable Side Information) is a free-form string carrying domain-specific diagnostic context: tool call logs, reasoning chains, error messages, retrieval results. The engine treats it as opaque — it flows from `execute` to `reflect` and is stored in candidate metadata for lineage inspection (GOAL-3.3). `None` if not provided.

**Data flow:** `execute` produces `Vec<ExecutionTrace>` → engine passes directly to `reflect(&candidate, &traces)` → reflect reads traces to diagnose failures and produce `Reflection`.

**Satisfies:** GOAL-3.2, GOAL-3.3

### 2.4 evaluate() for Re-evaluation

**Responsibility:** Provide a numeric-only scoring path that avoids the cost of full execution + trace capture.

**Key Details:**

- `evaluate` is deliberately separate from `execute` because re-evaluation backfill (GOAL-8.5a) and seed evaluation (GOAL-1.12) need only numeric scores — no traces, no ASI, no reflection.
- Typical implementation: `evaluate` runs the candidate on examples and computes scores (e.g., accuracy, F1) without capturing intermediate reasoning traces. This is significantly cheaper for LLM-based adapters since it may use a simpler prompt or skip chain-of-thought.
- The engine calls `evaluate` in these contexts:
  1. **Seed evaluation** (GOAL-1.12): before iteration 1, all seeds scored on initial minibatch.
  2. **New candidate acceptance** (GOAL-1.7a): child evaluated on current minibatch.
  3. **Re-evaluation backfill** (GOAL-8.5a): existing front members scored on previously unseen examples.
  4. **Validation scoring** (GOAL-8.6): all front members scored on the validation set.
- Score sanitization (GUARD-10) is applied by the engine after `evaluate` returns — the adapter need not handle NaN/Inf.

**Satisfies:** GOAL-3.5

### 2.5 Default Implementations

**Responsibility:** Provide sensible defaults for optional adapter methods to minimize boilerplate.

**Methods with defaults:**

| Method | Default Behavior | Rationale |
|---|---|---|
| `merge` | Returns `Err(AdapterError { source: "merge not implemented", retryable: false })` | Most adapters don't support merge initially. Non-retryable error tells the engine to skip merge without retry (GOAL-3.6). |

**Methods without defaults (all required):**

| Method | Why No Default |
|---|---|
| `execute` | Domain-specific LLM call — no generic implementation possible |
| `reflect` | Requires teacher LLM prompt engineering — domain-specific |
| `mutate` | Requires knowledge of prompt structure — domain-specific |
| `evaluate` | Scoring logic is task-specific — no universal metric |

A minimal working adapter implements exactly 4 methods. With the `Candidate` and `ExecutionTrace` structs already defined by the crate, an adapter can be written in ~30-50 lines for simple use cases (GOAL-3.8).

**Satisfies:** GOAL-3.6, GOAL-3.8

### 2.6 Cancellation Forwarding

**Responsibility:** Enable adapter implementations to detect and respond to engine cancellation.

**Key Details:**

- The `CancellationToken` (Feature 01 §2.6) is not passed directly to adapter methods. Instead, adapter implementations that need cancellation awareness receive the token at construction time and store it internally:

```rust
pub struct MyAdapter {
    llm_client: LlmClient,
    cancel: CancellationToken,
}

#[async_trait]
impl GEPAAdapter for MyAdapter {
    async fn execute(&self, candidate: &Candidate, examples: &[Example])
        -> Result<Vec<ExecutionTrace>, GEPAError>
    {
        for example in examples {
            if self.cancel.is_cancelled() {
                return Err(GEPAError::Cancelled);
            }
            // ... process example ...
        }
        Ok(traces)
    }
}
```

- The engine checks for `GEPAError::Cancelled` after every adapter call. On receiving `Cancelled`, the engine skips remaining steps and terminates with `TerminationReason::Cancelled` (Feature 01 §2.3).
- The engine also checks its own cancellation token between adapter calls — so even if the adapter doesn't implement cancellation checking, the engine will terminate after the current call completes (GOAL-1.11).
- The token is `Clone + Send + Sync + 'static` so adapters can share it across internal async tasks (e.g., parallel LLM calls within `execute`).

**Satisfies:** GOAL-3.7, GOAL-1.11

## 3. Example Adapter Implementation Sketch

A minimal adapter for a text-completion scoring task:

```rust
pub struct SimpleAdapter {
    client: LlmClient,
}

#[async_trait]
impl GEPAAdapter for SimpleAdapter {
    async fn execute(
        &self,
        candidate: &Candidate,
        examples: &[Example],
    ) -> Result<Vec<ExecutionTrace>, GEPAError> {
        let prompt = candidate.params.get("system_prompt")
            .ok_or_else(|| GEPAError::AdapterError {
                source: "missing system_prompt param".into(),
                retryable: false,
            })?;
        let mut traces = Vec::with_capacity(examples.len());
        for ex in examples {
            let output = self.client.complete(prompt, &ex.input.to_string()).await
                .map_err(|e| GEPAError::AdapterError {
                    source: e.to_string(),
                    retryable: true,
                })?;
            let score = if output.contains(ex.expected.as_deref().unwrap_or(""))
                { Some(1.0) } else { Some(0.0) };
            traces.push(ExecutionTrace {
                example_id: ex.id.clone(),
                input: ex.input.clone(),
                output,
                score,
                asi: None,
            });
        }
        Ok(traces)
    }

    async fn reflect(
        &self,
        candidate: &Candidate,
        traces: &[ExecutionTrace],
    ) -> Result<Reflection, GEPAError> {
        let failed: Vec<_> = traces.iter()
            .filter(|t| t.score.map_or(true, |s| s < 0.5))
            .collect();
        let diagnosis = format!("{}/{} examples failed", failed.len(), traces.len());
        Ok(Reflection {
            diagnosis,
            directions: vec!["Be more specific".into(), "Add examples".into()],
        })
    }

    async fn mutate(
        &self,
        parent: &Candidate,
        reflection: &Reflection,
        _ancestor_lessons: &[String],
    ) -> Result<Candidate, GEPAError> {
        let old_prompt = parent.params.get("system_prompt").unwrap();
        let new_prompt = format!("{}\n\n# Improvement: {}", old_prompt, reflection.diagnosis);
        let mut params = parent.params.clone();
        params.insert("system_prompt".into(), new_prompt);
        Ok(Candidate::new_child(parent, params))
    }

    async fn evaluate(
        &self,
        candidate: &Candidate,
        examples: &[Example],
    ) -> Result<Vec<f64>, GEPAError> {
        let prompt = candidate.params.get("system_prompt")
            .ok_or_else(|| GEPAError::AdapterError {
                source: "missing system_prompt param".into(),
                retryable: false,
            })?;
        let mut scores = Vec::with_capacity(examples.len());
        for ex in examples {
            let output = self.client.complete(prompt, &ex.input.to_string()).await
                .map_err(|e| GEPAError::AdapterError {
                    source: e.to_string(),
                    retryable: true,
                })?;
            let score = if output.contains(ex.expected.as_deref().unwrap_or(""))
                { 1.0 } else { 0.0 };
            scores.push(score);
        }
        Ok(scores)
    }
}
```

This sketch shows ~60 lines for all 4 required methods. A real implementation would use structured LLM calls for `reflect` and `mutate`, but the interface is the same. `merge` is not implemented — the default `Err` kicks in.

## 4. Integration Points

| This Feature | Depends On | Interface |
|---|---|---|
| §2.1 trait bounds | Feature 01 (Engine) §2.2 | Engine stores `A: GEPAAdapter` and calls methods sequentially |
| §2.2 execute output | Feature 01 (Engine) §2.2 step 5 | `Vec<ExecutionTrace>` passed to `reflect` |
| §2.2 evaluate output | Feature 02 (Pareto) §2.2 | Scores stored in eval cache, used for dominance |
| §2.3 `ExecutionTrace` | Feature 06 (State) | Serializable for checkpoint (trace stored in candidate metadata) |
| §2.6 `CancellationToken` | Feature 01 (Engine) §2.6 | Shared token, adapter checks independently |
| §2.2 error types | Feature 07 (Config) GOAL-7.5 | `GEPAError` variants determine retry behavior |
