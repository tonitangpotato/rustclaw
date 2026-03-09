//! Honcho-style User Modeling.
//!
//! Dialectic user profiling — builds a model of the user from conversations.
//! Tracks preferences, interaction style, topics of interest, and communication patterns.
//!
//! Inspired by Hermes Agent's Honcho integration for personalized AI experiences.

use std::collections::HashMap;

use chrono::{DateTime, Timelike, Utc};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

/// User model manager backed by SQLite.
pub struct UserModel {
    db: SqlitePool,
}

/// Complete user profile with all tracked data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserProfile {
    /// Unique user identifier.
    pub user_id: String,
    /// User preferences (explicit and inferred).
    pub preferences: HashMap<String, PreferenceEntry>,
    /// Detected interaction style.
    pub interaction_style: InteractionStyle,
    /// Topics the user is interested in.
    pub topics_of_interest: Vec<TopicInterest>,
    /// Communication patterns.
    pub communication_patterns: CommunicationPatterns,
    /// When the profile was last updated.
    pub updated_at: DateTime<Utc>,
}

/// A single preference entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreferenceEntry {
    /// Preference key.
    pub key: String,
    /// Preference value.
    pub value: String,
    /// Confidence score (0.0-1.0).
    pub confidence: f32,
    /// Source: "explicit" (user stated) or "inferred" (detected from behavior).
    pub source: String,
    /// When this preference was last updated.
    pub updated_at: DateTime<Utc>,
}

/// User's interaction style preferences.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InteractionStyle {
    /// Preferred response length (0.0=terse, 1.0=verbose).
    pub verbosity: f32,
    /// Formality level (0.0=casual, 1.0=formal).
    pub formality: f32,
    /// Technical expertise level (0.0=beginner, 1.0=expert).
    pub technical_level: f32,
    /// Emoji usage preference (0.0=none, 1.0=heavy).
    pub emoji_usage: f32,
    /// Primary language.
    pub language: String,
}

/// A topic the user is interested in.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopicInterest {
    /// Topic name.
    pub topic: String,
    /// Interest score (0.0-1.0).
    pub score: f32,
    /// Number of times this topic was mentioned.
    pub mention_count: u32,
    /// When this topic was last mentioned.
    pub last_mentioned: DateTime<Utc>,
}

/// Communication patterns observed from user messages.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CommunicationPatterns {
    /// Average message length in characters.
    pub avg_message_length: f32,
    /// Hours of the day (0-23) when user is typically active.
    pub active_hours: Vec<u8>,
    /// Average response time in seconds (time between user messages).
    pub response_time_avg_secs: f32,
    /// Preferred channel for communication.
    pub preferred_channel: Option<String>,
}

impl UserModel {
    /// Create a new user model with the given database pool.
    ///
    /// Creates the necessary tables if they don't exist.
    pub async fn new(db: SqlitePool) -> anyhow::Result<Self> {
        let model = Self { db };
        model.init_tables().await?;
        Ok(model)
    }

