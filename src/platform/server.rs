//! Platform HTTP API — registration, login, profile, instance management, call history.

use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{Html, IntoResponse, Response},
    routing::{delete, get, post, put},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use tower_http::cors::{Any, CorsLayer};

use crate::platform::auth::AuthUser;
use crate::platform::db::{PlatformDb, UserProfile};
use crate::platform::instance::InstanceManager;
use crate::platform::auth::AuthService;

// ─── Shared State ───────────────────────────────────────────

pub struct PlatformState {
    pub db: Arc<PlatformDb>,
    pub auth: AuthService,
    pub instances: Arc<InstanceManager>,
}

// ─── Request / Response Types ───────────────────────────────

#[derive(Deserialize)]
struct RegisterRequest {
    email: String,
    password: String,
}

#[derive(Deserialize)]
struct LoginRequest {
    email: String,
    password: String,
}

#[derive(Serialize)]
struct AuthResponse {
    token: String,
    user_id: i64,
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

#[derive(Deserialize)]
struct CreateInstanceRequest {
    channel_type: String,
    bot_token: String,
}

#[derive(Serialize)]
struct InstanceResponse {
    id: i64,
    channel_type: String,
    status: String,
    created_at: String,
}

#[derive(Serialize)]
struct MessageResponse {
    message: String,
}

// ─── Router ─────────────────────────────────────────────────

pub fn create_router(state: Arc<PlatformState>) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        // Public routes
        .route("/api/auth/register", post(register))
        .route("/api/auth/login", post(login))
        // Protected routes
        .route("/api/user/profile", get(get_profile).put(update_profile))
        .route("/api/instance", post(create_instance).get(get_instance).delete(delete_instance))
        .route("/api/calls", get(list_calls))
        .route("/api/calls/{id}", get(get_call))
        // Frontend
        .fallback(serve_frontend)
        .layer(cors)
        .with_state(state)
}

// ─── Helpers ────────────────────────────────────────────────

fn json_error(status: StatusCode, msg: &str) -> Response {
    (status, Json(ErrorResponse { error: msg.to_string() })).into_response()
}

fn json_ok<T: Serialize>(data: T) -> Response {
    (StatusCode::OK, Json(data)).into_response()
}

// ─── Auth Routes ────────────────────────────────────────────

async fn register(
    State(state): State<Arc<PlatformState>>,
    Json(req): Json<RegisterRequest>,
) -> Response {
    // Validate input.
    let email = req.email.trim().to_lowercase();
    if email.is_empty() || !email.contains('@') {
        return json_error(StatusCode::BAD_REQUEST, "Invalid email address");
    }
    if req.password.len() < 8 {
        return json_error(
            StatusCode::BAD_REQUEST,
            "Password must be at least 8 characters",
        );
    }

    // Check if email already exists.
    match state.db.get_user_by_email(&email).await {
        Ok(Some(_)) => {
            return json_error(StatusCode::CONFLICT, "Email already registered");
        }
        Ok(None) => {}
        Err(e) => {
            tracing::error!("DB error checking email: {}", e);
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error");
        }
    }

    // Hash password.
    let password_hash = match state.auth.hash_password(&req.password) {
        Ok(h) => h,
        Err(e) => {
            tracing::error!("Password hash error: {}", e);
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error");
        }
    };

    // Create user.
    let user_id = match state.db.create_user(&email, &password_hash).await {
        Ok(id) => id,
        Err(e) => {
            tracing::error!("DB error creating user: {}", e);
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, "Failed to create user");
        }
    };

    // Issue JWT.
    let token = match state.auth.create_token(user_id, &email) {
        Ok(t) => t,
        Err(e) => {
            tracing::error!("Token creation error: {}", e);
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error");
        }
    };

    tracing::info!("New user registered: {} (id: {})", email, user_id);
    (StatusCode::CREATED, Json(AuthResponse { token, user_id })).into_response()
}

async fn login(
    State(state): State<Arc<PlatformState>>,
    Json(req): Json<LoginRequest>,
) -> Response {
    let email = req.email.trim().to_lowercase();

    // Look up user.
    let user = match state.db.get_user_by_email(&email).await {
        Ok(Some(u)) => u,
        Ok(None) => {
            return json_error(StatusCode::UNAUTHORIZED, "Invalid email or password");
        }
        Err(e) => {
            tracing::error!("DB error during login: {}", e);
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error");
        }
    };

    // Verify password.
    match state.auth.verify_password(&req.password, &user.password_hash) {
        Ok(true) => {}
        Ok(false) => {
            return json_error(StatusCode::UNAUTHORIZED, "Invalid email or password");
        }
        Err(e) => {
            tracing::error!("Password verification error: {}", e);
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error");
        }
    }

    // Issue JWT.
    let token = match state.auth.create_token(user.id, &email) {
        Ok(t) => t,
        Err(e) => {
            tracing::error!("Token creation error: {}", e);
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error");
        }
    };

    tracing::info!("User logged in: {} (id: {})", email, user.id);
    json_ok(AuthResponse {
        token,
        user_id: user.id,
    })
}

// ─── Profile Routes ─────────────────────────────────────────

