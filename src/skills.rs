//! Auto Skill Generation — learn from agent experience.
//!
//! When the agent solves a complex problem, automatically generate a SKILL.md
//! file for future reuse. This is procedural memory — learning by doing.
//!
//! Inspired by Hermes Agent's auto-skill generation.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Context about a problem the agent solved.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProblemContext {
    /// The original user query/request.
    pub query: String,
    /// Tools that were used to solve the problem.
    pub tools_used: Vec<String>,
    /// High-level steps taken (from agent's perspective).
    pub steps_taken: Vec<String>,
    /// The final result/output.
    pub result: String,
    /// Time taken to solve (milliseconds).
    pub duration_ms: u64,
    /// Total tokens used.
    pub tokens_used: u64,
    /// Whether the solution was successful.
    pub success: bool,
}

/// A generated skill ready to be saved.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratedSkill {
    /// Skill name (kebab-case, used as directory name).
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// Full SKILL.md content.
    pub skill_md: String,
    /// Associated scripts: (filename, content).
    pub scripts: Vec<(String, String)>,
    /// When this skill was generated.
    pub created_at: DateTime<Utc>,
}

/// Skill generator that creates reusable skills from agent experience.
pub struct SkillGenerator {
    /// Directory where skills are stored.
    skills_dir: PathBuf,
    /// Minimum complexity score (0.0-1.0) to trigger skill generation.
    min_complexity: f32,
    /// Cache of existing skill names for deduplication.
    skill_cache: HashSet<String>,
}

impl SkillGenerator {
    /// Create a new skill generator.
    ///
    /// # Arguments
    /// * `skills_dir` - Directory to store generated skills
    /// * `min_complexity` - Minimum complexity score (0.0-1.0) to generate a skill
    pub fn new(skills_dir: impl Into<PathBuf>, min_complexity: f32) -> Self {
        let skills_dir = skills_dir.into();
        let min_complexity = min_complexity.clamp(0.0, 1.0);

        Self {
            skills_dir,
            min_complexity,
            skill_cache: HashSet::new(),
        }
    }

    /// Assess the complexity of a solved problem.
    ///
    /// Returns a score from 0.0 (trivial) to 1.0 (highly complex).
    ///
    /// Factors:
    /// - Number of tools used (more tools = more complex)
    /// - Number of steps taken (more steps = more complex)
    /// - Tokens used (more tokens = harder reasoning)
    /// - Duration (longer = more complex)
    /// - Tool diversity (using different tool types = more complex)
    pub fn assess_complexity(&self, ctx: &ProblemContext) -> f32 {
        if !ctx.success {
            return 0.0; // Don't learn from failures (for now)
        }

        let mut score = 0.0;

        // Tool count factor (0.0-0.25)
        // 1 tool = 0.05, 5+ tools = 0.25
        let tool_count = ctx.tools_used.len() as f32;
        score += (tool_count / 5.0).min(1.0) * 0.25;

        // Steps factor (0.0-0.25)
        // 1 step = 0.05, 10+ steps = 0.25
        let step_count = ctx.steps_taken.len() as f32;
        score += (step_count / 10.0).min(1.0) * 0.25;

        // Token usage factor (0.0-0.20)
        // <1000 = low, >10000 = high
        let token_factor = (ctx.tokens_used as f32 / 10000.0).min(1.0);
        score += token_factor * 0.20;

        // Duration factor (0.0-0.15)
        // <10s = low, >120s = high
        let duration_factor = (ctx.duration_ms as f32 / 120_000.0).min(1.0);
        score += duration_factor * 0.15;

        // Tool diversity factor (0.0-0.15)
        // Count unique tool "categories"
        let tool_categories = self.categorize_tools(&ctx.tools_used);
        let diversity_factor = (tool_categories.len() as f32 / 4.0).min(1.0);
        score += diversity_factor * 0.15;

        score.clamp(0.0, 1.0)
    }

    /// Categorize tools into high-level categories.
    fn categorize_tools(&self, tools: &[String]) -> HashSet<String> {
        let mut categories = HashSet::new();

        for tool in tools {
            let category = match tool.as_str() {
                "exec" => "shell",
                "read_file" | "write_file" | "edit_file" | "list_dir" | "search_files" => "filesystem",
                "web_fetch" => "web",
                "engram_recall" | "engram_store" | "engram_recall_associated" => "memory",
                "delegate_task" => "orchestration",
                _ => "other",
            };
            categories.insert(category.to_string());
        }

        categories
    }

