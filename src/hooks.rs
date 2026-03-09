//! Lifecycle hook system (inspired by IronClaw).
//!
//! 6 hook points for intercepting agent operations:
//! - BeforeInbound — before processing user message (→ Engram recall)
//! - BeforeToolCall — before executing a tool
//! - BeforeOutbound — before sending response (→ Engram store)
//! - OnSessionStart — when a new session starts
//! - OnSessionEnd — when a session ends
//! - TransformResponse — transform final response

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Points in the agent lifecycle where hooks can be attached.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HookPoint {
    BeforeInbound,
    BeforeToolCall,
    BeforeOutbound,
    OnSessionStart,
    OnSessionEnd,
    TransformResponse,
}

/// Outcome of a hook execution.
#[derive(Debug)]
pub enum HookOutcome {
    /// Continue processing (optionally with modified content).
    Continue(Option<String>),
    /// Skip this event entirely.
    Skip,
    /// Reject with an error message.
    Reject(String),
}

/// Context passed to hooks.
#[derive(Debug, Clone)]
pub struct HookContext {
    pub session_key: String,
    pub user_id: Option<String>,
    pub channel: Option<String>,
    pub content: String,
    pub metadata: serde_json::Value,
}

/// Trait for implementing hooks.
#[async_trait]
pub trait Hook: Send + Sync {
    /// Human-readable name for this hook.
    fn name(&self) -> &str;

    /// Which hook point this attaches to.
    fn point(&self) -> HookPoint;

    /// Priority (lower = runs first).
    fn priority(&self) -> i32 {
        100
    }

    /// Execute the hook.
    async fn execute(&self, ctx: &HookContext) -> anyhow::Result<HookOutcome>;
}

/// Registry that manages all hooks.
pub struct HookRegistry {
    hooks: Vec<Box<dyn Hook>>,
}

impl HookRegistry {
    pub fn new() -> Self {
        Self { hooks: Vec::new() }
    }

    /// Register a new hook.
    pub fn register(&mut self, hook: Box<dyn Hook>) {
        tracing::info!("Registered hook: {} at {:?}", hook.name(), hook.point());
        self.hooks.push(hook);
        // Sort by priority
        self.hooks.sort_by_key(|h| h.priority());
    }

    /// Number of registered hooks.
    pub fn count(&self) -> usize {
        self.hooks.len()
    }

    /// Run all hooks for a given point.
    pub async fn run(
        &self,
        point: HookPoint,
        ctx: &mut HookContext,
    ) -> anyhow::Result<HookOutcome> {
        for hook in self.hooks.iter().filter(|h| h.point() == point) {
            match hook.execute(ctx).await? {
                HookOutcome::Continue(Some(modified)) => {
                    ctx.content = modified;
                }
                HookOutcome::Continue(None) => {}
                HookOutcome::Skip => return Ok(HookOutcome::Skip),
                HookOutcome::Reject(reason) => return Ok(HookOutcome::Reject(reason)),
            }
        }
        Ok(HookOutcome::Continue(None))
    }
}
