//! Markdown → Telegram HTML converter.
//!
//! Uses pulldown-cmark to parse standard Markdown into an event stream,
//! then renders to Telegram's supported HTML subset:
//!   <b>, <i>, <u>, <s>, <code>, <pre>, <a>, <blockquote>
//!
//! Design: parse once, render once, no regex. Code blocks are never
//! double-escaped. Unsupported elements (images, tables) degrade gracefully
//! to text.

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd, CodeBlockKind};

/// Convert Markdown text to Telegram-compatible HTML.
///
/// Returns HTML string suitable for `parse_mode: "HTML"`.
/// All user text is HTML-escaped; structure is converted to Telegram tags.
pub fn to_telegram_html(markdown: &str) -> String {
    let opts = Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TABLES;
    let parser = Parser::new_ext(markdown, opts);

    let mut out = String::with_capacity(markdown.len());
    let mut in_code_block = false;
    let mut list_depth: u32 = 0;
    let mut ordered_index: Vec<u64> = Vec::new(); // stack for ordered list counters

    for event in parser {
        match event {
            // --- Block-level ---
            Event::Start(Tag::Heading { .. }) => {
                // Telegram has no heading tags — render as bold
                out.push_str("<b>");
            }
            Event::End(TagEnd::Heading(_)) => {
                out.push_str("</b>\n");
            }

            Event::Start(Tag::Paragraph) => {}
            Event::End(TagEnd::Paragraph) => {
                out.push('\n');
            }

            Event::Start(Tag::BlockQuote(_)) => {
                out.push_str("<blockquote>");
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                out.push_str("</blockquote>");
            }

            Event::Start(Tag::CodeBlock(kind)) => {
                in_code_block = true;
                match kind {
                    CodeBlockKind::Fenced(lang) if !lang.is_empty() => {
                        out.push_str(&format!("<pre><code class=\"language-{}\">", escape_html(&lang)));
                    }
                    _ => {
                        out.push_str("<pre><code>");
                    }
                }
            }
            Event::End(TagEnd::CodeBlock) => {
                in_code_block = false;
                out.push_str("</code></pre>\n");
            }

            // --- Lists ---
            Event::Start(Tag::List(first)) => {
                list_depth += 1;
                if let Some(start) = first {
                    ordered_index.push(start);
                } else {
                    ordered_index.push(0); // 0 = unordered marker
                }
            }
            Event::End(TagEnd::List(_)) => {
                list_depth = list_depth.saturating_sub(1);
                ordered_index.pop();
            }

            Event::Start(Tag::Item) => {
                // Indent nested lists
                let indent = "  ".repeat(list_depth.saturating_sub(1) as usize);
                if let Some(idx) = ordered_index.last_mut() {
                    if *idx == 0 {
                        // Unordered
                        out.push_str(&format!("{}• ", indent));
                    } else {
                        // Ordered
                        out.push_str(&format!("{}{}. ", indent, idx));
                        *idx += 1;
                    }
                }
            }
            Event::End(TagEnd::Item) => {
                // Ensure newline after list item
                if !out.ends_with('\n') {
                    out.push('\n');
                }
            }

            // --- Inline ---
            Event::Start(Tag::Emphasis) => out.push_str("<i>"),
            Event::End(TagEnd::Emphasis) => out.push_str("</i>"),

            Event::Start(Tag::Strong) => out.push_str("<b>"),
            Event::End(TagEnd::Strong) => out.push_str("</b>"),

            Event::Start(Tag::Strikethrough) => out.push_str("<s>"),
            Event::End(TagEnd::Strikethrough) => out.push_str("</s>"),

            Event::Start(Tag::Link { dest_url, title, .. }) => {
                out.push_str(&format!("<a href=\"{}\">", escape_html(&dest_url)));
                let _ = title; // title not supported in Telegram
            }
            Event::End(TagEnd::Link) => out.push_str("</a>"),

            Event::Code(code) => {
                out.push_str("<code>");
                out.push_str(&escape_html(&code));
                out.push_str("</code>");
            }

            Event::Text(text) => {
                if in_code_block {
                    // Inside code blocks: escape HTML but don't add formatting
                    out.push_str(&escape_html(&text));
                } else {
                    out.push_str(&escape_html(&text));
                }
            }

            Event::SoftBreak => out.push('\n'),
            Event::HardBreak => out.push('\n'),

            Event::Rule => {
                out.push_str("───────────\n");
            }

            // --- Graceful degradation ---
            Event::Start(Tag::Image { dest_url, .. }) => {
                out.push_str(&format!("[image: {}", escape_html(&dest_url)));
            }
            Event::End(TagEnd::Image) => {
                out.push(']');
            }

            // Tables: render as text rows
            Event::Start(Tag::Table(_)) => {}
            Event::End(TagEnd::Table) => { out.push('\n'); }
            Event::Start(Tag::TableHead) => {}
            Event::End(TagEnd::TableHead) => { out.push('\n'); }
            Event::Start(Tag::TableRow) => {}
            Event::End(TagEnd::TableRow) => { out.push('\n'); }
            Event::Start(Tag::TableCell) => {}
            Event::End(TagEnd::TableCell) => { out.push_str(" | "); }

            // Catch-all for anything else
            _ => {}
        }
    }

    // Clean up trailing whitespace
    let trimmed = out.trim_end().to_string();
    trimmed
}

