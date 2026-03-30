//! CLI commands for the `skillz` tool.

use crate::matcher::Matcher;
use crate::registry::SkillRegistry;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// skillz — Universal AI agent skill manager
#[derive(Parser, Debug)]
#[command(name = "skillz", version, about = "Universal AI agent skill management CLI")]
pub struct Cli {
    /// Path to skills directory (default: ./skills)
    #[arg(short, long, default_value = "skills")]
    pub dir: PathBuf,

    #[command(subcommand)]
    pub command: Commands,
}

/// Available subcommands.
#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Initialize a skills directory
    Init,

    /// Create a new skill from template
    New {
        /// Skill name (kebab-case)
        name: String,

        /// Description
        #[arg(short, long, default_value = "")]
        description: String,

        /// Tags (comma-separated)
        #[arg(short, long, default_value = "")]
        tags: String,
    },

    /// List all skills
    List {
        /// Show disabled skills too
        #[arg(short, long)]
        all: bool,

        /// Filter by tag
        #[arg(short, long)]
        tag: Option<String>,

        /// Output as JSON
        #[arg(long)]
        json: bool,
    },

    /// Show skill details
    Show {
        /// Skill name
        name: String,
    },

    /// Enable a skill
    Enable {
        /// Skill name
        name: String,
    },

    /// Disable a skill
    Disable {
        /// Skill name
        name: String,
    },

    /// Test trigger matching against input
    Test {
        /// Skill name (or "all" for all skills)
        name: String,

        /// Input text to test
        input: String,
    },

    /// Show registry statistics
    Stats,

    /// Validate all skills (check for errors)
    Validate,
}

/// Run the CLI.
pub fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Init => cmd_init(&cli.dir),
        Commands::New {
            name,
            description,
            tags,
        } => cmd_new(&cli.dir, &name, &description, &tags),
        Commands::List { all, tag, json } => cmd_list(&cli.dir, all, tag.as_deref(), json),
        Commands::Show { name } => cmd_show(&cli.dir, &name),
        Commands::Enable { name } => cmd_enable(&cli.dir, &name),
        Commands::Disable { name } => cmd_disable(&cli.dir, &name),
        Commands::Test { name, input } => cmd_test(&cli.dir, &name, &input),
        Commands::Stats => cmd_stats(&cli.dir),
        Commands::Validate => cmd_validate(&cli.dir),
    }
}

fn cmd_init(dir: &PathBuf) -> anyhow::Result<()> {
    let path = SkillRegistry::init_skills_dir(dir)?;
    println!("✅ Skills directory initialized: {}", path.display());
    println!("  Create your first skill: skillz new my-skill");
    Ok(())
}

fn cmd_new(dir: &PathBuf, name: &str, description: &str, tags_str: &str) -> anyhow::Result<()> {
    if !dir.exists() {
        SkillRegistry::init_skills_dir(dir)?;
    }

    let mut registry = SkillRegistry::load(dir)?;
    let tags: Vec<String> = tags_str
        .split(',')
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .collect();

    let desc = if description.is_empty() {
        format!("Skill: {}", name)
    } else {
        description.to_string()
    };

    let path = registry.create_skill(name, &desc, tags)?;
    println!("✅ Created skill: {}", name);
    println!("  File: {}", path.display());
    println!("  Edit the SKILL.md to add triggers and content.");
    Ok(())
}

