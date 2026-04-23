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
    store_api::{StorageMeta, RawStoreOutcome},
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
    /// Test-only constructor that builds a `MemoryManager` rooted at an
    /// arbitrary workspace directory with a fresh engram DB underneath it.
    ///
    /// This exists so that `#[cfg(test)]` harnesses — both in this crate
    /// and in integration tests — don't have to hand-construct private
    /// fields, which is fragile across refactors.
    ///
    /// The caller is responsible for keeping the `TempDir` alive for the
    /// lifetime of the returned manager (the DB file lives inside it).
    #[cfg(test)]
    pub(crate) fn for_testing(workspace_dir: &std::path::Path) -> anyhow::Result<Self> {
        // Write a minimal SOUL.md so drive extraction doesn't fail.
        std::fs::write(
            workspace_dir.join("SOUL.md"),
            "# SOUL.md\n\n## Vibe\nBe helpful.\n",
        )?;

        let db_path = workspace_dir.join("test-engram.db");
        let engram = engramai::Memory::new(
            db_path.to_str().ok_or_else(|| anyhow::anyhow!("non-utf8 db path"))?,
            None,
        )
        .map_err(|e| anyhow::anyhow!("engram open: {}", e))?;

        Ok(MemoryManager {
            engram: Mutex::new(engram),
            wm_registry: Mutex::new(SessionRegistry::with_defaults(
                15,
                WORKING_MEMORY_DECAY_SECS,
            )),
            anomaly_tracker: Mutex::new(BaselineTracker::new(100)),
            drives: Vec::new(),
            empathy_bus: None,
            workspace_dir: workspace_dir.to_string_lossy().into_owned(),
            db_path: db_path.to_string_lossy().into_owned(),
            auto_recall: true,
            auto_store: true,
            recall_limit: 5,
            namespace: None,
        })
    }

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
    ///
    /// # ISS-021 Phase 1
    /// The `envelope` parameter carries message-level context (sender, chat_type,
    /// reply_to, etc.) which is serialized into `StorageMeta::user_metadata` under
    /// the `envelope` key. Phase 3/4 recall will use this for sender/chat-aware
    /// disambiguation. Pass `None` when storing non-message content (checkpoints,
    /// explicit tool stores, tests).
    pub fn store(
        &self,
        content: &str,
        memory_type: MemoryType,
        importance: f64,
        source: Option<&str>,
        envelope: Option<&crate::context::Envelope>,
    ) -> anyhow::Result<()> {
        if !self.auto_store {
            return Ok(());
        }

        // Calculate importance boost based on drive alignment
        let boosted_importance = self.calculate_importance(content, importance);
        
        // Note: Layer is determined by consolidation process, not at store time.
        // Higher importance memories will be promoted to Core/Extended during consolidation.

        let mut engram = self.engram.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        let user_metadata = match envelope {
            Some(env) => serde_json::json!({ "envelope": env }),
            None => serde_json::Value::Null,
        };
        let meta = StorageMeta {
            importance_hint: Some(boosted_importance),
            source: source.map(|s| s.to_string()),
            namespace: None,
            user_metadata,
            memory_type_hint: Some(memory_type),
        };
        match engram.store_raw(content, meta) {
            Ok(RawStoreOutcome::Stored(_)) => {}
            Ok(RawStoreOutcome::Skipped { reason, content_hash }) => {
                tracing::debug!(
                    "engram skipped store: reason={:?} hash={}",
                    reason,
                    content_hash.as_str()
                );
            }
            Ok(RawStoreOutcome::Quarantined { id, reason }) => {
                tracing::warn!(
                    "engram quarantined store: id={} reason={:?}",
                    id.as_str(),
                    reason
                );
            }
            Err(e) => return Err(anyhow::anyhow!("engram store_raw: {}", e)),
        }

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
    ///
    /// See [`store`] for `envelope` semantics. Tool-initiated stores usually
    /// pass `None` (no message context), but callers can surface envelope data
    /// when it's meaningful (e.g. storing "what potato just said").
    pub fn store_explicit(
        &self,
        content: &str,
        memory_type: MemoryType,
        importance: f64,
        envelope: Option<&crate::context::Envelope>,
    ) -> anyhow::Result<()> {
        // Calculate boosted importance
        let boosted_importance = self.calculate_importance(content, importance);
        
        let mut engram = self.engram.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        let user_metadata = match envelope {
            Some(env) => serde_json::json!({ "envelope": env }),
            None => serde_json::Value::Null,
        };
        let meta = StorageMeta {
            importance_hint: Some(boosted_importance),
            source: Some("agent_tool".to_string()),
            namespace: None,
            user_metadata,
            memory_type_hint: Some(memory_type),
        };
        match engram.store_raw(content, meta) {
            Ok(RawStoreOutcome::Stored(_)) => {}
            Ok(RawStoreOutcome::Skipped { reason, content_hash }) => {
                tracing::debug!(
                    "engram skipped explicit store: reason={:?} hash={}",
                    reason,
                    content_hash.as_str()
                );
            }
            Ok(RawStoreOutcome::Quarantined { id, reason }) => {
                tracing::warn!(
                    "engram quarantined explicit store: id={} reason={:?}",
                    id.as_str(),
                    reason
                );
            }
            Err(e) => return Err(anyhow::anyhow!("engram store_raw: {}", e)),
        }
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

// ─────────────────────────────────────────────────────────────────────────────
// ISS-021 Phase 1: Recall Quality Baseline
//
// Measures Precision@3 on a hand-curated fixture set BEFORE the header-in-
// content refactor lands. The P_before number captured here is the counter-
// factual baseline the Phase 5 go/no-go gate compares against.
//
// Phases 2–5 are gated on observing a >= 0.15 absolute improvement in P@3
// on these fixtures between "old path (header-in-content)" and "new path
// (header-as-envelope)". If the gate fails, we do NOT run the full migration.
//
// Design ref: .gid/issues/ISS-021-message-context-side-channel/issue.md §Phase 1
// ─────────────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod recall_quality_baseline {
    use super::*;

    /// One fixture: corpus to seed, a query, and which corpus indices are
    /// the "gold" relevant hits.
    struct Fixture {
        name: &'static str,
        corpus: &'static [&'static str],
        query: &'static str,
        /// Indices into `corpus` of the memories that SHOULD appear in top-3.
        relevant_ixs: &'static [usize],
    }

    /// 10 balanced fixtures across distinct topics. Each has **3** clearly-
    /// relevant memories in a corpus of 8, so Precision@3 ranges over the
    /// full domain {0.0, 0.333, 0.667, 1.0} — no ceiling/saturation artifact.
    ///
    /// This is a deliberate change from the initial 1-relevant-per-fixture
    /// design, which saturated every fixture at 0.333 and made Phase 5's
    /// P_before → P_after comparison uninformative. Distractors are
    /// deliberately chosen to include some near-topic items (e.g. a different
    /// cryptographic protocol in the OAuth fixture) so recall quality
    /// differences actually matter.
    const FIXTURES: &[Fixture] = &[
        Fixture {
            name: "authentication-flow",
            corpus: &[
                // relevant (auth-related)
                "OAuth2 token refresh flow uses a refresh_token and client secret to mint a new access_token.",
                "JWT tokens encode claims as a signed base64 payload that resource servers verify with a public key.",
                "PKCE (Proof Key for Code Exchange) prevents authorization code interception in OAuth2 public clients.",
                // distractors (including one near-topic crypto item)
                "TLS 1.3 reduces the handshake to a single round-trip by encrypting the server response in the first reply.",
                "Sourdough bread requires a starter fermented for at least 5 days before first bake.",
                "The ferry schedule between Staten Island and Manhattan runs every 30 minutes.",
                "Rust's ownership model prevents data races at compile time.",
                "Jazz saxophone improvisation draws heavily on bebop vocabulary.",
            ],
            query: "how does oauth token refresh work",
            relevant_ixs: &[0, 1, 2],
        },
        Fixture {
            name: "rust-ownership",
            corpus: &[
                "In Rust, the borrow checker enforces that a value has at most one mutable reference at a time.",
                "Rust's move semantics transfer ownership when a value is assigned or passed by value.",
                "Lifetimes in Rust ensure references never outlive the data they point to, checked at compile time.",
                // distractors (one near-topic: C++ RAII, another language feature)
                "C++ RAII ties resource lifetime to object scope via constructors and destructors.",
                "Shakespeare's Hamlet was first performed around 1603 at the Globe Theatre.",
                "Mount Fuji is a stratovolcano that last erupted in 1707.",
                "Italian espresso is brewed at roughly 9 bars of pressure through 18g of ground coffee.",
                "The Pythagorean theorem relates the sides of a right triangle as a² + b² = c².",
            ],
            query: "rust borrow checker mutable reference rules",
            relevant_ixs: &[0, 1, 2],
        },
        Fixture {
            name: "baking-sourdough",
            corpus: &[
                "A sourdough starter is a symbiotic culture of wild yeast and lactobacilli feeding on flour.",
                "Bulk fermentation of sourdough develops gluten structure through stretch-and-folds over 4-6 hours.",
                "A high-hydration sourdough (>80% baker's percentage) produces an open, airy crumb.",
                // distractors (one near-topic: commercial bread)
                "Commercial baker's yeast is a single Saccharomyces cerevisiae strain industrially propagated.",
                "TCP congestion control uses AIMD — additive increase, multiplicative decrease.",
                "The French Revolution began in 1789 with the storming of the Bastille.",
                "Quantum entanglement links particles so measurement of one instantly affects the other.",
                "Marathon training plans typically peak 3 weeks before race day.",
            ],
            query: "sourdough starter wild yeast lactobacilli",
            relevant_ixs: &[0, 1, 2],
        },
        Fixture {
            name: "tcp-networking",
            corpus: &[
                "TCP uses a three-way handshake (SYN, SYN-ACK, ACK) to establish a connection.",
                "TCP congestion control uses AIMD — additive increase, multiplicative decrease — to probe bandwidth.",
                "The TCP sliding window allows a sender to transmit multiple segments before awaiting cumulative ack.",
                // distractors (one near-topic: UDP)
                "UDP is connectionless and does not guarantee delivery, ordering, or deduplication.",
                "Van Gogh painted Starry Night in 1889 during his stay at a mental asylum.",
                "The tango originated in the working-class neighborhoods of Buenos Aires.",
                "A Mediterranean diet emphasizes olive oil, fish, and whole grains.",
                "Chess endgames with king-and-pawn vs king often hinge on the opposition.",
            ],
            query: "tcp connection handshake syn ack",
            relevant_ixs: &[0, 1, 2],
        },
        Fixture {
            name: "espresso-extraction",
            corpus: &[
                "Proper espresso extraction yields 2:1 ratio (output:dose) in 25-30 seconds.",
                "Espresso grind size must be fine enough to produce 9 bars of backpressure without choking the machine.",
                "Channeling during espresso extraction leaves dry pucks and sour, under-extracted shots.",
                // distractors (one near-topic: other brew methods)
                "Pour-over coffee typically uses a 1:16 ratio brewed over 3-4 minutes with medium grind.",
                "The Kuiper belt contains many small icy bodies beyond Neptune's orbit.",
                "Ruby on Rails popularized the convention-over-configuration philosophy.",
                "Napoleon's retreat from Moscow in 1812 decimated the Grande Armée.",
                "A haiku consists of three lines with 5, 7, and 5 syllables respectively.",
            ],
            query: "espresso extraction ratio time",
            relevant_ixs: &[0, 1, 2],
        },
        Fixture {
            name: "quantum-entanglement",
            corpus: &[
                "Entangled particles share a quantum state such that measuring one determines the other's observable.",
                "Bell's theorem shows no local-hidden-variable theory can reproduce all entanglement predictions.",
                "Quantum teleportation uses a pre-shared entangled pair plus two classical bits to transmit a qubit state.",
                // distractors (one near-topic: classical physics)
                "Classical correlations differ from quantum entanglement because they admit local hidden variables.",
                "The Amazon rainforest produces roughly 20% of the world's oxygen.",
                "Baroque architecture is characterized by dramatic curves and rich ornamentation.",
                "A sonnet has 14 lines, typically in iambic pentameter.",
                "The Mariana Trench is the deepest known point in Earth's oceans.",
            ],
            query: "what is quantum entanglement between particles",
            relevant_ixs: &[0, 1, 2],
        },
        Fixture {
            name: "database-indexing",
            corpus: &[
                "A B-tree index speeds up range queries by keeping sorted pointers to table rows.",
                "Hash indexes give O(1) equality lookups but cannot serve range queries.",
                "Covering indexes include all columns a query needs so the heap table is never accessed.",
                // distractors (one near-topic: other DB internals)
                "The query planner chooses between nested-loop, hash-join, and merge-join based on row estimates.",
                "The Eiffel Tower was completed in 1889 for the Paris World's Fair.",
                "Jazz fusion emerged in the late 1960s, blending rock rhythms with jazz harmony.",
                "The koala has fingerprints almost indistinguishable from humans.",
                "Renaissance painters rediscovered linear perspective in the 15th century.",
            ],
            query: "btree index range query database",
            relevant_ixs: &[0, 1, 2],
        },
        Fixture {
            name: "mountain-climbing",
            corpus: &[
                "At altitudes above 8000 meters climbers enter the 'death zone' where oxygen is critically low.",
                "Acclimatization schedules below 8000m follow 'climb high, sleep low' to boost red blood cell count.",
                "Supplemental oxygen at high altitude keeps arterial saturation above the cognitive-impairment threshold.",
                // distractors (one near-topic: other endurance sport)
                "Marathon training at sea level does not acclimatize runners to altitude-race conditions.",
                "The Treaty of Westphalia in 1648 ended the Thirty Years' War.",
                "Merge sort has O(n log n) worst-case time complexity.",
                "Michelangelo's David stands 5.17 meters tall and was carved from a single marble block.",
                "Photosynthesis converts carbon dioxide and water into glucose using sunlight.",
            ],
            query: "death zone altitude oxygen mountain climbing",
            relevant_ixs: &[0, 1, 2],
        },
        Fixture {
            name: "ml-gradient-descent",
            corpus: &[
                "Stochastic gradient descent updates model weights using gradients from mini-batches.",
                "The learning rate in gradient descent controls step size and must be tuned to avoid overshoot.",
                "Adam optimizer augments SGD with per-parameter adaptive step sizes using first and second moments.",
                // distractors (one near-topic: other ML
                "Genetic algorithms search a parameter space without gradient information by mutation and crossover.",
                "The Silk Road connected China to the Mediterranean for over 1500 years.",
                "Cherry blossom season in Japan typically peaks in early April.",
                "A Rubik's cube has 43 quintillion possible configurations.",
                "The human heart pumps roughly 2000 gallons of blood per day.",
            ],
            query: "how does stochastic gradient descent work",
            relevant_ixs: &[0, 1, 2],
        },
        Fixture {
            name: "culinary-fermentation",
            corpus: &[
                "Kimchi is made by lacto-fermenting napa cabbage with chili, garlic, and fish sauce.",
                "Sauerkraut ferments shredded cabbage in brine using wild lactobacillus strains for 1-4 weeks.",
                "Miso is fermented with Aspergillus oryzae (koji) plus salt over months to years.",
                // distractors (one near-topic: non-fermented preservation)
                "Pickling with vinegar preserves vegetables through acidity without microbial fermentation.",
                "The Hubble Space Telescope has been operating since 1990.",
                "Go's goroutines are lightweight threads scheduled by the Go runtime.",
                "The tango originated in Buenos Aires in the late 19th century.",
                "Stonehenge was built in multiple phases between 3000 and 2000 BCE.",
            ],
            query: "kimchi lacto fermentation cabbage",
            relevant_ixs: &[0, 1, 2],
        },
    ];

    /// Build an isolated `MemoryManager` rooted at a freshly created temp DB
    /// with auto-store/recall enabled and a modest recall limit.
    ///
    /// Delegates to `MemoryManager::for_testing` so that field additions to
    /// the real struct don't silently break the test harness.
    fn make_manager() -> (MemoryManager, tempfile::TempDir) {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let manager = MemoryManager::for_testing(dir.path()).expect("for_testing");
        (manager, dir)
    }

    /// Compute Precision@3: fraction of top-3 results whose content matches
    /// one of the gold-relevant corpus entries. Range: {0.0, 1/3, 2/3, 1.0}
    /// given that each fixture has exactly 3 gold-relevant items.
    fn precision_at_3(results: &[RecalledMemory], fixture: &Fixture) -> f64 {
        let gold: Vec<&str> = fixture
            .relevant_ixs
            .iter()
            .map(|&i| fixture.corpus[i])
            .collect();
        let top_k: Vec<_> = results.iter().take(3).collect();
        let hits = top_k
            .iter()
            .filter(|r| gold.iter().any(|g| r.content == *g))
            .count();
        // Denominator is min(3, #gold, #results). With 3 gold items and
        // recall_limit=5, this is always 3 unless the recall path returned
        // fewer than 3 results (in which case P@3 is still computed over 3
        // to preserve the ceiling semantics — missing hits are missing hits).
        hits as f64 / 3.0
    }

    /// Baseline measurement — records P@3 across all 10 fixtures and writes
    /// the summary to stdout (`cargo test -- --nocapture` to see it).
    ///
    /// **This test does not fail on low scores** — it is a measurement, not a
    /// gate. The Phase 5 go/no-go compares a shadow-DB run's P@3 against the
    /// P_before captured here. A 0.15 absolute delta is the agreed significance
    /// threshold (see ISS-021 §Phase 1 baseline spec).
    #[test]
    fn recall_baseline_precision_at_3_per_fixture() {
        let mut scores = Vec::with_capacity(FIXTURES.len());
        let mut storage_audit: Vec<(&str, usize, usize)> = Vec::new(); // (name, stored_rows, quarantined)

        for fixture in FIXTURES {
            let (mgr, _guard) = make_manager();

            // Seed the corpus. Use a per-fixture session_key so working
            // memory doesn't cross-contaminate between fixtures.
            let session_key = format!("baseline-{}", fixture.name);
            for (i, item) in fixture.corpus.iter().enumerate() {
                mgr.store(
                    item,
                    MemoryType::Factual,
                    0.5,
                    Some(&format!("baseline-fixture-{}-ix{}", fixture.name, i)),
                    None,
                )
                .expect("store fixture memory");
            }

            // Storage audit: confirm every fixture item actually landed in
            // engram (nothing Skipped by dedup/rate-limit, nothing Quarantined
            // by the PII scanner). This is the Phase 1 integrity check for
            // the store_raw migration — a dirty baseline would invalidate
            // the Phase 5 P_before → P_after comparison.
            let (stored_count, quarantined) = {
                let engram = mgr.engram.lock().expect("engram lock");
                let all = engram.list(None).expect("list memories");
                let q = engram.count_quarantine().expect("count quarantine");
                (all.len(), q)
            };
            storage_audit.push((fixture.name, stored_count, quarantined));

            // Recall via the current (header-in-content) path. In Phase 5 the
            // shadow-DB comparison will re-run this exact harness against the
            // envelope-aware path to compute P_after.
            let (results, _full) = mgr
                .session_recall(fixture.query, &session_key)
                .expect("session_recall");

            let p3 = precision_at_3(&results, fixture);
            scores.push((fixture.name, p3, results.len()));
        }

        // Aggregate.
        let total: f64 = scores.iter().map(|(_, p, _)| *p).sum();
        let mean = total / scores.len() as f64;

        println!("\n=== ISS-021 Phase 1 Baseline: Precision@3 ===");
        println!("(header-in-content path, pre-envelope refactor)");
        println!("(3 gold-relevant items per fixture of 8; P@3 range: 0.0 / 0.333 / 0.667 / 1.0)");
        for (name, p3, n) in &scores {
            println!("  {:<26} P@3 = {:.3}  (recalled {})", name, p3, n);
        }
        println!("  ---");
        println!("  mean P@3  = {:.3}  (P_before)", mean);
        println!("  fixtures  = {}", scores.len());
        println!("=================================================\n");

        println!("=== Storage audit (store_raw outcome integrity) ===");
        for (name, stored, quarantined) in &storage_audit {
            println!("  {:<26} stored={}  quarantined={}", name, stored, quarantined);
        }
        println!("=================================================\n");

        // Integrity gate: every fixture's 8 items must be fully stored, none
        // quarantined. If this trips, the store_raw migration has a
        // behavioral change we didn't account for — baseline is invalid,
        // fix before proceeding to Phase 2.
        for (name, stored, quarantined) in &storage_audit {
            assert_eq!(
                *stored, 8,
                "fixture '{}' expected 8 stored items, found {} — \
                 store_raw is silently dropping fixture content",
                name, stored
            );
            assert_eq!(
                *quarantined, 0,
                "fixture '{}' has {} quarantined items — PII scanner or \
                 content filter is rejecting baseline fixture content; \
                 baseline is unusable for Phase 5 comparison",
                name, quarantined
            );
        }

        // Sanity guard: the HARNESS works (some fixture got at least one
        // hit somewhere in top-3). If this fails, the test infrastructure
        // itself is broken — it's NOT a quality gate on recall itself.
        let any_hit = scores.iter().any(|(_, p, _)| *p > 0.0);
        assert!(
            any_hit,
            "baseline harness produced zero hits across all 10 fixtures — \
             recall path is broken, not just low-quality. Investigate before \
             proceeding to Phase 2."
        );
    }

    /// Smoke test: zero-behaviour-change contract for the Phase 1 parallel
    /// entry point. Not a precision measurement — just asserts the envelope-
    /// threading path compiles, links, and doesn't panic when passed `None`.
    #[test]
    fn envelope_plumbing_compiles() {
        // This test exists to pin the Phase 1 contract: `Envelope` must be
        // constructible, cloneable, and serde-roundtrippable.
        use crate::context::{ChatType, Envelope, QuotedMessage};

        let env = Envelope {
            sender_id: Some("u1".into()),
            sender_name: Some("potato".into()),
            sender_username: Some("potatosoupup".into()),
            chat_type: ChatType::Group { title: Some("testing".into()) },
            reply_to: Some(QuotedMessage {
                text: "prior message".into(),
                sender_name: Some("someone".into()),
                sender_username: None,
                sender_id: None,
                message_id: Some(42),
            }),
            message_id: Some(100),
        };

        // Serde roundtrip works — Phase 2 persists this into
        // `StorageMeta::user_metadata` as `{"envelope": <json>}`.
        let json = serde_json::to_value(&env).expect("serialize envelope");
        let back: Envelope = serde_json::from_value(json).expect("deserialize envelope");
        assert_eq!(back.sender_id, env.sender_id);
        assert_eq!(back.message_id, env.message_id);
        match (&back.chat_type, &env.chat_type) {
            (ChatType::Group { title: a }, ChatType::Group { title: b }) => assert_eq!(a, b),
            _ => panic!("chat_type roundtrip lost variant"),
        }

        // `render_for_prompt` still works — legacy header rendering path must
        // remain intact through Phase 1 (Phase 2+3 introduce the envelope
        // side-channel, Phase 4 removes this call path).
        let prefix = env.render_for_prompt("telegram");
        assert!(prefix.contains("POTATO") || prefix.contains("potato"));
        assert!(prefix.contains("testing"));
    }

    // ─── Phase 5b: Counterfactual Measurement ───────────────────────
    //
    // Controlled experiment — for each fixture, seed the corpus twice:
    //   1. `polluted`: each item prepended with a realistic Telegram header
    //      (matches pre-Phase-2+3 behavior and the legacy rows the diagnostic
    //      scan found in prod — see `memory_migrate::scan_db`).
    //   2. `clean`: each item stored as-is (post-Phase-2+3 behavior).
    //
    // All other knobs (embedding model, recall path, session_key scheme, query,
    // recall limit) held identical. delta = P_clean - P_polluted isolates
    // header pollution as the only variable.
    //
    // Adversarial design: the polluted variant shares the SAME header
    // across all 8 corpus items per fixture, mimicking prod where the vast
    // majority of legacy rows carry `id:7539582820`. If the embedding model
    // is header-sensitive, this pulls polluted items into a tight cluster
    // away from the query vector, amplifying the delta.
    //
    // Gate: delta ≥ 0.15 → Phase 5c wet migration justified.
    //       delta  < 0.15 → pollution not dominant, file separate issue.
    //
    // This test does NOT fail on any particular delta — it's a measurement,
    // not a correctness gate. The decision lives in the Phase 5b Execution
    // Record in `.gid/issues/ISS-021-message-context-side-channel/issue.md`.

    /// Fixed realistic pollution header matching the legacy format identified
    /// by `memory_migrate::HEADER_STRIP_RE` / `channel_header_regex`.
    const POLLUTION_HEADER: &str =
        "[TELEGRAM potato (@potatosoupup) id:7539582820 Thu 2026-04-23 12:00 -04:00]\n\n";

    /// Run one fixture in one mode; returns (P@3, stored_count, quarantined).
    fn run_fixture_mode(
        fixture: &Fixture,
        mode: &str,
        pollute: bool,
    ) -> (f64, usize, usize) {
        let (mgr, _guard) = make_manager();
        let session_key = format!("5b-{}-{}", mode, fixture.name);

        for (i, item) in fixture.corpus.iter().enumerate() {
            let content = if pollute {
                format!("{}{}", POLLUTION_HEADER, item)
            } else {
                (*item).to_string()
            };
            mgr.store(
                &content,
                MemoryType::Factual,
                0.5,
                Some(&format!("5b-{}-{}-ix{}", mode, fixture.name, i)),
                None,
            )
            .expect("store fixture memory (phase 5b)");
        }

        let (stored, quarantined) = {
            let engram = mgr.engram.lock().expect("engram lock");
            let all = engram.list(None).expect("list memories");
            let q = engram.count_quarantine().expect("count quarantine");
            (all.len(), q)
        };

        let (results, _full) = mgr
            .session_recall(fixture.query, &session_key)
            .expect("session_recall (phase 5b)");

        // Strip the pollution header before matching — this is exactly what
        // `memory_migrate::run_migrate_envelope` would do in the wet path, so
        // P@3 in `polluted` mode reflects *recall quality with contaminated
        // embeddings but the same reference corpus semantics*. Without this,
        // we'd be measuring a trivial string-equality artifact instead of
        // the embedding-contamination effect we actually care about.
        let p3 = precision_at_3_normalized(&results, fixture, pollute);
        (p3, stored, quarantined)
    }

    /// Precision@3 that normalizes recalled content by stripping the
    /// pollution header before comparing to gold strings. Keeps the
    /// comparison apples-to-apples across modes.
    fn precision_at_3_normalized(
        results: &[RecalledMemory],
        fixture: &Fixture,
        strip_header: bool,
    ) -> f64 {
        let gold: Vec<&str> = fixture
            .relevant_ixs
            .iter()
            .map(|&i| fixture.corpus[i])
            .collect();
        let top_k: Vec<_> = results.iter().take(3).collect();
        let hits = top_k
            .iter()
            .filter(|r| {
                let c: &str = if strip_header {
                    r.content
                        .strip_prefix(POLLUTION_HEADER)
                        .unwrap_or(&r.content)
                } else {
                    &r.content
                };
                gold.iter().any(|g| c == *g)
            })
            .count();
        hits as f64 / 3.0
    }

    #[test]
    fn recall_counterfactual_header_pollution_phase_5b() {
        // Diagnostic: which recall path is this harness actually exercising?
        // If Ollama is down or the embedding provider errored, we'd silently
        // fall back to FTS — which treats the fixed Telegram header as a set
        // of term tokens present in every row (common-mode), so delta would
        // be ~0 for trivial reasons, not because the embedding model is
        // header-robust. We need to know which regime we're in before
        // interpreting the delta.
        let (probe_mgr, _probe_guard) = make_manager();
        let embedding_regime = {
            let engram = probe_mgr.engram.lock().expect("engram lock");
            if engram.is_embedding_available() {
                "EMBEDDING (Ollama/nomic-embed-text live)"
            } else if engram.has_embedding_support() {
                "EMBEDDING-DEGRADED (provider present but unavailable — FTS fallback)"
            } else {
                "FTS-ONLY (no embedding provider — delta is uninterpretable)"
            }
        };
        drop(probe_mgr);
        println!("\n[Phase 5b] Recall regime: {}", embedding_regime);

        let mut per_fixture: Vec<(&str, f64, f64)> = Vec::with_capacity(FIXTURES.len());
        let mut audit: Vec<(&str, usize, usize, usize, usize)> =
            Vec::with_capacity(FIXTURES.len()); // (name, clean_stored, clean_q, polluted_stored, polluted_q)

        for fixture in FIXTURES {
            let (p_clean, c_stored, c_q) = run_fixture_mode(fixture, "clean", false);
            let (p_polluted, p_stored, p_q) = run_fixture_mode(fixture, "polluted", true);
            per_fixture.push((fixture.name, p_clean, p_polluted));
            audit.push((fixture.name, c_stored, c_q, p_stored, p_q));
        }

        let mean_clean: f64 =
            per_fixture.iter().map(|(_, c, _)| *c).sum::<f64>() / per_fixture.len() as f64;
        let mean_polluted: f64 =
            per_fixture.iter().map(|(_, _, p)| *p).sum::<f64>() / per_fixture.len() as f64;
        let delta = mean_clean - mean_polluted;

        println!("\n=== ISS-021 Phase 5b: Counterfactual (header pollution) ===");
        println!("(controlled experiment: same corpus/query, header is the only variable)");
        println!(
            "{:<26} {:>10} {:>10} {:>10}",
            "fixture", "P_clean", "P_polluted", "delta"
        );
        for (name, c, p) in &per_fixture {
            println!(
                "{:<26} {:>10.3} {:>10.3} {:>10.3}",
                name,
                c,
                p,
                c - p
            );
        }
        println!("  ---");
        println!("  mean P_clean    = {:.3}", mean_clean);
        println!("  mean P_polluted = {:.3}", mean_polluted);
        println!("  DELTA           = {:+.3}", delta);
        println!("  gate threshold  = 0.150");
        let decision = if delta >= 0.15 {
            "ACCEPT — Phase 5c wet migration justified"
        } else if delta <= -0.05 {
            "REJECT + anomaly — polluted scored HIGHER; investigate embedding weirdness"
        } else {
            "REJECT — pollution not dominant; open new issue for real bottleneck"
        };
        println!("  decision        = {}", decision);
        println!("============================================================\n");

        println!("=== Phase 5b storage audit ===");
        for (name, cs, cq, ps, pq) in &audit {
            println!(
                "  {:<26} clean(stored={}, q={})  polluted(stored={}, q={})",
                name, cs, cq, ps, pq
            );
        }
        println!("==============================\n");

        // Integrity gates (same spirit as Phase 1 baseline audit).
        for (name, cs, cq, ps, pq) in &audit {
            assert_eq!(
                *cs, 8,
                "fixture '{}' clean-mode stored {} rows, expected 8",
                name, cs
            );
            assert_eq!(
                *cq, 0,
                "fixture '{}' clean-mode quarantined {} rows (should be 0)",
                name, cq
            );
            assert_eq!(
                *ps, 8,
                "fixture '{}' polluted-mode stored {} rows, expected 8 — \
                 PII scanner may be reacting to the Telegram header",
                name, ps
            );
            assert_eq!(
                *pq, 0,
                "fixture '{}' polluted-mode quarantined {} rows — \
                 PII scanner rejected content with channel header; this \
                 invalidates the Phase 5b counterfactual (different storage \
                 population between modes). Investigate before interpreting delta.",
                name, pq
            );
        }

        // Sanity guard: harness produced SOME hits.
        let any_hit = per_fixture.iter().any(|(_, c, p)| *c > 0.0 || *p > 0.0);
        assert!(
            any_hit,
            "Phase 5b harness produced zero hits across all 20 runs — \
             recall path is broken, measurement is invalid."
        );
        let _ = (mean_clean, mean_polluted, delta);
    }

    /// Sanity probe (NOT the Phase 5b measurement): confirm the harness
    /// actually reacts to content changes. Seed corpus with each item
    /// prepended by a LARGE off-topic text block. If the recall path is
    /// content-sensitive, P@3 should drop noticeably vs. clean; if not,
    /// the Phase 5b delta=0 result is uninterpretable (harness broken).
    ///
    /// This test asserts P_contaminated < P_clean by at least 0.10 on
    /// average. If it fails, Phase 5b measurement is void.
    #[test]
    fn recall_harness_sanity_reacts_to_large_content_changes() {
        const HEAVY_DISTRACTOR: &str = "\
            The migratory patterns of the arctic tern span from pole to pole, \
            covering roughly 70000 kilometers per year. Baleen whales filter \
            krill through their keratin plates. Neolithic agriculture began \
            in the Fertile Crescent around 10000 BCE. Ferromagnetic domains \
            align under applied magnetic fields. The Doppler effect shifts \
            perceived frequency for moving sources. Plate tectonics drives \
            continental drift at a few centimeters per year. The citric acid \
            cycle produces ATP in mitochondria. Sumerian cuneiform is the \
            earliest known writing system. Nuclear fusion in stellar cores \
            creates heavier elements from hydrogen. The speed of sound in \
            air at sea level is roughly 343 meters per second.\n\n";

        let mut clean_total = 0.0;
        let mut heavy_total = 0.0;

        for fixture in FIXTURES {
            // Clean run.
            let (mgr_c, _g1) = make_manager();
            let key_c = format!("sanity-clean-{}", fixture.name);
            for (i, item) in fixture.corpus.iter().enumerate() {
                mgr_c
                    .store(
                        item,
                        MemoryType::Factual,
                        0.5,
                        Some(&format!("sanity-clean-{}-{}", fixture.name, i)),
                        None,
                    )
                    .expect("store");
            }
            let (r_c, _) = mgr_c
                .session_recall(fixture.query, &key_c)
                .expect("recall clean");
            clean_total += precision_at_3_normalized(&r_c, fixture, false);

            // Heavy-distractor run — each item gets a large off-topic prefix.
            let (mgr_h, _g2) = make_manager();
            let key_h = format!("sanity-heavy-{}", fixture.name);
            for (i, item) in fixture.corpus.iter().enumerate() {
                let content = format!("{}{}", HEAVY_DISTRACTOR, item);
                mgr_h
                    .store(
                        &content,
                        MemoryType::Factual,
                        0.5,
                        Some(&format!("sanity-heavy-{}-{}", fixture.name, i)),
                        None,
                    )
                    .expect("store");
            }
            let (r_h, _) = mgr_h
                .session_recall(fixture.query, &key_h)
                .expect("recall heavy");
            // Strip the heavy prefix before matching gold (same normalization
            // idea as Phase 5b — isolate the embedding effect, not a string-
            // equality artefact).
            let gold: Vec<&str> = fixture
                .relevant_ixs
                .iter()
                .map(|&i| fixture.corpus[i])
                .collect();
            let hits = r_h
                .iter()
                .take(3)
                .filter(|r| {
                    let c = r.content.strip_prefix(HEAVY_DISTRACTOR).unwrap_or(&r.content);
                    gold.iter().any(|g| c == *g)
                })
                .count();
            heavy_total += hits as f64 / 3.0;
        }

        let n = FIXTURES.len() as f64;
        let p_clean = clean_total / n;
        let p_heavy = heavy_total / n;
        let drop = p_clean - p_heavy;

        println!("\n=== Phase 5b sanity probe: does recall react to content? ===");
        println!("  P_clean    = {:.3}", p_clean);
        println!("  P_heavy    = {:.3}  (corpus with large off-topic prefix)", p_heavy);
        println!("  drop       = {:+.3}", drop);
        println!("===========================================================\n");

        // If heavy-distractor content doesn't measurably hurt recall, the
        // harness is not actually exercising the embedding-similarity path,
        // and the Phase 5b delta=0 result cannot be trusted.
        assert!(
            drop >= 0.10,
            "Phase 5b harness sanity check FAILED: a large off-topic prefix \
             produced drop={:.3}, expected >= 0.10. Recall path likely not \
             using content embeddings — Phase 5b measurement is invalid \
             until this is explained.",
            drop
        );
    }
}
