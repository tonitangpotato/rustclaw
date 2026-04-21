//! Interoceptive Signal Emitters — Layer 1 of the Emotion System.
//!
//! Collects raw operational metrics from RustClaw's runtime and converts
//! them into InteroceptiveSignals for the hub. These are FACTS, not
//! LLM-derived interpretations.
//!
//! Architecture (potato's 3-layer design):
//!   Layer 1: Signal Collection (this module) — pure metric → signal conversion
//!   Layer 2: Signal Processing (engramai InteroceptiveHub) — aggregation + somatic markers
//!   Layer 3: Behavior Modulation (prompt injection + regulation actions)
//!
//! Five signal lines:
//!   1. OperationalLoad  — token budget pressure
//!   2. ExecutionStress   — loop depth, retries, tool failures
//!   3. CognitiveFlow     — task completion, latency, session coherence
//!   4. ResourcePressure  — disk, memory, queue depth
//!   5. SomaticMarkers    — aggregated arousal/valence/dominance/urgency (computed from 1-4)

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use engramai::interoceptive::{InteroceptiveSignal, SignalContext, SignalSource};

// ═══════════════════════════════════════════════════════════════════════
//  Operational Load — Token budget pressure
// ═══════════════════════════════════════════════════════════════════════

/// Tracks token consumption rate and budget pressure.
///
/// Fed by TokenTracker (already exists in llm.rs). Converts raw counters
/// into valence/arousal signals.
#[derive(Debug)]
pub struct OperationalLoadMeter {
    /// Snapshot of total tokens at last sample.
    last_total_tokens: AtomicU64,
    /// Timestamp of last sample.
    last_sample: Mutex<Instant>,
    /// Configured hourly budget (from TokenTracker).
    hourly_budget: AtomicU64,
}

impl OperationalLoadMeter {
    pub fn new(hourly_budget: u64) -> Self {
        Self {
            last_total_tokens: AtomicU64::new(0),
            last_sample: Mutex::new(Instant::now()),
            hourly_budget: AtomicU64::new(hourly_budget),
        }
    }

    /// Sample current token state and produce a signal.
    ///
    /// Call this periodically (every heartbeat or every N requests).
    pub fn sample(&self, total_tokens: u64, hourly_tokens: u64) -> InteroceptiveSignal {
        let budget = self.hourly_budget.load(Ordering::Relaxed);
        let prev_total = self.last_total_tokens.swap(total_tokens, Ordering::Relaxed);

        // Compute rate
        let elapsed = {
            let mut last = self.last_sample.lock().unwrap();
            let elapsed = last.elapsed();
            *last = Instant::now();
            elapsed
        };

        let tokens_delta = total_tokens.saturating_sub(prev_total);
        let elapsed_secs = elapsed.as_secs_f64().max(1.0);
        let tokens_per_second = tokens_delta as f64 / elapsed_secs;

        // Budget utilization: how much of the hourly budget is consumed
        let budget_used_pct = if budget > 0 {
            (hourly_tokens as f64 / budget as f64).clamp(0.0, 1.0)
        } else {
            0.0
        };

        // Runway: at current rate, how long until budget is exhausted
        let budget_runway_secs = if tokens_per_second > 0.0 && budget > 0 {
            let remaining = budget.saturating_sub(hourly_tokens) as f64;
            remaining / tokens_per_second
        } else {
            f64::MAX
        };

        // Valence: negative when budget pressure is high
        // 0-50% budget → positive (0.5 to 0.0)
        // 50-80% → slightly negative (0.0 to -0.3)
        // 80-100% → very negative (-0.3 to -1.0)
        let valence = if budget_used_pct < 0.5 {
            0.5 - budget_used_pct
        } else if budget_used_pct < 0.8 {
            -((budget_used_pct - 0.5) / 0.3) * 0.3
        } else {
            -0.3 - ((budget_used_pct - 0.8) / 0.2) * 0.7
        };

        // Arousal: proportional to consumption rate
        let arousal = budget_used_pct;

        InteroceptiveSignal::new(SignalSource::OperationalLoad, None, valence, arousal)
            .with_context(SignalContext::TokenPressure {
                budget_used_pct,
                tokens_per_second,
                budget_runway_secs: budget_runway_secs.min(86400.0),
            })
    }

