//! Platform Layer — multi-tenant hosting for AI Phone Agent.
//!
//! Users sign up, configure a Telegram bot, and get a hosted AI assistant
//! that can make phone calls on their behalf.

pub mod auth;
pub mod db;
pub mod instance;
pub mod server;

use std::sync::Arc;

use anyhow::Result;
use tracing;

use crate::platform::auth::AuthService;
use crate::platform::db::PlatformDb;
use crate::platform::instance::InstanceManager;
use crate::platform::server::PlatformState;

/// Platform configuration.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PlatformConfig {
    /// Whether the platform server is enabled.
    #[serde(default)]
    pub enabled: bool,

    /// Port to listen on (default: 8082).
    #[serde(default = "default_platform_port")]
    pub port: u16,

    /// SQLite database path (default: "platform.db").
    #[serde(default = "default_db_path")]
    pub db_path: String,

    /// JWT secret for auth tokens. Should be set in config or PLATFORM_JWT_SECRET env var.
    #[serde(default = "default_jwt_secret")]
    pub jwt_secret: String,
}

fn default_platform_port() -> u16 {
    8082
}

fn default_db_path() -> String {
    "platform.db".to_string()
}

fn default_jwt_secret() -> String {
    std::env::var("PLATFORM_JWT_SECRET").unwrap_or_else(|_| "change-me-in-production".to_string())
}

impl Default for PlatformConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            port: default_platform_port(),
            db_path: default_db_path(),
            jwt_secret: default_jwt_secret(),
        }
    }
}

/// Top-level platform service. Owns DB, auth, instance manager, and HTTP server.
pub struct PlatformService {
    pub state: Arc<PlatformState>,
    pub config: PlatformConfig,
}

impl PlatformService {
    /// Initialize the platform: open DB, run migrations, create services.
    pub async fn new(config: PlatformConfig) -> Result<Self> {
        let db = Arc::new(PlatformDb::new(&config.db_path).await?);
        let auth = AuthService::new(&config.jwt_secret);
        let instances = Arc::new(InstanceManager::new(Arc::clone(&db)));

        let state = Arc::new(PlatformState {
            db,
            auth,
            instances,
        });

        Ok(Self { state, config })
    }

    /// Start the HTTP server and restore active instances.
    pub async fn start(self) -> Result<()> {
        if !self.config.enabled {
            tracing::info!("Platform disabled");
            return Ok(());
        }

        // Restore previously-active instances from DB
        if let Err(e) = self.state.instances.restart_all().await {
            tracing::warn!("Failed to restart some instances on boot: {}", e);
        }

        let addr = std::net::SocketAddr::from(([0, 0, 0, 0], self.config.port));
        let router = server::create_router(Arc::clone(&self.state));

        tokio::spawn(async move {
            let listener = match tokio::net::TcpListener::bind(addr).await {
                Ok(l) => l,
                Err(e) => {
                    tracing::warn!("Platform server failed to bind {}: {} — running without platform server", addr, e);
                    return;
                }
            };
            tracing::info!("Platform server listening on http://{}", addr);
            if let Err(e) = axum::serve(listener, router).await {
                tracing::warn!("Platform server error: {}", e);
            }
        });

        Ok(())
    }
}
