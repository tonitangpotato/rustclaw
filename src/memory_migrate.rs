//! Engram memory maintenance helpers (ISS-021 Phase 5a).
//!
//! # Scope
//!
//! This module implements the **read-only diagnostic** for the in-content
//! channel-header pollution that accumulated in the engram database before
//! ISS-021 Phase 2+3 moved envelope metadata into `StorageMeta::user_metadata`.
//!
//! It intentionally does **not** implement the wet-run migration path
//! (content rewrite + re-embedding). That path is gated on Phase 5b, which
//! must first prove via counterfactual measurement that migration would
//! actually improve recall quality. See
//! `.gid/issues/ISS-021-message-context-side-channel/issue.md`.
//!
//! # Why a direct SQLite read (not the engramai API)?
//!
//! The diagnostic needs only `content` + `metadata` for every row. Going
//! through `engramai::Memory` would spin up embeddings, baseline trackers,
//! and extractor state for a job that just walks a single table. A bare
//! rusqlite read is faster, safer (no writes possible), and decouples the
//! tool from engramai's evolving public API.

use anyhow::{anyhow, Context, Result};
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;
use std::path::Path;

use crate::context::Envelope;

/// Summary stats for a single migration scan. Printed to stdout and also
/// returned so tests can assert on it directly without parsing output.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ScanReport {
    /// Total live (non-soft-deleted) rows examined.
    pub total_rows: usize,
    /// Rows whose `metadata` already contains an `envelope` key — skipped.
    pub already_migrated: usize,
    /// Rows with no envelope AND whose content matched the header regex.
    pub match_count: usize,
    /// Rows with no envelope AND whose content did NOT match — most common
    /// case (plain text, LLM responses, synthesis outputs, etc.).
    pub unmatched: usize,
    /// Per-channel tally among `match_count`. Gives a quick sense of which
    /// channels contributed most to the legacy pollution.
    pub by_channel: Vec<(String, usize)>,
}

/// CLI entry point for `rustclaw memory migrate-envelope`.
pub fn run_migrate_envelope(
    db_path: &str,
    dry_run: bool,
    backup_to: Option<&str>,
    sample_limit: usize,
) -> Result<()> {
    if !dry_run {
        // Phase 5a is intentionally read-only. A wet run must be preceded by
        // Phase 5b's counterfactual evidence — blocking here prevents an
        // eager operator from bypassing that gate.
        return Err(anyhow!(
            "wet-run migration is not enabled: Phase 5a is diagnostic-only. \
             Phase 5b (counterfactual measurement) must produce a recall \
             delta >= 0.15 before wet run is unlocked. See \
             .gid/issues/ISS-021-message-context-side-channel/issue.md"
        ));
    }

    if backup_to.is_some() {
        // Stable flag surface for Phase 5b/5c; no-op in dry run. Announce
        // so the operator isn't surprised.
        println!(
            "ℹ  --backup-to is ignored in dry-run mode (no writes performed); \
             the flag is accepted for forward compatibility."
        );
    }

    let report = scan_db(db_path, sample_limit)?;
    print_report(&report);
    Ok(())
}

/// Open the engram DB read-only and walk every live row. Read-only open
/// flags guarantee no accidental mutation even if a future bug tried to
/// call `execute()`.
pub fn scan_db(db_path: &str, sample_limit: usize) -> Result<ScanReport> {
    let path = Path::new(db_path);
    if !path.exists() {
        return Err(anyhow!(
            "database not found at {} (hint: run from workspace root or pass --db PATH)",
            db_path
        ));
    }

    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .with_context(|| format!("opening {} read-only", db_path))?;

    // We only need content + metadata; avoid selecting unrelated columns to
    // keep the row fetch tight even on large DBs.
    let mut stmt = conn.prepare(
        "SELECT id, content, metadata FROM memories WHERE deleted_at IS NULL",
    )?;
    let rows = stmt.query_map([], |row| {
        let id: String = row.get(0)?;
        let content: String = row.get(1)?;
        let metadata: Option<String> = row.get(2)?;
        Ok((id, content, metadata))
    })?;

    let mut report = ScanReport::default();
    let mut samples: Vec<Sample> = Vec::new();
    // Small Vec-based tally keeps output order deterministic (by first
    // encounter) — HashMap iteration order would shuffle test assertions.
    let mut channel_counts: Vec<(String, usize)> = Vec::new();

    for row in rows {
        let (id, content, metadata) = row?;
        report.total_rows += 1;

        if metadata_has_envelope(metadata.as_deref()) {
            report.already_migrated += 1;
            continue;
        }

        match Envelope::strip_from_content(&content) {
            Some(result) => {
                report.match_count += 1;
                // Tally by channel label.
                if let Some(entry) =
                    channel_counts.iter_mut().find(|(c, _)| c == &result.channel)
                {
                    entry.1 += 1;
                } else {
                    channel_counts.push((result.channel.clone(), 1));
                }
                if samples.len() < sample_limit {
                    samples.push(Sample {
                        id,
                        channel: result.channel.clone(),
                        header_body: result.header_body.clone(),
                        had_reply_block: result.reply_block.is_some(),
                        before_preview: preview(&content, 160),
                        after_preview: preview(&result.stripped_content, 160),
                    });
                }
            }
            None => {
                report.unmatched += 1;
            }
        }
    }

    // Stash samples + channel tally on the report via a side channel —
    // keep ScanReport's public shape stable (Eq/Default friendly) while
    // letting print_report format the extras.
    report.by_channel = channel_counts;
    LAST_SAMPLES.with(|s| *s.borrow_mut() = samples);
    Ok(report)
}

