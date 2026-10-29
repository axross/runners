//! case-insensitive comparison, spelled out so that each caller says how much of
//! Unicode it means.

/// reports whether two strings are equal ignoring ASCII case, as owners, repository
/// names, labels and entry names compare because their patterns allow nothing else.
#[must_use]
pub fn eq_ascii(first: &str, second: &str) -> bool {
    first.eq_ignore_ascii_case(second)
}

/// lowercases every character that has a single-character lowercase form and
/// leaves the others alone.
#[must_use]
pub fn lowercase(text: &str) -> String {
    text.chars().map(simple_lowercase).collect()
}

/// reports whether two strings are equal character by character under simple case
/// folding, with no normalization, ignorable characters or context-sensitive
/// lowercasing; token paths are the only compared values that can hold non-ASCII
/// characters.
#[must_use]
pub fn eq_folded(first: &str, second: &str) -> bool {
    let mut first = first.chars();
    let mut second = second.chars();
    loop {
        match (first.next(), second.next()) {
            (None, None) => return true,
            (Some(a), Some(b)) if a == b || fold_char(a) == fold_char(b) => {}
            _ => return false,
        }
    }
}

/// returns `text` folded as [`eq_folded`] folds it.
#[must_use]
pub fn folded(text: &str) -> String {
    text.chars().map(fold_char).collect()
}

/// reports whether `text` starts with `prefix` under the same folding as
/// [`eq_folded`].
#[must_use]
pub fn starts_with_folded(text: &str, prefix: &str) -> bool {
    let mut text = text.chars();
    prefix.chars().all(|expected| {
        text.next()
            .is_some_and(|actual| actual == expected || fold_char(actual) == fold_char(expected))
    })
}

fn single(mut chars: impl Iterator<Item = char>) -> Option<char> {
    let only = chars.next()?;
    chars.next().is_none().then_some(only)
}

fn simple_lowercase(c: char) -> char {
    single(c.to_lowercase()).unwrap_or(c)
}

/// approximates Unicode simple case folding with the standard library: a character
/// folds to the lowercase of its uppercase when both are one character, and otherwise
/// stays, as simple folding has no mapping for it either; the dotless i is the one
/// character it misjudges, because only Turkic folding maps it.
fn fold_char(c: char) -> char {
    if c == '\u{0131}' {
        return c;
    }
    let upper = single(c.to_uppercase()).unwrap_or(c);
    simple_lowercase(upper)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folds_latin_and_greek_case_pairs() {
        assert!(eq_folded("\u{c9}", "\u{e9}"));
        assert!(eq_folded("\u{3c3}", "\u{3c2}"));
        assert!(eq_folded("\u{212a}", "k"));
    }

    #[test]
    fn does_not_normalize_or_expand() {
        assert!(!eq_folded("\u{e9}", "e\u{301}"));
        assert!(!eq_folded("\u{df}", "ss"));
        assert!(!eq_folded("\u{131}", "i"));
        assert!(!eq_folded("a\u{ad}", "a"));
    }

    #[test]
    fn simple_folding_differs_from_powershell_eq_on_declared_pairs() {
        // PowerShell 7 on Linux compares these as different, while simple case folding
        // equates each with its partner; docs/operations/cli.md declares them as
        // divergences.
        assert!(eq_folded("\u{17f}", "s"));
        assert!(eq_folded("\u{17f}", "S"));
        assert!(eq_folded("\u{345}", "\u{3b9}"));
        assert!(eq_folded("\u{345}", "\u{399}"));
        assert!(eq_folded("\u{1e9b}", "\u{1e61}"));
        assert!(eq_folded("\u{1e9b}", "\u{1e60}"));
        // the Kelvin sign folds to k here; PowerShell's answer follows the
        // platform's globalization data.
        assert!(eq_folded("\u{212a}", "k"));
        assert!(eq_folded("\u{212a}", "K"));
    }

    #[test]
    fn compares_length_as_well_as_characters() {
        assert!(!eq_folded("ab", "a"));
        assert!(!eq_folded("a", "ab"));
    }

    #[test]
    fn lowercase_keeps_characters_without_a_single_lowercase() {
        assert_eq!(lowercase("AbC\u{212a}\u{130}"), "abck\u{130}");
    }
}