    /// Initialize database tables.
    async fn init_tables(&self) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS user_profiles (
                user_id TEXT PRIMARY KEY,
                verbosity REAL DEFAULT 0.5,
                formality REAL DEFAULT 0.5,
                technical_level REAL DEFAULT 0.5,
                emoji_usage REAL DEFAULT 0.3,
                language TEXT DEFAULT 'en',
                avg_message_length REAL DEFAULT 50.0,
                active_hours TEXT DEFAULT '[]',
                response_time_avg_secs REAL DEFAULT 0.0,
                preferred_channel TEXT,
                updated_at INTEGER NOT NULL
            )
            "#,
        )
        .execute(&self.db)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS user_preferences (
                user_id TEXT NOT NULL,
                key TEXT NOT NULL,
                value TEXT NOT NULL,
                confidence REAL DEFAULT 0.5,
                source TEXT DEFAULT 'inferred',
                updated_at INTEGER NOT NULL,
                PRIMARY KEY (user_id, key)
            )
            "#,
        )
        .execute(&self.db)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS user_topics (
                user_id TEXT NOT NULL,
                topic TEXT NOT NULL,
                score REAL DEFAULT 0.5,
                mention_count INTEGER DEFAULT 1,
                last_mentioned INTEGER NOT NULL,
                PRIMARY KEY (user_id, topic)
            )
            "#,
        )
        .execute(&self.db)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS user_messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                user_id TEXT NOT NULL,
                message_length INTEGER NOT NULL,
                channel TEXT,
                timestamp INTEGER NOT NULL,
                hour_of_day INTEGER NOT NULL
            )
            "#,
        )
        .execute(&self.db)
        .await?;

        // Index for efficient queries
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_user_messages_user ON user_messages(user_id)",
        )
        .execute(&self.db)
        .await?;

        Ok(())
    }

    /// Observe an incoming message and update the user profile.
    ///
    /// Analyzes:
    /// - Message length (for verbosity estimation)
    /// - Language detection
    /// - Technical term detection
    /// - Active hours
    /// - Topic extraction
    pub async fn observe_message(
        &self,
        user_id: &str,
        message: &str,
        channel: &str,
        timestamp: i64,
    ) -> anyhow::Result<()> {
        let dt = DateTime::<Utc>::from_timestamp(timestamp, 0)
            .unwrap_or_else(Utc::now);
        let hour = dt.hour() as i64;
        let message_length = message.len() as i64;

        // Record the message for pattern analysis
        sqlx::query(
            r#"
            INSERT INTO user_messages (user_id, message_length, channel, timestamp, hour_of_day)
            VALUES (?, ?, ?, ?, ?)
            "#,
        )
        .bind(user_id)
        .bind(message_length)
        .bind(channel)
        .bind(timestamp)
        .bind(hour)
        .execute(&self.db)
        .await?;

        // Ensure user profile exists
        self.ensure_profile_exists(user_id).await?;

        // Update channel preference
        sqlx::query(
            r#"
            UPDATE user_profiles SET preferred_channel = ?, updated_at = ?
            WHERE user_id = ?
            "#,
        )
        .bind(channel)
        .bind(timestamp)
        .bind(user_id)
        .execute(&self.db)
        .await?;

        // Detect and update language
        let detected_lang = detect_language(message);
        if !detected_lang.is_empty() {
            sqlx::query(
                "UPDATE user_profiles SET language = ? WHERE user_id = ?",
            )
            .bind(&detected_lang)
            .bind(user_id)
            .execute(&self.db)
            .await?;
        }

        // Detect and update emoji usage
        let emoji_ratio = calculate_emoji_ratio(message);
        sqlx::query(
            r#"
            UPDATE user_profiles 
            SET emoji_usage = (emoji_usage * 0.9 + ? * 0.1)
            WHERE user_id = ?
            "#,
        )
        .bind(emoji_ratio)
        .bind(user_id)
        .execute(&self.db)
        .await?;

        // Detect technical level
        let tech_score = detect_technical_level(message);
        sqlx::query(
            r#"
            UPDATE user_profiles 
            SET technical_level = (technical_level * 0.9 + ? * 0.1)
            WHERE user_id = ?
            "#,
        )
        .bind(tech_score)
        .bind(user_id)
        .execute(&self.db)
        .await?;

        // Extract and track topics
        let topics = extract_topics(message);
        for topic in topics {
            self.update_topic_interest(user_id, &topic, timestamp).await?;
        }

        Ok(())
    }

    /// Ensure a user profile exists, creating a default one if needed.
    async fn ensure_profile_exists(&self, user_id: &str) -> anyhow::Result<()> {
        let now = Utc::now().timestamp();
        sqlx::query(
            r#"
            INSERT OR IGNORE INTO user_profiles (user_id, updated_at)
            VALUES (?, ?)
            "#,
        )
        .bind(user_id)
        .bind(now)
        .execute(&self.db)
        .await?;
        Ok(())
    }

    /// Update topic interest for a user.
    async fn update_topic_interest(
        &self,
        user_id: &str,
        topic: &str,
        timestamp: i64,
    ) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO user_topics (user_id, topic, score, mention_count, last_mentioned)
            VALUES (?, ?, 0.5, 1, ?)
            ON CONFLICT(user_id, topic) DO UPDATE SET
                score = MIN(1.0, score + 0.1),
                mention_count = mention_count + 1,
                last_mentioned = ?
            "#,
        )
        .bind(user_id)
        .bind(topic)
        .bind(timestamp)
        .bind(timestamp)
        .execute(&self.db)
        .await?;
        Ok(())
    }

    /// Get the complete profile for a user.
    pub async fn get_profile(&self, user_id: &str) -> anyhow::Result<Option<UserProfile>> {
        // Get base profile
        let row = sqlx::query_as::<_, ProfileRow>(
            r#"
            SELECT user_id, verbosity, formality, technical_level, emoji_usage,
                   language, avg_message_length, active_hours, response_time_avg_secs,
                   preferred_channel, updated_at
            FROM user_profiles WHERE user_id = ?
            "#,
        )
        .bind(user_id)
        .fetch_optional(&self.db)
        .await?;

        let row = match row {
            Some(r) => r,
            None => return Ok(None),
        };

        // Get preferences
        let pref_rows = sqlx::query_as::<_, PreferenceRow>(
            "SELECT key, value, confidence, source, updated_at FROM user_preferences WHERE user_id = ?",
        )
        .bind(user_id)
        .fetch_all(&self.db)
        .await?;

        let preferences: HashMap<String, PreferenceEntry> = pref_rows
            .into_iter()
            .map(|r| {
                (
                    r.key.clone(),
                    PreferenceEntry {
                        key: r.key,
                        value: r.value,
                        confidence: r.confidence,
                        source: r.source,
                        updated_at: DateTime::<Utc>::from_timestamp(r.updated_at, 0)
                            .unwrap_or_else(Utc::now),
                    },
                )
            })
            .collect();

        // Get topics
        let topic_rows = sqlx::query_as::<_, TopicRow>(
            "SELECT topic, score, mention_count, last_mentioned FROM user_topics WHERE user_id = ? ORDER BY score DESC LIMIT 20",
        )
        .bind(user_id)
        .fetch_all(&self.db)
        .await?;

        let topics_of_interest: Vec<TopicInterest> = topic_rows
            .into_iter()
            .map(|r| TopicInterest {
                topic: r.topic,
                score: r.score,
                mention_count: r.mention_count as u32,
                last_mentioned: DateTime::<Utc>::from_timestamp(r.last_mentioned, 0)
                    .unwrap_or_else(Utc::now),
            })
            .collect();

        // Calculate active hours from message history
        let active_hours = self.calculate_active_hours(user_id).await?;

        // Parse stored active hours or use calculated ones
        let active_hours_vec: Vec<u8> = if row.active_hours.is_empty() || row.active_hours == "[]" {
            active_hours
        } else {
            serde_json::from_str(&row.active_hours).unwrap_or(active_hours)
        };

        // Calculate average message length
        let avg_length = self.calculate_avg_message_length(user_id).await?;

        let profile = UserProfile {
            user_id: row.user_id,
            preferences,
            interaction_style: InteractionStyle {
                verbosity: row.verbosity,
                formality: row.formality,
                technical_level: row.technical_level,
                emoji_usage: row.emoji_usage,
                language: row.language,
            },
            topics_of_interest,
            communication_patterns: CommunicationPatterns {
                avg_message_length: avg_length,
                active_hours: active_hours_vec,
                response_time_avg_secs: row.response_time_avg_secs,
                preferred_channel: row.preferred_channel,
            },
            updated_at: DateTime::<Utc>::from_timestamp(row.updated_at, 0)
                .unwrap_or_else(Utc::now),
        };

        Ok(Some(profile))
    }

    /// Calculate active hours from message history.
    async fn calculate_active_hours(&self, user_id: &str) -> anyhow::Result<Vec<u8>> {
        let rows = sqlx::query_as::<_, HourCount>(
            r#"
            SELECT hour_of_day, COUNT(*) as count
            FROM user_messages
            WHERE user_id = ?
            GROUP BY hour_of_day
            ORDER BY count DESC
            LIMIT 5
            "#,
        )
        .bind(user_id)
        .fetch_all(&self.db)
        .await?;

        let hours: Vec<u8> = rows.into_iter().map(|r| r.hour_of_day as u8).collect();
        Ok(hours)
    }

    /// Calculate average message length.
    async fn calculate_avg_message_length(&self, user_id: &str) -> anyhow::Result<f32> {
        let row = sqlx::query_as::<_, AvgLength>(
            "SELECT AVG(message_length) as avg FROM user_messages WHERE user_id = ?",
        )
        .bind(user_id)
        .fetch_optional(&self.db)
        .await?;

        Ok(row.and_then(|r| r.avg).unwrap_or(50.0) as f32)
    }

    /// Set an explicit preference for a user.
    pub async fn set_preference(
        &self,
        user_id: &str,
        key: &str,
        value: &str,
        source: &str,
    ) -> anyhow::Result<()> {
        let now = Utc::now().timestamp();
        let confidence = if source == "explicit" { 1.0 } else { 0.7 };

        self.ensure_profile_exists(user_id).await?;

        sqlx::query(
            r#"
            INSERT INTO user_preferences (user_id, key, value, confidence, source, updated_at)
            VALUES (?, ?, ?, ?, ?, ?)
            ON CONFLICT(user_id, key) DO UPDATE SET
                value = excluded.value,
                confidence = excluded.confidence,
                source = excluded.source,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(user_id)
        .bind(key)
        .bind(value)
        .bind(confidence)
        .bind(source)
        .bind(now)
        .execute(&self.db)
        .await?;

        Ok(())
    }

    /// Get a specific preference for a user.
    pub async fn get_preference(
        &self,
        user_id: &str,
        key: &str,
    ) -> anyhow::Result<Option<PreferenceEntry>> {
        let row = sqlx::query_as::<_, PreferenceRow>(
            "SELECT key, value, confidence, source, updated_at FROM user_preferences WHERE user_id = ? AND key = ?",
        )
        .bind(user_id)
        .bind(key)
        .fetch_optional(&self.db)
        .await?;

        Ok(row.map(|r| PreferenceEntry {
            key: r.key,
            value: r.value,
            confidence: r.confidence,
            source: r.source,
            updated_at: DateTime::<Utc>::from_timestamp(r.updated_at, 0)
                .unwrap_or_else(Utc::now),
        }))
    }

    /// Infer interaction style from message history.
    pub async fn infer_style(&self, user_id: &str) -> anyhow::Result<InteractionStyle> {
        let profile = self.get_profile(user_id).await?;
        
        match profile {
            Some(p) => Ok(p.interaction_style),
            None => Ok(InteractionStyle::default()),
        }
    }

    /// Generate a context prompt snippet describing the user's preferences.
    ///
    /// This can be injected into the system prompt to personalize responses.
    pub async fn get_context_prompt(&self, user_id: &str) -> anyhow::Result<String> {
        let profile = match self.get_profile(user_id).await? {
            Some(p) => p,
            None => return Ok(String::new()),
        };

        let mut parts: Vec<String> = Vec::new();

        // Verbosity
        if profile.interaction_style.verbosity < 0.3 {
            parts.push("prefers concise, brief responses".to_string());
        } else if profile.interaction_style.verbosity > 0.7 {
            parts.push("prefers detailed, thorough responses".to_string());
        }

        // Formality
        if profile.interaction_style.formality < 0.3 {
            parts.push("casual communication style".to_string());
        } else if profile.interaction_style.formality > 0.7 {
            parts.push("formal communication style".to_string());
        }

        // Technical level
        if profile.interaction_style.technical_level > 0.7 {
            parts.push("technically proficient".to_string());
        } else if profile.interaction_style.technical_level < 0.3 {
            parts.push("prefers non-technical explanations".to_string());
        }

        // Language
        let lang = &profile.interaction_style.language;
        if lang != "en" && !lang.is_empty() {
            parts.push(format!("primary language: {}", lang));
        }

        // Emoji
        if profile.interaction_style.emoji_usage > 0.5 {
            parts.push("uses emojis frequently".to_string());
        }

        // Top topics
        if !profile.topics_of_interest.is_empty() {
            let topics: Vec<&str> = profile
                .topics_of_interest
                .iter()
                .take(3)
                .map(|t| t.topic.as_str())
                .collect();
            if !topics.is_empty() {
                parts.push(format!("interested in: {}", topics.join(", ")));
            }
        }

        if parts.is_empty() {
            return Ok(String::new());
        }

        Ok(format!("User context: {}", parts.join("; ")))
    }

    /// Decay topic interest scores over time (recency bias).
    ///
    /// Multiplies all scores by the decay factor (e.g., 0.95).
    pub async fn decay_interests(&self, decay_factor: f32) -> anyhow::Result<()> {
        sqlx::query("UPDATE user_topics SET score = score * ?")
            .bind(decay_factor)
            .execute(&self.db)
            .await?;

        // Remove topics with very low scores
        sqlx::query("DELETE FROM user_topics WHERE score < 0.05")
            .execute(&self.db)
            .await?;

        Ok(())
    }

    /// Clean up old message history (keep last N days).
    pub async fn cleanup_old_messages(&self, days: i64) -> anyhow::Result<u64> {
        let cutoff = Utc::now().timestamp() - (days * 24 * 60 * 60);
        
        let result = sqlx::query("DELETE FROM user_messages WHERE timestamp < ?")
            .bind(cutoff)
            .execute(&self.db)
            .await?;

        Ok(result.rows_affected())
    }
}

