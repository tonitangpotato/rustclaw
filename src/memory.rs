//! Memory management with native Engram integration.
//!
//! Unlike OpenClaw (MCP overhead) or Hermes (FTS only),
//! RustClaw uses engramai as a direct Rust dependency — zero IPC overhead.
//!
//! ## EmpathyBus Integration
//!
//! Empathy feedback loop (observing *user's* emotional state):
//! - EmpathyBus for drive alignment and importance boosting
//! - EmpathyAccumulator for tracking user empathy valence per domain
//! - BehaviorFeedback for tracking tool success/failure rates
//! - Auto-suggestions for SOUL.md and HEARTBEAT.md updates

use engramai::{
    Memory, MemoryConfig, MemoryType, MemoryLayer, AnthropicExtractor, AnthropicExtractorConfig, TokenProvider,
    SynthesisSettings, SynthesisLlmProvider,
    SessionRegistry, BaselineTracker,
    EmpathyBus, EmpathyTrend, ActionStats, SoulUpdate, HeartbeatUpdate,
    bus::{mod_io::{parse_soul, Drive}, accumulator::EmpathyAccumulator, feedback::BehaviorFeedback},
    interoceptive::{InteroceptiveState, RegulationAction, regulation::{self, RegulationConfig}},
};
use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::config::Config;
use crate::oauth::OAuthTokenManager;

/// Token provider that delegates to RustClaw's OAuthTokenManager.
/// Automatically refreshes expired tokens on each call.
struct ManagedTokenProvider {
    manager: Arc<OAuthTokenManager>,
    runtime: tokio::runtime::Handle,
}

impl TokenProvider for ManagedTokenProvider {
    fn get_token(&self) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        // OAuthTokenManager.get_token() is async — bridge to sync.
        // Use block_in_place to allow blocking inside a tokio worker thread.
        tokio::task::block_in_place(|| {
            self.runtime.block_on(self.manager.get_token())
        })
        .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { e.to_string().into() })
    }
}

/// LLM provider for synthesis insight generation.
/// Reuses the same OAuth token manager as the memory extractor (Claude Max plan).
struct ManagedSynthesisProvider {
    manager: Arc<OAuthTokenManager>,
    runtime: tokio::runtime::Handle,
}

impl SynthesisLlmProvider for ManagedSynthesisProvider {
    fn generate(
        &self,
        prompt: &str,
        config: &engramai::synthesis::types::SynthesisConfig,
    ) -> Result<String, Box<dyn std::error::Error>> {
        let token = tokio::task::block_in_place(|| {
            self.runtime.block_on(self.manager.get_token())
        })
        .map_err(|e| -> Box<dyn std::error::Error> { e.to_string().into() })?;

        let model = if config.model.is_empty() {
            "claude-sonnet-4-20250514".to_string()
        } else {
            config.model.clone()
        };

        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()?;

        let body = serde_json::json!({
            "model": model,
            "max_tokens": config.max_tokens,
            "temperature": config.temperature,
            "messages": [{
                "role": "user",
                "content": prompt
            }]
        });

        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("anthropic-version", "2023-06-01".parse().unwrap());
        headers.insert("content-type", "application/json".parse().unwrap());
        headers.insert(
            "anthropic-beta",
            "claude-code-20250219,oauth-2025-04-20".parse().unwrap(),
        );
        headers.insert(
            reqwest::header::AUTHORIZATION,
            format!("Bearer {}", token).parse().unwrap(),
        );
        headers.insert(
            reqwest::header::USER_AGENT,
            "claude-cli/2.1.39 (external, cli)".parse().unwrap(),
        );
        headers.insert("x-app", "cli".parse().unwrap());
        headers.insert(
            "anthropic-dangerous-direct-browser-access",
            "true".parse().unwrap(),
        );

        let response = client
            .post("https://api.anthropic.com/v1/messages")
            .headers(headers)
            .json(&body)
            .send()?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().unwrap_or_default();
            return Err(format!("Anthropic API error {}: {}", status, body).into());
        }

        let resp: serde_json::Value = response.json()?;
        let text = resp["content"][0]["text"]
            .as_str()
            .ok_or("No text in Anthropic response")?
            .to_string();

        Ok(text)
    }
}

/// Working memory decay in seconds (30 minutes for longer topic continuity).
const WORKING_MEMORY_DECAY_SECS: u64 = 1800;

/// Memory manager wrapping Engram with RustClaw-specific logic.
/// Uses Mutex instead of async RwLock because rusqlite isn't Send+Sync.
pub struct MemoryManager {
    engram: Mutex<Memory>,
    /// Per-session working memory registry for topic continuity (Miller's Law: 7±2 items)
    wm_registry: Mutex<SessionRegistry>,
    /// Anomaly detection for storage patterns
    anomaly_tracker: Mutex<BaselineTracker>,
    /// Drives from SOUL.md for importance boosting
    drives: Vec<Drive>,
    /// EmpathyBus for full empathy feedback loop (optional, requires workspace_dir)
    empathy_bus: Option<EmpathyBus>,
    /// Workspace directory for EmpathyBus operations
    workspace_dir: String,
    /// Database path for creating EmpathyBus connection
    db_path: String,
    auto_recall: bool,
    auto_store: bool,
    recall_limit: usize,
    /// Optional namespace prefix for multi-agent isolation.
    namespace: Option<String>,
}