    /// Check if a skill should be generated for this problem.
    ///
    /// Returns true if:
    /// - Complexity exceeds threshold
    /// - Solution was successful
    /// - No similar skill already exists
    pub async fn should_generate_skill(&self, ctx: &ProblemContext) -> bool {
        if !ctx.success {
            return false;
        }

        let complexity = self.assess_complexity(ctx);
        if complexity < self.min_complexity {
            tracing::debug!(
                "Complexity {:.2} below threshold {:.2}, skipping skill generation",
                complexity,
                self.min_complexity
            );
            return false;
        }

        // Check if similar skill exists
        if let Some(similar) = self.find_similar_skill(&ctx.query) {
            tracing::debug!("Similar skill already exists: {}", similar);
            return false;
        }

        true
    }

    /// Generate a skill from the problem context.
    ///
    /// # Arguments
    /// * `ctx` - Problem context with tools used, steps taken, etc.
    /// * `agent_summary` - Agent's own summary of what it did and learned
    ///
    /// # Returns
    /// A GeneratedSkill ready to be saved.
    pub async fn generate_skill(
        &self,
        ctx: &ProblemContext,
        agent_summary: &str,
    ) -> Result<GeneratedSkill> {
        let name = self.generate_skill_name(&ctx.query);
        let description = self.extract_description(agent_summary, ctx);
        let when_to_use = self.infer_triggers(ctx);
        let steps = self.format_steps(&ctx.steps_taken, agent_summary);
        let tools_section = self.format_tools_required(&ctx.tools_used);
        let example_section = self.format_example(ctx);

        let skill_md = format!(
            r#"# {}

> Auto-generated skill from agent experience

## Description
{}

## When to Use
{}

## Steps
{}

## Tools Required
{}

## Example
{}

---
*Generated: {}*
*Complexity score: {:.2}*
*Tokens used: {}*
*Duration: {}ms*
"#,
            name.replace('-', " ").to_title_case(),
            description,
            when_to_use,
            steps,
            tools_section,
            example_section,
            Utc::now().format("%Y-%m-%d %H:%M UTC"),
            self.assess_complexity(ctx),
            ctx.tokens_used,
            ctx.duration_ms,
        );

        // Extract any scripts mentioned in the agent summary or result
        let scripts = self.extract_scripts(agent_summary, &ctx.result);

        Ok(GeneratedSkill {
            name,
            description,
            skill_md,
            scripts,
            created_at: Utc::now(),
        })
    }

    /// Generate a kebab-case skill name from the query.
    fn generate_skill_name(&self, query: &str) -> String {
        // Take first ~50 chars, extract key words
        let truncated = query.chars().take(50).collect::<String>();

        // Remove common filler words
        let filler = [
            "the", "a", "an", "to", "for", "and", "or", "but", "in", "on", "at", "by", "with",
            "from", "is", "are", "was", "were", "be", "been", "being", "have", "has", "had",
            "do", "does", "did", "will", "would", "could", "should", "may", "might", "must",
            "can", "please", "help", "me", "i", "you", "we", "they", "it", "this", "that",
        ];

        let lowercased = truncated.to_lowercase();
        let words: Vec<&str> = lowercased
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty() && w.len() > 2 && !filler.contains(w))
            .take(4)
            .collect();