/// Return true iff `metadata` parses as a JSON object containing a non-null
/// `envelope` field. Missing metadata, unparseable metadata, and explicit
/// nulls all count as "no envelope present".
fn metadata_has_envelope(metadata: Option<&str>) -> bool {
    let Some(raw) = metadata else { return false };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return false;
    }
    match serde_json::from_str::<Value>(trimmed) {
        Ok(Value::Object(map)) => {
            // Two possible shapes in the wild:
            //   (a) Legacy (pre-ISS-021): `{"envelope": {...}}` at the top.
            //   (b) Current (ISS-021 Phase 2+3+4): engramai nests user-supplied
            //       metadata under a `user` key → `{"engram": {...}, "user": {"envelope": {...}}}`.
            // Either counts as "already has envelope"; only a null/missing
            // value means the row is a legitimate migration candidate.
            let top_level = map.get("envelope").map(|v| !v.is_null()).unwrap_or(false);
            let nested_under_user = map
                .get("user")
                .and_then(|u| u.as_object())
                .and_then(|u| u.get("envelope"))
                .map(|v| !v.is_null())
                .unwrap_or(false);
            top_level || nested_under_user
        }
        _ => false,
    }
}

/// Truncate to `max_chars` characters (not bytes) on a char boundary and
/// append an ellipsis if truncated. UTF-8 safe.
fn preview(s: &str, max_chars: usize) -> String {
    let char_count = s.chars().count();
    if char_count <= max_chars {
        return s.replace('\n', "⏎");
    }
    let mut out: String = s.chars().take(max_chars).collect();
    out.push('…');
    out.replace('\n', "⏎")
}

/// A single match surfaced in the dry-run preview.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Sample {
    id: String,
    channel: String,
    header_body: String,
    had_reply_block: bool,
    before_preview: String,
    after_preview: String,
}