    pub fn set_hourly_budget(&self, budget: u64) {
        self.hourly_budget.store(budget, Ordering::Relaxed);
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Execution Stress — Loop depth, retries, tool failures
// ═══════════════════════════════════════════════════════════════════════

/// Tracks agentic loop execution stress.
///
/// Updated in real-time as the agent processes tool calls.
#[derive(Debug)]
pub struct ExecutionStressMeter {
    /// Current loop depth (0 = not in loop).
    loop_depth: AtomicU32,
    /// Retry count for current task.
    retry_count: AtomicU32,
    /// Recent tool call outcomes: ring buffer of success (1) / failure (0).
    tool_outcomes: Mutex<RingBuffer<bool>>,
    /// Consecutive failures counter.
    consecutive_failures: AtomicU32,
}

impl ExecutionStressMeter {
    pub fn new() -> Self {
        Self {
            loop_depth: AtomicU32::new(0),
            retry_count: AtomicU32::new(0),
            tool_outcomes: Mutex::new(RingBuffer::new(50)), // last 50 tool calls
            consecutive_failures: AtomicU32::new(0),
        }
    }

    /// Record entering a deeper loop level.
    pub fn enter_loop(&self) {
        self.loop_depth.fetch_add(1, Ordering::Relaxed);
    }

    /// Record exiting a loop level.
    pub fn exit_loop(&self) {
        let prev = self.loop_depth.fetch_sub(1, Ordering::Relaxed);
        if prev == 0 {
            // Underflow protection
            self.loop_depth.store(0, Ordering::Relaxed);
        }
    }

    /// Record a retry attempt.
    pub fn record_retry(&self) {
        self.retry_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Reset retry counter (task completed or abandoned).
    pub fn reset_retries(&self) {
        self.retry_count.store(0, Ordering::Relaxed);
    }

    /// Record a tool call outcome.
    pub fn record_tool_outcome(&self, success: bool) {
        if let Ok(mut outcomes) = self.tool_outcomes.lock() {
            outcomes.push(success);
        }
        if success {
            self.consecutive_failures.store(0, Ordering::Relaxed);
        } else {
            self.consecutive_failures.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Get the current count of consecutive failures.
    pub fn consecutive_failures(&self) -> u32 {
        self.consecutive_failures.load(Ordering::Relaxed)
    }

    /// Produce a stress signal from current state.
    pub fn sample(&self) -> InteroceptiveSignal {
        let depth = self.loop_depth.load(Ordering::Relaxed);
        let retries = self.retry_count.load(Ordering::Relaxed);
        let consec_fail = self.consecutive_failures.load(Ordering::Relaxed);

        let failure_rate = self
            .tool_outcomes
            .lock()
            .map(|outcomes| {
                let total = outcomes.len();
                if total == 0 {
                    return 0.0;
                }
                let failures = outcomes.iter().filter(|&&s| !s).count();
                failures as f64 / total as f64
            })
            .unwrap_or(0.0);

        // Valence: negative when stressed
        // Weighted combination of stress factors
        let depth_stress = (depth as f64 / 10.0).clamp(0.0, 1.0); // >10 iterations = max stress
        let retry_stress = (retries as f64 / 5.0).clamp(0.0, 1.0); // >5 retries = max stress
        let failure_stress = failure_rate;
        let consec_stress = (consec_fail as f64 / 3.0).clamp(0.0, 1.0); // 3+ consecutive = max

        let stress_composite = (depth_stress * 0.2
            + retry_stress * 0.3
            + failure_stress * 0.3
            + consec_stress * 0.2)
            .clamp(0.0, 1.0);

        // Map composite stress to valence: 0 stress = +0.5, 1.0 stress = -1.0
        let valence = 0.5 - stress_composite * 1.5;

        // Arousal: directly proportional to stress
        let arousal = stress_composite;

        InteroceptiveSignal::new(SignalSource::ExecutionStress, None, valence, arousal)
            .with_context(SignalContext::LoopStress {
                loop_depth: depth,
                retry_count: retries,
                tool_failure_rate: failure_rate,
                consecutive_failures: consec_fail,
            })
    }
}

impl Default for ExecutionStressMeter {
    fn default() -> Self {
        Self::new()
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Cognitive Flow — Task completion, latency, session coherence
// ═══════════════════════════════════════════════════════════════════════

/// Tracks cognitive flow: how well tasks are being completed.
#[derive(Debug)]
pub struct CognitiveFlowMeter {
    /// Recent task outcomes: ring buffer of success (true) / failure (false).
    task_outcomes: Mutex<RingBuffer<bool>>,
    /// Last response latency.
    last_latency: Mutex<Option<Duration>>,
    /// Session start time.
    session_start: Instant,
}

impl CognitiveFlowMeter {
    pub fn new() -> Self {
        Self {
            task_outcomes: Mutex::new(RingBuffer::new(20)), // last 20 tasks
            last_latency: Mutex::new(None),
            session_start: Instant::now(),
        }
    }

    /// Record a task completion (success or failure).
    pub fn record_task(&self, success: bool) {
        if let Ok(mut outcomes) = self.task_outcomes.lock() {
            outcomes.push(success);
        }
    }

    /// Record response latency for the most recent request.
    pub fn record_latency(&self, latency: Duration) {
        if let Ok(mut last) = self.last_latency.lock() {
            *last = Some(latency);
        }
    }

    /// Produce a flow signal.
    pub fn sample(&self) -> InteroceptiveSignal {
        let completion_rate = self
            .task_outcomes
            .lock()
            .map(|outcomes| {
                let total = outcomes.len();
                if total == 0 {
                    return 0.5; // neutral when no data
                }
                let successes = outcomes.iter().filter(|&&s| s).count();
                successes as f64 / total as f64
            })
            .unwrap_or(0.5);

        let latency_ms = self
            .last_latency
            .lock()
            .ok()
            .and_then(|l| *l)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        let session_duration_secs = self.session_start.elapsed().as_secs();

        // Valence: positive when completing tasks successfully
        // completion_rate 0.0 → valence -0.8
        // completion_rate 0.5 → valence 0.0
        // completion_rate 1.0 → valence +0.8
        let valence = (completion_rate - 0.5) * 1.6;

        // Arousal: high latency or very long sessions → higher arousal
        let latency_factor = (latency_ms as f64 / 30_000.0).clamp(0.0, 1.0); // 30s = max
        let session_factor = (session_duration_secs as f64 / 14400.0).clamp(0.0, 0.5); // 4h = 0.5
        let arousal = (latency_factor * 0.7 + session_factor * 0.3).clamp(0.0, 1.0);

        InteroceptiveSignal::new(SignalSource::CognitiveFlow, None, valence, arousal)
            .with_context(SignalContext::TaskFlow {
                task_completion_rate: completion_rate,
                response_latency_ms: latency_ms,
                session_duration_secs,
            })
    }
}

impl Default for CognitiveFlowMeter {
    fn default() -> Self {
        Self::new()
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Resource Pressure — Disk, queue depth
// ═══════════════════════════════════════════════════════════════════════

/// Tracks system resource pressure.
#[derive(Debug)]
pub struct ResourcePressureMeter {
    /// Number of pending tasks in queue.
    queue_depth: AtomicU32,
}

impl ResourcePressureMeter {
    pub fn new() -> Self {
        Self {
            queue_depth: AtomicU32::new(0),
        }
    }

    /// Update queue depth.
    pub fn set_queue_depth(&self, depth: u32) {
        self.queue_depth.store(depth, Ordering::Relaxed);
    }

    /// Increment queue depth.
    pub fn task_enqueued(&self) {
        self.queue_depth.fetch_add(1, Ordering::Relaxed);
    }

    /// Decrement queue depth.
    pub fn task_dequeued(&self) {
        let prev = self.queue_depth.fetch_sub(1, Ordering::Relaxed);
        if prev == 0 {
            self.queue_depth.store(0, Ordering::Relaxed);
        }
    }

    /// Sample resource pressure. Reads disk free space from the OS.
    pub fn sample(&self) -> InteroceptiveSignal {
        let disk_free_gb = get_disk_free_gb();
        let queue = self.queue_depth.load(Ordering::Relaxed);

        // Valence: negative when resources are scarce
        // Disk: <5GB = very negative, 5-15GB = slightly negative, >15GB = neutral/positive
        let disk_valence = if disk_free_gb < 5.0 {
            -0.8 - (5.0 - disk_free_gb) / 5.0 * 0.2 // -0.8 to -1.0
        } else if disk_free_gb < 15.0 {
            -((15.0 - disk_free_gb) / 10.0) * 0.8 // 0.0 to -0.8
        } else {
            0.2 // comfortable
        };

        // Queue: 0 = calm, 5+ = stressed
        let queue_valence = if queue == 0 {
            0.2
        } else {
            -(queue as f64 / 5.0).clamp(0.0, 1.0) * 0.5
        };

        let valence = disk_valence * 0.7 + queue_valence * 0.3;

        // Arousal: high when resources are critically low
        let disk_arousal = if disk_free_gb < 5.0 {
            0.9
        } else if disk_free_gb < 10.0 {
            0.5
        } else {
            0.1
        };
        let queue_arousal = (queue as f64 / 10.0).clamp(0.0, 0.8);
        let arousal = (disk_arousal * 0.6 + queue_arousal * 0.4).clamp(0.0, 1.0);

        InteroceptiveSignal::new(SignalSource::ResourcePressure, None, valence, arousal)
            .with_context(SignalContext::SystemPressure {
                disk_free_gb,
                queue_depth: queue,
            })
    }
}

impl Default for ResourcePressureMeter {
    fn default() -> Self {
        Self::new()
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Output Productivity — Read/Write ratio, repeat reads, spawn tracking
// ═══════════════════════════════════════════════════════════════════════

/// Tracks whether the agent is actually producing output or just "busy reading".
///
/// Three sub-detectors:
/// 1. ReadWriteRatio — too many reads with zero writes → "假忙" (fake busy)
/// 2. RepeatRead — same file read 3+ times → stuck in a loop
/// 3. SpawnBlind — spawn_specialist fired but output never checked
#[derive(Debug)]
pub struct OutputProductivityMeter {
    /// Recent tool calls: ring buffer of (tool_name_category, file_path_hash).
    recent_calls: Mutex<Vec<ToolCallRecord>>,
    /// Max window size for recent calls.
    window_size: usize,
    /// Cached warning text from last sample.
    last_warning: Mutex<Option<String>>,
}

/// Categorized tool call record for output tracking.
#[derive(Debug, Clone)]
struct ToolCallRecord {
    category: ToolCategory,
    /// For read operations: hash of the file path to detect repeats.
    file_hash: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
enum ToolCategory {
    Read,       // read_file, search_files, list_dir, web_fetch
    Write,      // write_file, edit_file
    Spawn,      // spawn_specialist
    Other,      // everything else
}

impl OutputProductivityMeter {
    pub fn new() -> Self {
        Self {
            recent_calls: Mutex::new(Vec::with_capacity(20)),
            window_size: 12, // look at last 12 tool calls
            last_warning: Mutex::new(None),
        }
    }

    /// Record a tool call. Call this after every tool execution.
    pub fn record_tool_call(&self, tool_name: &str, first_arg: Option<&str>) {
        let category = match tool_name {
            "read_file" | "search_files" | "list_dir" | "web_fetch" => ToolCategory::Read,
            "write_file" | "edit_file" => ToolCategory::Write,
            "spawn_specialist" => ToolCategory::Spawn,
            _ => ToolCategory::Other,
        };

        let file_hash = if category == ToolCategory::Read {
            first_arg.map(|path| {
                use std::hash::{Hash, Hasher};
                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                path.hash(&mut hasher);
                hasher.finish()
            })
        } else {
            None
        };

        if let Ok(mut calls) = self.recent_calls.lock() {
            calls.push(ToolCallRecord { category, file_hash });
            // Keep only the last window_size entries
            if calls.len() > self.window_size * 2 {
                let drain_to = calls.len() - self.window_size;
                calls.drain(..drain_to);
            }
        }
    }

    /// Produce a productivity signal with anomaly detection.
    pub fn sample(&self) -> InteroceptiveSignal {
        let calls = self.recent_calls.lock().unwrap();
        let window: Vec<_> = if calls.len() > self.window_size {
            calls[calls.len() - self.window_size..].to_vec()
        } else {
            calls.clone()
        };
        drop(calls);

        if window.len() < 4 {
            // Not enough data yet
            return InteroceptiveSignal::new(
                SignalSource::CognitiveFlow, // reuse existing source
                Some("output_productivity".to_string()),
                0.0, // neutral
                0.0,
            );
        }

        let read_count = window.iter().filter(|c| c.category == ToolCategory::Read).count();
        let write_count = window.iter().filter(|c| c.category == ToolCategory::Write).count();
        let spawn_count = window.iter().filter(|c| c.category == ToolCategory::Spawn).count();

        // Detector 1: ReadWriteRatio — reads >= 6 with zero writes
        let read_heavy = read_count >= 6 && write_count == 0;

        // Detector 2: RepeatRead — same file read 3+ times
        let repeat_read = {
            let mut hash_counts = std::collections::HashMap::new();
            for call in window.iter() {
                if let Some(hash) = call.file_hash {
                    *hash_counts.entry(hash).or_insert(0u32) += 1;
                }
            }
            hash_counts.values().any(|&count| count >= 3)
        };

        // Detector 3: SpawnBlind — spawned but no subsequent read to check output
        // (simplified: spawned recently but no writes followed)
        let spawn_no_output = spawn_count > 0 && write_count == 0 && read_count < 2;

        // Composite score
        let anomaly_count = read_heavy as u8 + repeat_read as u8 + spawn_no_output as u8;

        let valence = match anomaly_count {
            0 => 0.3,      // healthy: producing output
            1 => -0.3,     // mild concern
            2 => -0.6,     // significant concern
            _ => -0.9,     // critical: multiple signals firing
        };

        let arousal = match anomaly_count {
            0 => 0.1,
            1 => 0.4,
            2 => 0.7,
            _ => 0.9,
        };

        let signal = InteroceptiveSignal::new(
            SignalSource::CognitiveFlow,
            Some("output_productivity".to_string()),
            valence,
            arousal,
        );

        // Attach warning text for prompt injection (stored separately, not in SignalContext)
        if anomaly_count > 0 {
            let mut warnings = Vec::new();
            if read_heavy {
                warnings.push(format!(
                    "ReadWriteRatio: {} reads, 0 writes in last {} calls. Are you preparing or procrastinating?",
                    read_count, window.len()
                ));
            }
            if repeat_read {
                warnings.push("RepeatRead: Same file read 3+ times. Info is already in context — start writing.".to_string());
            }
            if spawn_no_output {
                warnings.push("SpawnBlind: Spawned sub-agent but no output produced or checked.".to_string());
            }
            let warning_text = warnings.join(" | ");
            if let Ok(mut w) = self.last_warning.lock() {
                *w = Some(warning_text);
            }
        } else if let Ok(mut w) = self.last_warning.lock() {
            *w = None;
        }

        signal
    }

    /// Get human-readable warnings for prompt injection.
    /// Returns None if everything is healthy.
    pub fn get_warnings(&self) -> Option<String> {
        // Read from cached warning (updated by last sample() call)
        self.last_warning.lock().ok().and_then(|w| w.clone()).map(|text| {
            format!("⚠️ Output productivity alert: {}", text)
        })
    }

    /// Reset the meter (e.g., at session start or after a successful write burst).
    pub fn reset(&self) {
        if let Ok(mut calls) = self.recent_calls.lock() {
            calls.clear();
        }
        if let Ok(mut w) = self.last_warning.lock() {
            *w = None;
        }
    }
}

impl Default for OutputProductivityMeter {
    fn default() -> Self {
        Self::new()
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Somatic Summary — Aggregated PAD (Pleasure-Arousal-Dominance) + Urgency
// ═══════════════════════════════════════════════════════════════════════

/// Aggregated somatic state computed from the four signal lines.
///
/// This is the "gut feeling" — a compressed representation of all runtime
/// signals. Not stored in engramai; computed locally for prompt injection.
#[derive(Debug, Clone, Copy)]
pub struct SomaticSummary {
    /// Pleasure/valence: -1.0 (distressed) to +1.0 (thriving).
    pub valence: f64,
    /// Arousal: 0.0 (calm) to 1.0 (high alert).
    pub arousal: f64,
    /// Dominance/control: 0.0 (helpless) to 1.0 (in control).
    pub dominance: f64,
    /// Urgency: 0.0 (no pressure) to 1.0 (critical deadline/budget).
    pub urgency: f64,
}

impl SomaticSummary {
    /// Compute from the four signal lines.
    pub fn from_signals(
        load: &InteroceptiveSignal,
        stress: &InteroceptiveSignal,
        flow: &InteroceptiveSignal,
        resource: &InteroceptiveSignal,
    ) -> Self {
        // Valence: weighted average
        let valence = (load.valence * 0.25
            + stress.valence * 0.30
            + flow.valence * 0.30
            + resource.valence * 0.15)
            .clamp(-1.0, 1.0);

        // Arousal: max-biased (if any line is alarmed, we should be alert)
        let max_arousal = load
            .arousal
            .max(stress.arousal)
            .max(flow.arousal)
            .max(resource.arousal);
        let avg_arousal =
            (load.arousal + stress.arousal + flow.arousal + resource.arousal) / 4.0;
        let arousal = (max_arousal * 0.6 + avg_arousal * 0.4).clamp(0.0, 1.0);

        // Dominance: inverse of stress. Low loop depth + low failures = high control
        // stress.valence is already negative when stressed, so we invert:
        let dominance = ((stress.valence + 1.0) / 2.0).clamp(0.0, 1.0);

        // Urgency: token pressure + resource pressure
        let urgency = (load.arousal * 0.6 + resource.arousal * 0.4).clamp(0.0, 1.0);

        Self {
            valence,
            arousal,
            dominance,
            urgency,
        }
    }

    /// Format for system prompt injection.
    pub fn to_prompt_section(&self) -> String {
        let feeling = match (self.valence > 0.2, self.arousal > 0.5) {
            (true, false) => "calm and productive",
            (true, true) => "energized and focused",
            (false, false) => "subdued but stable",
            (false, true) => "stressed and alert",
        };

        format!(
            "- **Somatic**: {} (valence {:.2}, arousal {:.2}, dominance {:.2}, urgency {:.2})",
            feeling, self.valence, self.arousal, self.dominance, self.urgency
        )
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Signal Emitter — Orchestrates all meters
// ═══════════════════════════════════════════════════════════════════════

/// The main signal emitter. Owns all four meters, provides a unified
/// interface for sampling and feeding signals to the InteroceptiveHub.
pub struct SignalEmitter {
    pub operational_load: OperationalLoadMeter,
    pub execution_stress: ExecutionStressMeter,
    pub cognitive_flow: CognitiveFlowMeter,
    pub resource_pressure: ResourcePressureMeter,
    pub output_productivity: OutputProductivityMeter,
}

impl SignalEmitter {
    pub fn new(hourly_token_budget: u64) -> Self {
        Self {
            operational_load: OperationalLoadMeter::new(hourly_token_budget),
            execution_stress: ExecutionStressMeter::new(),
            cognitive_flow: CognitiveFlowMeter::new(),
            resource_pressure: ResourcePressureMeter::new(),
            output_productivity: OutputProductivityMeter::new(),
        }
    }

    /// Sample all five signal lines and return them with a somatic summary.
    ///
    /// The caller should feed these signals to the InteroceptiveHub.
    pub fn sample_all(
        &self,
        total_tokens: u64,
        hourly_tokens: u64,
    ) -> (Vec<InteroceptiveSignal>, SomaticSummary) {
        let load = self.operational_load.sample(total_tokens, hourly_tokens);
        let stress = self.execution_stress.sample();
        let flow = self.cognitive_flow.sample();
        let resource = self.resource_pressure.sample();
        let output = self.output_productivity.sample();

        let summary = SomaticSummary::from_signals(&load, &stress, &flow, &resource);

        (vec![load, stress, flow, resource, output], summary)
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Utilities
// ═══════════════════════════════════════════════════════════════════════

/// Ring buffer for recent outcomes.
#[derive(Debug)]
struct RingBuffer<T> {
    data: Vec<T>,
    capacity: usize,
    pos: usize,
    full: bool,
}

impl<T: Clone> RingBuffer<T> {
    fn new(capacity: usize) -> Self {
        Self {
            data: Vec::with_capacity(capacity),
            capacity,
            pos: 0,
            full: false,
        }
    }

    fn push(&mut self, item: T) {
        if self.data.len() < self.capacity {
            self.data.push(item);
        } else {
            self.data[self.pos] = item;
            self.full = true;
        }
        self.pos = (self.pos + 1) % self.capacity;
    }

    fn len(&self) -> usize {
        if self.full {
            self.capacity
        } else {
            self.data.len()
        }
    }

    fn iter(&self) -> impl Iterator<Item = &T> {
        self.data.iter()
    }
}

/// Get available disk space in GB for the root filesystem.
fn get_disk_free_gb() -> f64 {
    #[cfg(unix)]
    {
        use std::ffi::CString;
        use std::mem::MaybeUninit;

        let path = CString::new("/").unwrap();
        let mut stat = MaybeUninit::<libc::statvfs>::uninit();
        let result = unsafe { libc::statvfs(path.as_ptr(), stat.as_mut_ptr()) };
        if result == 0 {
            let stat = unsafe { stat.assume_init() };
            let free_bytes = stat.f_bavail as u64 * stat.f_frsize as u64;
            free_bytes as f64 / (1024.0 * 1024.0 * 1024.0)
        } else {
            50.0 // assume OK if we can't read
        }
    }
    #[cfg(not(unix))]
    {
        50.0 // default for non-unix
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Tests
// ═══════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operational_load_low_usage() {
        let meter = OperationalLoadMeter::new(2_000_000);
        let sig = meter.sample(1000, 100_000); // 5% of 2M budget
        assert!(sig.valence > 0.0, "low usage should be positive valence: {}", sig.valence);
        assert!(sig.arousal < 0.2, "low usage should have low arousal: {}", sig.arousal);
    }

    #[test]
    fn operational_load_high_usage() {
        let meter = OperationalLoadMeter::new(2_000_000);
        let sig = meter.sample(1_800_000, 1_800_000); // 90% of budget
        assert!(sig.valence < -0.3, "high usage should be negative valence: {}", sig.valence);
        assert!(sig.arousal > 0.5, "high usage should have high arousal: {}", sig.arousal);
    }

    #[test]
    fn execution_stress_no_stress() {
        let meter = ExecutionStressMeter::new();
        let sig = meter.sample();
        assert!(sig.valence > 0.0, "no stress should be positive: {}", sig.valence);
        assert!(sig.arousal < 0.1, "no stress should have low arousal: {}", sig.arousal);
    }

    #[test]
    fn execution_stress_under_pressure() {
        let meter = ExecutionStressMeter::new();
        meter.enter_loop();
        meter.enter_loop();
        meter.enter_loop();
        meter.record_retry();
        meter.record_retry();
        meter.record_tool_outcome(false);
        meter.record_tool_outcome(false);
        meter.record_tool_outcome(true);

        let sig = meter.sample();
        assert!(sig.valence < 0.0, "stressed should be negative: {}", sig.valence);
        assert!(sig.arousal > 0.2, "stressed should have elevated arousal: {}", sig.arousal);
    }

    #[test]
    fn execution_stress_consecutive_failures() {
        let meter = ExecutionStressMeter::new();
        meter.record_tool_outcome(false);
        meter.record_tool_outcome(false);
        meter.record_tool_outcome(false);

        let sig = meter.sample();
        assert_eq!(meter.consecutive_failures.load(Ordering::Relaxed), 3);
        assert!(sig.valence < 0.0, "3 consecutive failures should be negative: {}", sig.valence);
    }

    #[test]
    fn execution_stress_reset_on_success() {
        let meter = ExecutionStressMeter::new();
        meter.record_tool_outcome(false);
        meter.record_tool_outcome(false);
        meter.record_tool_outcome(true); // resets consecutive counter
        assert_eq!(meter.consecutive_failures.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn cognitive_flow_no_data() {
        let meter = CognitiveFlowMeter::new();
        let sig = meter.sample();
        // With no data, should be neutral
        assert!((sig.valence - 0.0).abs() < 0.1, "no-data flow should be ~neutral: {}", sig.valence);
    }

    #[test]
    fn cognitive_flow_successful() {
        let meter = CognitiveFlowMeter::new();
        for _ in 0..5 {
            meter.record_task(true);
        }
        let sig = meter.sample();
        assert!(sig.valence > 0.5, "successful flow should be very positive: {}", sig.valence);
    }

    #[test]
    fn cognitive_flow_failing() {
        let meter = CognitiveFlowMeter::new();
        for _ in 0..5 {
            meter.record_task(false);
        }
        let sig = meter.sample();
        assert!(sig.valence < -0.5, "failing flow should be very negative: {}", sig.valence);
    }

    #[test]
    fn resource_pressure_healthy() {
        let meter = ResourcePressureMeter::new();
        // Can't easily test disk (reads real FS), but queue at 0 should contribute positively
        let sig = meter.sample();
        // With no queue pressure and presumably decent disk, should be ok
        assert!(sig.arousal < 0.7, "healthy resources should not be too alarming: {}", sig.arousal);
    }

    #[test]
    fn resource_pressure_queue_depth() {
        let meter = ResourcePressureMeter::new();
        meter.set_queue_depth(8);
        let sig = meter.sample();
        // High queue should add some stress
        assert!(sig.valence < 0.3, "high queue should reduce valence");
    }

    #[test]
    fn somatic_summary_calm_state() {
        let load = InteroceptiveSignal::new(SignalSource::OperationalLoad, None, 0.4, 0.1);
        let stress = InteroceptiveSignal::new(SignalSource::ExecutionStress, None, 0.5, 0.0);
        let flow = InteroceptiveSignal::new(SignalSource::CognitiveFlow, None, 0.6, 0.1);
        let resource = InteroceptiveSignal::new(SignalSource::ResourcePressure, None, 0.2, 0.1);

        let summary = SomaticSummary::from_signals(&load, &stress, &flow, &resource);
        assert!(summary.valence > 0.3, "calm state should be positive: {}", summary.valence);
        assert!(summary.arousal < 0.3, "calm state should have low arousal: {}", summary.arousal);
        assert!(summary.dominance > 0.5, "calm state should feel in control: {}", summary.dominance);
    }

    #[test]
    fn somatic_summary_stressed_state() {
        let load = InteroceptiveSignal::new(SignalSource::OperationalLoad, None, -0.5, 0.8);
        let stress = InteroceptiveSignal::new(SignalSource::ExecutionStress, None, -0.8, 0.9);
        let flow = InteroceptiveSignal::new(SignalSource::CognitiveFlow, None, -0.3, 0.4);
        let resource = InteroceptiveSignal::new(SignalSource::ResourcePressure, None, -0.6, 0.7);

        let summary = SomaticSummary::from_signals(&load, &stress, &flow, &resource);
        assert!(summary.valence < -0.3, "stressed state should be negative: {}", summary.valence);
        assert!(summary.arousal > 0.5, "stressed state should be alert: {}", summary.arousal);
        assert!(summary.dominance < 0.3, "stressed state should feel low control: {}", summary.dominance);
    }

    #[test]
    fn ring_buffer_basic() {
        let mut buf: RingBuffer<i32> = RingBuffer::new(3);
        buf.push(1);
        buf.push(2);
        assert_eq!(buf.len(), 2);

        buf.push(3);
        buf.push(4); // overwrites 1
        assert_eq!(buf.len(), 3);

        let items: Vec<_> = buf.iter().cloned().collect();
        assert_eq!(items, vec![4, 2, 3]); // pos=1 was overwritten with 4
    }

    #[test]
    fn signal_emitter_full_sample() {
        let emitter = SignalEmitter::new(2_000_000);
        let (signals, summary) = emitter.sample_all(1000, 50_000);

        assert_eq!(signals.len(), 5); // load, stress, flow, resource, output
        assert!(summary.valence > -1.0 && summary.valence <= 1.0);
        assert!(summary.arousal >= 0.0 && summary.arousal <= 1.0);
        assert!(summary.dominance >= 0.0 && summary.dominance <= 1.0);
        assert!(summary.urgency >= 0.0 && summary.urgency <= 1.0);
    }

    // ═══════════════════════════════════════════════════════════════
    //  Output Productivity Meter Tests
    // ═══════════════════════════════════════════════════════════════

    #[test]
    fn output_productivity_no_data() {
        let meter = OutputProductivityMeter::new();
        let sig = meter.sample();
        // With <4 calls, should be neutral
        assert!(sig.valence.abs() < 0.01, "no data should be neutral: {}", sig.valence);
        assert!(meter.get_warnings().is_none(), "no warnings with no data");
    }

    #[test]
    fn output_productivity_healthy_mix() {
        let meter = OutputProductivityMeter::new();
        // Mix of reads and writes — healthy
        meter.record_tool_call("read_file", Some("/a.rs"));
        meter.record_tool_call("read_file", Some("/b.rs"));
        meter.record_tool_call("write_file", Some("/c.rs"));
        meter.record_tool_call("read_file", Some("/d.rs"));
        meter.record_tool_call("edit_file", Some("/e.rs"));
        let sig = meter.sample();
        assert!(sig.valence > 0.0, "healthy mix should be positive: {}", sig.valence);
        assert!(meter.get_warnings().is_none(), "no warnings for healthy mix");
    }

    #[test]
    fn output_productivity_read_heavy_triggers() {
        let meter = OutputProductivityMeter::new();
        // 8 reads, 0 writes → should trigger ReadWriteRatio
        for i in 0..8 {
            meter.record_tool_call("read_file", Some(&format!("/file{}.rs", i)));
        }
        let sig = meter.sample();
        assert!(sig.valence < 0.0, "read-heavy should be negative: {}", sig.valence);
        let warnings = meter.get_warnings();
        assert!(warnings.is_some(), "should produce warning");
        assert!(warnings.unwrap().contains("ReadWriteRatio"), "should mention ReadWriteRatio");
    }

    #[test]
    fn output_productivity_repeat_read_triggers() {
        let meter = OutputProductivityMeter::new();
        // Same file 4 times → should trigger RepeatRead
        meter.record_tool_call("read_file", Some("/same.rs"));
        meter.record_tool_call("read_file", Some("/other.rs"));
        meter.record_tool_call("read_file", Some("/same.rs"));
        meter.record_tool_call("read_file", Some("/same.rs"));
        meter.record_tool_call("read_file", Some("/same.rs"));
        let sig = meter.sample();
        assert!(sig.valence < 0.0, "repeat-read should be negative: {}", sig.valence);
        let warnings = meter.get_warnings();
        assert!(warnings.is_some(), "should produce warning");
        assert!(warnings.unwrap().contains("RepeatRead"), "should mention RepeatRead");
    }

    #[test]
    fn output_productivity_write_clears_warning() {
        let meter = OutputProductivityMeter::new();
        // Start with reads
        for i in 0..6 {
            meter.record_tool_call("read_file", Some(&format!("/file{}.rs", i)));
        }
        meter.sample();
        assert!(meter.get_warnings().is_some(), "should warn after 6 reads");
        
        // Now write — adds to window, dilutes reads
        meter.record_tool_call("write_file", Some("/output.rs"));
        meter.record_tool_call("write_file", Some("/output2.rs"));
        meter.record_tool_call("write_file", Some("/output3.rs"));
        meter.sample();
        // After writes, read_count in window still >= 6 but write_count > 0
        assert!(meter.get_warnings().is_none(), "should clear after writes");
    }

    #[test]
    fn output_productivity_reset() {
        let meter = OutputProductivityMeter::new();
        for i in 0..8 {
            meter.record_tool_call("read_file", Some(&format!("/file{}.rs", i)));
        }
        meter.sample();
        assert!(meter.get_warnings().is_some());
        
        meter.reset();
        meter.sample();
        assert!(meter.get_warnings().is_none(), "should be clean after reset");
    }

    #[test]
    fn output_productivity_spawn_blind() {
        let meter = OutputProductivityMeter::new();
        meter.record_tool_call("spawn_specialist", None);
        meter.record_tool_call("spawn_specialist", None);
        // Only spawns, no reads or writes → spawn_no_output fires
        // Need 4+ calls for the meter to activate
        meter.record_tool_call("exec", None);
        meter.record_tool_call("exec", None);
        let sig = meter.sample();
        assert!(sig.valence < 0.0, "spawn-blind should be negative: {}", sig.valence);
    }

    #[test]
    fn disk_free_gb_returns_reasonable_value() {
        let gb = get_disk_free_gb();
        assert!(gb > 0.0, "disk free should be positive: {}", gb);
        assert!(gb < 10000.0, "disk free should be reasonable: {}", gb);
    }
}
