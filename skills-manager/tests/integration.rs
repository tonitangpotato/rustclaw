//! Integration tests for skills-manager.

use skills_manager::{Matcher, Parser, Skill, SkillRegistry};
use std::fs;
use std::path::Path;
use tempfile::TempDir;

/// Helper: create a skill directory with SKILL.md
fn create_skill(dir: &Path, name: &str, content: &str) {
    let skill_dir = dir.join(name);
    fs::create_dir_all(&skill_dir).unwrap();
    fs::write(skill_dir.join("SKILL.md"), content).unwrap();
}

/// Full pipeline: create skills → load registry → match input → verify results
#[test]
fn test_full_pipeline() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();

    // Create a diverse set of skills
    create_skill(
        dir,
        "idea-intake",
        r#"---
name: idea-intake
description: Process incoming ideas and links
triggers:
  patterns:
    - "http://"
    - "https://"
  keywords:
    - "idea"
    - "想法"
    - "intake this"
    - "记录一下"
priority: 80
always_load: true
tags: [productivity, capture]
---

# Idea Intake Pipeline

Process ideas, links, and media into structured knowledge.

## Steps
1. Extract content
2. Analyze and summarize
3. Store in IDEAS.md
"#,
    );

    create_skill(
        dir,
        "code-review",
        r#"---
name: code-review
description: Review code changes for quality
triggers:
  keywords:
    - "review"
    - "code review"
    - "PR"
    - "pull request"
  globs:
    - "*.rs"
    - "*.py"
    - "*.ts"
priority: 70
tags: [dev, quality]
---

# Code Review Skill

Automated code review with best practices.
"#,
    );

    create_skill(
        dir,
        "web-research",
        r#"---
name: web-research
description: Research topics on the web
triggers:
  keywords:
    - "research"
    - "look up"
    - "find out"
  regex:
    - "what is .+"
    - "how (do|does|to) .+"
priority: 60
tags: [research, web]
---

# Web Research Skill

Deep research on any topic.
"#,
    );

    create_skill(
        dir,
        "disabled-skill",
        r#"---
name: disabled-skill
description: This is disabled
triggers:
  keywords: [anything]
status: disabled
---

Disabled content.
"#,
    );

    // Load registry
    let registry = SkillRegistry::load(dir).unwrap();
    assert_eq!(registry.len(), 4);
    assert_eq!(registry.enabled().count(), 3);

    // Check always-load
    let always = registry.always_load_skills();
    assert_eq!(always.len(), 1);
    assert_eq!(always[0].name(), "idea-intake");

    // Create matcher
    let matcher = Matcher::new(&registry);

    // Test URL matching → idea-intake
    let results = matcher.match_input("check out https://github.com/rust-lang/rust");
    assert!(!results.is_empty());
    assert_eq!(results[0].skill.name(), "idea-intake");

    // Test keyword matching → code-review
    let results = matcher.match_input("can you review my PR?");
    assert!(!results.is_empty());
    let names: Vec<_> = results.iter().map(|r| r.skill.name()).collect();
    assert!(names.contains(&"code-review"));

    // Test regex matching → web-research
    let results = matcher.match_input("what is the capital of France?");
    assert!(!results.is_empty());
    let names: Vec<_> = results.iter().map(|r| r.skill.name()).collect();
    assert!(names.contains(&"web-research"));

    // Test disabled skill doesn't match
    let results = matcher.match_input("anything goes here");
    let names: Vec<_> = results.iter().map(|r| r.skill.name()).collect();
    assert!(!names.contains(&"disabled-skill"));

    // Test match_with_always_load
    let (matched, always) = matcher.match_with_always_load("check https://example.com");
    assert_eq!(always.len(), 1);
    assert_eq!(always[0].name(), "idea-intake");
    // idea-intake shouldn't be in matched since it's in always
    assert!(!matched.iter().any(|m| m.skill.name() == "idea-intake"));

    // Stats
    let stats = registry.stats();
    assert_eq!(stats.total, 4);
    assert_eq!(stats.enabled, 3);
    assert_eq!(stats.disabled, 1);
    assert!(stats.tags.contains(&"productivity".to_string()));
    assert!(stats.tags.contains(&"dev".to_string()));
}

