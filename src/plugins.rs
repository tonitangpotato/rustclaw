//! Plugin system for extending RustClaw.
//!
//! Plugins are Rust trait objects that can:
//! - Register additional tools
//! - Add lifecycle hooks
//! - Execute code on load/unload
//!
//! Plugins are loaded at startup and unloaded on shutdown.

use std::sync::Arc;

use async_trait::async_trait;

use crate::config::Config;
use crate::hooks::Hook;
use crate::memory::MemoryManager;
use crate::tools::Tool;

/// Context passed to plugins during initialization.
pub struct PluginContext {
    /// Workspace directory path.
    pub workspace_dir: String,
    /// Reference to the config.
    pub config: Arc<Config>,
    /// Reference to the memory manager.
    pub memory: Arc<MemoryManager>,
}

impl PluginContext {
    /// Create a new plugin context.
    pub fn new(
        workspace_dir: impl Into<String>,
        config: Arc<Config>,
        memory: Arc<MemoryManager>,
    ) -> Self {
        Self {
            workspace_dir: workspace_dir.into(),
            config,
            memory,
        }
    }
}

/// Trait for implementing plugins.
///
/// Plugins extend RustClaw by:
/// - Registering tools that the LLM can call
/// - Adding hooks to intercept agent lifecycle events
/// - Running initialization code at startup
///
/// # Example
///
/// ```ignore
/// pub struct MyPlugin;
///
/// #[async_trait]
/// impl Plugin for MyPlugin {
///     fn name(&self) -> &str { "my-plugin" }
///     fn version(&self) -> &str { "0.1.0" }
///
///     async fn on_load(&self, ctx: &PluginContext) -> anyhow::Result<()> {
///         tracing::info!("MyPlugin loaded!");
///         Ok(())
///     }
///
///     fn tools(&self) -> Vec<Box<dyn Tool>> {
///         vec![Box::new(MyCustomTool)]
///     }
/// }
/// ```
#[async_trait]
pub trait Plugin: Send + Sync {
    /// Plugin name (unique identifier).
    fn name(&self) -> &str;

    /// Plugin version string.
    fn version(&self) -> &str;

    /// Called once at startup when the plugin is loaded.
    ///
    /// Use this for initialization: setting up connections,
    /// loading data, etc.
    async fn on_load(&self, ctx: &PluginContext) -> anyhow::Result<()>;

    /// Called on shutdown when the plugin is unloaded.
    ///
    /// Use this for cleanup: closing connections, flushing data, etc.
    /// Default implementation does nothing.
    async fn on_unload(&self) -> anyhow::Result<()> {
        Ok(())
    }

    /// Register tools this plugin provides.
    ///
    /// These tools will be made available to the LLM.
    /// Default returns an empty list.
    fn tools(&self) -> Vec<Box<dyn Tool>> {
        vec![]
    }

    /// Register hooks this plugin provides.
    ///
    /// These hooks will be called during agent lifecycle events.
    /// Default returns an empty list.
    fn hooks(&self) -> Vec<Box<dyn Hook>> {
        vec![]
    }
}

/// Registry that manages all loaded plugins.
pub struct PluginRegistry {
    /// Loaded plugins.
    plugins: Vec<Box<dyn Plugin>>,
    /// Whether the registry has been initialized.
    initialized: bool,
}

impl PluginRegistry {
    /// Create a new empty plugin registry.
    pub fn new() -> Self {
        Self {
            plugins: Vec::new(),
            initialized: false,
        }
    }

    /// Register a plugin.
    ///
    /// Plugins must be registered before `load_all` is called.
    pub fn register(&mut self, plugin: Box<dyn Plugin>) {
        tracing::info!(
            "Registering plugin: {} v{}",
            plugin.name(),
            plugin.version()
        );
        self.plugins.push(plugin);
    }

