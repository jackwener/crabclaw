/// Truncate a string to at most `max_bytes` bytes at a safe UTF-8 char boundary.
///
/// Returns the full string if it's shorter than `max_bytes`.
/// Otherwise, finds the last char boundary at or before `max_bytes`.
///
/// # Examples
/// ```
/// use crabclaw::core::utils::safe_truncate;
/// assert_eq!(safe_truncate("hello", 10), "hello");
/// assert_eq!(safe_truncate("hello", 3), "hel");
/// // Chinese chars are 3 bytes each, so 5 bytes → only first char
/// assert_eq!(safe_truncate("你好", 5), "你");
/// ```
pub fn safe_truncate(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    // Find the last char boundary at or before max_bytes
    let safe = s
        .char_indices()
        .map(|(i, _)| i)
        .take_while(|&i| i <= max_bytes)
        .last()
        .unwrap_or(0);
    &s[..safe]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_ascii_within_limit() {
        assert_eq!(safe_truncate("hello world", 100), "hello world");
    }

    #[test]
    fn truncate_ascii_exact_boundary() {
        assert_eq!(safe_truncate("hello", 5), "hello");
    }

    #[test]
    fn truncate_ascii_cuts() {
        assert_eq!(safe_truncate("hello world", 5), "hello");
    }

    #[test]
    fn truncate_empty_string() {
        assert_eq!(safe_truncate("", 10), "");
    }

    #[test]
    fn truncate_zero_max() {
        assert_eq!(safe_truncate("hello", 0), "");
    }

    // ── Multi-byte UTF-8 tests ──────────────────────────────────────

    #[test]
    fn truncate_chinese_within_limit() {
        // 3 chars × 3 bytes each = 9 bytes, fits in 100
        assert_eq!(safe_truncate("你好世界", 100), "你好世界");
    }

    #[test]
    fn truncate_chinese_at_char_boundary() {
        // "你好" = 6 bytes, max=6 → keep both
        assert_eq!(safe_truncate("你好世界", 6), "你好");
    }

    #[test]
    fn truncate_chinese_mid_char() {
        // "你" = bytes 0..3, max=4 → only "你" fits (next char starts at 3, 3+3=6 > 4)
        assert_eq!(safe_truncate("你好世界", 4), "你");
        // max=5 → same, "你" (3 bytes), next starts at 3 but ends at 6 > 5
        assert_eq!(safe_truncate("你好世界", 5), "你");
    }

    #[test]
    fn truncate_chinese_exact_100_bytes() {
        // Simulate the original bug: 100 bytes, Chinese chars are 3 bytes
        // 33 chars = 99 bytes + 1 byte = 100, falls in middle of char 34
        let text: String = "你".repeat(34); // 102 bytes
        let result = safe_truncate(&text, 100);
        // Should get 33 chars = 99 bytes (can't fit 34th char which would be 102)
        assert_eq!(result.len(), 99);
        assert_eq!(result.chars().count(), 33);
    }

    #[test]
    fn truncate_emoji() {
        // 🦀 = 4 bytes (U+1F980)
        assert_eq!(safe_truncate("🦀🦀🦀", 4), "🦀");
        assert_eq!(safe_truncate("🦀🦀🦀", 5), "🦀");
        assert_eq!(safe_truncate("🦀🦀🦀", 8), "🦀🦀");
    }

    #[test]
    fn truncate_mixed_ascii_and_chinese() {
        // "hi你好" = 2 + 3 + 3 = 8 bytes
        // max=5 → "hi你" (2+3=5 bytes fits exactly)
        assert_eq!(safe_truncate("hi你好", 5), "hi你");
        // max=4 → only "hi" fits (next char '你' would push to 5)
        assert_eq!(safe_truncate("hi你好", 4), "hi");
        // max=3 → only "hi" (2 bytes < 3, next char '你' = 3 bytes would push to 5 > 3)
        assert_eq!(safe_truncate("hi你好", 3), "hi");
        // max=2 → "hi" exactly
        assert_eq!(safe_truncate("hi你好", 2), "hi");
    }

    #[test]
    fn truncate_japanese_katakana() {
        // カタカナ: each 3 bytes
        assert_eq!(safe_truncate("カタカナ", 6), "カタ");
        assert_eq!(safe_truncate("カタカナ", 7), "カタ");
    }
}
