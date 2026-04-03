//! Web dashboard for RustClaw agent monitoring and control.
//!
//! Provides a simple web interface with:
//! - Agent status and uptime
//! - Active sessions overview
//! - Task list (from orchestrator)
//! - Specialist agents status
//! - Configuration viewer (secrets redacted)
//! - Message injection API

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;

use axum::{
    extract::{Json, Path, State},
    http::{header, Method, StatusCode},
    middleware::{self, Next},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use tower::ServiceBuilder;
use tower_http::cors::{Any, CorsLayer};

use crate::agent::AgentRunner;
use crate::config::Config;

/// Dashboard configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardConfig {
    /// Whether dashboard is enabled
    #[serde(default)]
    pub enabled: bool,

    /// Port to listen on
    #[serde(default = "default_port")]
    pub port: u16,

    /// Optional bearer token for authentication
    pub auth_token: Option<String>,
}

fn default_port() -> u16 {
    8080
}

impl Default for DashboardConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            port: default_port(),
            auth_token: None,
        }
    }
}

/// Shared state for the dashboard.
pub struct DashboardState {
    /// Reference to the agent runner
    pub runner: Arc<AgentRunner>,

    /// Agent configuration (for display)
    pub config: Config,

    /// Server start time
    pub start_time: Instant,

    /// Dashboard config
    pub dashboard_config: DashboardConfig,
}

// ─── API Response Types ──────────────────────────────────────

#[derive(Serialize)]
struct StatusResponse {
    status: String,
    agent_name: Option<String>,
    uptime_seconds: u64,
    model: String,
    memory: MemoryStats,
    active_sessions: usize,
}

#[derive(Serialize)]
struct MemoryStats {
    engram_enabled: bool,
    embedding_status: String,
}

#[derive(Serialize)]
struct SessionInfo {
    key: String,
    message_count: usize,
    last_message: Option<String>,
    total_tokens: u64,
}

#[derive(Serialize)]
struct SessionsResponse {
    sessions: Vec<SessionInfo>,
    total: usize,
}

#[derive(Serialize)]
struct TaskInfo {
    id: String,
    status: String,
    description: String,
}

#[derive(Serialize)]
struct TasksResponse {
    tasks: Vec<TaskInfo>,
    total: usize,
}

#[derive(Serialize)]
struct AgentInfo {
    id: String,
    name: String,
    model: Option<String>,
    workspace: Option<String>,
    is_default: bool,
    status: String,
}

#[derive(Serialize)]
struct AgentsResponse {
    agents: Vec<AgentInfo>,
    total: usize,
}

#[derive(Serialize)]
struct ConfigResponse {
    workspace: Option<String>,
    llm: LlmConfigRedacted,
    channels: ChannelsConfigRedacted,
    memory: MemoryConfigInfo,
}

#[derive(Serialize)]
struct LlmConfigRedacted {
    provider: String,
    model: String,
    max_tokens: u32,
    temperature: f32,
}

#[derive(Serialize)]
struct ChannelsConfigRedacted {
    telegram_enabled: bool,
    discord_enabled: bool,
    slack_enabled: bool,
}

#[derive(Serialize)]
struct MemoryConfigInfo {
    engram_enabled: bool,
    auto_recall: bool,
    auto_store: bool,
    recall_limit: usize,
}

#[derive(Deserialize)]
struct MessageRequest {
    session_key: String,
    message: String,
    user_id: Option<String>,
}

