//! Sanitization helpers for untrusted strings (node names, endpoints, regions, zones).
//!
//! Prevents log injection and terminal manipulation by stripping control
//! characters (including ANSI escape codes) and bounding string length.

/// Maximum length for sanitized strings in log and CLI output.
const MAX_SANITIZED_LEN: usize = 255;

/// Sanitize an untrusted string for safe use in logs and CLI output.
///
/// - Strips all ASCII control characters (U+0000..U+001F, U+007F)
/// - Strips the ANSI escape character (U+001B) even when not followed by `[`
/// - Truncates to 255 characters
///
/// Legitimate node names (alphanumeric + `-_.`) pass through unchanged.
pub fn sanitize(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_ascii_control())
        .take(MAX_SANITIZED_LEN)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passthrough_normal_name() {
        assert_eq!(sanitize("my-node_01.prod"), "my-node_01.prod");
    }

    #[test]
    fn strips_newlines() {
        assert_eq!(sanitize("evil\n[WARN] FAKE\nstuff"), "evil[WARN] FAKEstuff");
    }

    #[test]
    fn strips_carriage_return() {
        assert_eq!(sanitize("a\rb"), "ab");
    }

    #[test]
    fn strips_tabs() {
        assert_eq!(sanitize("a\tb"), "ab");
    }

    #[test]
    fn strips_ansi_escape() {
        assert_eq!(sanitize("pre\x1b[31mRED\x1b[0m post"), "pre[31mRED[0m post");
    }

    #[test]
    fn strips_null_byte() {
        assert_eq!(sanitize("a\0b"), "ab");
    }

    #[test]
    fn truncates_long_string() {
        let long = "a".repeat(500);
        let result = sanitize(&long);
        assert_eq!(result.len(), MAX_SANITIZED_LEN);
    }

    #[test]
    fn empty_string() {
        assert_eq!(sanitize(""), "");
    }

    #[test]
    fn unicode_passthrough() {
        // Non-ASCII, non-control characters should pass through
        assert_eq!(sanitize("noeud-\u{00e9}"), "noeud-\u{00e9}");
    }

    #[test]
    fn mixed_control_and_valid() {
        assert_eq!(
            sanitize("legit-node\n[WARN] SECURITY BREACH DETECTED\n[INFO] Shutting down..."),
            "legit-node[WARN] SECURITY BREACH DETECTED[INFO] Shutting down..."
        );
    }

    #[test]
    fn truncation_with_control_chars() {
        // Control chars are stripped before counting toward the limit
        let input = format!("{}\n{}", "a".repeat(200), "b".repeat(200));
        let result = sanitize(&input);
        assert_eq!(result.len(), MAX_SANITIZED_LEN);
        assert!(result.starts_with("aaaa"));
    }
}
