//! # skills-manager
//!
//! Universal AI agent skill management library.
//!
//! Skills are markdown files with YAML frontmatter that define automated workflows,
//! trigger conditions, and domain knowledge for AI agents. This library provides:
//!
//! - **Schema**: Structured metadata for skills (triggers, tags, priority)
//! - **Parser**: Parse YAML frontmatter + markdown body from skill files
//! - **Registry**: Load, cache, and query skills from a directory
//! - **Matcher**: Match user input against skill triggers to find relevant skills
//!
//! ## Skill Format
//!
//! ```markdown
//! ---
//! name: web-scraping
//! description: Extract content from web pages
//! triggers:
//!   patterns: ["http://", "https://"]
//!   keywords: ["scrape", "fetch page", "download"]
//! priority: 80
//! always_load: false
//! tags: [web, extraction]
//! ---
//!
//! # Web Scraping Skill
//! ... (markdown content with instructions for the agent)
//! ```
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use skills_manager::{SkillRegistry, Matcher};
//!
//! let registry = SkillRegistry::load("./skills").unwrap();
//! let matcher = Matcher::new(&registry);
//!
//! // Find skills matching user input
//! let matches = matcher.match_input("check out https://example.com");
//! for m in &matches {
//!     println!("Matched skill: {} (score: {:.2})", m.skill.name(), m.score);
//! }
//!
//! // Get always-loaded skills (inject into system prompt)
//! let always = registry.always_load_skills();
//! ```

pub mod cli;
pub mod matcher;
pub mod parser;
pub mod registry;
pub mod schema;

// Re-export main types for convenience
pub use matcher::{MatchResult, Matcher};
pub use parser::Parser;
pub use registry::SkillRegistry;
pub use schema::{Skill, SkillMetadata, SkillStatus, TriggerConfig};