/// Escape HTML special characters.
fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Strip all markdown formatting, returning plain text.
/// Used as ultimate fallback when HTML parse also fails.
pub fn strip_markdown(markdown: &str) -> String {
    let opts = Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES;
    let parser = Parser::new_ext(markdown, opts);
    let mut out = String::with_capacity(markdown.len());
    let mut in_code_block = false;

    for event in parser {
        match event {
            Event::Start(Tag::CodeBlock(_)) => { in_code_block = true; }
            Event::End(TagEnd::CodeBlock) => { in_code_block = false; out.push('\n'); }
            Event::Start(Tag::Item) => { out.push_str("• "); }
            Event::End(TagEnd::Item) => { if !out.ends_with('\n') { out.push('\n'); } }
            Event::End(TagEnd::Paragraph) => { out.push('\n'); }
            Event::End(TagEnd::Heading(_)) => { out.push('\n'); }
            Event::Text(text) | Event::Code(text) => { out.push_str(&text); }
            Event::SoftBreak | Event::HardBreak => { out.push('\n'); }
            Event::Rule => { out.push_str("───────────\n"); }
            _ => {}
        }
    }

    out.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bold_italic() {
        assert_eq!(
            to_telegram_html("**bold** and *italic*"),
            "<b>bold</b> and <i>italic</i>"
        );
    }

    #[test]
    fn test_inline_code() {
        assert_eq!(
            to_telegram_html("use `println!` here"),
            "use <code>println!</code> here"
        );
    }

    #[test]
    fn test_code_block() {
        let md = "```rust\nfn main() {}\n```";
        let html = to_telegram_html(md);
        assert!(html.contains("<pre><code class=\"language-rust\">"));
        assert!(html.contains("fn main() {}"));
        assert!(html.contains("</code></pre>"));
    }

    #[test]
    fn test_code_block_html_escape() {
        let md = "```\n<script>alert(1)</script>\n```";
        let html = to_telegram_html(md);
        assert!(html.contains("&lt;script&gt;"));
    }

    #[test]
    fn test_heading_to_bold() {
        assert_eq!(
            to_telegram_html("## Section Title"),
            "<b>Section Title</b>"
        );
    }

    #[test]
    fn test_unordered_list() {
        let md = "- first\n- second\n- third";
        let html = to_telegram_html(md);
        assert!(html.contains("• first"));
        assert!(html.contains("• second"));
        assert!(html.contains("• third"));
    }

    #[test]
    fn test_ordered_list() {
        let md = "1. one\n2. two\n3. three";
        let html = to_telegram_html(md);
        assert!(html.contains("1. one"));
        assert!(html.contains("2. two"));
        assert!(html.contains("3. three"));
    }

    #[test]
    fn test_link() {
        assert_eq!(
            to_telegram_html("[click here](https://example.com)"),
            "<a href=\"https://example.com\">click here</a>"
        );
    }

    #[test]
    fn test_blockquote() {
        let html = to_telegram_html("> quoted text");
        assert!(html.contains("<blockquote>"));
        assert!(html.contains("quoted text"));
        assert!(html.contains("</blockquote>"));
    }

    #[test]
    fn test_strikethrough() {
        assert_eq!(
            to_telegram_html("~~deleted~~"),
            "<s>deleted</s>"
        );
    }

    #[test]
    fn test_html_escape_in_text() {
        assert_eq!(
            to_telegram_html("x < y && y > z"),
            "x &lt; y &amp;&amp; y &gt; z"
        );
    }

    #[test]
    fn test_horizontal_rule() {
        let html = to_telegram_html("---");
        assert!(html.contains("───────────"));
    }

    #[test]
    fn test_strip_markdown() {
        let plain = strip_markdown("**bold** and `code`");
        assert_eq!(plain, "bold and code");
    }

    #[test]
    fn test_nested_list() {
        let md = "- outer\n  - inner\n- outer2";
        let html = to_telegram_html(md);
        assert!(html.contains("• outer"));
        assert!(html.contains("  • inner"));
        assert!(html.contains("• outer2"));
    }

    #[test]
    fn test_empty_input() {
        assert_eq!(to_telegram_html(""), "");
    }

    #[test]
    fn test_plain_text_passthrough() {
        assert_eq!(
            to_telegram_html("just plain text"),
            "just plain text"
        );
    }
}