async fn get_profile(
    State(state): State<Arc<PlatformState>>,
    auth: AuthUser,
) -> Response {
    match state.db.get_profile(auth.0.sub).await {
        Ok(Some(profile)) => json_ok(profile),
        Ok(None) => {
            // Return an empty profile skeleton.
            json_ok(UserProfile {
                user_id: auth.0.sub,
                full_name: None,
                phone: None,
                address: None,
                timezone: None,
                contacts: vec![],
                insurance: None,
            })
        }
        Err(e) => {
            tracing::error!("DB error fetching profile: {}", e);
            json_error(StatusCode::INTERNAL_SERVER_ERROR, "Failed to fetch profile")
        }
    }
}

async fn update_profile(
    State(state): State<Arc<PlatformState>>,
    auth: AuthUser,
    Json(mut profile): Json<UserProfile>,
) -> Response {
    // Force user_id from token (don't trust client).
    profile.user_id = auth.0.sub;

    match state.db.upsert_profile(auth.0.sub, &profile).await {
        Ok(()) => json_ok(MessageResponse {
            message: "Profile updated".into(),
        }),
        Err(e) => {
            tracing::error!("DB error updating profile: {}", e);
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to update profile",
            )
        }
    }
}

// ─── Instance Routes ────────────────────────────────────────

async fn create_instance(
    State(state): State<Arc<PlatformState>>,
    auth: AuthUser,
    Json(req): Json<CreateInstanceRequest>,
) -> Response {
    let user_id = auth.0.sub;

    // Only allow one instance per user for MVP.
    if let Ok(Some(existing)) = state.db.get_instance(user_id).await {
        if existing.status == "active" {
            return json_error(
                StatusCode::CONFLICT,
                "You already have an active instance. Delete it first.",
            );
        }
    }

    // Validate bot token by starting the instance.
    if let Err(e) = state
        .instances
        .start_instance(user_id, &req.bot_token)
        .await
    {
        return json_error(
            StatusCode::BAD_REQUEST,
            &format!("Failed to start bot: {}", e),
        );
    }

    // Persist to DB.
    let instance_id = match state
        .db
        .create_instance(user_id, &req.channel_type, &req.bot_token)
        .await
    {
        Ok(id) => id,
        Err(e) => {
            // Roll back the runtime instance.
            state.instances.stop_instance(user_id).await.ok();
            tracing::error!("DB error creating instance: {}", e);
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to save instance",
            );
        }
    };

    tracing::info!(
        "Instance created for user {} (id: {}, type: {})",
        user_id,
        instance_id,
        req.channel_type
    );

    (
        StatusCode::CREATED,
        Json(InstanceResponse {
            id: instance_id,
            channel_type: req.channel_type,
            status: "active".into(),
            created_at: chrono::Utc::now().to_rfc3339(),
        }),
    )
        .into_response()
}

async fn get_instance(
    State(state): State<Arc<PlatformState>>,
    auth: AuthUser,
) -> Response {
    match state.db.get_instance(auth.0.sub).await {
        Ok(Some(inst)) => {
            // Also check runtime status.
            let runtime_status = state
                .instances
                .get_status(auth.0.sub)
                .await
                .unwrap_or_else(|| inst.status.clone());

            json_ok(InstanceResponse {
                id: inst.id,
                channel_type: inst.channel_type,
                status: runtime_status,
                created_at: inst.created_at,
            })
        }
        Ok(None) => json_error(StatusCode::NOT_FOUND, "No instance found"),
        Err(e) => {
            tracing::error!("DB error fetching instance: {}", e);
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to fetch instance",
            )
        }
    }
}

async fn delete_instance(
    State(state): State<Arc<PlatformState>>,
    auth: AuthUser,
) -> Response {
    let user_id = auth.0.sub;

    // Stop the runtime instance.
    state.instances.stop_instance(user_id).await.ok();

    // Remove from DB.
    match state.db.delete_instance(user_id).await {
        Ok(()) => json_ok(MessageResponse {
            message: "Instance deleted".into(),
        }),
        Err(e) => {
            tracing::error!("DB error deleting instance: {}", e);
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to delete instance",
            )
        }
    }
}

// ─── Call Routes ────────────────────────────────────────────

async fn list_calls(
    State(state): State<Arc<PlatformState>>,
    auth: AuthUser,
) -> Response {
    match state.db.list_calls(auth.0.sub, 50).await {
        Ok(calls) => json_ok(calls),
        Err(e) => {
            tracing::error!("DB error listing calls: {}", e);
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to list calls",
            )
        }
    }
}

async fn get_call(
    State(state): State<Arc<PlatformState>>,
    auth: AuthUser,
    Path(id): Path<i64>,
) -> Response {
    match state.db.get_call(id).await {
        Ok(Some(call)) => {
            // Ensure the call belongs to the authenticated user.
            if call.user_id != auth.0.sub {
                return json_error(StatusCode::FORBIDDEN, "Access denied");
            }
            json_ok(call)
        }
        Ok(None) => json_error(StatusCode::NOT_FOUND, "Call not found"),
        Err(e) => {
            tracing::error!("DB error fetching call: {}", e);
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to fetch call",
            )
        }
    }
}

// ─── Frontend ───────────────────────────────────────────────

async fn serve_frontend() -> impl IntoResponse {
    Html(include_str!("../../static/platform/index.html"))
}