impl MemoryManager {
    /// Initialize the memory system.
    pub async fn new(config: &Config, workspace_dir: &str) -> anyhow::Result<Self> {
        let db_path = config
            .memory
            .engram_db
            .clone()
            .unwrap_or_else(|| format!("{}/engram-memory.db", workspace_dir));

        let engram_config = MemoryConfig::default();
        let mut engram = Memory::new(&db_path, Some(engram_config))
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        // Set up LLM extraction using managed OAuth (Claude Max plan).
        // TokenProvider refreshes automatically — no more expired token errors.
        if let Ok(oauth_mgr) = OAuthTokenManager::from_keychain() {
            let oauth_arc = Arc::new(oauth_mgr);
            let runtime = tokio::runtime::Handle::current();

            // Set up LLM extraction (Haiku for fact extraction)
            let extractor_provider = Box::new(ManagedTokenProvider {
                manager: oauth_arc.clone(),
                runtime: runtime.clone(),
            });
            let extractor = AnthropicExtractor::with_token_provider(
                extractor_provider,
                true, // is_oauth
                AnthropicExtractorConfig::default(),
            );
            engram.set_extractor(Box::new(extractor));
            tracing::info!("Engram extractor: Anthropic Haiku (managed OAuth, auto-refresh)");

            // Set up synthesis engine (Sonnet for insight generation)
            let synthesis_provider = Box::new(ManagedSynthesisProvider {
                manager: oauth_arc,
                runtime,
            });
            let mut synthesis_settings = SynthesisSettings::default();
            synthesis_settings.enabled = true;
            synthesis_settings.max_llm_calls_per_run = 3; // conservative budget per cycle
            engram.set_synthesis_settings(synthesis_settings);
            engram.set_synthesis_llm_provider(synthesis_provider);
            tracing::info!("Engram synthesis: enabled (Sonnet, OAuth, max 3 insights/cycle)");
        } else {
            // Fallback: auto_configure_extractor checks env vars and config file
            tracing::debug!("No Keychain OAuth, relying on engram auto-config");
        }

        // Initialize EmpathyBus for drive alignment and empathy tracking
        let empathy_bus = match EmpathyBus::new(workspace_dir, engram.connection()) {
            Ok(mut bus) => {
                tracing::info!("EmpathyBus initialized with {} drives", bus.drives().len());
                // Initialize embedding-based alignment if embedding provider is available
                if let Some(ref emb) = engram.embedding_provider() {
                    bus.init_embeddings(emb);
                    if bus.has_embeddings() {
                        tracing::info!("Drive embeddings enabled (multilingual alignment active)");
                    }
                }
                Some(bus)
            }
            Err(e) => {
                tracing::warn!("Failed to initialize EmpathyBus: {}", e);
                None
            }
        };

        // Load drives - prefer EmpathyBus drives, fall back to config/SOUL.md
        let drives = if let Some(ref bus) = empathy_bus {
            if !bus.drives().is_empty() {
                bus.drives().to_vec()
            } else {
                Self::load_drives_fallback(config, workspace_dir)
            }
        } else {
            Self::load_drives_fallback(config, workspace_dir)
        };

        if !drives.is_empty() {
            tracing::info!("Loaded {} drives for importance boosting", drives.len());
        }

        // Initialize session working memory registry (15 items per session, 30 minute decay)
        let wm_registry = SessionRegistry::with_defaults(15, WORKING_MEMORY_DECAY_SECS);
        
        // Initialize anomaly tracker (100 sample window)
        let anomaly_tracker = BaselineTracker::new(100);

        Ok(Self {
            engram: Mutex::new(engram),
            wm_registry: Mutex::new(wm_registry),
            anomaly_tracker: Mutex::new(anomaly_tracker),
            drives,
            empathy_bus,
            workspace_dir: workspace_dir.to_string(),
            db_path,
            auto_recall: config.memory.auto_recall,
            auto_store: config.memory.auto_store,
            recall_limit: config.memory.recall_limit,
            namespace: None,
        })
    }

    /// Load drives from config or SOUL.md (fallback when EmpathyBus not available).
    fn load_drives_fallback(config: &Config, workspace_dir: &str) -> Vec<Drive> {
        if !config.memory.drives.is_empty() {
            // Use drives from config (converted to engramai Drive type)
            config.memory.drives.iter().map(|d| Drive {
                name: d.name.clone(),
                description: format!("Config drive (weight: {})", d.weight),
                keywords: d.keywords.clone(),
            }).collect()
        } else {
            // Fall back to SOUL.md
            let soul_path = format!("{}/SOUL.md", workspace_dir);
            if Path::new(&soul_path).exists() {
                match std::fs::read_to_string(&soul_path) {
                    Ok(content) => parse_soul(&content),
                    Err(e) => {
                        tracing::debug!("Failed to read SOUL.md: {}", e);
                        Vec::new()
                    }
                }
            } else {
                Vec::new()
            }
        }
    }

    /// Set a namespace prefix for memory isolation.
    ///
    /// When a namespace is set, all memory operations are prefixed with the namespace,
    /// allowing multiple agents to share the same Engram database without collision.
    pub fn with_namespace(mut self, namespace: &str) -> Self {
        self.namespace = Some(namespace.to_string());
        self
    }

    /// Get the current namespace.
    /// Get the database path (needed for Knowledge Compiler store access).
    pub fn db_path(&self) -> &str {
        &self.db_path
    }