// Database row types
#[derive(sqlx::FromRow)]
struct ProfileRow {
    user_id: String,
    verbosity: f32,
    formality: f32,
    technical_level: f32,
    emoji_usage: f32,
    language: String,
    avg_message_length: f32,
    active_hours: String,
    response_time_avg_secs: f32,
    preferred_channel: Option<String>,
    updated_at: i64,
}

#[derive(sqlx::FromRow)]
struct PreferenceRow {
    key: String,
    value: String,
    confidence: f32,
    source: String,
    updated_at: i64,
}

#[derive(sqlx::FromRow)]
struct TopicRow {
    topic: String,
    score: f32,
    mention_count: i64,
    last_mentioned: i64,
}

#[derive(sqlx::FromRow)]
struct HourCount {
    hour_of_day: i64,
    #[allow(dead_code)]
    count: i64,
}

#[derive(sqlx::FromRow)]
struct AvgLength {
    avg: Option<f64>,
}

// Helper functions for analysis

/// Detect the primary language of a message.
fn detect_language(text: &str) -> String {
    // Simple heuristic based on character ranges
    let mut cjk_count = 0;
    let mut latin_count = 0;
    let mut cyrillic_count = 0;

    for c in text.chars() {
        if c >= '\u{4e00}' && c <= '\u{9fff}' {
            cjk_count += 1;
        } else if c >= '\u{0400}' && c <= '\u{04ff}' {
            cyrillic_count += 1;
        } else if c.is_ascii_alphabetic() {
            latin_count += 1;
        }
    }

    let total = cjk_count + latin_count + cyrillic_count;
    if total == 0 {
        return "en".to_string();
    }

    if cjk_count as f32 / total as f32 > 0.3 {
        "zh".to_string()
    } else if cyrillic_count as f32 / total as f32 > 0.3 {
        "ru".to_string()
    } else {
        "en".to_string()
    }
}