    /// Load all registered plugins.
    ///
    /// Calls `on_load` for each plugin in registration order.
    /// Returns an error if any plugin fails to load.
    pub async fn load_all(&mut self, ctx: &PluginContext) -> anyhow::Result<()> {
        if self.initialized {
            anyhow::bail!("Plugins already initialized");
        }

        tracing::info!("Loading {} plugin(s)...", self.plugins.len());

        for plugin in &self.plugins {
            tracing::info!(
                "Loading plugin: {} v{}",
                plugin.name(),
                plugin.version()
            );
            plugin.on_load(ctx).await.map_err(|e| {
                anyhow::anyhow!("Failed to load plugin '{}': {}", plugin.name(), e)
            })?;
        }

        self.initialized = true;
        tracing::info!("All plugins loaded successfully");
        Ok(())
    }

    /// Unload all plugins.
    ///
    /// Calls `on_unload` for each plugin in reverse order.
    /// Continues even if a plugin fails to unload (logs error).
    pub async fn unload_all(&mut self) {
        if !self.initialized {
            return;
        }

        tracing::info!("Unloading {} plugin(s)...", self.plugins.len());

        // Unload in reverse order
        for plugin in self.plugins.iter().rev() {
            tracing::info!("Unloading plugin: {}", plugin.name());
            if let Err(e) = plugin.on_unload().await {
                tracing::error!(
                    "Error unloading plugin '{}': {}",
                    plugin.name(),
                    e
                );
            }
        }

        self.initialized = false;
        tracing::info!("All plugins unloaded");
    }

    /// Get all tools from all plugins.
    pub fn collect_tools(&self) -> Vec<Box<dyn Tool>> {
        let mut tools = Vec::new();
        for plugin in &self.plugins {
            let plugin_tools = plugin.tools();
            if !plugin_tools.is_empty() {
                tracing::debug!(
                    "Plugin '{}' provides {} tool(s)",
                    plugin.name(),
                    plugin_tools.len()
                );
            }
            tools.extend(plugin_tools);
        }
        tools
    }

    /// Get all hooks from all plugins.
    pub fn collect_hooks(&self) -> Vec<Box<dyn Hook>> {
        let mut hooks = Vec::new();
        for plugin in &self.plugins {
            let plugin_hooks = plugin.hooks();
            if !plugin_hooks.is_empty() {
                tracing::debug!(
                    "Plugin '{}' provides {} hook(s)",
                    plugin.name(),
                    plugin_hooks.len()
                );
            }
            hooks.extend(plugin_hooks);
        }
        hooks
    }

    /// Number of registered plugins.
    pub fn count(&self) -> usize {
        self.plugins.len()
    }

    /// Check if plugins have been loaded.
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }

    /// Get plugin names.
    pub fn plugin_names(&self) -> Vec<&str> {
        self.plugins.iter().map(|p| p.name()).collect()
    }
}

impl Default for PluginRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Shared plugin registry handle.
pub type SharedPluginRegistry = Arc<tokio::sync::RwLock<PluginRegistry>>;

/// Create a shared plugin registry.
pub fn create_plugin_registry() -> SharedPluginRegistry {
    Arc::new(tokio::sync::RwLock::new(PluginRegistry::new()))
}

// ─── Built-in Example Plugin ─────────────────────────────────

/// Example plugin that demonstrates the plugin system.
///
/// This plugin does nothing but can be used as a template.
pub struct ExamplePlugin;

#[async_trait]
impl Plugin for ExamplePlugin {
    fn name(&self) -> &str {
        "example"
    }

    fn version(&self) -> &str {
        "0.1.0"
    }

    async fn on_load(&self, _ctx: &PluginContext) -> anyhow::Result<()> {
        tracing::info!("Example plugin loaded!");
        Ok(())
    }

    async fn on_unload(&self) -> anyhow::Result<()> {
        tracing::info!("Example plugin unloaded!");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestPlugin {
        name: String,
    }

    #[async_trait]
    impl Plugin for TestPlugin {
        fn name(&self) -> &str {
            &self.name
        }

        fn version(&self) -> &str {
            "1.0.0"
        }

        async fn on_load(&self, _ctx: &PluginContext) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn test_registry_registration() {
        let mut registry = PluginRegistry::new();
        registry.register(Box::new(TestPlugin {
            name: "test1".into(),
        }));
        registry.register(Box::new(TestPlugin {
            name: "test2".into(),
        }));

        assert_eq!(registry.count(), 2);
        assert_eq!(registry.plugin_names(), vec!["test1", "test2"]);
    }
}