fn cmd_list(dir: &PathBuf, show_all: bool, tag: Option<&str>, json: bool) -> anyhow::Result<()> {
    let registry = SkillRegistry::load(dir)?;

    let skills: Vec<_> = if let Some(tag) = tag {
        registry.by_tag(tag)
    } else if show_all {
        registry.by_priority()
    } else {
        let mut s: Vec<_> = registry.enabled().collect();
        s.sort_by(|a, b| b.priority().cmp(&a.priority()));
        s
    };

    if json {
        let items: Vec<serde_json::Value> = skills
            .iter()
            .map(|s| {
                serde_json::json!({
                    "name": s.name(),
                    "description": s.description(),
                    "priority": s.priority(),
                    "status": s.metadata.status.to_string(),
                    "always_load": s.always_load(),
                    "tags": s.tags(),
                    "has_triggers": s.metadata.triggers.has_triggers(),
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&items)?);
        return Ok(());
    }

    if skills.is_empty() {
        println!("No skills found in {}", dir.display());
        println!("Create one: skillz new my-skill");
        return Ok(());
    }

    println!("📦 Skills ({})", skills.len());
    println!("{:-<60}", "");

    for skill in &skills {
        let status_icon = if skill.is_enabled() { "✅" } else { "⛔" };
        let always_icon = if skill.always_load() { " 🔄" } else { "" };
        let trigger_icon = if skill.metadata.triggers.has_triggers() {
            " ⚡"
        } else {
            ""
        };

        println!(
            "{} {} [pri:{}]{}{} — {}",
            status_icon,
            skill.name(),
            skill.priority(),
            always_icon,
            trigger_icon,
            if skill.description().is_empty() {
                "(no description)"
            } else {
                skill.description()
            }
        );

        if !skill.tags().is_empty() {
            println!(
                "    tags: {}",
                skill
                    .tags()
                    .iter()
                    .map(|t| format!("#{}", t))
                    .collect::<Vec<_>>()
                    .join(" ")
            );
        }
    }

    println!("{:-<60}", "");
    println!(
        "Legend: ✅ enabled  ⛔ disabled  🔄 always-load  ⚡ has triggers"
    );
    Ok(())
}

fn cmd_show(dir: &PathBuf, name: &str) -> anyhow::Result<()> {
    let registry = SkillRegistry::load(dir)?;
    let skill = registry
        .get(name)
        .ok_or_else(|| anyhow::anyhow!("skill '{}' not found", name))?;

    println!("📋 Skill: {}", skill.name());
    println!("  Description: {}", skill.description());
    println!("  Version:     {}", skill.metadata.version);
    println!("  Priority:    {}", skill.priority());
    println!("  Status:      {}", skill.metadata.status);
    println!("  Always load: {}", skill.always_load());
    println!("  Tags:        {:?}", skill.tags());

    if let Some(path) = &skill.source_path {
        println!("  Source:      {}", path.display());
    }

    let triggers = &skill.metadata.triggers;
    if triggers.has_triggers() {
        println!("\n  Triggers:");
        if !triggers.patterns.is_empty() {
            println!("    Patterns: {:?}", triggers.patterns);
        }
        if !triggers.keywords.is_empty() {
            println!("    Keywords: {:?}", triggers.keywords);
        }
        if !triggers.regex.is_empty() {
            println!("    Regex:    {:?}", triggers.regex);
        }
        if !triggers.globs.is_empty() {
            println!("    Globs:    {:?}", triggers.globs);
        }
    } else {
        println!("\n  Triggers: (none)");
    }

    // Validation
    let errors = skill.metadata.validate();
    if !errors.is_empty() {
        println!("\n  ⚠️  Validation errors:");
        for err in errors {
            println!("    - {}", err);
        }
    }

    println!("\n--- Body ({} bytes) ---", skill.body.len());
    println!("{}", skill.body);

    Ok(())
}

fn cmd_enable(dir: &PathBuf, name: &str) -> anyhow::Result<()> {
    let mut registry = SkillRegistry::load(dir)?;
    if registry.enable(name) {
        registry.persist_status(name)?;
        println!("✅ Enabled skill: {}", name);
    } else {
        anyhow::bail!("skill '{}' not found", name);
    }
    Ok(())
}

fn cmd_disable(dir: &PathBuf, name: &str) -> anyhow::Result<()> {
    let mut registry = SkillRegistry::load(dir)?;
    if registry.disable(name) {
        registry.persist_status(name)?;
        println!("⛔ Disabled skill: {}", name);
    } else {
        anyhow::bail!("skill '{}' not found", name);
    }
    Ok(())
}

fn cmd_test(dir: &PathBuf, name: &str, input: &str) -> anyhow::Result<()> {
    let registry = SkillRegistry::load(dir)?;
    let matcher = Matcher::new(&registry);

    println!("🧪 Testing: \"{}\"", input);
    println!();

    if name == "all" {
        let results = matcher.match_input(input);
        if results.is_empty() {
            println!("  No skills matched.");
        } else {
            for result in &results {
                println!(
                    "  ✅ {} — score: {:.3}",
                    result.skill.name(),
                    result.score
                );
                for trigger in &result.matched_triggers {
                    println!(
                        "      [{:8}] \"{}\" → \"{}\"",
                        trigger.trigger_type, trigger.trigger, trigger.matched_text
                    );
                }
            }
        }
    } else {
        let skill = registry
            .get(name)
            .ok_or_else(|| anyhow::anyhow!("skill '{}' not found", name))?;

        let results = matcher.match_input(input);
        let result = results.iter().find(|r| r.skill.name() == name);

        match result {
            Some(r) => {
                println!("  ✅ MATCHED — score: {:.3}", r.score);
                for trigger in &r.matched_triggers {
                    println!(
                        "      [{:8}] \"{}\" → \"{}\"",
                        trigger.trigger_type, trigger.trigger, trigger.matched_text
                    );
                }
            }
            None => {
                println!("  ❌ NO MATCH");
                if !skill.metadata.triggers.has_triggers() {
                    println!("  ⚠️  This skill has no triggers defined.");
                }
                if !skill.is_enabled() {
                    println!("  ⚠️  This skill is disabled.");
                }
            }
        }
    }

    Ok(())
}

fn cmd_stats(dir: &PathBuf) -> anyhow::Result<()> {
    let registry = SkillRegistry::load(dir)?;
    let stats = registry.stats();
    print!("{}", stats);
    Ok(())
}

fn cmd_validate(dir: &PathBuf) -> anyhow::Result<()> {
    let registry = SkillRegistry::load(dir)?;
    let mut has_errors = false;

    println!("🔍 Validating {} skills...\n", registry.len());

    for skill in registry.by_priority() {
        let errors = skill.metadata.validate();
        if errors.is_empty() {
            println!("  ✅ {}", skill.name());
        } else {
            has_errors = true;
            println!("  ❌ {}", skill.name());
            for err in &errors {
                println!("      - {}", err);
            }
        }
    }

    println!();
    if has_errors {
        println!("⚠️  Some skills have validation errors.");
    } else {
        println!("✅ All skills valid!");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_parse_init() {
        let cli = Cli::try_parse_from(["skillz", "init"]).unwrap();
        assert!(matches!(cli.command, Commands::Init));
        assert_eq!(cli.dir, PathBuf::from("skills"));
    }

    #[test]
    fn test_cli_parse_new() {
        let cli = Cli::try_parse_from([
            "skillz",
            "--dir",
            "/custom/path",
            "new",
            "my-skill",
            "-d",
            "My description",
            "-t",
            "web,api",
        ])
        .unwrap();
        assert!(matches!(cli.command, Commands::New { .. }));
        assert_eq!(cli.dir, PathBuf::from("/custom/path"));
    }

    #[test]
    fn test_cli_parse_list() {
        let cli = Cli::try_parse_from(["skillz", "list", "--all", "--tag", "web"]).unwrap();
        assert!(matches!(
            cli.command,
            Commands::List {
                all: true,
                tag: Some(_),
                json: false,
            }
        ));
    }

    #[test]
    fn test_cli_parse_test() {
        let cli = Cli::try_parse_from(["skillz", "test", "all", "check https://example.com"])
            .unwrap();
        assert!(matches!(cli.command, Commands::Test { .. }));
    }

    #[test]
    fn test_cli_parse_enable_disable() {
        let cli = Cli::try_parse_from(["skillz", "enable", "my-skill"]).unwrap();
        assert!(matches!(cli.command, Commands::Enable { .. }));

        let cli = Cli::try_parse_from(["skillz", "disable", "my-skill"]).unwrap();
        assert!(matches!(cli.command, Commands::Disable { .. }));
    }
}