        if words.is_empty() {
            format!("skill-{}", Utc::now().timestamp())
        } else {
            words.join("-")
        }
    }

    /// Extract a description from the agent summary.
    fn extract_description(&self, summary: &str, ctx: &ProblemContext) -> String {
        // Use first sentence of summary if available
        let first_sentence = summary
            .split(|c| c == '.' || c == '\n')
            .next()
            .unwrap_or("")
            .trim();

        if first_sentence.len() > 20 {
            first_sentence.to_string()
        } else {
            // Fall back to describing the query
            format!(
                "Solves problems related to: {}",
                ctx.query.chars().take(100).collect::<String>()
            )
        }
    }

    /// Infer when this skill should be triggered.
    fn infer_triggers(&self, ctx: &ProblemContext) -> String {
        let mut triggers: Vec<String> = Vec::new();

        // Look for keywords in the query
        let query_lower = ctx.query.to_lowercase();

        if query_lower.contains("file") || query_lower.contains("read") || query_lower.contains("write") {
            triggers.push("- File operations are needed".to_string());
        }
        if query_lower.contains("search") || query_lower.contains("find") || query_lower.contains("grep") {
            triggers.push("- Searching for content in files".to_string());
        }
        if query_lower.contains("web") || query_lower.contains("http") || query_lower.contains("url") {
            triggers.push("- Web content needs to be fetched".to_string());
        }
        if query_lower.contains("run") || query_lower.contains("exec") || query_lower.contains("command") {
            triggers.push("- Shell commands need to be executed".to_string());
        }
        if query_lower.contains("fix") || query_lower.contains("bug") || query_lower.contains("error") {
            triggers.push("- Debugging or fixing issues".to_string());
        }
        if query_lower.contains("create") || query_lower.contains("build") || query_lower.contains("make") {
            triggers.push("- Creating new files or content".to_string());
        }

        if triggers.is_empty() {
            triggers.push(format!("- User asks about: {}", ctx.query.chars().take(50).collect::<String>()));
        }

        triggers.join("\n")
    }

    /// Format steps as a numbered list.
    fn format_steps(&self, steps: &[String], summary: &str) -> String {
        if steps.is_empty() {
            // Try to extract steps from summary
            let lines: Vec<&str> = summary
                .lines()
                .filter(|l| {
                    let trimmed = l.trim();
                    !trimmed.is_empty()
                        && (trimmed.starts_with('-')
                            || trimmed.starts_with('*')
                            || trimmed.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false))
                })
                .take(10)
                .collect();

            if !lines.is_empty() {
                return lines
                    .iter()
                    .enumerate()
                    .map(|(i, l)| format!("{}. {}", i + 1, l.trim_start_matches(|c| c == '-' || c == '*' || c == ' ')))
                    .collect::<Vec<_>>()
                    .join("\n");
            }

            return "1. Analyze the request\n2. Execute necessary operations\n3. Verify results".to_string();
        }

        steps
            .iter()
            .enumerate()
            .map(|(i, step)| format!("{}. {}", i + 1, step))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Format tools required section.
    fn format_tools_required(&self, tools: &[String]) -> String {
        if tools.is_empty() {
            return "- No specific tools required".to_string();
        }

        let unique_tools: HashSet<&String> = tools.iter().collect();
        unique_tools
            .iter()
            .map(|t| format!("- `{}`", t))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Format the example section.
    fn format_example(&self, ctx: &ProblemContext) -> String {
        let truncated_query = if ctx.query.chars().count() > 200 {
            format!("{}...", ctx.query.chars().take(200).collect::<String>())
        } else {
            ctx.query.clone()
        };

        let truncated_result = if ctx.result.chars().count() > 500 {
            format!("{}...", ctx.result.chars().take(500).collect::<String>())
        } else {
            ctx.result.clone()
        };

        format!(
            "**Query:**\n```\n{}\n```\n\n**Result:**\n```\n{}\n```",
            truncated_query, truncated_result
        )
    }

    /// Extract scripts from the agent's work.
    fn extract_scripts(&self, summary: &str, result: &str) -> Vec<(String, String)> {
        let mut scripts = Vec::new();

        // Look for code blocks that look like scripts
        let combined = format!("{}\n{}", summary, result);

        let code_block_pattern = regex::Regex::new(r"```(\w+)?\n([\s\S]*?)```").ok();

        if let Some(re) = code_block_pattern {
            for cap in re.captures_iter(&combined) {
                let lang = cap.get(1).map(|m| m.as_str()).unwrap_or("sh");
                let code = cap.get(2).map(|m| m.as_str()).unwrap_or("");

                // Only save substantial scripts (>5 lines)
                if code.lines().count() > 5 {
                    let ext = match lang {
                        "python" | "py" => "py",
                        "javascript" | "js" => "js",
                        "typescript" | "ts" => "ts",
                        "rust" | "rs" => "rs",
                        "bash" | "sh" | "shell" => "sh",
                        _ => "txt",
                    };
                    let filename = format!("script_{}.{}", scripts.len() + 1, ext);
                    scripts.push((filename, code.to_string()));
                }
            }
        }

        scripts
    }

    /// Save a generated skill to disk.
    pub async fn save_skill(&self, skill: &GeneratedSkill) -> Result<PathBuf> {
        let skill_dir = self.skills_dir.join(&skill.name);

        // Create skill directory
        tokio::fs::create_dir_all(&skill_dir)
            .await
            .context("Failed to create skill directory")?;

        // Write SKILL.md
        let skill_path = skill_dir.join("SKILL.md");
        tokio::fs::write(&skill_path, &skill.skill_md)
            .await
            .context("Failed to write SKILL.md")?;

        // Write any scripts
        for (filename, content) in &skill.scripts {
            let script_path = skill_dir.join(filename);
            tokio::fs::write(&script_path, content)
                .await
                .with_context(|| format!("Failed to write {}", filename))?;
        }

        tracing::info!(
            "Saved skill '{}' to {}",
            skill.name,
            skill_dir.display()
        );

        Ok(skill_dir)
    }

    /// List all existing auto-generated skills.
    pub fn list_skills(&self) -> Result<Vec<String>> {
        let mut skills = Vec::new();

        if !self.skills_dir.exists() {
            return Ok(skills);
        }

        for entry in std::fs::read_dir(&self.skills_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_dir() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    // Check if it has a SKILL.md
                    if path.join("SKILL.md").exists() {
                        skills.push(name.to_string());
                    }
                }
            }
        }

        Ok(skills)
    }

    /// Find a similar skill by description.
    ///
    /// Uses basic keyword matching. Returns skill name if found.
    pub fn find_similar_skill(&self, description: &str) -> Option<String> {
        let desc_words: HashSet<String> = description
            .to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| w.len() > 3)
            .map(|w| w.to_string())
            .collect();

        if desc_words.is_empty() {
            return None;
        }

        // Check cached skills
        for skill_name in &self.skill_cache {
            let skill_words: HashSet<String> = skill_name
                .split('-')
                .map(|w| w.to_lowercase())
                .collect();

            // If 50% or more words overlap, consider it similar
            let overlap: HashSet<_> = desc_words.intersection(&skill_words).collect();
            let similarity = overlap.len() as f32 / desc_words.len().min(skill_words.len()) as f32;

            if similarity >= 0.5 {
                return Some(skill_name.clone());
            }
        }

        // Also check disk
        if let Ok(skills) = self.list_skills() {
            for skill_name in skills {
                let skill_words: HashSet<String> = skill_name
                    .split('-')
                    .map(|w| w.to_lowercase())
                    .collect();

                let overlap: HashSet<_> = desc_words.intersection(&skill_words).collect();
                let similarity = overlap.len() as f32 / desc_words.len().min(skill_words.len()).max(1) as f32;

                if similarity >= 0.5 {
                    return Some(skill_name);
                }
            }
        }

        None
    }

    /// Refresh the skill cache from disk.
    pub fn refresh_cache(&mut self) -> Result<()> {
        self.skill_cache = self.list_skills()?.into_iter().collect();
        Ok(())
    }

    /// Get the skills directory path.
    pub fn skills_dir(&self) -> &Path {
        &self.skills_dir
    }

    /// Get the minimum complexity threshold.
    pub fn min_complexity(&self) -> f32 {
        self.min_complexity
    }

    /// Set a new minimum complexity threshold.
    pub fn set_min_complexity(&mut self, threshold: f32) {
        self.min_complexity = threshold.clamp(0.0, 1.0);
    }
}