#[derive(Serialize)]
struct MessageResponse {
    success: bool,
    response: Option<String>,
    error: Option<String>,
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

#[derive(Serialize)]
struct TokensResponse {
    total_input: u64,
    total_output: u64,
    total_requests: u64,
    total_cache_read: u64,
    total_cache_write: u64,
    /// Total tokens (input + output)
    total_tokens: u64,
    /// Tokens used in last hour (sliding window)
    hourly_tokens: u64,
    /// Requests in last hour
    hourly_requests: u64,
}

#[derive(Serialize)]
struct OrchestratorResponse {
    enabled: bool,
    specialists: Vec<SpecialistInfo>,
    pending_tasks: usize,
    completed_tasks: usize,
    failed_tasks: usize,
}

#[derive(Serialize)]
struct SpecialistInfo {
    id: String,
    name: String,
    role: String,
    status: String,
    budget_tokens: Option<u64>,
    budget_used: u64,
}

// ─── Auth Middleware ─────────────────────────────────────────

async fn auth_middleware(
    State(state): State<Arc<DashboardState>>,
    req: axum::extract::Request,
    next: Next,
) -> Response {
    // If no auth token configured, allow all requests
    let Some(ref expected_token) = state.dashboard_config.auth_token else {
        return next.run(req).await;
    };

    // Skip auth for root path (dashboard HTML)
    if req.uri().path() == "/" {
        return next.run(req).await;
    }

    // Check Authorization header
    let auth_header = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok());

    match auth_header {
        Some(header) if header.starts_with("Bearer ") => {
            let token = &header[7..];
            if token == expected_token {
                return next.run(req).await;
            }
        }
        _ => {}
    }

    // Auth failed
    (
        StatusCode::UNAUTHORIZED,
        Json(ErrorResponse {
            error: "Unauthorized: Bearer token required".to_string(),
        }),
    )
        .into_response()
}

// ─── API Handlers ────────────────────────────────────────────

async fn get_status(State(state): State<Arc<DashboardState>>) -> impl IntoResponse {
    let uptime = state.start_time.elapsed().as_secs();

    // Get session count from SessionManager
    let active_sessions = state.runner.sessions().count().await;

    // Get embedding status
    let embedding_status = state.runner.embedding_status();

    // Read agent name from IDENTITY.md
    let agent_name = read_agent_name(&state.config.workspace);

    Json(StatusResponse {
        status: "running".to_string(),
        agent_name,
        uptime_seconds: uptime,
        model: state.config.llm.model.clone(),
        memory: MemoryStats {
            engram_enabled: state.config.memory.auto_recall || state.config.memory.auto_store,
            embedding_status,
        },
        active_sessions,
    })
}

async fn get_sessions(State(state): State<Arc<DashboardState>>) -> impl IntoResponse {
    let sessions = state.runner.sessions().list_sessions().await;
    let total = sessions.len();
    let session_infos: Vec<SessionInfo> = sessions
        .into_iter()
        .map(|s| {
            let last_msg = s.messages.last().and_then(|m| {
                m.content.iter().find_map(|block| {
                    if let crate::llm::ContentBlock::Text { text } = block {
                        let end = text.len().min(100);
                        let end = text.floor_char_boundary(end);
                        Some(text[..end].to_string())
                    } else {
                        None
                    }
                })
            });
            SessionInfo {
                key: s.key,
                message_count: s.messages.len(),
                last_message: last_msg,
                total_tokens: s.total_tokens,
            }
        })
        .collect();
    Json(SessionsResponse {
        sessions: session_infos,
        total,
    })
}

async fn get_tasks(State(_state): State<Arc<DashboardState>>) -> impl IntoResponse {
    // Tasks are now available via /api/orchestrator endpoint
    Json(TasksResponse {
        tasks: vec![],
        total: 0,
    })
}

/// Get token usage statistics.
async fn get_tokens(State(_state): State<Arc<DashboardState>>) -> impl IntoResponse {
    let stats = crate::llm::token_tracker().snapshot();
    Json(TokensResponse {
        total_input: stats.total_input,
        total_output: stats.total_output,
        total_requests: stats.total_requests,
        total_cache_read: stats.total_cache_read,
        total_cache_write: stats.total_cache_write,
        total_tokens: stats.total_input + stats.total_output,
        hourly_tokens: stats.hourly_tokens,
        hourly_requests: stats.hourly_requests,
    })
}

