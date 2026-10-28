//! the text of an error as it may reach a log: no secret, one line, short.

/// the most characters of an error that reach a log, before the ellipsis.
pub const MAX_ERROR_CHARS: usize = 500;

/// replaces `secret` with `<token>`, collapses whitespace, and cuts the text to
/// [`MAX_ERROR_CHARS`] characters followed by `...`, as the PowerShell supervisor does
/// for a registration error.
#[must_use]
pub fn scrub_error_text(text: &str, secret: &str) -> String {
    let replaced = if secret.is_empty() {
        text.to_string()
    } else {
        text.replace(secret, "<token>")
    };
    let collapsed = replaced.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() > MAX_ERROR_CHARS {
        let mut cut: String = collapsed.chars().take(MAX_ERROR_CHARS).collect();
        cut.push_str("...");
        return cut;
    }
    collapsed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_the_token_and_collapses_whitespace() {
        assert_eq!(
            scrub_error_text(
                "  bad\tcredentials\r\n for ghp_abc  and ghp_abc ",
                "ghp_abc"
            ),
            "bad credentials for <token> and <token>"
        );
    }

    #[test]
    fn cuts_at_500_characters_and_adds_an_ellipsis() {
        let exact = "a".repeat(500);
        assert_eq!(scrub_error_text(&exact, "t"), exact);
        let longer = "a".repeat(501);
        assert_eq!(scrub_error_text(&longer, "t"), format!("{exact}..."));
        let multibyte = "é".repeat(600);
        assert_eq!(scrub_error_text(&multibyte, "t").chars().count(), 503);
    }

    #[test]
    fn the_token_is_replaced_before_the_cut_so_no_part_of_it_survives_the_cut() {
        let text = format!("{}ghp_abc", "x ".repeat(248));
        let scrubbed = scrub_error_text(&text, "ghp_abc");
        assert!(scrubbed.ends_with("<tok..."), "{scrubbed}");
        assert!(!scrubbed.contains("ghp"));
    }

    #[test]
    fn an_empty_secret_replaces_nothing() {
        assert_eq!(scrub_error_text("a b", ""), "a b");
    }
}