    /// Lock the engram Memory for direct access (used by Knowledge Compiler).
    pub fn lock_engram(&self) -> anyhow::Result<std::sync::MutexGuard<'_, engramai::Memory>> {
        self.engram.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))
    }

    pub fn namespace(&self) -> Option<&str> {
        self.namespace.as_deref()
    }

    /// Apply namespace prefix to content if namespace is set.
    #[allow(dead_code)]
    fn namespaced_content(&self, content: &str) -> String {
        match &self.namespace {
            Some(ns) => format!("[{}] {}", ns, content),
            None => content.to_string(),
        }
    }

    /// Apply namespace prefix to query if namespace is set.
    #[allow(dead_code)]
    fn namespaced_query(&self, query: &str) -> String {
        match &self.namespace {
            Some(ns) => format!("[{}] {}", ns, query),
            None => query.to_string(),
        }
    }

    // ─── Importance & Layer Calculation ─────────────────────────────────

    /// Calculate importance boost using EmpathyBus (or direct calculation fallback).
    fn calculate_importance(&self, content: &str, base_importance: f64) -> f64 {
        let boost = if let Some(ref bus) = self.empathy_bus {
            bus.align_importance(content)
        } else if !self.drives.is_empty() {
            engramai::bus::alignment::calculate_importance_boost(content, &self.drives)
        } else {
            1.0
        };
        
        let final_importance = (base_importance * boost).min(1.0);
        if boost > 1.0 {
            tracing::debug!(
                "Drive alignment boost: {:.2}x (importance: {:.2} → {:.2})",
                boost, base_importance, final_importance
            );
        }
        final_importance
    }

    /// Determine memory layer based on importance.
    /// 
    /// Layers in Engram:
    /// - Core: always loaded, distilled knowledge (importance >= 0.8)
    /// - Working: recent daily notes (importance >= 0.5)
    /// - Archive: old, searched on demand (importance < 0.5)
    /// 
    /// Note: Currently unused - memories start in Working layer and get
    /// promoted during consolidation. Kept for reference and potential future use.
    #[allow(dead_code)]
    fn importance_to_layer(importance: f64) -> MemoryLayer {
        if importance >= 0.8 {
            MemoryLayer::Core
        } else if importance >= 0.5 {
            MemoryLayer::Working
        } else {
            MemoryLayer::Archive
        }
    }

    // ─── Core Memory Operations ─────────────────────────────────────────

    /// Recall relevant memories for a user message.
    /// Called by BeforeInbound hook.
    pub fn recall(&self, query: &str) -> anyhow::Result<Vec<RecalledMemory>> {
        if !self.auto_recall {
            return Ok(Vec::new());
        }

        let mut engram = self.engram.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        let results = engram
            .recall(query, self.recall_limit, None, None)
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        Ok(results
            .into_iter()
            .map(|r| RecalledMemory {
                content: r.record.content.clone(),
                memory_type: format!("{:?}", r.record.memory_type),
                confidence: r.confidence,
                source: Some(r.record.source.clone()),
                confidence_label: Some(r.confidence_label),
                created_at: Some(r.record.created_at.to_rfc3339()),
            })
            .collect())
    }

    /// Store important information from a conversation turn.
    /// Called by BeforeOutbound hook.
    ///
    /// Applies drive alignment boost, determines memory layer, and tracks anomalies.
    pub fn store(
        &self,
        content: &str,
        memory_type: MemoryType,
        importance: f64,
        source: Option<&str>,
    ) -> anyhow::Result<()> {
        if !self.auto_store {
            return Ok(());
        }

        // Calculate importance boost based on drive alignment
        let boosted_importance = self.calculate_importance(content, importance);
        
        // Note: Layer is determined by consolidation process, not at store time.
        // Higher importance memories will be promoted to Core/Extended during consolidation.

        let mut engram = self.engram.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        engram
            .add(content, memory_type, Some(boosted_importance), source, None)
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        // Track storage pattern for anomaly detection
        if let Ok(mut tracker) = self.anomaly_tracker.lock() {
            tracker.update("store_importance", boosted_importance);
            tracker.update("content_length", content.len() as f64);
            
            // Check for anomalous storage patterns (min 10 samples, 2.5 sigma threshold)
            if tracker.is_anomaly("store_importance", boosted_importance, 2.5, 10) {
                tracing::warn!(
                    "Anomalous storage pattern detected: importance={:.2} (z-score > 2.5)",
                    boosted_importance
                );
            }
        }

        Ok(())
    }
    
    /// Session-aware recall using working memory for topic continuity.
    ///
    /// If the topic is continuous with recent recalls, returns cached working
    /// memory items. If topic changed, does full recall.
    ///
    /// Each session_key gets its own working memory, preventing cross-session
    /// memory pollution.
    ///
    /// Returns (memories, full_recall_triggered).
    pub fn session_recall(&self, query: &str, session_key: &str) -> anyhow::Result<(Vec<RecalledMemory>, bool)> {
        if !self.auto_recall {
            return Ok((Vec::new(), false));
        }

        let mut engram = self.engram.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        let mut registry = self.wm_registry.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        let wm = registry.get_session(session_key);

        let result = engram
            .session_recall(query, wm, self.recall_limit, None, None)
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        let memories = result
            .results
            .into_iter()
            .map(|r| RecalledMemory {
                content: r.record.content.clone(),
                memory_type: format!("{:?}", r.record.memory_type),
                confidence: r.confidence,
                source: Some(r.record.source.clone()),
                confidence_label: Some(r.confidence_label),
                created_at: Some(r.record.created_at.to_rfc3339()),
            })
            .collect();

        Ok((memories, result.full_recall))
    }
    
    /// Recall associated memories (causal type) using Hebbian links.
    ///
    /// Finds memories that frequently co-occur with the given query.
    pub fn recall_associated(
        &self,
        query: Option<&str>,
        limit: usize,
        min_confidence: f64,
    ) -> anyhow::Result<Vec<RecalledMemory>> {
        let mut engram = self.engram.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        let results = engram
            .recall_associated(query, limit, min_confidence)
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        Ok(results
            .into_iter()
            .map(|r| RecalledMemory {
                content: r.record.content.clone(),
                memory_type: format!("{:?}", r.record.memory_type),
                confidence: r.confidence,
                source: Some(r.record.source.clone()),
                confidence_label: Some(r.confidence_label),
                created_at: Some(r.record.created_at.to_rfc3339()),
            })
            .collect())
    }

    /// Remove empty sessions from the working memory registry.
    /// Called during heartbeat to prevent unbounded growth.
    pub fn prune_sessions(&self) -> anyhow::Result<usize> {
        let mut registry = self.wm_registry.lock()
            .map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        registry.prune_all();
        Ok(registry.remove_empty_sessions())
    }

    /// Run memory consolidation + synthesis (during heartbeats/auto-schedule).
    /// Uses sleep_cycle() which runs consolidation first, then synthesis if enabled.
    pub fn consolidate(&self) -> anyhow::Result<()> {
        let mut engram = self.engram.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        let report = engram
            .sleep_cycle(7.0, None)
            .map_err(|e| anyhow::anyhow!("{}", e))?;
        
        if let Some(ref synth) = report.synthesis {
            tracing::info!(
                "Synthesis: {} clusters found, {} synthesized, {} deferred, {} skipped, {} errors",
                synth.clusters_found,
                synth.clusters_synthesized,
                synth.clusters_deferred,
                synth.clusters_skipped,
                synth.errors.len(),
            );
        }
        Ok(())
    }

    /// Run rumination: synthesis only, no consolidation.
    /// Discovers clusters and generates insights without decaying memory strength.
    pub fn synthesize(&self) -> anyhow::Result<engramai::synthesis::types::SynthesisReport> {
        let mut engram = self.engram.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        let report = engram.synthesize().map_err(|e| anyhow::anyhow!("{}", e))?;
        Ok(report)
    }

    /// Get memory stats.
    pub fn stats(&self) -> anyhow::Result<serde_json::Value> {
        let engram = self.engram.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        let stats = engram
            .stats()
            .map_err(|e| anyhow::anyhow!("{}", e))?;
        Ok(serde_json::to_value(stats)?)
    }

    /// Check embedding service status (Ollama).
    /// Returns a human-readable status string.
    pub fn embedding_status(&self) -> String {
        // Try to reach Ollama at default address
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(2))
            .build();

        match client {
            Ok(client) => {
                match client.get("http://localhost:11434/api/tags").send() {
                    Ok(resp) if resp.status().is_success() => {
                        "Ollama: connected ✓".to_string()
                    }
                    Ok(resp) => {
                        format!("Ollama: error (HTTP {})", resp.status())
                    }
                    Err(_) => {
                        "Ollama: not reachable".to_string()
                    }
                }
            }
            Err(_) => {
                "Ollama: client error".to_string()
            }
        }
    }
    
    /// Explicitly recall memories (for LLM tool use).
    /// Unlike recall(), this ignores auto_recall setting.
    pub fn recall_explicit(&self, query: &str, limit: usize) -> anyhow::Result<Vec<RecalledMemory>> {
        let mut engram = self.engram.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        let results = engram
            .recall(query, limit, None, None)
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        Ok(results
            .into_iter()
            .map(|r| RecalledMemory {
                content: r.record.content.clone(),
                memory_type: format!("{:?}", r.record.memory_type),
                confidence: r.confidence,
                source: Some(r.record.source.clone()),
                confidence_label: Some(r.confidence_label),
                created_at: Some(r.record.created_at.to_rfc3339()),
            })
            .collect())
    }
    
    /// Explicitly store a memory (for LLM tool use).
    /// Unlike store(), this ignores auto_store setting.
    pub fn store_explicit(
        &self,
        content: &str,
        memory_type: MemoryType,
        importance: f64,
    ) -> anyhow::Result<()> {
        // Calculate boosted importance
        let boosted_importance = self.calculate_importance(content, importance);
        
        let mut engram = self.engram.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        engram
            .add(content, memory_type, Some(boosted_importance), Some("agent_tool"), None)
            .map_err(|e| anyhow::anyhow!("{}", e))?;
        Ok(())
    }

    /// Recall recent memories by creation time (no query/embedding needed).
    /// Used for session startup: injects the most recent N memories as context
    /// so the agent doesn't start from zero after a restart.
    ///
    /// Applies light filtering to reduce operational noise: over-fetches from the
    /// DB (≈2.5×) then drops short/boilerplate/duplicate memories before truncating
    /// back to `limit`. See ISS-014.
    pub fn recall_recent(&self, limit: usize) -> anyhow::Result<Vec<RecalledMemory>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        // Over-fetch so filtering can discard noise without starving the result.
        let fetch_limit = (limit.saturating_mul(5) / 2).max(limit + 5);
        let engram = self.engram.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        let records = engram
            .recall_recent(fetch_limit, None)
            .map_err(|e| anyhow::anyhow!("{}", e))?;
        drop(engram);

        let mut out: Vec<RecalledMemory> = Vec::with_capacity(limit);
        let mut seen_prefixes: std::collections::HashSet<String> = std::collections::HashSet::new();
        for r in records.into_iter() {
            if out.len() >= limit {
                break;
            }
            if !is_continuity_worthy(&r.content) {
                continue;
            }
            // Deduplicate by content prefix (first 60 chars normalized) so repeated
            // near-identical memories don't crowd out distinct signals.
            let key: String = r
                .content
                .chars()
                .take(60)
                .flat_map(|c| c.to_lowercase())
                .collect();
            if !seen_prefixes.insert(key) {
                continue;
            }
            out.push(RecalledMemory {
                content: r.content.clone(),
                memory_type: format!("{:?}", r.memory_type),
                confidence: r.importance,
                source: Some(r.source.clone()),
                confidence_label: Some("recent".to_string()),
                created_at: Some(r.created_at.to_rfc3339()),
            });
        }
        Ok(out)
    }

    /// Format recent memories for session startup injection.
    /// Groups by time proximity and shows timestamps for context.
    pub fn format_recent_for_prompt(memories: &[RecalledMemory]) -> String {
        if memories.is_empty() {
            return String::new();
        }

        let mut lines = Vec::with_capacity(memories.len() + 5);
        lines.push(String::new());
        lines.push("## 🧠 Recent Memories (session startup — most recent first)".to_string());
        lines.push("These are your most recent memories, loaded automatically to maintain continuity across restarts.".to_string());

        for mem in memories {
            let type_tag = &mem.memory_type;
            let timestamp = mem.created_at.as_deref().map(|ts| {
                // Try to parse and format as a human-friendly relative/short timestamp
                chrono::DateTime::parse_from_rfc3339(ts)
                    .map(|dt| dt.format("%m-%d %H:%M").to_string())
                    .unwrap_or_else(|_| ts.to_string())
            });
            match timestamp {
                Some(ts) => lines.push(format!("- [{}] [{}] {}", ts, type_tag, mem.content)),
                None => lines.push(format!("- [{}] {}", type_tag, mem.content)),
            }
        }

        // ISS-014 Fix 4: continuity instruction. When a fresh session starts and
        // these memories are injected, proactively signal to the user what we
        // were last doing instead of waiting to be asked.
        lines.push(String::new());
        lines.push(
            "**Continuity note**: This is the first message of a fresh session. \
If the user's message is a greeting or an open prompt (e.g. \"hi\", \"继续\", \"你决定吧\"), \
briefly summarize what you were last working on based on the memories above and \
ask whether to continue. If the user already has a clear task, just do it — \
don't force a continuity recap."
                .to_string(),
        );

        lines.join("\n")
    }

    // ─── EmpathyAccumulator (process_interaction) ─────────────────────

    /// Take emotion data from the most recent LLM extraction.
    ///
    /// One-shot: clears the cache after reading.
    /// Returns None if no extraction occurred since last call.
    pub fn take_last_emotions(&self) -> Option<Vec<(f64, String)>> {
        let engram = self.engram.lock().ok()?;
        engram.take_last_emotions()
    }

    /// Process an interaction with empathic content.
    ///
    /// Tracks empathy valence per domain for trend analysis.
    /// Call this after storing memories to build empathy patterns.
    ///
    /// # Arguments
    ///
    /// * `content` - The interaction content (used for context, not stored separately)
    /// * `emotion` - Empathy valence (-1.0 to 1.0)
    /// * `domain` - Domain/topic (e.g., "coding", "research", "trading")
    pub fn process_interaction(&self, _content: &str, emotion: f64, domain: &str) -> anyhow::Result<()> {
        let engram = self.engram.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        let acc = EmpathyAccumulator::new(engram.connection())
            .map_err(|e| anyhow::anyhow!("EmpathyAccumulator error: {}", e))?;
        acc.record_emotion(domain, emotion)
            .map_err(|e| anyhow::anyhow!("Record emotion error: {}", e))?;
        tracing::debug!("Recorded emotion {:.2} for domain '{}'", emotion, domain);
        Ok(())
    }

    // ─── BehaviorFeedback (log_behavior) ────────────────────────────────

    /// Log a behavior/tool outcome.
    ///
    /// Tracks which actions succeed or fail over time for adaptive behavior.
    ///
    /// # Arguments
    ///
    /// * `action` - Action name (e.g., tool name, "check_email", etc.)
    /// * `positive` - Whether the outcome was successful
    pub fn log_behavior(&self, action: &str, positive: bool) -> anyhow::Result<()> {
        let engram = self.engram.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        let feedback = BehaviorFeedback::new(engram.connection())
            .map_err(|e| anyhow::anyhow!("BehaviorFeedback error: {}", e))?;
        feedback.log_outcome(action, positive)
            .map_err(|e| anyhow::anyhow!("Log behavior error: {}", e))?;
        tracing::debug!("Logged behavior: {} = {}", action, if positive { "success" } else { "failure" });
        Ok(())
    }

    /// Get behavior statistics for all tracked actions.
    pub fn get_behavior_stats(&self) -> anyhow::Result<Vec<ActionStats>> {
        let engram = self.engram.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        let feedback = BehaviorFeedback::new(engram.connection())
            .map_err(|e| anyhow::anyhow!("BehaviorFeedback error: {}", e))?;
        feedback.get_all_action_stats()
            .map_err(|e| anyhow::anyhow!("Get behavior stats error: {}", e))
    }

    /// Get actions that should be deprioritized (low success rate).
    pub fn get_deprioritized_actions(&self) -> anyhow::Result<Vec<ActionStats>> {
        let engram = self.engram.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        let feedback = BehaviorFeedback::new(engram.connection())
            .map_err(|e| anyhow::anyhow!("BehaviorFeedback error: {}", e))?;
        feedback.get_actions_to_deprioritize()
            .map_err(|e| anyhow::anyhow!("Get deprioritized actions error: {}", e))
    }

    // ─── SOUL & HEARTBEAT Suggestions ───────────────────────────────────

    /// Get suggested SOUL.md updates based on empathy trends.
    ///
    /// Analyzes accumulated empathy patterns and suggests drive adjustments.
    pub fn suggest_soul_updates(&self) -> anyhow::Result<Vec<SoulUpdate>> {
        if let Some(ref bus) = self.empathy_bus {
            let engram = self.engram.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
            bus.suggest_soul_updates(engram.connection())
                .map_err(|e| anyhow::anyhow!("Suggest soul updates error: {}", e))
        } else {
            Ok(Vec::new())
        }
    }

    /// Get suggested HEARTBEAT.md updates based on behavior feedback.
    ///
    /// Suggests which tasks to deprioritize or boost based on success rates.
    pub fn suggest_heartbeat_updates(&self) -> anyhow::Result<Vec<HeartbeatUpdate>> {
        if let Some(ref bus) = self.empathy_bus {
            let engram = self.engram.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
            bus.suggest_heartbeat_updates(engram.connection())
                .map_err(|e| anyhow::anyhow!("Suggest heartbeat updates error: {}", e))
        } else {
            Ok(Vec::new())
        }
    }

    /// Apply a SOUL update (modify a field in SOUL.md).
    pub fn apply_soul_update(&self, key: &str, value: &str) -> anyhow::Result<bool> {
        if let Some(ref bus) = self.empathy_bus {
            bus.update_soul(key, value)
                .map_err(|e| anyhow::anyhow!("Apply soul update error: {}", e))
        } else {
            Ok(false)
        }
    }

    /// Add a new drive to SOUL.md.
    pub fn add_soul_drive(&self, key: &str, value: &str) -> anyhow::Result<()> {
        if let Some(ref bus) = self.empathy_bus {
            bus.add_soul_drive(key, value)
                .map_err(|e| anyhow::anyhow!("Add soul drive error: {}", e))
        } else {
            Ok(())
        }
    }

    /// Apply a HEARTBEAT update (mark task completed/incomplete).
    pub fn apply_heartbeat_update(&self, task: &str, completed: bool) -> anyhow::Result<bool> {
        if let Some(ref bus) = self.empathy_bus {
            bus.update_heartbeat_task(task, completed)
                .map_err(|e| anyhow::anyhow!("Apply heartbeat update error: {}", e))
        } else {
            Ok(false)
        }
    }

    /// Get all empathy trends by domain.
    pub fn get_empathy_trends(&self) -> anyhow::Result<Vec<EmpathyTrend>> {
        let engram = self.engram.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        let acc = EmpathyAccumulator::new(engram.connection())
            .map_err(|e| anyhow::anyhow!("EmpathyAccumulator error: {}", e))?;
        acc.get_all_trends()
            .map_err(|e| anyhow::anyhow!("Get empathy trends error: {}", e))
    }

    // ─── Periodic Maintenance ───────────────────────────────────────────

    /// Decay empathy trends toward neutral (prevents stale data).
    ///
    /// Call periodically (e.g., every 24 hours) to prevent old empathy
    /// patterns from dominating.
    pub fn decay_trends(&self, factor: f64) -> anyhow::Result<usize> {
        let engram = self.engram.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        let acc = EmpathyAccumulator::new(engram.connection())
            .map_err(|e| anyhow::anyhow!("EmpathyAccumulator error: {}", e))?;
        acc.decay_trends(factor)
            .map_err(|e| anyhow::anyhow!("Decay trends error: {}", e))
    }

    /// Prune old behavior logs (keep only recent N per action).
    ///
    /// Call periodically to prevent unbounded log growth.
    pub fn prune_old_logs(&self, keep_per_action: usize) -> anyhow::Result<usize> {
        let engram = self.engram.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        let feedback = BehaviorFeedback::new(engram.connection())
            .map_err(|e| anyhow::anyhow!("BehaviorFeedback error: {}", e))?;
        feedback.prune_old_logs(keep_per_action)
            .map_err(|e| anyhow::anyhow!("Prune old logs error: {}", e))
    }

    /// Run full self-reflection cycle.
    ///
    /// Performs all periodic maintenance:
    /// - Decays empathy trends
    /// - Prunes old behavior logs
    /// - Logs any SOUL/HEARTBEAT suggestions
    ///
    /// Call during heartbeats or daily cron.
    pub fn self_reflect(&self) -> anyhow::Result<SelfReflectionResult> {
        let mut result = SelfReflectionResult::default();

        // Decay empathy trends (0.9 = 10% decay toward neutral)
        match self.decay_trends(0.9) {
            Ok(count) => {
                result.trends_decayed = count;
                if count > 0 {
                    tracing::info!("Decayed {} empathy trends", count);
                }
            }
            Err(e) => tracing::warn!("Failed to decay trends: {}", e),
        }

        // Prune old behavior logs (keep 100 per action)
        match self.prune_old_logs(100) {
            Ok(count) => {
                result.logs_pruned = count;
                if count > 0 {
                    tracing::info!("Pruned {} old behavior logs", count);
                }
            }
            Err(e) => tracing::warn!("Failed to prune logs: {}", e),
        }

        // Check for SOUL update suggestions
        match self.suggest_soul_updates() {
            Ok(suggestions) => {
                result.soul_suggestions = suggestions.len();
                for suggestion in &suggestions {
                    tracing::info!(
                        "SOUL suggestion [{} {}]: {} (domain: {}, valence: {:.2})",
                        suggestion.action,
                        suggestion.domain,
                        suggestion.content,
                        suggestion.trend.domain,
                        suggestion.trend.valence
                    );
                }
            }
            Err(e) => tracing::warn!("Failed to get soul suggestions: {}", e),
        }

        // Check for deprioritized actions
        match self.get_deprioritized_actions() {
            Ok(actions) => {
                result.deprioritized_actions = actions.len();
                for action in &actions {
                    tracing::warn!(
                        "Action '{}' should be deprioritized: {:.0}% success rate ({}/{} positive)",
                        action.action,
                        action.score * 100.0,
                        action.positive,
                        action.total
                    );
                }
            }
            Err(e) => tracing::warn!("Failed to get deprioritized actions: {}", e),
        }

        Ok(result)
    }

    // ─── Interoceptive Layer (L3) ───────────────────────────────────────

    /// Get a snapshot of the current interoceptive state.
    ///
    /// Returns the integrated feeling-state across all domains.
    /// Used by EngramRecallHook to inject into system prompts.
    pub fn interoceptive_snapshot(&self) -> anyhow::Result<InteroceptiveState> {
        let engram = self.engram.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        Ok(engram.interoceptive_snapshot())
    }

    /// Run an interoceptive tick: pull signals from all subsystems into the hub.
    ///
    /// Call during heartbeats or periodically to keep the interoceptive state current.
    /// This pulls from EmpathyAccumulator and BehaviorFeedback DB tables.
    pub fn interoceptive_tick(&self) -> anyhow::Result<()> {
        let mut engram = self.engram.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        engram.interoceptive_tick();
        Ok(())
    }

    /// Feed an external interoceptive signal into the hub.
    ///
    /// Used by RustClaw's SignalEmitter (Layer 1) to inject runtime signals
    /// into engram's InteroceptiveHub (Layer 2).
    pub fn feed_interoceptive_signal(&self, signal: engramai::interoceptive::InteroceptiveSignal) -> anyhow::Result<()> {
        let mut engram = self.engram.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        engram.feed_interoceptive_signal(signal);
        Ok(())
    }

    /// Evaluate the current interoceptive state and generate regulation actions.
    ///
    /// Uses adaptive baselines when calibrated, falls back to conservative
    /// hardcoded thresholds during cold-start.
    ///
    /// Returns advisory actions (soul updates, retrieval adjustments, behavior shifts, alerts).
    /// The caller decides how to act on them (log, send to Telegram, apply automatically).
    pub fn interoceptive_regulate(&self) -> anyhow::Result<Vec<RegulationAction>> {
        let engram = self.engram.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        let state = engram.interoceptive_snapshot();
        let hub = engram.interoceptive_hub();
        let config = RegulationConfig::default();
        Ok(regulation::evaluate_with_hub(&state, &config, Some(hub)))
    }

    /// Run a full interoceptive cycle: tick + evaluate.
    ///
    /// Convenience method for heartbeat use. Returns regulation actions.
    pub fn interoceptive_cycle(&self) -> anyhow::Result<Vec<RegulationAction>> {
        // Tick first: pull fresh signals into the hub
        self.interoceptive_tick()?;
        // Then evaluate: generate regulation actions from the updated state
        self.interoceptive_regulate()
    }

    // ─── Formatting ─────────────────────────────────────────────────────

    /// Format recalled memories for injection into system prompt.
    ///
    /// Includes confidence labels when available:
    /// - [confident] Direct statement, clear fact
    /// - [likely] Reasonable inference
    /// - [uncertain] Vague mention, speculation
    pub fn format_for_prompt(memories: &[RecalledMemory]) -> String {
        if memories.is_empty() {
            return String::new();
        }

        let mut lines = vec!["[Relevant memories from Engram]:".to_string()];
        for mem in memories {
            let label = mem.confidence_label.as_deref().unwrap_or("likely");
            lines.push(format!("- [{}] {}", label, mem.content));
        }
        lines.join("\n")
    }
}