/// Get orchestrator status.
async fn get_orchestrator(State(state): State<Arc<DashboardState>>) -> impl IntoResponse {
    if !state.config.orchestrator.enabled {
        return Json(OrchestratorResponse {
            enabled: false,
            specialists: vec![],
            pending_tasks: 0,
            completed_tasks: 0,
            failed_tasks: 0,
        });
    }

    // Get specialist info from config (current state)
    let specialists: Vec<SpecialistInfo> = state
        .config
        .orchestrator
        .specialists
        .iter()
        .map(|s| SpecialistInfo {
            id: s.id.clone(),
            name: s.name.clone().unwrap_or_else(|| s.id.clone()),
            role: s.role.clone(),
            status: "idle".to_string(), // Would be dynamic with real orchestrator state
            budget_tokens: s.budget_tokens,
            budget_used: 0,
        })
        .collect();

    Json(OrchestratorResponse {
        enabled: true,
        specialists,
        pending_tasks: 0,
        completed_tasks: 0,
        failed_tasks: 0,
    })
}

/// Read agent name from IDENTITY.md in workspace.
fn read_agent_name(workspace: &Option<String>) -> Option<String> {
    let workspace_dir = workspace.as_deref().unwrap_or(".");
    let identity_path = std::path::Path::new(workspace_dir).join("IDENTITY.md");
    
    if !identity_path.exists() {
        return None;
    }

    let content = std::fs::read_to_string(&identity_path).ok()?;
    
    // Look for "**Name:**" or "- **Name:**" pattern
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("- **Name:**") {
            return Some(rest.trim().to_string());
        }
        if let Some(rest) = trimmed.strip_prefix("**Name:**") {
            return Some(rest.trim().to_string());
        }
    }
    
    None
}

async fn get_agents(State(state): State<Arc<DashboardState>>) -> impl IntoResponse {
    // Get agents from config (defined agents)
    let mut agents: Vec<AgentInfo> = state
        .config
        .agents
        .iter()
        .map(|a| AgentInfo {
            id: a.id.clone(),
            name: a.name.clone().unwrap_or_else(|| a.id.clone()),
            model: a.model.clone(),
            workspace: a.workspace.clone(),
            is_default: a.default,
            status: "idle".to_string(),
        })
        .collect();

    // Also include orchestrator specialists if configured
    if state.config.orchestrator.enabled {
        for spec in &state.config.orchestrator.specialists {
            agents.push(AgentInfo {
                id: spec.id.clone(),
                name: spec.name.clone().unwrap_or_else(|| spec.id.clone()),
                model: spec.model.clone(),
                workspace: spec.workspace.clone(),
                is_default: false,
                status: format!("specialist:{}", spec.role),
            });
        }
    }

    let total = agents.len();
    Json(AgentsResponse { agents, total })
}

async fn get_config(State(state): State<Arc<DashboardState>>) -> impl IntoResponse {
    Json(ConfigResponse {
        workspace: state.config.workspace.clone(),
        llm: LlmConfigRedacted {
            provider: state.config.llm.provider.clone(),
            model: state.config.llm.model.clone(),
            max_tokens: state.config.llm.max_tokens.unwrap_or(0),
            temperature: state.config.llm.temperature,
        },
        channels: ChannelsConfigRedacted {
            telegram_enabled: state.config.channels.telegram.is_some(),
            discord_enabled: state.config.channels.discord.is_some(),
            slack_enabled: state.config.channels.slack.is_some(),
        },
        memory: MemoryConfigInfo {
            engram_enabled: true,
            auto_recall: state.config.memory.auto_recall,
            auto_store: state.config.memory.auto_store,
            recall_limit: state.config.memory.recall_limit,
        },
    })
}

async fn post_message(
    State(state): State<Arc<DashboardState>>,
    Json(req): Json<MessageRequest>,
) -> impl IntoResponse {
    match state
        .runner
        .process_message(&req.session_key, &req.message, req.user_id.as_deref(), Some("dashboard"))
        .await
    {
        Ok(response) => Json(MessageResponse {
            success: true,
            response: Some(response),
            error: None,
        }),
        Err(e) => Json(MessageResponse {
            success: false,
            response: None,
            error: Some(e.to_string()),
        }),
    }
}

async fn get_dashboard_html() -> impl IntoResponse {
    Html(DASHBOARD_HTML)
}

// ─── Dashboard HTML ──────────────────────────────────────────

