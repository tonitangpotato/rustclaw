//! Memory management with native Engram integration.
//!
//! Unlike OpenClaw (MCP overhead) or Hermes (FTS only),
//! RustClaw uses engramai as a direct Rust dependency — zero IPC overhead.

use engramai::{Memory, MemoryConfig, MemoryType};
use std::sync::Mutex;

use crate::config::Config;

/// Memory manager wrapping Engram with RustClaw-specific logic.
/// Uses Mutex instead of async RwLock because rusqlite isn't Send+Sync.
pub struct MemoryManager {
    engram: Mutex<Memory>,
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
        let engram = Memory::new(&db_path, Some(engram_config))
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        Ok(Self {
            engram: Mutex::new(engram),
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
            })
            .collect())
    }

    /// Store important information from a conversation turn.
    /// Called by BeforeOutbound hook.
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

        let mut engram = self.engram.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        engram
            .add(content, memory_type, Some(importance), source, None)
            .map_err(|e| anyhow::anyhow!("{}", e))?;
        Ok(())
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
    pub fn format_for_prompt(memories: &[RecalledMemory]) -> String {
        if memories.is_empty() {
            return String::new();
        }

        let mut lines = vec!["[Relevant memories from Engram]:".to_string()];
        for mem in memories {
            lines.push(format!(
                "- {} (confidence: {:.2})",
                mem.content, mem.confidence
            ));
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
}