/// Extension trait for title case conversion.
trait ToTitleCase {
    fn to_title_case(&self) -> String;
}

impl ToTitleCase for str {
    fn to_title_case(&self) -> String {
        self.split_whitespace()
            .map(|word| {
                let mut chars = word.chars();
                match chars.next() {
                    None => String::new(),
                    Some(first) => {
                        first.to_uppercase().chain(chars.flat_map(|c| c.to_lowercase())).collect()
                    }
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_context() -> ProblemContext {
        ProblemContext {
            query: "How do I search for all TODO comments in Rust files?".to_string(),
            tools_used: vec![
                "exec".to_string(),
                "search_files".to_string(),
                "read_file".to_string(),
            ],
            steps_taken: vec![
                "Used search_files to find TODO patterns".to_string(),
                "Read each matching file for context".to_string(),
                "Summarized findings".to_string(),
            ],
            result: "Found 15 TODO comments across 8 files.".to_string(),
            duration_ms: 5000,
            tokens_used: 3500,
            success: true,
        }
    }

    #[test]
    fn test_assess_complexity() {
        let generator = SkillGenerator::new("/tmp/skills", 0.5);
        let ctx = sample_context();

        let score = generator.assess_complexity(&ctx);
        assert!(score > 0.0);
        assert!(score < 1.0);
        // 3 tools + 3 steps + moderate tokens/duration should give medium score
        assert!(score > 0.2);
        assert!(score < 0.7);
    }

    #[test]
    fn test_assess_complexity_failure() {
        let generator = SkillGenerator::new("/tmp/skills", 0.5);
        let mut ctx = sample_context();
        ctx.success = false;

        let score = generator.assess_complexity(&ctx);
        assert_eq!(score, 0.0);
    }

    #[test]
    fn test_generate_skill_name() {
        let generator = SkillGenerator::new("/tmp/skills", 0.5);

        let name = generator.generate_skill_name("How do I search for TODO comments in Rust files?");
        assert!(name.contains("search") || name.contains("todo") || name.contains("rust") || name.contains("comments"));
        assert!(!name.contains(' ')); // Should be kebab-case
    }

    #[test]
    fn test_categorize_tools() {
        let generator = SkillGenerator::new("/tmp/skills", 0.5);

        let tools = vec![
            "exec".to_string(),
            "read_file".to_string(),
            "write_file".to_string(),
            "web_fetch".to_string(),
        ];

        let categories = generator.categorize_tools(&tools);
        assert!(categories.contains("shell"));
        assert!(categories.contains("filesystem"));
        assert!(categories.contains("web"));
    }

    #[test]
    fn test_title_case() {
        assert_eq!("hello world".to_title_case(), "Hello World");
        assert_eq!("rust-skill-name".replace('-', " ").to_title_case(), "Rust Skill Name");
    }

    #[tokio::test]
    async fn test_generate_skill() {
        let generator = SkillGenerator::new("/tmp/skills", 0.5);
        let ctx = sample_context();
        let summary = "I searched for TODO comments using grep and found them across multiple files.";

        let skill = generator.generate_skill(&ctx, summary).await.unwrap();

        assert!(!skill.name.is_empty());
        assert!(skill.skill_md.contains("## Description"));
        assert!(skill.skill_md.contains("## Steps"));
        assert!(skill.skill_md.contains("## Tools Required"));
        assert!(skill.skill_md.contains("search_files"));
    }

    #[test]
    fn test_find_similar_skill_empty() {
        let generator = SkillGenerator::new("/tmp/skills", 0.5);
        let similar = generator.find_similar_skill("search todo comments");
        // Should be None because skill_cache is empty
        assert!(similar.is_none());
    }

    #[test]
    fn test_extract_scripts() {
        let generator = SkillGenerator::new("/tmp/skills", 0.5);

        let summary = r#"
Here's a Python script:
```python
import os
import sys

def main():
    print("Hello")
    for i in range(10):
        print(i)

if __name__ == "__main__":
    main()
```
"#;

        let scripts = generator.extract_scripts(summary, "");
        assert_eq!(scripts.len(), 1);
        assert!(scripts[0].0.ends_with(".py"));
        assert!(scripts[0].1.contains("def main()"));
    }

    #[tokio::test]
    async fn test_should_generate_skill() {
        let generator = SkillGenerator::new("/tmp/skills-test-should-gen", 0.3);

        // Simple task - should not generate
        let simple_ctx = ProblemContext {
            query: "Hello".to_string(),
            tools_used: vec!["read_file".to_string()],
            steps_taken: vec!["Read file".to_string()],
            result: "Done".to_string(),
            duration_ms: 100,
            tokens_used: 500,
            success: true,
        };
        assert!(!generator.should_generate_skill(&simple_ctx).await);

        // Complex task - should generate
        let complex_ctx = ProblemContext {
            query: "Build a complete REST API with authentication and database".to_string(),
            tools_used: vec![
                "write_file".to_string(),
                "exec".to_string(),
                "read_file".to_string(),
                "edit_file".to_string(),
                "search_files".to_string(),
            ],
            steps_taken: vec![
                "Created project structure".to_string(),
                "Set up database".to_string(),
                "Implemented auth".to_string(),
                "Added routes".to_string(),
                "Tested endpoints".to_string(),
                "Fixed issues".to_string(),
                "Deployed".to_string(),
            ],
            result: "API running at localhost:8080".to_string(),
            duration_ms: 60000,
            tokens_used: 15000,
            success: true,
        };
        assert!(generator.should_generate_skill(&complex_ctx).await);
    }
}