const DASHBOARD_HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>RustClaw Dashboard</title>
    <script src="https://cdn.tailwindcss.com"></script>
    <script>
        tailwind.config = {
            darkMode: 'class',
            theme: {
                extend: {
                    colors: {
                        claw: {
                            50: '#fef2f2',
                            100: '#fee2e2',
                            500: '#ef4444',
                            600: '#dc2626',
                            700: '#b91c1c',
                        }
                    }
                }
            }
        }
    </script>
    <style>
        .status-dot {
            animation: pulse 2s infinite;
        }
        @keyframes pulse {
            0%, 100% { opacity: 1; }
            50% { opacity: 0.5; }
        }
    </style>
</head>
<body class="bg-gray-900 text-gray-100 min-h-screen">
    <div class="container mx-auto px-4 py-8">
        <!-- Header -->
        <div class="flex items-center justify-between mb-8">
            <div class="flex items-center gap-3">
                <span class="text-3xl">🦀</span>
                <h1 class="text-3xl font-bold text-claw-500">RustClaw Dashboard</h1>
            </div>
            <div id="status-indicator" class="flex items-center gap-2">
                <span class="status-dot w-3 h-3 bg-green-500 rounded-full"></span>
                <span id="status-text" class="text-sm text-gray-400">Connecting...</span>
            </div>
        </div>

        <!-- Stats Grid -->
        <div class="grid grid-cols-1 md:grid-cols-5 gap-4 mb-8">
            <div class="bg-gray-800 rounded-lg p-4 border border-gray-700">
                <div class="text-sm text-gray-400 mb-1">Uptime</div>
                <div id="uptime" class="text-2xl font-mono">--:--:--</div>
            </div>
            <div class="bg-gray-800 rounded-lg p-4 border border-gray-700">
                <div class="text-sm text-gray-400 mb-1">Model</div>
                <div id="model" class="text-lg font-mono truncate">Loading...</div>
            </div>
            <div class="bg-gray-800 rounded-lg p-4 border border-gray-700">
                <div class="text-sm text-gray-400 mb-1">Active Sessions</div>
                <div id="sessions-count" class="text-2xl font-mono">0</div>
            </div>
            <div class="bg-gray-800 rounded-lg p-4 border border-gray-700">
                <div class="text-sm text-gray-400 mb-1">Agents</div>
                <div id="agents-count" class="text-2xl font-mono">0</div>
            </div>
            <div class="bg-gray-800 rounded-lg p-4 border border-gray-700">
                <div class="text-sm text-gray-400 mb-1">Embedding</div>
                <div id="embedding-status" class="text-sm font-mono truncate">Loading...</div>
            </div>
        </div>

        <!-- Main Content -->
        <div class="grid grid-cols-1 lg:grid-cols-2 gap-6">
            <!-- Sessions Panel -->
            <div class="bg-gray-800 rounded-lg border border-gray-700">
                <div class="p-4 border-b border-gray-700">
                    <h2 class="text-lg font-semibold">Active Sessions</h2>
                </div>
                <div id="sessions-list" class="p-4 space-y-2 max-h-64 overflow-y-auto">
                    <div class="text-gray-500 text-sm">No active sessions</div>
                </div>
            </div>

            <!-- Agents Panel -->
            <div class="bg-gray-800 rounded-lg border border-gray-700">
                <div class="p-4 border-b border-gray-700">
                    <h2 class="text-lg font-semibold">Specialist Agents</h2>
                </div>
                <div id="agents-list" class="p-4 space-y-2 max-h-64 overflow-y-auto">
                    <div class="text-gray-500 text-sm">No agents configured</div>
                </div>
            </div>
        </div>

        <!-- Message Injection -->
        <div class="mt-6 bg-gray-800 rounded-lg border border-gray-700">
            <div class="p-4 border-b border-gray-700">
                <h2 class="text-lg font-semibold">Inject Message</h2>
            </div>
            <div class="p-4">
                <div class="flex gap-4 mb-4">
                    <input type="text" id="session-input" placeholder="Session key (e.g., telegram:123)" 
                           class="flex-1 bg-gray-700 border border-gray-600 rounded px-3 py-2 text-sm focus:outline-none focus:border-claw-500">
                </div>
                <div class="flex gap-4">
                    <textarea id="message-input" placeholder="Message to inject..." rows="2"
                              class="flex-1 bg-gray-700 border border-gray-600 rounded px-3 py-2 text-sm focus:outline-none focus:border-claw-500 resize-none"></textarea>
                    <button onclick="sendMessage()" 
                            class="bg-claw-600 hover:bg-claw-700 px-4 py-2 rounded font-medium transition-colors">
                        Send
                    </button>
                </div>
                <div id="message-result" class="mt-4 text-sm hidden"></div>
            </div>
        </div>

        <!-- Config Panel -->
        <div class="mt-6 bg-gray-800 rounded-lg border border-gray-700">
            <div class="p-4 border-b border-gray-700 flex items-center justify-between">
                <h2 class="text-lg font-semibold">Configuration</h2>
                <button onclick="toggleConfig()" class="text-sm text-gray-400 hover:text-white">
                    Toggle
                </button>
            </div>
            <div id="config-panel" class="p-4 hidden">
                <pre id="config-content" class="text-xs bg-gray-900 p-4 rounded overflow-x-auto"></pre>
            </div>
        </div>

        <!-- Footer -->
        <div class="mt-8 text-center text-sm text-gray-500">
            RustClaw — Rust-native AI Agent Framework
        </div>
    </div>

    <script>
        let authToken = localStorage.getItem('rustclaw_token') || '';

        async function fetchApi(endpoint, options = {}) {
            const headers = { 'Content-Type': 'application/json' };
            if (authToken) {
                headers['Authorization'] = `Bearer ${authToken}`;
            }
            const res = await fetch(`/api${endpoint}`, { ...options, headers });
            if (res.status === 401) {
                authToken = prompt('Enter auth token:');
                localStorage.setItem('rustclaw_token', authToken);
                return fetchApi(endpoint, options);
            }
            return res.json();
        }

        function formatUptime(seconds) {
            const h = Math.floor(seconds / 3600);
            const m = Math.floor((seconds % 3600) / 60);
            const s = seconds % 60;
            return `${h.toString().padStart(2, '0')}:${m.toString().padStart(2, '0')}:${s.toString().padStart(2, '0')}`;
        }

        async function updateStatus() {
            try {
                const data = await fetchApi('/status');
                document.getElementById('status-text').textContent = data.status;
                document.getElementById('uptime').textContent = formatUptime(data.uptime_seconds);
                document.getElementById('model').textContent = data.model;
                document.getElementById('sessions-count').textContent = data.active_sessions;
                if (data.memory && data.memory.embedding_status) {
                    const embeddingEl = document.getElementById('embedding-status');
                    embeddingEl.textContent = data.memory.embedding_status;
                    // Color based on status
                    if (data.memory.embedding_status.includes('✓')) {
                        embeddingEl.className = 'text-sm font-mono truncate text-green-400';
                    } else {
                        embeddingEl.className = 'text-sm font-mono truncate text-yellow-400';
                    }
                }
            } catch (e) {
                document.getElementById('status-text').textContent = 'Error';
                document.querySelector('.status-dot').className = 'status-dot w-3 h-3 bg-red-500 rounded-full';
            }
        }

        async function updateSessions() {
            try {
                const data = await fetchApi('/sessions');
                const list = document.getElementById('sessions-list');
                if (data.sessions.length === 0) {
                    list.innerHTML = '<div class="text-gray-500 text-sm">No active sessions</div>';
                } else {
                    list.innerHTML = data.sessions.map(s => `
                        <div class="bg-gray-700 rounded p-2 text-sm">
                            <div class="font-mono text-claw-500">${s.key}</div>
                            <div class="text-gray-400">${s.message_count} messages • ${s.total_tokens} tokens</div>
                        </div>
                    `).join('');
                }
            } catch (e) {
                console.error('Failed to fetch sessions:', e);
            }
        }

        async function updateAgents() {
            try {
                const data = await fetchApi('/agents');
                document.getElementById('agents-count').textContent = data.total;
                const list = document.getElementById('agents-list');
                if (data.agents.length === 0) {
                    list.innerHTML = '<div class="text-gray-500 text-sm">No agents configured</div>';
                } else {
                    list.innerHTML = data.agents.map(a => `
                        <div class="bg-gray-700 rounded p-2 text-sm flex items-center justify-between">
                            <div>
                                <span class="font-medium">${a.name}</span>
                                ${a.is_default ? '<span class="ml-2 text-xs bg-claw-600 px-1 rounded">default</span>' : ''}
                            </div>
                            <span class="text-gray-400 text-xs">${a.status}</span>
                        </div>
                    `).join('');
                }
            } catch (e) {
                console.error('Failed to fetch agents:', e);
            }
        }

        async function loadConfig() {
            try {
                const data = await fetchApi('/config');
                document.getElementById('config-content').textContent = JSON.stringify(data, null, 2);
            } catch (e) {
                document.getElementById('config-content').textContent = 'Failed to load config';
            }
        }

        function toggleConfig() {
            const panel = document.getElementById('config-panel');
            panel.classList.toggle('hidden');
        }

        async function sendMessage() {
            const sessionKey = document.getElementById('session-input').value.trim();
            const message = document.getElementById('message-input').value.trim();
            const resultDiv = document.getElementById('message-result');

            if (!sessionKey || !message) {
                resultDiv.className = 'mt-4 text-sm text-yellow-400';
                resultDiv.textContent = 'Please enter both session key and message';
                resultDiv.classList.remove('hidden');
                return;
            }

            try {
                const data = await fetchApi('/message', {
                    method: 'POST',
                    body: JSON.stringify({ session_key: sessionKey, message })
                });

                if (data.success) {
                    resultDiv.className = 'mt-4 text-sm text-green-400';
                    resultDiv.textContent = 'Response: ' + (data.response || '(empty)');
                } else {
                    resultDiv.className = 'mt-4 text-sm text-red-400';
                    resultDiv.textContent = 'Error: ' + data.error;
                }
            } catch (e) {
                resultDiv.className = 'mt-4 text-sm text-red-400';
                resultDiv.textContent = 'Request failed: ' + e.message;
            }
            resultDiv.classList.remove('hidden');
        }

        // Initial load
        updateStatus();
        updateSessions();
        updateAgents();
        loadConfig();

        // Refresh every 5 seconds
        setInterval(updateStatus, 5000);
        setInterval(updateSessions, 10000);
        setInterval(updateAgents, 10000);
    </script>