/// Test skill creation and persistence
#[test]
fn test_create_and_reload() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path().join("skills");

    // Init
    SkillRegistry::init_skills_dir(&dir).unwrap();

    // Create skills
    let mut registry = SkillRegistry::load(&dir).unwrap();
    assert!(registry.is_empty());

    registry
        .create_skill("skill-alpha", "First skill", vec!["test".to_string()])
        .unwrap();
    registry
        .create_skill("skill-beta", "Second skill", vec!["test".to_string()])
        .unwrap();

    assert_eq!(registry.len(), 2);

    // Reload from disk — should find both
    let registry2 = SkillRegistry::load(&dir).unwrap();
    assert_eq!(registry2.len(), 2);
    assert!(registry2.get("skill-alpha").is_some());
    assert!(registry2.get("skill-beta").is_some());
}

/// Test enable/disable persistence
#[test]
fn test_toggle_persistence() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();

    create_skill(
        dir,
        "toggleable",
        "---\nname: toggleable\ndescription: Toggle me\n---\n\nToggle content.",
    );

    // Disable and persist
    let mut registry = SkillRegistry::load(dir).unwrap();
    assert!(registry.get("toggleable").unwrap().is_enabled());

    registry.disable("toggleable");
    registry.persist_status("toggleable").unwrap();

    // Reload — should still be disabled
    let registry2 = SkillRegistry::load(dir).unwrap();
    assert!(!registry2.get("toggleable").unwrap().is_enabled());

    // Re-enable and persist
    let mut registry3 = SkillRegistry::load(dir).unwrap();
    registry3.enable("toggleable");
    registry3.persist_status("toggleable").unwrap();

    let registry4 = SkillRegistry::load(dir).unwrap();
    assert!(registry4.get("toggleable").unwrap().is_enabled());
}

/// Test legacy skill migration (no frontmatter)
#[test]
fn test_legacy_skill_compatibility() {
    let tmp = TempDir::new().unwrap();

    // Create old-style skill (like RustClaw's current format)
    create_skill(
        tmp.path(),
        "idea-intake",
        r#"# SKILL: Idea Intake Pipeline

> Automatically process incoming ideas, links, and media into structured knowledge.

## Trigger Conditions

This skill activates when potato sends:
- A URL/link (auto-detect `http://` or `https://`)
- A voice/audio message describing an idea
- An explicit "idea:" or "想法:" prefix
"#,
    );

    let registry = SkillRegistry::load(tmp.path()).unwrap();
    assert_eq!(registry.len(), 1);

    let skill = registry.get("idea-intake").unwrap();
    assert_eq!(skill.name(), "idea-intake"); // Uses dir name for legacy
    assert!(skill.is_enabled());
    assert_eq!(skill.priority(), 50); // default
    assert!(skill.body.contains("Idea Intake Pipeline"));
}

/// Test parser roundtrip
#[test]
fn test_parser_roundtrip() {
    let original = r#"---
name: roundtrip-test
description: Test serialization roundtrip
triggers:
  patterns:
    - "http://"
  keywords:
    - test
    - roundtrip
priority: 75
always_load: true
tags:
  - test
  - serialization
---

# Roundtrip Test

This content should survive roundtrip serialization.

## Steps
1. Parse
2. Serialize
3. Parse again
4. Compare
"#;

    let parser = Parser::new();

    // Parse
    let skill = parser.parse_str(original).unwrap();
    assert_eq!(skill.name(), "roundtrip-test");
    assert_eq!(skill.priority(), 75);
    assert!(skill.always_load());

    // Serialize
    let serialized = Parser::serialize_skill(&skill).unwrap();

    // Parse again
    let reparsed = parser.parse_str(&serialized).unwrap();

    // Compare
    assert_eq!(reparsed.name(), skill.name());
    assert_eq!(reparsed.description(), skill.description());
    assert_eq!(reparsed.priority(), skill.priority());
    assert_eq!(reparsed.always_load(), skill.always_load());
    assert_eq!(reparsed.tags(), skill.tags());
    assert!(reparsed.body.contains("Roundtrip Test"));
    assert!(reparsed.body.contains("survive roundtrip"));
}