/// Calculate emoji ratio in text.
fn calculate_emoji_ratio(text: &str) -> f32 {
    let emoji_count = text.chars().filter(|c| is_emoji(*c)).count();
    let total = text.chars().count();
    
    if total == 0 {
        return 0.0;
    }
    
    (emoji_count as f32 / total as f32).min(1.0)
}

/// Check if a character is an emoji.
fn is_emoji(c: char) -> bool {
    matches!(c,
        '\u{1F600}'..='\u{1F64F}' |  // Emoticons
        '\u{1F300}'..='\u{1F5FF}' |  // Misc Symbols and Pictographs
        '\u{1F680}'..='\u{1F6FF}' |  // Transport and Map
        '\u{1F700}'..='\u{1F77F}' |  // Alchemical Symbols
        '\u{1F780}'..='\u{1F7FF}' |  // Geometric Shapes Extended
        '\u{1F800}'..='\u{1F8FF}' |  // Supplemental Arrows-C
        '\u{1F900}'..='\u{1F9FF}' |  // Supplemental Symbols and Pictographs
        '\u{1FA00}'..='\u{1FA6F}' |  // Chess Symbols
        '\u{1FA70}'..='\u{1FAFF}' |  // Symbols and Pictographs Extended-A
        '\u{2600}'..='\u{26FF}'   |  // Misc symbols
        '\u{2700}'..='\u{27BF}'      // Dingbats
    )
}

