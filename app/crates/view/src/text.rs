//! helpers for the plain text the view shows. Text from the configuration, Docker or
//! the agent is data: the view never marks it up, and the page sets it as text.

use marshal_cli::instance::restart::{DELAYS, STOPPING_FAILURE, WINDOW};

/// where the agent's logs are, as the window names it.
pub const LOG_FOLDER: &str = "%LOCALAPPDATA%\\marshal\\logs\\";

/// where the configuration is when the agent does not say.
pub const DEFAULT_CONFIGURATION: &str = "%LOCALAPPDATA%\\marshal\\config.toml";

/// what to do so a pending migration of `config.json` runs: it runs only when the
/// agent starts.
pub const MIGRATE_ON_NEXT_START: &str =
    "Marshal migrates config.json when it next starts: quit Marshal and launch it again.";

/// how many failures of the runtime core within the restart window stop the instance.
pub const RESTART_FAILURES: usize = STOPPING_FAILURE;

/// the restart window in whole minutes.
#[must_use]
pub fn restart_window_minutes() -> u64 {
    WINDOW.as_secs() / 60
}

/// the waits before the core restarts, as the banner says them: "10, 30 or 60 s".
#[must_use]
pub fn restart_waits() -> String {
    let seconds: Vec<String> = DELAYS
        .iter()
        .map(|wait| wait.as_secs().to_string())
        .collect();
    match seconds.split_last() {
        Some((last, first)) if !first.is_empty() => format!("{} or {last} s", first.join(", ")),
        _ => format!("{} s", seconds.join(", ")),
    }
}

/// the prefixes GitHub gives its tokens; a word that starts with one and goes on for
/// a token's length is replaced, in case the agent's own masking ever missed one.
const TOKEN_PREFIXES: [&str; 6] = ["github_pat_", "ghp_", "gho_", "ghu_", "ghs_", "ghr_"];

/// the fewest characters after its prefix a word needs to be taken for a token.
const TOKEN_BODY_MIN: usize = 16;

const REDACTED: &str = "<token>";

fn is_word_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// `text` with every token-shaped word replaced; the only change it makes.
#[must_use]
pub fn plain(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    let mut previous_is_word = false;
    while let Some(c) = rest.chars().next() {
        let token = (!previous_is_word)
            .then(|| {
                TOKEN_PREFIXES
                    .iter()
                    .find(|prefix| rest.starts_with(**prefix))
            })
            .flatten()
            .and_then(|prefix| {
                let body = rest.get(prefix.len()..).unwrap_or_default();
                let bytes = word_bytes(body);
                let length = body.get(..bytes).map_or(0, |word| word.chars().count());
                (length >= TOKEN_BODY_MIN).then_some(prefix.len() + bytes)
            });
        if let Some(bytes) = token {
            out.push_str(REDACTED);
            rest = rest.get(bytes..).unwrap_or_default();
            previous_is_word = true;
            continue;
        }
        out.push(c);
        previous_is_word = is_word_char(c);
        rest = rest.get(c.len_utf8()..).unwrap_or_default();
    }
    out
}

/// the bytes at the start of `text` that are word characters.
fn word_bytes(text: &str) -> usize {
    text.char_indices()
        .find(|(_, c)| !is_word_char(*c))
        .map_or(text.len(), |(index, _)| index)
}

/// the first letter in upper case.
#[must_use]
pub fn capitalised(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

/// a code of the agent's open sets, as words: `some_code` reads "Some code".
#[must_use]
pub fn humanised(code: &str) -> String {
    capitalised(&code.replace('_', " "))
}

/// `names` as a list in a sentence: "a", "a and b", "a, b and c".
#[must_use]
pub fn list_of(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [only] => only.clone(),
        [init @ .., last] => format!("{} and {last}", init.join(", ")),
    }
}

/// `count` with the noun in its singular or its plural.
#[must_use]
pub fn counted(count: u32, singular: &str, plural: &str) -> String {
    format!("{count} {}", if count == 1 { singular } else { plural })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_without_a_token_is_unchanged_including_markup() {
        let text = "<b>bold</b> & \"quoted\" <script>alert(1)</script> ghp_short";
        assert_eq!(plain(text), text);
    }

    #[test]
    fn every_prefix_of_a_token_is_replaced() {
        for prefix in TOKEN_PREFIXES {
            let token = format!("{prefix}0123456789abcdefghijABCDEFGHIJ");
            assert_eq!(
                plain(&format!("bad credentials for {token}, retrying")),
                "bad credentials for <token>, retrying"
            );
        }
    }

    #[test]
    fn a_prefix_inside_a_longer_word_is_not_a_token() {
        assert_eq!(
            plain("xghp_0123456789abcdefghij"),
            "xghp_0123456789abcdefghij"
        );
    }

    #[test]
    fn multibyte_text_survives() {
        assert_eq!(
            plain("ünïcode ghp_0123456789abcdefg ✓"),
            "ünïcode <token> ✓"
        );
    }

    #[test]
    fn lists_read_as_a_sentence() {
        let names = |list: &[&str]| list.iter().map(ToString::to_string).collect::<Vec<_>>();
        assert_eq!(list_of(&names(&["a"])), "a");
        assert_eq!(list_of(&names(&["a", "b"])), "a and b");
        assert_eq!(list_of(&names(&["a", "b", "c"])), "a, b and c");
    }

    #[test]
    fn a_code_reads_as_words() {
        assert_eq!(humanised("some_new_state"), "Some new state");
        assert_eq!(capitalised(""), "");
    }
}
