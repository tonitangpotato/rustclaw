/// Safe string utilities for multi-byte text (CJK, emoji, etc.)
///
/// RULE: Never use `&text[..n]` with a byte index directly.
/// Always use these functions instead.

/// Truncate a string to at most `max_chars` characters, append "..." if truncated.
/// This counts *characters*, not bytes — safe for CJK, emoji, etc.
pub fn truncate_chars(s: &str, max_chars: usize) -> String {
    let mut chars = s.chars();
    let truncated: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{}...", truncated)
    } else {
        truncated
    }
}

/// Truncate a string to at most `max_bytes` bytes on a valid char boundary.
/// Returns a slice (no allocation). Does NOT append "...".
pub fn truncate_bytes(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// Split a string into chunks of at most `max_bytes` bytes each,
/// preferring to split at newlines. All slices are on valid char boundaries.
pub fn split_message(text: &str, max_bytes: usize) -> Vec<&str> {
    if text.len() <= max_bytes {
        return vec![text];
    }

    let mut chunks = Vec::new();
    let mut start = 0;

    while start < text.len() {
        let mut end = std::cmp::min(start + max_bytes, text.len());
        // Back up to a char boundary
        while end > start && !text.is_char_boundary(end) {
            end -= 1;
        }
        // Try to split at a newline
        let split_at = if end < text.len() {
            text[start..end]
                .rfind('\n')
                .map(|pos| start + pos + 1)
                .unwrap_or(end)
        } else {
            end
        };
        if split_at <= start {
            // No valid split found; advance to next char boundary
            start = text.ceil_char_boundary(start + 1);
            continue;
        }
        chunks.push(&text[start..split_at]);
        start = split_at;
    }

    chunks
}

/// Like `split_message` but also tries spaces as fallback split points.
pub fn split_message_with_spaces(text: &str, max_bytes: usize) -> Vec<&str> {
    if text.len() <= max_bytes {
        return vec![text];
    }

    let mut chunks = Vec::new();
    let mut start = 0;

    while start < text.len() {
        let mut end = std::cmp::min(start + max_bytes, text.len());
        while end > start && !text.is_char_boundary(end) {
            end -= 1;
        }
        let split_at = if end < text.len() {
            text[start..end]
                .rfind('\n')
                .or_else(|| text[start..end].rfind(' '))
                .map(|pos| start + pos + 1)
                .unwrap_or(end)
        } else {
            end
        };
        if split_at <= start {
            start = text.ceil_char_boundary(start + 1);
            continue;
        }
        chunks.push(&text[start..split_at]);
        start = split_at;
    }

    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_chars_ascii() {
        assert_eq!(truncate_chars("hello world", 5), "hello...");
        assert_eq!(truncate_chars("hello", 5), "hello");
        assert_eq!(truncate_chars("hi", 5), "hi");
    }

    #[test]
    fn test_truncate_chars_cjk() {
        let s = "你好世界测试文字";
        assert_eq!(truncate_chars(s, 4), "你好世界...");
        assert_eq!(truncate_chars(s, 100), s);
    }

    #[test]
    fn test_truncate_bytes_cjk() {
        let s = "你好世界"; // 12 bytes (3 per char)
        assert_eq!(truncate_bytes(s, 6), "你好");
        assert_eq!(truncate_bytes(s, 7), "你好"); // backs up from mid-char
        assert_eq!(truncate_bytes(s, 100), s);
    }

    #[test]
    fn test_split_message_ascii() {
        let s = "hello\nworld\nfoo";
        let chunks = split_message(s, 8);
        assert_eq!(chunks, vec!["hello\n", "world\n", "foo"]);
    }

    #[test]
    fn test_split_message_cjk() {
        let s = "你好世界\n测试文字";
        // Each CJK char is 3 bytes, \n is 1 byte
        // "你好世界\n" = 13 bytes, "测试文字" = 12 bytes
        let chunks = split_message(s, 15);
        assert_eq!(chunks, vec!["你好世界\n", "测试文字"]);
    }

    #[test]
    fn test_split_message_no_panic_on_emoji() {
        let s = "Hello 🎉🎊🎈 World";
        let _chunks = split_message(s, 10); // should not panic
    }
}