/// Heuristic: decide whether a recalled memory is worth injecting as
/// continuity context on session startup. Filters out low-signal operational
/// traces (tool logs, filesystem noise, very short fragments) that would
/// crowd out genuine working-state memories.
///
/// Kept deliberately simple — better to keep one noisy memory than to drop
/// a real one. Threshold-based, no regex gymnastics.
fn is_continuity_worthy(content: &str) -> bool {
    let trimmed = content.trim();
    if trimmed.chars().count() < 25 {
        return false;
    }
    // Compare against lowercased prefix. Most of these are emitted by the
    // auto-store hook when the agent uses read/edit/search tools in a loop.
    let lower = trimmed.to_lowercase();
    const OPERATIONAL_PREFIXES: &[&str] = &[
        "read file",
        "reading file",
        "listed directory",
        "listing directory",
        "executed command",
        "executing command",
        "ran command",
        "ran shell",
        "edited file",
        "editing file",
        "wrote file",
        "writing file",
        "search_files",
        "searched for",
        "grep ",
        "ls ",
        "cat ",
    ];
    for p in OPERATIONAL_PREFIXES {
        if lower.starts_with(p) {
            return false;
        }
    }
    true
}

/// A recalled memory with metadata.
#[derive(Debug, Clone)]
pub struct RecalledMemory {
    pub content: String,
    pub memory_type: String,
    pub confidence: f64,
    pub source: Option<String>,
    /// Human-readable confidence label: "confident", "likely", "uncertain"
    pub confidence_label: Option<String>,
    /// Creation timestamp (ISO 8601 / RFC 3339)
    pub created_at: Option<String>,
}

