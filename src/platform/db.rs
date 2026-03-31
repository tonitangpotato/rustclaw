//! Platform SQLite database — users, profiles, instances, call logs.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};
use sqlx::Row;

// ─── Data Types ──────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: i64,
    pub email: String,
    pub password_hash: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserProfile {
    pub user_id: i64,
    pub full_name: Option<String>,
    pub phone: Option<String>,
    pub address: Option<String>,
    pub timezone: Option<String>,
    pub contacts: Vec<Contact>,
    pub insurance: Option<Insurance>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Contact {
    pub name: String,
    pub phone: String,
    pub address: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Insurance {
    pub provider: String,
    pub member_id: String,
    pub group: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Instance {
    pub id: i64,
    pub user_id: i64,
    pub channel_type: String,
    pub bot_token: String,
    pub status: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallLog {
    pub id: Option<i64>,
    pub user_id: i64,
    pub call_sid: String,
    pub to_number: String,
    pub purpose: String,
    pub status: String,
    pub transcript: Option<String>,
    pub summary: Option<String>,
    pub duration_secs: Option<i64>,
    pub created_at: Option<String>,
}

// ─── Database ────────────────────────────────────────────────

pub struct PlatformDb {
    pool: SqlitePool,
}

impl PlatformDb {
    /// Open (or create) the SQLite database and run migrations.
    pub async fn new(db_path: &str) -> Result<Self> {
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect(&format!("sqlite:{}?mode=rwc", db_path))
            .await
            .with_context(|| format!("Failed to open platform DB at {}", db_path))?;

        let db = Self { pool };
        db.migrate().await?;

        tracing::info!("Platform DB initialized: {}", db_path);
        Ok(db)
    }

    /// Run all CREATE TABLE IF NOT EXISTS migrations.
    pub async fn migrate(&self) -> Result<()> {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS users (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                email TEXT NOT NULL UNIQUE,
                password_hash TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            )",
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS user_profiles (
                user_id INTEGER PRIMARY KEY REFERENCES users(id),
                full_name TEXT,
                phone TEXT,
                address TEXT,
                timezone TEXT,
                contacts TEXT NOT NULL DEFAULT '[]',
                insurance TEXT
            )",
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS instances (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                user_id INTEGER NOT NULL REFERENCES users(id),
                channel_type TEXT NOT NULL,
                bot_token TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'active',
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            )",
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS call_logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                user_id INTEGER NOT NULL REFERENCES users(id),
                call_sid TEXT NOT NULL,
                to_number TEXT NOT NULL,
                purpose TEXT NOT NULL,
                status TEXT NOT NULL,
                transcript TEXT,
                summary TEXT,
                duration_secs INTEGER,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            )",
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    // ─── Users ───────────────────────────────────────────────

    /// Create a user and return the new user's ID.
    pub async fn create_user(&self, email: &str, password_hash: &str) -> Result<i64> {
        let result = sqlx::query(
            "INSERT INTO users (email, password_hash) VALUES (?, ?)",
        )
        .bind(email)
        .bind(password_hash)
        .execute(&self.pool)
        .await
        .with_context(|| format!("Failed to create user: {}", email))?;

        Ok(result.last_insert_rowid())
    }

    /// Look up a user by email.
    pub async fn get_user_by_email(&self, email: &str) -> Result<Option<User>> {
        let row = sqlx::query(
            "SELECT id, email, password_hash, created_at FROM users WHERE email = ?",
        )
        .bind(email)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| User {
            id: r.get("id"),
            email: r.get("email"),
            password_hash: r.get("password_hash"),
            created_at: r.get("created_at"),
        }))
    }

    /// Look up a user by ID.
    pub async fn get_user_by_id(&self, id: i64) -> Result<Option<User>> {
        let row = sqlx::query(
            "SELECT id, email, password_hash, created_at FROM users WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| User {
            id: r.get("id"),
            email: r.get("email"),
            password_hash: r.get("password_hash"),
            created_at: r.get("created_at"),
        }))
    }

    // ─── Profiles ────────────────────────────────────────────

    /// Insert or replace the user profile. contacts and insurance are stored as JSON.
    pub async fn upsert_profile(&self, user_id: i64, profile: &UserProfile) -> Result<()> {
        let contacts_json =
            serde_json::to_string(&profile.contacts).unwrap_or_else(|_| "[]".into());
        let insurance_json = profile
            .insurance
            .as_ref()
            .map(|i| serde_json::to_string(i).unwrap_or_else(|_| "null".into()));

        sqlx::query(
            "INSERT INTO user_profiles (user_id, full_name, phone, address, timezone, contacts, insurance)
             VALUES (?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(user_id) DO UPDATE SET
                full_name = excluded.full_name,
                phone = excluded.phone,
                address = excluded.address,
                timezone = excluded.timezone,
                contacts = excluded.contacts,
                insurance = excluded.insurance",
        )
        .bind(user_id)
        .bind(&profile.full_name)
        .bind(&profile.phone)
        .bind(&profile.address)
        .bind(&profile.timezone)
        .bind(&contacts_json)
        .bind(&insurance_json)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Fetch a user's profile.
    pub async fn get_profile(&self, user_id: i64) -> Result<Option<UserProfile>> {
        let row = sqlx::query(
            "SELECT user_id, full_name, phone, address, timezone, contacts, insurance
             FROM user_profiles WHERE user_id = ?",
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| {
            let contacts_raw: String = r.get("contacts");
            let insurance_raw: Option<String> = r.get("insurance");

            UserProfile {
                user_id: r.get("user_id"),
                full_name: r.get("full_name"),
                phone: r.get("phone"),
                address: r.get("address"),
                timezone: r.get("timezone"),
                contacts: serde_json::from_str(&contacts_raw).unwrap_or_default(),
                insurance: insurance_raw
                    .and_then(|s| serde_json::from_str(&s).ok()),
            }
        }))
    }

    // ─── Instances ───────────────────────────────────────────

    /// Create a new instance (bot connection) for a user.
    pub async fn create_instance(
        &self,
        user_id: i64,
        channel_type: &str,
        bot_token: &str,
    ) -> Result<i64> {
        let result = sqlx::query(
            "INSERT INTO instances (user_id, channel_type, bot_token, status) VALUES (?, ?, ?, 'active')",
        )
        .bind(user_id)
        .bind(channel_type)
        .bind(bot_token)
        .execute(&self.pool)
        .await?;

        Ok(result.last_insert_rowid())
    }

    /// Get the user's instance (one instance per user for MVP).
    pub async fn get_instance(&self, user_id: i64) -> Result<Option<Instance>> {
        let row = sqlx::query(
            "SELECT id, user_id, channel_type, bot_token, status, created_at
             FROM instances WHERE user_id = ? ORDER BY id DESC LIMIT 1",
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| Instance {
            id: r.get("id"),
            user_id: r.get("user_id"),
            channel_type: r.get("channel_type"),
            bot_token: r.get("bot_token"),
            status: r.get("status"),
            created_at: r.get("created_at"),
        }))
    }

    /// Update instance status (active / stopped / error).
    pub async fn update_instance_status(&self, id: i64, status: &str) -> Result<()> {
        sqlx::query("UPDATE instances SET status = ? WHERE id = ?")
            .bind(status)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Delete a user's instance(s).
    pub async fn delete_instance(&self, user_id: i64) -> Result<()> {
        sqlx::query("DELETE FROM instances WHERE user_id = ?")
            .bind(user_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// List all instances with status = 'active'.
    pub async fn list_active_instances(&self) -> Result<Vec<Instance>> {
        let rows = sqlx::query(
            "SELECT id, user_id, channel_type, bot_token, status, created_at
             FROM instances WHERE status = 'active'",
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| Instance {
                id: r.get("id"),
                user_id: r.get("user_id"),
                channel_type: r.get("channel_type"),
                bot_token: r.get("bot_token"),
                status: r.get("status"),
                created_at: r.get("created_at"),
            })
            .collect())
    }

    // ─── Call Logs ───────────────────────────────────────────

    /// Insert a call log and return its ID.
    pub async fn log_call(&self, user_id: i64, call: &CallLog) -> Result<i64> {
        let result = sqlx::query(
            "INSERT INTO call_logs (user_id, call_sid, to_number, purpose, status, transcript, summary, duration_secs)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(user_id)
        .bind(&call.call_sid)
        .bind(&call.to_number)
        .bind(&call.purpose)
        .bind(&call.status)
        .bind(&call.transcript)
        .bind(&call.summary)
        .bind(call.duration_secs)
        .execute(&self.pool)
        .await?;

        Ok(result.last_insert_rowid())
    }

    /// Get a single call log by ID.
    pub async fn get_call(&self, id: i64) -> Result<Option<CallLog>> {
        let row = sqlx::query(
            "SELECT id, user_id, call_sid, to_number, purpose, status, transcript, summary, duration_secs, created_at
             FROM call_logs WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| CallLog {
            id: Some(r.get("id")),
            user_id: r.get("user_id"),
            call_sid: r.get("call_sid"),
            to_number: r.get("to_number"),
            purpose: r.get("purpose"),
            status: r.get("status"),
            transcript: r.get("transcript"),
            summary: r.get("summary"),
            duration_secs: r.get("duration_secs"),
            created_at: Some(r.get("created_at")),
        }))
    }

    /// List a user's recent calls, newest first.
    pub async fn list_calls(&self, user_id: i64, limit: i64) -> Result<Vec<CallLog>> {
        let rows = sqlx::query(
            "SELECT id, user_id, call_sid, to_number, purpose, status, transcript, summary, duration_secs, created_at
             FROM call_logs WHERE user_id = ? ORDER BY id DESC LIMIT ?",
        )
        .bind(user_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| CallLog {
                id: Some(r.get("id")),
                user_id: r.get("user_id"),
                call_sid: r.get("call_sid"),
                to_number: r.get("to_number"),
                purpose: r.get("purpose"),
                status: r.get("status"),
                transcript: r.get("transcript"),
                summary: r.get("summary"),
                duration_secs: r.get("duration_secs"),
                created_at: Some(r.get("created_at")),
            })
            .collect())
    }
}