// Thread-local stash for samples — this tool runs single-threaded from
// main(), so a thread_local is the simplest way to avoid threading samples
// through the return type without changing ScanReport's public shape.
thread_local! {
    static LAST_SAMPLES: std::cell::RefCell<Vec<Sample>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

fn print_report(report: &ScanReport) {
    println!();
    println!("═══════════════════════════════════════════════════════════");
    println!("  ISS-021 Phase 5a — Envelope Migration Dry Run");
    println!("═══════════════════════════════════════════════════════════");
    println!("  Total live rows scanned:     {:>6}", report.total_rows);
    println!("  Already have envelope:       {:>6}", report.already_migrated);
    println!("  ── Of rows without envelope ──");
    println!("  Header pollution detected:   {:>6}", report.match_count);
    println!("  Clean (no header found):     {:>6}", report.unmatched);
    println!();

    if report.match_count > 0 {
        println!("  By channel:");
        for (channel, count) in &report.by_channel {
            println!("    {:<10} {:>6}", channel, count);
        }
        println!();
    }

    LAST_SAMPLES.with(|s| {
        let samples = s.borrow();
        if samples.is_empty() {
            return;
        }
        println!("  Sample matches (up to {} shown):", samples.len());
        println!("  ─────────────────────────────────");
        for (i, sample) in samples.iter().enumerate() {
            println!("  [{}] id={}", i + 1, sample.id);
            println!("       channel:  {}", sample.channel);
            println!("       header:   {}", sample.header_body);
            if sample.had_reply_block {
                println!("       reply:    <present>");
            }
            println!("       before:   {}", sample.before_preview);
            println!("       after:    {}", sample.after_preview);
            println!();
        }
    });

    println!("  Status: DRY RUN — no database changes made.");
    if report.match_count > 0 {
        println!(
            "  Next:   Phase 5b counterfactual measurement must justify wet run."
        );
    } else {
        println!("  Next:   No migration needed. Nothing to do.");
    }
    println!("═══════════════════════════════════════════════════════════");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_envelope_detection() {
        // Top-level (legacy pre-ISS-021 shape) → true
        assert!(metadata_has_envelope(Some(
            r#"{"envelope":{"sender_id":"1"}}"#
        )));
        // Nested under "user" (current engramai shape) → true
        assert!(metadata_has_envelope(Some(
            r#"{"engram":{},"user":{"envelope":{"sender_id":"2"}}}"#
        )));
        // Top-level but null → false
        assert!(!metadata_has_envelope(Some(r#"{"envelope":null}"#)));
        // Nested null → false
        assert!(!metadata_has_envelope(Some(
            r#"{"user":{"envelope":null}}"#
        )));
        // Other keys only (no envelope anywhere) → false
        assert!(!metadata_has_envelope(Some(r#"{"other":"x"}"#)));
        // user present but no envelope child → false
        assert!(!metadata_has_envelope(Some(r#"{"user":{"other":1}}"#)));
        // "envelope" as substring in a value (not a key) → false
        assert!(!metadata_has_envelope(Some(
            r#"{"engram":{"note":"discusses envelope design"},"user":{}}"#
        )));
        // Empty object → false
        assert!(!metadata_has_envelope(Some(r#"{}"#)));
        // Null literal → false
        assert!(!metadata_has_envelope(Some("null")));
        // Empty / whitespace / missing → false
        assert!(!metadata_has_envelope(Some("")));
        assert!(!metadata_has_envelope(Some("   ")));
        assert!(!metadata_has_envelope(None));
        // Malformed JSON → false (defensive)
        assert!(!metadata_has_envelope(Some("{not json")));
        // Array at top level → false
        assert!(!metadata_has_envelope(Some(r#"[1,2]"#)));
    }

    #[test]
    fn preview_truncates_on_char_boundary() {
        let s = "hello";
        assert_eq!(preview(s, 10), "hello");
        assert_eq!(preview(s, 3), "hel…");
    }

    #[test]
    fn preview_handles_utf8_safely() {
        let s = "你好世界";
        // 2 chars of a 4-char string = truncated with ellipsis.
        assert_eq!(preview(s, 2), "你好…");
        // Entire string fits.
        assert_eq!(preview(s, 10), "你好世界");
    }

    #[test]
    fn preview_escapes_newlines() {
        let s = "line1\nline2";
        assert_eq!(preview(s, 100), "line1⏎line2");
    }

    #[test]
    fn scan_errors_when_db_missing() {
        let err = scan_db("/nonexistent/path/to/engram.db", 5).unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("not found"), "got: {}", msg);
    }

    /// End-to-end scan against an in-memory fixture DB. Exercises the full
    /// pipeline: open → query → regex match → metadata parse → tally.
    #[test]
    fn scan_end_to_end_on_fixture_db() {
        use tempfile::NamedTempFile;

        let tmp = NamedTempFile::new().unwrap();
        let db_path = tmp.path().to_str().unwrap();

        let conn = Connection::open(db_path).unwrap();
        // Minimal schema subset — we only read id/content/metadata/deleted_at.
        conn.execute(
            "CREATE TABLE memories (
                id TEXT PRIMARY KEY,
                content TEXT NOT NULL,
                metadata TEXT,
                deleted_at TEXT
            )",
            [],
        )
        .unwrap();

        let inserts = [
            // Polluted: telegram header, no envelope metadata → should match.
            (
                "a",
                "[TELEGRAM id:1 Mon 2026-01-01 00:00 UTC]\n\nhello from telegram",
                Option::<&str>::None,
                Option::<&str>::None,
            ),
            // Polluted: discord → should match, different channel.
            (
                "b",
                "[DISCORD user id:2 Tue 2026-01-02 00:00 UTC]\n\nhi discord",
                None,
                None,
            ),
            // Already migrated: has envelope metadata → must be skipped.
            (
                "c",
                "[TELEGRAM id:3 Wed 2026-01-03 00:00 UTC]\n\nwith envelope",
                Some(r#"{"envelope":{"sender_id":"3"}}"#),
                None,
            ),
            // Clean: no header → unmatched bucket.
            ("d", "plain content", None, None),
            // Content starting with `[` but not a known channel → unmatched.
            ("e", "[RFC] some proposal text", None, None),
            // Soft-deleted → excluded from scan entirely.
            (
                "f",
                "[TELEGRAM id:6 Thu 2026-01-04 00:00 UTC]\n\ndeleted",
                None,
                Some("2026-01-05T00:00:00Z"),
            ),
        ];

        for (id, content, metadata, deleted_at) in inserts {
            conn.execute(
                "INSERT INTO memories (id, content, metadata, deleted_at) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![id, content, metadata, deleted_at],
            )
            .unwrap();
        }
        drop(conn);

        let report = scan_db(db_path, 10).unwrap();
        assert_eq!(report.total_rows, 5, "soft-deleted row must be excluded");
        assert_eq!(report.already_migrated, 1);
        assert_eq!(report.match_count, 2);
        assert_eq!(report.unmatched, 2);

        let telegram_count = report
            .by_channel
            .iter()
            .find(|(c, _)| c == "TELEGRAM")
            .map(|(_, n)| *n);
        let discord_count = report
            .by_channel
            .iter()
            .find(|(c, _)| c == "DISCORD")
            .map(|(_, n)| *n);
        assert_eq!(telegram_count, Some(1));
        assert_eq!(discord_count, Some(1));
    }

    #[test]
    fn wet_run_is_blocked() {
        // Phase 5a safety gate: dry_run=false must refuse to proceed even if
        // the DB path is valid.
        let err = run_migrate_envelope("whatever.db", false, None, 5).unwrap_err();
        let msg = format!("{}", err);
        assert!(
            msg.contains("wet-run migration is not enabled"),
            "unexpected error: {}",
            msg
        );
    }
}