/// Detect technical level from message content.
fn detect_technical_level(text: &str) -> f32 {
    let technical_terms = [
        "api", "http", "json", "database", "server", "client", "async",
        "function", "variable", "class", "method", "compile", "runtime",
        "docker", "kubernetes", "git", "commit", "branch", "merge",
        "sql", "query", "index", "cache", "memory", "cpu", "gpu",
        "algorithm", "recursion", "iteration", "stack", "heap",
        "rust", "python", "javascript", "typescript", "golang",
        "llm", "model", "token", "embedding", "vector", "neural",
    ];

    let text_lower = text.to_lowercase();
    let words: Vec<&str> = text_lower.split_whitespace().collect();
    
    if words.is_empty() {
        return 0.5;
    }

    let tech_count = words
        .iter()
        .filter(|w| technical_terms.iter().any(|t| w.contains(t)))
        .count();

    (tech_count as f32 / words.len() as f32 * 5.0).min(1.0)
}

/// Extract topics from message content.
fn extract_topics(text: &str) -> Vec<String> {
    let topic_keywords = [
        ("coding", &["code", "programming", "developer", "software"][..]),
        ("ai", &["ai", "machine learning", "llm", "gpt", "claude", "model"]),
        ("rust", &["rust", "cargo", "tokio", "async"]),
        ("python", &["python", "pip", "django", "flask"]),
        ("devops", &["docker", "kubernetes", "k8s", "deploy", "ci/cd"]),
        ("database", &["database", "sql", "postgres", "mongodb", "redis"]),
        ("web", &["web", "http", "api", "rest", "graphql"]),
        ("crypto", &["crypto", "bitcoin", "ethereum", "blockchain", "nft"]),
        ("gaming", &["game", "gaming", "steam", "playstation", "xbox"]),
        ("music", &["music", "song", "album", "spotify", "artist"]),
    ];

    let text_lower = text.to_lowercase();
    let mut found_topics = Vec::new();

    for (topic, keywords) in topic_keywords {
        if keywords.iter().any(|k| text_lower.contains(k)) {
            found_topics.push(topic.to_string());
        }
    }

    found_topics
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_language() {
        assert_eq!(detect_language("Hello, how are you?"), "en");
        assert_eq!(detect_language("你好，你怎么样？"), "zh");
        assert_eq!(detect_language("Привет, как дела?"), "ru");
        assert_eq!(detect_language("Hello 你好 mixed"), "en"); // More latin
    }

    #[test]
    fn test_emoji_ratio() {
        assert!(calculate_emoji_ratio("Hello") < 0.01);
        assert!(calculate_emoji_ratio("Hello 😊") > 0.0);
        assert!(calculate_emoji_ratio("😊😊😊") > 0.9);
    }

    #[test]
    fn test_technical_level() {
        assert!(detect_technical_level("hello how are you") < 0.3);
        assert!(detect_technical_level("I'm learning rust and tokio") > 0.3);
        assert!(detect_technical_level("The API returns JSON with async HTTP calls") > 0.5);
    }

    #[test]
    fn test_extract_topics() {
        let topics = extract_topics("I'm working on a Python web API with PostgreSQL");
        assert!(topics.contains(&"python".to_string()));
        assert!(topics.contains(&"web".to_string()));
        assert!(topics.contains(&"database".to_string()));
    }

    #[test]
    fn test_is_emoji() {
        assert!(is_emoji('😊'));
        assert!(is_emoji('🚀'));
        assert!(!is_emoji('A'));
        assert!(!is_emoji('中'));
    }
}