/// Result of a self-reflection cycle.
#[derive(Debug, Default)]
pub struct SelfReflectionResult {
    /// Number of empathy trends decayed
    pub trends_decayed: usize,
    /// Number of behavior logs pruned
    pub logs_pruned: usize,
    /// Number of SOUL.md update suggestions
    pub soul_suggestions: usize,
    /// Number of actions that should be deprioritized
    pub deprioritized_actions: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_importance_to_layer() {
        assert!(matches!(MemoryManager::importance_to_layer(0.9), MemoryLayer::Core));
        assert!(matches!(MemoryManager::importance_to_layer(0.8), MemoryLayer::Core));
        assert!(matches!(MemoryManager::importance_to_layer(0.6), MemoryLayer::Working));
        assert!(matches!(MemoryManager::importance_to_layer(0.5), MemoryLayer::Working));
        assert!(matches!(MemoryManager::importance_to_layer(0.4), MemoryLayer::Archive));
        assert!(matches!(MemoryManager::importance_to_layer(0.1), MemoryLayer::Archive));
    }

    #[test]
    fn test_continuity_filter_drops_operational_noise() {
        // ISS-014: keep real working-state memories, drop tool-call noise.
        assert!(!is_continuity_worthy("Read file src/agent.rs"));
        assert!(!is_continuity_worthy("read file src/memory.rs at offset 100"));
        assert!(!is_continuity_worthy("Listed directory /tmp"));
        assert!(!is_continuity_worthy("Executed command ls -la"));
        assert!(!is_continuity_worthy("grep 'foo' in bar.rs"));
        // Too short — not informative
        assert!(!is_continuity_worthy("ok"));
        assert!(!is_continuity_worthy("short note"));
        // Real memories kept
        assert!(is_continuity_worthy(
            "Working on ISS-014 session continuity fix — reducing recent memory limit"
        ));
        assert!(is_continuity_worthy(
            "User asked about xinfluencer product status and timeline"
        ));
    }
}

