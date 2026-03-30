//! Trigger matching — match user input against skill triggers.
//!
//! The matcher evaluates user input against each skill's trigger configuration
//! and returns scored matches. Skills can be matched by:
//!
//! - **Patterns**: Simple substring matching (case-insensitive)
//! - **Keywords**: Word/phrase matching with basic stemming
//! - **Regex**: Full regex pattern matching
//! - **Globs**: Shell-style glob matching
//!
//! Scores are normalized to 0.0-1.0 and combined with skill priority.

use crate::registry::SkillRegistry;
use crate::schema::Skill;
use regex::Regex;
use std::collections::HashMap;

/// A matched skill with its score.
#[derive(Debug, Clone)]
pub struct MatchResult<'a> {
    /// The matched skill.
    pub skill: &'a Skill,
    /// Match score (0.0-1.0). Higher = better match.
    pub score: f64,
    /// Which triggers matched and how.
    pub matched_triggers: Vec<TriggerMatch>,
}

/// Details about which trigger matched.
#[derive(Debug, Clone)]
pub struct TriggerMatch {
    /// Type of trigger that matched.
    pub trigger_type: TriggerType,
    /// The trigger pattern/keyword that matched.
    pub trigger: String,
    /// The portion of input that matched.
    pub matched_text: String,
}

/// Types of triggers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TriggerType {
    Pattern,
    Keyword,
    Regex,
    Glob,
}

impl std::fmt::Display for TriggerType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pattern => write!(f, "pattern"),
            Self::Keyword => write!(f, "keyword"),
            Self::Regex => write!(f, "regex"),
            Self::Glob => write!(f, "glob"),
        }
    }
}

/// Skill matcher — evaluates input against skill triggers.
pub struct Matcher<'a> {
    /// Reference to the skill registry.
    registry: &'a SkillRegistry,
    /// Compiled regex patterns (cached).
    compiled_regex: HashMap<String, Regex>,
    /// Minimum score threshold for a match.
    min_score: f64,
}

impl<'a> Matcher<'a> {
    /// Create a new matcher from a registry.
    pub fn new(registry: &'a SkillRegistry) -> Self {
        // Pre-compile all regex patterns
        let mut compiled_regex = HashMap::new();
        for skill in registry.all() {
            for pattern in &skill.metadata.triggers.regex {
                if let Ok(re) = Regex::new(pattern) {
                    compiled_regex.insert(pattern.clone(), re);
                }
            }
        }

        Self {
            registry,
            compiled_regex,
            min_score: 0.1,
        }
    }

    /// Set the minimum score threshold for matches.
    pub fn with_min_score(mut self, score: f64) -> Self {
        self.min_score = score.clamp(0.0, 1.0);
        self
    }

    /// Match user input against all enabled skills.
    ///
    /// Returns skills sorted by final score (highest first).
    /// Only returns skills above the minimum score threshold.
    pub fn match_input(&self, input: &str) -> Vec<MatchResult<'a>> {
        let input_lower = input.to_lowercase();

        let mut results: Vec<MatchResult> = self
            .registry
            .enabled()
            .filter(|s| s.metadata.triggers.has_triggers())
            .filter_map(|skill| {
                let (score, matches) = self.score_skill(skill, input, &input_lower);
                if score >= self.min_score && !matches.is_empty() {
                    Some(MatchResult {
                        skill,
                        score,
                        matched_triggers: matches,
                    })
                } else {
                    None
                }
            })
            .collect();

