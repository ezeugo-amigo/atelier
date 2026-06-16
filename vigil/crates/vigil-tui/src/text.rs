//! Small text helpers shared across the TUI render paths.

/// Truncate `text` to at most `max_chars` display characters, appending an
/// ellipsis when content was dropped. Splits on char boundaries so multi-byte
/// characters are never cut in half.
pub(crate) fn truncate(text: &str, max_chars: usize) -> String {
    let mut chars = text.chars();
    let head: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{head}...")
    } else {
        head
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_caps_long_text_with_ellipsis() {
        let long = "x".repeat(500);
        let truncated = truncate(&long, 100);
        assert_eq!(truncated.chars().count(), 103);
        assert!(truncated.ends_with("..."));
        assert_eq!(&truncated[..100], &long[..100]);
    }

    #[test]
    fn truncate_leaves_short_text_untouched() {
        assert_eq!(truncate("hello", 100), "hello");
    }

    #[test]
    fn truncate_at_exact_length_has_no_ellipsis() {
        let exact = "y".repeat(100);
        assert_eq!(truncate(&exact, 100), exact);
    }

    #[test]
    fn truncate_respects_char_boundaries() {
        // Each emoji is multiple bytes; truncation must not split one.
        let truncated = truncate("😀😀😀😀", 2);
        assert_eq!(truncated, "😀😀...");
    }
}
