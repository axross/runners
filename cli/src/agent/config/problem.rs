//! a configuration problem as the protocol carries it: the validator words each TOML
//! problem as `line N: path: message`, or a file it cannot read as
//! `line N, column C: message`, and this splits that wording into its fields. a
//! problem never holds a value of the file, because the validator never quotes one.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Problem {
    /// the TOML key path, such as `pool[1].runners`, when the problem names one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    #[serde(default)]
    pub message: String,
}

/// the fields of one problem as the validator words it; wording of another form is
/// kept whole as the message.
#[must_use]
pub fn problem(text: &str) -> Problem {
    let (line, rest) = match text
        .strip_prefix("line ")
        .and_then(|rest| rest.split_once(": "))
    {
        Some((place, rest)) => match place.split(", column ").next().map(str::parse) {
            Some(Ok(line)) => (Some(line), rest),
            _ => (None, text),
        },
        None => (None, text),
    };
    let (path, message) = match rest.split_once(": ") {
        Some((path, message)) if !path.contains(char::is_whitespace) => {
            ((!path.is_empty()).then(|| path.to_string()), message)
        }
        _ => (None, rest),
    };
    Problem {
        path,
        line,
        message: message.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields(text: &str) -> (Option<String>, Option<u32>, String) {
        let Problem {
            path,
            line,
            message,
        } = problem(text);
        (path, line, message)
    }

    #[test]
    fn a_rule_problem_gives_its_line_path_and_message() {
        assert_eq!(
            fields("line 18: pool[1].runners: must be a whole number from 0 to 64"),
            (
                Some("pool[1].runners".to_string()),
                Some(18),
                "must be a whole number from 0 to 64".to_string()
            )
        );
        assert_eq!(
            fields("line 21: pool[0].tokn: unknown key"),
            (
                Some("pool[0].tokn".to_string()),
                Some(21),
                "unknown key".to_string()
            )
        );
    }

    #[test]
    fn a_syntax_error_gives_its_line_and_no_path() {
        assert_eq!(
            problem("line 14, column 9: syntax error: expected `=`"),
            Problem {
                path: None,
                line: Some(14),
                message: "syntax error: expected `=`".to_string()
            }
        );
    }

    #[test]
    fn a_problem_at_the_root_has_no_path() {
        assert_eq!(
            problem("line 1: : the file has no pools"),
            Problem {
                path: None,
                line: Some(1),
                message: "the file has no pools".to_string()
            }
        );
    }

    #[test]
    fn other_wording_is_kept_whole() {
        for text in [
            "there is no configuration at /x/config.toml",
            "line one: pool: x",
            "config.toml is not valid: a b: c",
        ] {
            let problem = problem(text);
            assert_eq!(problem.line, None, "{text}");
            assert_eq!(problem.message, text, "{text}");
        }
    }
}