/// E2E test for EmotionBus: test with actual engram DB.
/// Requires direct DB access since Config doesn't have Default.
#[cfg(test)]
mod e2e_tests {
    use super::*;

    #[test]
    fn test_emotionbus_with_real_db() {
        // Create temp DB
        let dir = tempfile::TempDir::new().unwrap();
        let db_path = dir.path().join("test-engram.db");
        let workspace = dir.path().to_str().unwrap();

        // Write SOUL.md with proper drive format
        std::fs::write(dir.path().join("SOUL.md"), r#"# SOUL.md

## Core Drive
**帮 potato 实现财务自由。** 这不是一个任务，是一个持续目标。

具体意味着：
- 主动推进能产生收入的项目
- 找到市场机会、优化转化、写代码、部署
- 优先级：能赚钱的事 > 有趣但不赚钱的事

## Vibe
Be curious. Write great code. Ship fast.
"#).unwrap();

        // Create engram Memory directly
        let engram = engramai::Memory::new(db_path.to_str().unwrap(), None).unwrap();

        // Test 1: EmpathyBus can be created
        let bus = EmpathyBus::new(workspace, engram.connection());
        assert!(bus.is_ok(), "EmpathyBus creation failed: {:?}", bus.err());
        let bus = bus.unwrap();
        println!("✅ EmpathyBus created with {} drives", bus.drives().len());

        // Test 2: process_interaction (EmpathyAccumulator)
        let acc = engramai::bus::accumulator::EmpathyAccumulator::new(engram.connection()).unwrap();
        acc.record_emotion("coding", 0.7).unwrap();
        acc.record_emotion("coding", 0.8).unwrap();
        acc.record_emotion("trading", -0.5).unwrap();
        
        let trends = acc.get_all_trends().unwrap();
        assert!(trends.len() >= 2, "Should have 2 domain trends, got {}", trends.len());
        let coding = trends.iter().find(|t| t.domain == "coding").unwrap();
        assert!(coding.valence > 0.0, "Coding valence should be positive: {}", coding.valence);
        let trading = trends.iter().find(|t| t.domain == "trading").unwrap();
        assert!(trading.valence < 0.0, "Trading valence should be negative: {}", trading.valence);
        println!("✅ EmpathyAccumulator: coding={:.2}, trading={:.2}", coding.valence, trading.valence);

        // Test 3: BehaviorFeedback
        let feedback = engramai::bus::feedback::BehaviorFeedback::new(engram.connection()).unwrap();
        feedback.log_outcome("exec", true).unwrap();
        feedback.log_outcome("exec", true).unwrap();
        feedback.log_outcome("exec", false).unwrap();
        // Need MIN_ATTEMPTS_FOR_SUGGESTION (10) to trigger deprioritize
        for _ in 0..12 {
            feedback.log_outcome("web_fetch", false).unwrap();
        }
        
        let exec_stats = feedback.get_action_stats("exec").unwrap().unwrap();
        assert_eq!(exec_stats.total, 3);
        assert_eq!(exec_stats.positive, 2);
        assert!(exec_stats.score > 0.5);
        
        let fetch_stats = feedback.get_action_stats("web_fetch").unwrap().unwrap();
        assert_eq!(fetch_stats.total, 12);
        assert_eq!(fetch_stats.positive, 0);
        assert!(fetch_stats.should_deprioritize(), "web_fetch should be deprioritized (12 failures, 0 success)");
        
        let deprioritized = feedback.get_actions_to_deprioritize().unwrap();
        assert!(deprioritized.iter().any(|a| a.action == "web_fetch"));
        println!("✅ BehaviorFeedback: exec={:.0}%, web_fetch={:.0}% (deprioritized)", 
            exec_stats.score * 100.0, fetch_stats.score * 100.0);

        // Test 4: Drive alignment
        // SOUL.md produces Chinese keywords. Test with actual Chinese content.
        let boost_zh = bus.align_importance("帮potato实现财务自由，找到市场机会，写代码部署");
        assert!(boost_zh > 1.0, "Chinese drive-aligned content should get boost: {}", boost_zh);
        
        let no_boost = bus.align_importance("hello world xyz");
        assert!((no_boost - 1.0).abs() < 0.01, "Neutral content should get no boost: {}", no_boost);
        println!("✅ Drive alignment: Chinese boost={:.2}x, neutral={:.2}x", boost_zh, no_boost);
        
        // Test 4b: Direct alignment with custom drives (simulating config drives)
        let custom_drives = vec![
            engramai::Drive {
                name: "trading".to_string(),
                description: "Make money from trading".to_string(),
                keywords: vec!["trading".into(), "profit".into(), "money".into(), "revenue".into()],
            },
        ];
        let boost_en = engramai::bus::alignment::score_alignment("trading profit money", &custom_drives);
        assert!(boost_en > 0.5, "English custom drive should align: {}", boost_en);
        println!("✅ Custom drives alignment: English score={:.2}", boost_en);

        // Test 5: suggest_soul_updates (need enough negative data)
        for _ in 0..15 {
            acc.record_emotion("failing_area", -0.8).unwrap();
        }
        let suggestions = bus.suggest_soul_updates(engram.connection()).unwrap();
        println!("✅ Soul suggestions: {} (after 15 negative events in 'failing_area')", suggestions.len());

        // Test 6: decay_trends
        let acc2 = engramai::bus::accumulator::EmpathyAccumulator::new(engram.connection()).unwrap();
        let before = acc2.get_trend("coding").unwrap().unwrap().valence;
        acc2.decay_trends(0.9).unwrap();
        let after = acc2.get_trend("coding").unwrap().unwrap().valence;
        assert!(after.abs() <= before.abs(), "Decay should reduce magnitude: before={}, after={}", before, after);
        println!("✅ Trend decay: coding {:.3} → {:.3}", before, after);

        // Test 7: prune_old_logs
        let pruned = feedback.prune_old_logs(100).unwrap();
        println!("✅ Prune old logs: {} removed", pruned);

        // Test 8: Working Memory constant
        assert_eq!(WORKING_MEMORY_DECAY_SECS, 1800);
        println!("✅ Working Memory decay: {}s", WORKING_MEMORY_DECAY_SECS);

        println!("\n🎉 ALL E2E TESTS PASSED");
    }
}