</body>
</html>
"#;

// ─── Server ──────────────────────────────────────────────────

/// Start the dashboard server.
pub async fn start_dashboard(
    config: DashboardConfig,
    agent_config: Config,
    runner: Arc<AgentRunner>,
) -> anyhow::Result<()> {
    if !config.enabled {
        tracing::info!("Dashboard disabled");
        return Ok(());
    }

    let state = Arc::new(DashboardState {
        runner,
        config: agent_config,
        start_time: Instant::now(),
        dashboard_config: config.clone(),
    });

    // Build router
    let api_routes = Router::new()
        .route("/status", get(get_status))
        .route("/sessions", get(get_sessions))
        .route("/tasks", get(get_tasks))
        .route("/agents", get(get_agents))
        .route("/config", get(get_config))
        .route("/message", post(post_message))
        .route("/tokens", get(get_tokens))
        .route("/orchestrator", get(get_orchestrator));

    let app = Router::new()
        .route("/", get(get_dashboard_html))
        .nest("/api", api_routes)
        .layer(
            ServiceBuilder::new()
                .layer(CorsLayer::new().allow_origin(Any).allow_methods([Method::GET, Method::POST]))
                .layer(middleware::from_fn_with_state(state.clone(), auth_middleware)),
        )
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], config.port));
    tracing::info!("Dashboard listening on http://{}", addr);

    // Run server in background
    tokio::spawn(async move {
        let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
        axum::serve(listener, app).await.unwrap();
    });

    Ok(())
}
