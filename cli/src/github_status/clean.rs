//! GitHub's text is untrusted: a workflow or job name comes from the repository's
//! workflow files, which a pull request can change, so it is shown only as plain
//! text without control or bidirectional formatting characters, and cut.

/// the most Unicode code points of a name, its ellipsis included.
pub const NAME_CHARS: usize = 120;

/// the most characters of GitHub's message that reach the log, its ellipsis included.
pub const MESSAGE_CHARS: usize = 200;

/// the Unicode bidirectional formatting characters: the marks, embeddings,
/// overrides and isolates.
fn is_bidi_format(c: char) -> bool {
    matches!(
        c,
        '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}'
    )
}

/// the text without control or bidirectional formatting characters, with runs of
/// white space made one space, cut to `limit` code points with an ellipsis.
#[must_use]
pub fn cut(text: &str, limit: usize) -> String {
    let kept: String = text
        .chars()
        .map(|c| if c.is_whitespace() { ' ' } else { c })
        .filter(|c| !c.is_control() && !is_bidi_format(*c))
        .collect();
    let spaced = kept
        .split(' ')
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if spaced.chars().count() <= limit {
        return spaced;
    }
    let mut short: String = spaced.chars().take(limit.saturating_sub(1)).collect();
    short.push('…');
    short
}

/// a workflow or job name as the `github` topic carries it.
#[must_use]
pub fn name(text: &str) -> String {
    cut(text, NAME_CHARS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controls_and_every_bidirectional_format_character_are_removed() {
        let text = "a\u{0007}b\u{202E}c\u{2066}d\u{200F}e\u{061C}f\u{2069}g\u{0085}h";
        assert_eq!(name(text), "abcdefg h");
    }

    #[test]
    fn white_space_and_line_breaks_become_single_spaces() {
        assert_eq!(name("  CI \n\t test  (ubuntu)\r\n"), "CI test (ubuntu)");
    }

    #[test]
    fn a_name_is_cut_to_120_code_points_with_its_ellipsis() {
        let long = "é".repeat(130);
        let short = name(&long);
        assert_eq!(short.chars().count(), 120);
        assert!(short.ends_with('…'));
        assert_eq!(name(&"x".repeat(120)), "x".repeat(120));
    }

    #[test]
    fn letters_of_other_scripts_stay() {
        assert_eq!(name("テスト · ✓"), "テスト · ✓");
    }
}
