//! Authentication — argon2 password hashing, JWT tokens, Axum extractor.

use anyhow::{anyhow, Result};
use argon2::{
    password_hash::SaltString, Argon2, PasswordHash, PasswordHasher, PasswordVerifier,
};
use axum::{
    extract::FromRequestParts,
    http::{request::Parts, StatusCode},
};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

// ─── Global JWT secret ──────────────────────────────────────
// Set once at startup via `init_jwt_secret`, read by the AuthUser extractor.

static JWT_SECRET: OnceLock<String> = OnceLock::new();

/// Must be called once before using `AuthUser` extractor.
pub fn init_jwt_secret(secret: &str) {
    let _ = JWT_SECRET.set(secret.to_string());
}

fn get_jwt_secret() -> &'static str {
    JWT_SECRET
        .get()
        .map(|s| s.as_str())
        .unwrap_or("change-me-in-production")
}

// ─── JWT Claims ─────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    /// User ID.
    pub sub: i64,
    /// User email.
    pub email: String,
    /// Expiration (unix timestamp).
    pub exp: usize,
}

// ─── Auth Service ───────────────────────────────────────────

pub struct AuthService {
    jwt_secret: String,
}

impl AuthService {
    pub fn new(jwt_secret: &str) -> Self {
        // Also set the global so the extractor can see it.
        init_jwt_secret(jwt_secret);
        Self {
            jwt_secret: jwt_secret.to_string(),
        }
    }

    /// Hash a plaintext password with argon2id.
    pub fn hash_password(&self, password: &str) -> Result<String> {
        let salt = SaltString::generate(&mut OsRng);
        let argon2 = Argon2::default();
        let hash = argon2
            .hash_password(password.as_bytes(), &salt)
            .map_err(|e| anyhow!("Password hashing failed: {}", e))?;
        Ok(hash.to_string())
    }

    /// Verify a plaintext password against an argon2 hash.
    pub fn verify_password(&self, password: &str, hash: &str) -> Result<bool> {
        let parsed = PasswordHash::new(hash)
            .map_err(|e| anyhow!("Invalid password hash format: {}", e))?;
        Ok(Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok())
    }

    /// Create a JWT with 24-hour expiry.
    pub fn create_token(&self, user_id: i64, email: &str) -> Result<String> {
        let exp = chrono::Utc::now()
            .checked_add_signed(chrono::Duration::hours(24))
            .ok_or_else(|| anyhow!("Timestamp overflow"))?
            .timestamp() as usize;

        let claims = Claims {
            sub: user_id,
            email: email.to_string(),
            exp,
        };

        let token = encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(self.jwt_secret.as_bytes()),
        )
        .map_err(|e| anyhow!("JWT encode error: {}", e))?;

        Ok(token)
    }

    /// Verify and decode a JWT, returning the claims.
    pub fn verify_token(&self, token: &str) -> Result<Claims> {
        let data = decode::<Claims>(
            token,
            &DecodingKey::from_secret(self.jwt_secret.as_bytes()),
            &Validation::default(),
        )
        .map_err(|e| anyhow!("JWT decode error: {}", e))?;

        Ok(data.claims)
    }
}

// ─── Axum Extractor ─────────────────────────────────────────

/// Axum extractor that reads `Authorization: Bearer <token>` and validates the JWT.
///
/// Usage in handlers:  `async fn handler(auth: AuthUser) -> impl IntoResponse { ... }`
pub struct AuthUser(pub Claims);

#[async_trait::async_trait]
impl<S: Send + Sync> FromRequestParts<S> for AuthUser {
    type Rejection = StatusCode;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        // Extract the Authorization header.
        let auth_header = parts
            .headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .ok_or(StatusCode::UNAUTHORIZED)?;

        // Strip the "Bearer " prefix.
        let token = auth_header
            .strip_prefix("Bearer ")
            .or_else(|| auth_header.strip_prefix("bearer "))
            .ok_or(StatusCode::UNAUTHORIZED)?;

        // Decode with the global secret.
        let secret = get_jwt_secret();
        let data = decode::<Claims>(
            token,
            &DecodingKey::from_secret(secret.as_bytes()),
            &Validation::default(),
        )
        .map_err(|_| StatusCode::UNAUTHORIZED)?;

        Ok(AuthUser(data.claims))
    }
}