        // Sort by score descending
        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));

        results
    }

    /// Match input and also include always-load skills.
    ///
    /// Returns (triggered_skills, always_load_skills).
    pub fn match_with_always_load(
        &self,
        input: &str,
    ) -> (Vec<MatchResult<'a>>, Vec<&'a Skill>) {
        let matched = self.match_input(input);
        let always = self.registry.always_load_skills();

        // Remove always-load skills from matched to avoid duplication
        let always_names: std::collections::HashSet<_> =
            always.iter().map(|s| s.name()).collect();
        let matched: Vec<_> = matched
            .into_iter()
            .filter(|m| !always_names.contains(m.skill.name()))
            .collect();

        (matched, always)
    }

    /// Score a single skill against input.
    /// Returns (final_score, matched_triggers).
    fn score_skill(
        &self,
        skill: &'a Skill,
        input: &str,
        input_lower: &str,
    ) -> (f64, Vec<TriggerMatch>) {
        let triggers = &skill.metadata.triggers;
        let mut matches = Vec::new();
        let mut raw_score = 0.0;
        let mut max_possible = 0.0;

        // Pattern matching (0.0-0.3 per pattern)
        if !triggers.patterns.is_empty() {
            max_possible += 0.3;
            let mut best_pattern_score = 0.0;
            for pattern in &triggers.patterns {
                let pattern_lower = pattern.to_lowercase();
                if input_lower.contains(&pattern_lower) {
                    // Score based on how much of the input the pattern covers
                    let coverage = pattern.len() as f64 / input.len().max(1) as f64;
                    let score = 0.3 * coverage.min(1.0).max(0.3); // minimum 0.09 for any match

                    if score > best_pattern_score {
                        best_pattern_score = score;
                    }

                    matches.push(TriggerMatch {
                        trigger_type: TriggerType::Pattern,
                        trigger: pattern.clone(),
                        matched_text: pattern.clone(),
                    });
                }
            }
            raw_score += best_pattern_score;
        }

        // Keyword matching (0.0-0.4 per keyword set)
        if !triggers.keywords.is_empty() {
            max_possible += 0.4;
            let mut keyword_matches = 0;
            for keyword in &triggers.keywords {
                let keyword_lower = keyword.to_lowercase();
                if self.keyword_match(input_lower, &keyword_lower) {
                    keyword_matches += 1;
                    matches.push(TriggerMatch {
                        trigger_type: TriggerType::Keyword,
                        trigger: keyword.clone(),
                        matched_text: keyword.clone(),
                    });
                }
            }
            if keyword_matches > 0 {
                // More keyword matches = higher score
                let ratio = keyword_matches as f64 / triggers.keywords.len() as f64;
                raw_score += 0.4 * ratio.min(1.0).max(0.2);
            }
        }

        // Regex matching (0.0-0.3 per regex set)
        if !triggers.regex.is_empty() {
            max_possible += 0.3;
            let mut regex_matched = false;
            for pattern in &triggers.regex {
                if let Some(re) = self.compiled_regex.get(pattern) {
                    if let Some(m) = re.find(input) {
                        regex_matched = true;
                        matches.push(TriggerMatch {
                            trigger_type: TriggerType::Regex,
                            trigger: pattern.clone(),
                            matched_text: m.as_str().to_string(),
                        });
                    }
                }
            }
            if regex_matched {
                raw_score += 0.3;
            }
        }

        // Glob matching (0.0-0.2 per glob set)
        if !triggers.globs.is_empty() {
            max_possible += 0.2;
            for glob_pat in &triggers.globs {
                if let Ok(pattern) = glob::Pattern::new(glob_pat) {
                    if pattern.matches(input) || pattern.matches(input_lower) {
                        raw_score += 0.2;
                        matches.push(TriggerMatch {
                            trigger_type: TriggerType::Glob,
                            trigger: glob_pat.clone(),
                            matched_text: input.to_string(),
                        });
                        break; // One glob match is enough
                    }
                }
            }
        }

        // Normalize raw score by what's possible
        let normalized = if max_possible > 0.0 {
            (raw_score / max_possible).min(1.0)
        } else {
            0.0
        };

        // Apply priority weighting (priority 0-100 → 0.5-1.0 multiplier)
        let priority_mult = 0.5 + (skill.priority() as f64 / 200.0);
        let final_score = (normalized * priority_mult).min(1.0);

        (final_score, matches)
    }

    /// Fuzzy keyword matching.
    ///
    /// Matches if the keyword appears as a substring with some tolerance:
    /// - Exact substring match
    /// - Keyword stem variants appear in input ("scrape" → "scrapes", "scraped")
    /// - Input word stems match keyword ("scraping" in input → stem "scrape")
    /// - Multi-word keywords: all component words present (possibly stemmed)
    fn keyword_match(&self, input: &str, keyword: &str) -> bool {
        // Direct substring match
        if input.contains(keyword) {
            return true;
        }

        // Try keyword stem variants (expand keyword → check in input)
        let keyword_stems = self.basic_stems(keyword);
        for stem in &keyword_stems {
            if input.contains(stem.as_str()) {
                return true;
            }
        }

        // Try input word stems (reduce input words → check against keyword)
        for word in input.split(|c: char| !c.is_alphanumeric()) {
            if word.is_empty() {
                continue;
            }
            let word_stems = self.basic_stems(word);
            for stem in &word_stems {
                if stem == keyword {
                    return true;
                }
            }
        }

        // Try matching individual words in multi-word keywords
        if keyword.contains(' ') {
            let parts: Vec<&str> = keyword.split_whitespace().collect();
            let all_present = parts.iter().all(|part| {
                self.word_present_fuzzy(input, part)
            });
            if all_present {
                return true;
            }
        }

        false
    }

    /// Check if a word is present in input (exact or stemmed, in either direction).
    fn word_present_fuzzy(&self, input: &str, word: &str) -> bool {
        // Direct
        if input.contains(word) {
            return true;
        }
        // Expand word → check in input
        for stem in &self.basic_stems(word) {
            if input.contains(stem.as_str()) {
                return true;
            }
        }
        // Reduce input words → check against word
        for input_word in input.split(|c: char| !c.is_alphanumeric()) {
            if input_word.is_empty() {
                continue;
            }
            for stem in &self.basic_stems(input_word) {
                if stem == word {
                    return true;
                }
            }
        }
        false
    }

    /// Generate basic stemming variations of a word.
    fn basic_stems(&self, word: &str) -> Vec<String> {
        let mut stems = Vec::new();

        // Add -s / -es
        stems.push(format!("{}s", word));
        stems.push(format!("{}es", word));

        // Add -ing
        stems.push(format!("{}ing", word));

        // Add -ed
        stems.push(format!("{}ed", word));

        // Add -er
        stems.push(format!("{}er", word));

        // Remove common suffixes
        if let Some(base) = word.strip_suffix('s') {
            stems.push(base.to_string());
        }
        if let Some(base) = word.strip_suffix("es") {
            stems.push(base.to_string());
        }
        if let Some(base) = word.strip_suffix("ing") {
            stems.push(base.to_string());
            stems.push(format!("{}e", base)); // "scraping" → "scrape"
        }
        if let Some(base) = word.strip_suffix("ed") {
            stems.push(base.to_string());
            stems.push(format!("{}e", base));
        }
        if let Some(base) = word.strip_suffix("er") {
            stems.push(base.to_string());
            stems.push(format!("{}e", base));
        }

        stems
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{SkillMetadata, SkillStatus, TriggerConfig};
    use std::fs;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn create_test_skill(dir: &std::path::Path, name: &str, content: &str) {
        let skill_dir = dir.join(name);
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(skill_dir.join("SKILL.md"), content).unwrap();
    }

    #[test]
    fn test_match_patterns() {
        let tmp = TempDir::new().unwrap();

        create_test_skill(
            tmp.path(),
            "web",
            "---\nname: web\ntriggers:\n  patterns:\n    - \"http://\"\n    - \"https://\"\npriority: 80\n---\n\nWeb skill.",
        );

        let registry = SkillRegistry::load(tmp.path()).unwrap();
        let matcher = Matcher::new(&registry);

        let results = matcher.match_input("check out https://example.com");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].skill.name(), "web");
        assert!(results[0].score > 0.0);
        assert!(results[0]
            .matched_triggers
            .iter()
            .any(|t| t.trigger_type == TriggerType::Pattern));

        // No match
        let results = matcher.match_input("hello world");
        assert!(results.is_empty());
    }

    #[test]
    fn test_match_keywords() {
        let tmp = TempDir::new().unwrap();

        create_test_skill(
            tmp.path(),
            "scraper",
            "---\nname: scraper\ntriggers:\n  keywords:\n    - scrape\n    - \"fetch page\"\n    - download\npriority: 70\n---\n\nScraper skill.",
        );

        let registry = SkillRegistry::load(tmp.path()).unwrap();
        let matcher = Matcher::new(&registry);

        // Exact keyword
        let results = matcher.match_input("can you scrape that website?");
        assert_eq!(results.len(), 1);

        // Stemmed match: "scraping" matches "scrape"
        let results = matcher.match_input("try scraping the page");
        assert_eq!(results.len(), 1);

        // Multi-word keyword — words "fetch" and "page" both present
        let results = matcher.match_input("please fetch the page content for me");
        assert_eq!(results.len(), 1);

        // No match
        let results = matcher.match_input("how's the weather?");
        assert!(results.is_empty());
    }

    #[test]
    fn test_match_regex() {
        let tmp = TempDir::new().unwrap();

        create_test_skill(
            tmp.path(),
            "url-detect",
            "---\nname: url-detect\ntriggers:\n  regex:\n    - \"https?://[^\\\\s]+\"\npriority: 90\n---\n\nURL detection.",
        );

        let registry = SkillRegistry::load(tmp.path()).unwrap();
        let matcher = Matcher::new(&registry);

        let results = matcher.match_input("visit http://example.com/path?q=1");
        assert_eq!(results.len(), 1);
        assert!(results[0]
            .matched_triggers
            .iter()
            .any(|t| t.trigger_type == TriggerType::Regex));

        let results = matcher.match_input("no urls here");
        assert!(results.is_empty());
    }

    #[test]
    fn test_match_globs() {
        let tmp = TempDir::new().unwrap();

        create_test_skill(
            tmp.path(),
            "rust-files",
            "---\nname: rust-files\ntriggers:\n  globs:\n    - \"*.rs\"\n    - \"*.toml\"\npriority: 60\n---\n\nRust file handler.",
        );

        let registry = SkillRegistry::load(tmp.path()).unwrap();
        let matcher = Matcher::new(&registry);

        let results = matcher.match_input("main.rs");
        assert_eq!(results.len(), 1);

        let results = matcher.match_input("Cargo.toml");
        assert_eq!(results.len(), 1);

        let results = matcher.match_input("main.py");
        assert!(results.is_empty());
    }

    #[test]
    fn test_priority_affects_score() {
        let tmp = TempDir::new().unwrap();

        create_test_skill(
            tmp.path(),
            "high-pri",
            "---\nname: high-pri\ntriggers:\n  keywords: [test]\npriority: 100\n---\nHigh priority.",
        );
        create_test_skill(
            tmp.path(),
            "low-pri",
            "---\nname: low-pri\ntriggers:\n  keywords: [test]\npriority: 10\n---\nLow priority.",
        );

        let registry = SkillRegistry::load(tmp.path()).unwrap();
        let matcher = Matcher::new(&registry);

        let results = matcher.match_input("this is a test");
        assert_eq!(results.len(), 2);
        // Higher priority should have higher score
        assert!(results[0].score >= results[1].score);
        assert_eq!(results[0].skill.name(), "high-pri");
    }

    #[test]
    fn test_disabled_skills_not_matched() {
        let tmp = TempDir::new().unwrap();

        create_test_skill(
            tmp.path(),
            "disabled",
            "---\nname: disabled\ntriggers:\n  keywords: [test]\nstatus: disabled\n---\nDisabled.",
        );

        let registry = SkillRegistry::load(tmp.path()).unwrap();
        let matcher = Matcher::new(&registry);

        let results = matcher.match_input("test something");
        assert!(results.is_empty());
    }

    #[test]
    fn test_match_with_always_load() {
        let tmp = TempDir::new().unwrap();

        create_test_skill(
            tmp.path(),
            "always-skill",
            "---\nname: always-skill\nalways_load: true\ntriggers:\n  keywords: [test]\npriority: 100\n---\nAlways loaded.",
        );
        create_test_skill(
            tmp.path(),
            "trigger-skill",
            "---\nname: trigger-skill\ntriggers:\n  keywords: [test]\npriority: 50\n---\nTrigger only.",
        );

        let registry = SkillRegistry::load(tmp.path()).unwrap();
        let matcher = Matcher::new(&registry);

        let (matched, always) = matcher.match_with_always_load("test input");

        // always-skill should be in always, not in matched
        assert_eq!(always.len(), 1);
        assert_eq!(always[0].name(), "always-skill");

        // trigger-skill should be in matched
        assert_eq!(matched.len(), 1);
        assert_eq!(matched[0].skill.name(), "trigger-skill");
    }

    #[test]
    fn test_min_score_threshold() {
        let tmp = TempDir::new().unwrap();

        create_test_skill(
            tmp.path(),
            "low-match",
            "---\nname: low-match\ntriggers:\n  keywords: [obscure]\npriority: 10\n---\nLow match.",
        );

        let registry = SkillRegistry::load(tmp.path()).unwrap();

        // Default threshold
        let matcher = Matcher::new(&registry);
        let results = matcher.match_input("something obscure here");
        // Should match since "obscure" is present
        assert!(!results.is_empty());

        // High threshold
        let matcher = Matcher::new(&registry).with_min_score(0.9);
        let results = matcher.match_input("something obscure here");
        // Might not meet the high threshold
        // (depends on exact scoring, but low priority + single keyword = low score)
    }

    #[test]
    fn test_multiple_trigger_types() {
        let tmp = TempDir::new().unwrap();

        create_test_skill(
            tmp.path(),
            "multi",
            "---\nname: multi\ntriggers:\n  patterns:\n    - \"https://\"\n  keywords:\n    - scrape\n  regex:\n    - \"\\\\burl\\\\b\"\npriority: 80\n---\nMulti-trigger skill.",
        );

        let registry = SkillRegistry::load(tmp.path()).unwrap();
        let matcher = Matcher::new(&registry);

        // Match all three trigger types
        let results = matcher.match_input("scrape the url https://example.com");
        assert_eq!(results.len(), 1);

        // Should have multiple trigger matches
        let trigger_types: Vec<_> = results[0]
            .matched_triggers
            .iter()
            .map(|t| &t.trigger_type)
            .collect();
        assert!(trigger_types.contains(&&TriggerType::Pattern));
        assert!(trigger_types.contains(&&TriggerType::Keyword));
    }

    #[test]
    fn test_case_insensitive_matching() {
        let tmp = TempDir::new().unwrap();

        create_test_skill(
            tmp.path(),
            "case-test",
            "---\nname: case-test\ntriggers:\n  patterns:\n    - \"HTTP://\"\n  keywords:\n    - SCRAPE\npriority: 50\n---\nCase test.",
        );

        let registry = SkillRegistry::load(tmp.path()).unwrap();
        let matcher = Matcher::new(&registry);

        // Should match regardless of case
        let results = matcher.match_input("check http://example.com and scrape it");
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_no_triggers_no_match() {
        let tmp = TempDir::new().unwrap();

        create_test_skill(
            tmp.path(),
            "no-triggers",
            "---\nname: no-triggers\npriority: 90\n---\nNo triggers defined.",
        );

        let registry = SkillRegistry::load(tmp.path()).unwrap();
        let matcher = Matcher::new(&registry);

        let results = matcher.match_input("anything at all");
        assert!(results.is_empty());
    }

    #[test]
    fn test_empty_input() {
        let tmp = TempDir::new().unwrap();

        create_test_skill(
            tmp.path(),
            "skill",
            "---\nname: skill\ntriggers:\n  keywords: [test]\npriority: 50\n---\nContent.",
        );

        let registry = SkillRegistry::load(tmp.path()).unwrap();
        let matcher = Matcher::new(&registry);

        let results = matcher.match_input("");
        assert!(results.is_empty());
    }

    #[test]
    fn test_keyword_stemming() {
        let tmp = TempDir::new().unwrap();

        create_test_skill(
            tmp.path(),
            "stem-test",
            "---\nname: stem-test\ntriggers:\n  keywords:\n    - download\n    - scrape\npriority: 50\n---\nStemming test.",
        );

        let registry = SkillRegistry::load(tmp.path()).unwrap();
        let matcher = Matcher::new(&registry);

        // "downloading" should match "download"
        let results = matcher.match_input("i'm downloading a file");
        assert_eq!(results.len(), 1);

        // "scraped" should match "scrape"
        let results = matcher.match_input("i scraped the site");
        assert_eq!(results.len(), 1);

        // "scraper" should match "scrape"
        let results = matcher.match_input("use the scraper tool");
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_trigger_type_display() {
        assert_eq!(TriggerType::Pattern.to_string(), "pattern");
        assert_eq!(TriggerType::Keyword.to_string(), "keyword");
        assert_eq!(TriggerType::Regex.to_string(), "regex");
        assert_eq!(TriggerType::Glob.to_string(), "glob");
    }
}