/// Test prompt generation with truncation
#[test]
fn test_prompt_skills_ordering_and_truncation() {
    let tmp = TempDir::new().unwrap();

    // High priority, small body
    create_skill(
        tmp.path(),
        "high",
        "---\nname: high\npriority: 90\nmax_body_size: 4096\n---\n\nHigh priority content.",
    );

    // Low priority, large body that will be truncated
    let big_body = "x".repeat(10000);
    create_skill(
        tmp.path(),
        "big-low",
        &format!(
            "---\nname: big-low\npriority: 10\nmax_body_size: 100\n---\n\n{}",
            big_body
        ),
    );

    // Disabled, should not appear
    create_skill(
        tmp.path(),
        "disabled",
        "---\nname: disabled\nstatus: disabled\npriority: 100\n---\n\nDisabled.",
    );

    let registry = SkillRegistry::load(tmp.path()).unwrap();
    let prompts = registry.prompt_skills();

    // Only 2 enabled skills
    assert_eq!(prompts.len(), 2);

    // Ordered by priority: high first
    assert_eq!(prompts[0].0, "high");
    assert_eq!(prompts[1].0, "big-low");

    // High is not truncated
    assert!(!prompts[0].1.contains("truncated"));

    // Big-low is truncated
    assert!(prompts[1].1.contains("truncated"));
    assert!(prompts[1].1.len() < 200); // truncated to max_body_size + marker
}

/// Test concurrent matching performance
#[test]
fn test_matching_performance() {
    let tmp = TempDir::new().unwrap();

    // Create 20 skills with various triggers
    for i in 0..20 {
        create_skill(
            tmp.path(),
            &format!("skill-{:02}", i),
            &format!(
                "---\nname: skill-{:02}\ntriggers:\n  patterns:\n    - \"pattern{}\"\n  keywords:\n    - \"keyword{}\"\n    - \"term{}\"\npriority: {}\ntags:\n  - group{}\n---\n\nContent for skill {}.",
                i, i, i, i, (i * 5) % 100, i % 3, i
            ),
        );
    }

    let registry = SkillRegistry::load(tmp.path()).unwrap();
    assert_eq!(registry.len(), 20);

    let matcher = Matcher::new(&registry);

    // Time the matching
    let start = std::time::Instant::now();
    for _ in 0..100 {
        let _ = matcher.match_input("test pattern5 with keyword10 and some extra text");
    }
    let elapsed = start.elapsed();

    // 100 matches against 20 skills should be well under 100ms
    assert!(
        elapsed.as_millis() < 100,
        "Matching took {}ms for 100 iterations, expected <100ms",
        elapsed.as_millis()
    );
}

/// Test Chinese trigger matching
#[test]
fn test_chinese_triggers() {
    let tmp = TempDir::new().unwrap();

    create_skill(
        tmp.path(),
        "chinese-intake",
        r#"---
name: chinese-intake
description: 处理中文输入
triggers:
  keywords:
    - "想法"
    - "记录一下"
    - "收藏"
priority: 80
---

# 中文技能

处理中文输入和想法。
"#,
    );

    let registry = SkillRegistry::load(tmp.path()).unwrap();
    let matcher = Matcher::new(&registry);

    let results = matcher.match_input("我有一个想法");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].skill.name(), "chinese-intake");

    let results = matcher.match_input("帮我记录一下这个");
    assert_eq!(results.len(), 1);
}

/// Test edge case: skill with all trigger types
#[test]
fn test_all_trigger_types() {
    let tmp = TempDir::new().unwrap();

    create_skill(
        tmp.path(),
        "all-triggers",
        r#"---
name: all-triggers
description: Skill with every trigger type
triggers:
  patterns:
    - "https://"
  keywords:
    - "fetch"
  regex:
    - "\\d{3}-\\d{4}"
  globs:
    - "*.html"
priority: 90
---

All trigger types.
"#,
    );

    let registry = SkillRegistry::load(tmp.path()).unwrap();
    let matcher = Matcher::new(&registry);

    // Pattern match
    let r = matcher.match_input("go to https://example.com");
    assert_eq!(r.len(), 1);

    // Keyword match
    let r = matcher.match_input("fetch the data please");
    assert_eq!(r.len(), 1);

    // Regex match
    let r = matcher.match_input("call 555-1234");
    assert_eq!(r.len(), 1);

    // Glob match
    let r = matcher.match_input("index.html");
    assert_eq!(r.len(), 1);

    // No match
    let r = matcher.match_input("nothing relevant here");
    assert!(r.is_empty());
}
