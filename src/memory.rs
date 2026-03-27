//! Memory management with native Engram integration.
//!
//! Unlike OpenClaw (MCP overhead) or Hermes (FTS only),
//! RustClaw uses engramai as a direct Rust dependency — zero IPC overhead.

use engramai::{
    Memory, MemoryConfig, MemoryType, AnthropicExtractor,
    SessionWorkingMemory, BaselineTracker,
    bus::{mod_io::{parse_soul, Drive}, alignment::calculate_importance_boost},
};
use std::path::Path;
use std::sync::Mutex;

use crate::config::Config;
use crate::oauth::OAuthTokenManager;

/// Memory manager wrapping Engram with RustClaw-specific logic.
/// Uses Mutex instead of async RwLock because rusqlite isn't Send+Sync.
pub struct MemoryManager {
    engram: Mutex<Memory>,
    /// Session working memory for topic continuity (Miller's Law: 7±2 items)
    wm: Mutex<SessionWorkingMemory>,
    /// Anomaly detection for storage patterns
    anomaly_tracker: Mutex<BaselineTracker>,
    /// Drives from SOUL.md for importance boosting
    drives: Vec<Drive>,
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

        // Set up LLM extraction using OAuth token from Keychain (Claude Max plan)
        if let Ok(oauth_mgr) = OAuthTokenManager::from_keychain() {
            // get_token is async but we're in async context via new()
            // Use blocking approach: read token directly from the manager's initial state
            if let Ok(token) = tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(oauth_mgr.get_token())
            }) {
                engram.set_extractor(Box::new(AnthropicExtractor::new(&token, true)));
                tracing::info!("Engram extractor: Anthropic Haiku (OAuth from Keychain)");
            } else {
                tracing::debug!("OAuth token unavailable, extractor disabled");
            }
        } else {
            // Fallback: auto_configure_extractor checks env vars and config file
            tracing::debug!("No Keychain OAuth, relying on engram auto-config");
        }

        // Load drives from SOUL.md for importance boosting
        let soul_path = format!("{}/SOUL.md", workspace_dir);
        let drives = if Path::new(&soul_path).exists() {
            match std::fs::read_to_string(&soul_path) {
                Ok(content) => {
                    let drives = parse_soul(&content);
                    tracing::info!("Loaded {} drives from SOUL.md", drives.len());
                    drives
                }
                Err(e) => {
                    tracing::debug!("Failed to read SOUL.md: {}", e);
                    Vec::new()
                }
            }
        } else {
            Vec::new()
        };

        // Initialize session working memory (15 items, 5 minute decay)
        let wm = SessionWorkingMemory::new(15, 300);
        
        // Initialize anomaly tracker (100 sample window)
        let anomaly_tracker = BaselineTracker::new(100);

        Ok(Self {
            engram: Mutex::new(engram),
            wm: Mutex::new(wm),
            anomaly_tracker: Mutex::new(anomaly_tracker),
            drives,
            auto_recall: config.memory.auto_recall,
            auto_store: config.memory.auto_store,
            recall_limit: config.memory.recall_limit,
            namespace: None,
        })
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
    pub fn namespace(&self) -> Option<&str> {
        self.namespace.as_deref()
    }

    /// Apply namespace prefix to content if namespace is set.
    fn namespaced_content(&self, content: &str) -> String {
        match &self.namespace {
            Some(ns) => format!("[{}] {}", ns, content),
            None => content.to_string(),
        }
    }

    /// Apply namespace prefix to query if namespace is set.
    fn namespaced_query(&self, query: &str) -> String {
        match &self.namespace {
            Some(ns) => format!("[{}] {}", ns, query),
            None => query.to_string(),
        }
    }

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
                confidence: r.activation,
                source: Some(r.record.source.clone()),
                confidence_label: Some(r.confidence_label),
            })
            .collect())
    }

    /// Store important information from a conversation turn.
    /// Called by BeforeOutbound hook.
    ///
    /// Applies drive alignment boost and tracks anomalies.
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
        let boosted_importance = if !self.drives.is_empty() {
            let boost = calculate_importance_boost(content, &self.drives);
            let final_importance = (importance * boost).min(1.0);
            if boost > 1.0 {
                tracing::debug!(
                    "Drive alignment boost: {:.2}x (importance: {:.2} → {:.2})",
                    boost, importance, final_importance
                );
            }
            final_importance
        } else {
            importance
        };

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
    /// Returns (memories, full_recall_triggered).
    pub fn session_recall(&self, query: &str) -> anyhow::Result<(Vec<RecalledMemory>, bool)> {
        if !self.auto_recall {
            return Ok((Vec::new(), false));
        }

        let mut engram = self.engram.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        let mut wm = self.wm.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;

        let result = engram
            .session_recall(query, &mut wm, self.recall_limit, None, None)
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        let memories = result
            .results
            .into_iter()
            .map(|r| RecalledMemory {
                content: r.record.content.clone(),
                memory_type: format!("{:?}", r.record.memory_type),
                confidence: r.activation,
                source: Some(r.record.source.clone()),
                confidence_label: Some(r.confidence_label),
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
                confidence: r.activation,
                source: Some(r.record.source.clone()),
                confidence_label: Some(r.confidence_label),
            })
            .collect())
    }

    /// Run memory consolidation (during heartbeats).
    pub fn consolidate(&self) -> anyhow::Result<()> {
        let mut engram = self.engram.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        engram
            .consolidate(7.0)
            .map_err(|e| anyhow::anyhow!("{}", e))?;
        Ok(())
    }

    /// Get memory stats.
    pub fn stats(&self) -> anyhow::Result<serde_json::Value> {
        let engram = self.engram.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        let stats = engram
            .stats()
            .map_err(|e| anyhow::anyhow!("{}", e))?;
        Ok(serde_json::to_value(stats)?)
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
                confidence: r.activation,
                source: Some(r.record.source.clone()),
                confidence_label: Some(r.confidence_label),
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
        let mut engram = self.engram.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        engram
            .add(content, memory_type, Some(importance), Some("agent_tool"), None)
            .map_err(|e| anyhow::anyhow!("{}", e))?;
        Ok(())
    }

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

/// A recalled memory with metadata.
#[derive(Debug, Clone)]
pub struct RecalledMemory {
    pub content: String,
    pub memory_type: String,
    pub confidence: f64,
    pub source: Option<String>,
    /// Human-readable confidence label: "confident", "likely", "uncertain"
    pub confidence_label: Option<String>,
}
