//! A strict, order-preserving JSON reader.
//!
//! It accepts RFC 8259 JSON and nothing more: no comments, no trailing commas.
//! Numbers stay as the text the file holds, so a caller decides what an
//! out-of-range literal means. Object members keep document order, and the
//! duplicate-key behaviour is the one the PowerShell validator shows: a repeated
//! key keeps its last value at the position of its last occurrence, while two keys
//! that differ only in case are an error.

use std::collections::HashMap;

use crate::fold;

/// The deepest nesting the reader accepts, so that hostile input cannot exhaust
/// the stack.
const MAX_DEPTH: usize = 128;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Number(Number),
    String(String),
    Array(Vec<Value>),
    Object(Object),
}

/// A JSON number, kept as its literal text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Number {
    literal: String,
}

impl Number {
    /// Wraps the literal text of a number the reader produced.
    pub(crate) fn from_literal(literal: String) -> Self {
        Self { literal }
    }

    /// Returns the value of an integer literal such as `8` or `-3`, or `None` for
    /// a literal with a fraction or an exponent, such as `8.0` or `1e1`, and for
    /// an integer too large to represent.
    #[must_use]
    pub fn as_integer(&self) -> Option<i128> {
        if self.literal.contains(['.', 'e', 'E']) {
            return None;
        }
        self.literal.parse().ok()
    }

    /// Returns the nearest double. A literal beyond the range of a double gives
    /// infinity, as it does in PowerShell.
    #[must_use]
    pub fn as_f64(&self) -> f64 {
        self.literal.parse().unwrap_or(f64::NAN)
    }

    #[must_use]
    pub fn literal(&self) -> &str {
        &self.literal
    }
}

/// A JSON object whose members keep document order.
///
/// A repeated key leaves a gap where its earlier occurrence was, so inserting
/// stays linear: `index` maps each key's folded form to its slot, and a slot
/// holds `None` once a later occurrence replaced it.
#[derive(Debug, Clone, Default)]
pub struct Object {
    members: Vec<Option<(String, Value)>>,
    index: HashMap<String, usize>,
}

impl PartialEq for Object {
    fn eq(&self, other: &Self) -> bool {
        self.iter().eq(other.iter())
    }
}

impl Object {
    /// Returns the member whose name equals `name` ignoring ASCII case, the way
    /// PowerShell finds a property.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.iter()
            .find(|(key, _)| fold::eq_ascii(key, name))
            .map(|(_, value)| value)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &Value)> {
        self.members
            .iter()
            .flatten()
            .map(|(key, value)| (key.as_str(), value))
    }

    pub(crate) fn insert(&mut self, key: String, value: Value) -> Result<(), String> {
        let folded = fold::folded(&key);
        if let Some(&slot) = self.index.get(&folded) {
            let earlier = self.members.get_mut(slot);
            if earlier
                .as_ref()
                .is_some_and(|member| member.as_ref().is_some_and(|(known, _)| *known != key))
            {
                return Err(format!("keys with different casing: '{key}'"));
            }
            if let Some(member) = earlier {
                *member = None;
            }
        }
        self.index.insert(folded, self.members.len());
        self.members.push(Some((key, value)));
        Ok(())
    }
}

/// Why a document could not be read, with the position of the problem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub message: String,
    pub line: usize,
    pub column: usize,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} (line {}, column {})",
            self.message, self.line, self.column
        )
    }
}

/// Reads a document. A leading byte-order mark is ignored. An empty or
/// whitespace-only document reads as `Null`, which is a non-object root for the
/// caller to report.
pub fn parse(bytes: &[u8]) -> Result<Value, ParseError> {
    let text = std::str::from_utf8(bytes).map_err(|error| ParseError {
        message: format!(
            "the file is not valid UTF-8 (invalid byte at offset {})",
            error.valid_up_to()
        ),
        line: 1,
        column: 1,
    })?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut parser = Parser {
        chars: text.chars().collect(),
        position: 0,
    };
    parser.skip_whitespace();
    if parser.peek().is_none() {
        return Ok(Value::Null);
    }
    let value = parser.value(0)?;
    parser.skip_whitespace();
    if parser.peek().is_some() {
        return Err(parser.error("unexpected text after the value"));
    }
    Ok(value)
}

struct Parser {
    chars: Vec<char>,
    position: usize,
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.position).copied()
    }

    fn bump(&mut self) -> Option<char> {
        let next = self.peek();
        if next.is_some() {
            self.position += 1;
        }
        next
    }

    fn error(&self, message: &str) -> ParseError {
        let consumed = self.chars.get(..self.position).unwrap_or(&self.chars);
        let line = consumed.iter().filter(|c| **c == '\n').count() + 1;
        let column = consumed.iter().rev().take_while(|c| **c != '\n').count() + 1;
        ParseError {
            message: message.to_string(),
            line,
            column,
        }
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.peek(), Some(' ' | '\t' | '\n' | '\r')) {
            self.position += 1;
        }
    }

    fn expect(&mut self, wanted: char, message: &str) -> Result<(), ParseError> {
        if self.peek() == Some(wanted) {
            self.position += 1;
            Ok(())
        } else {
            Err(self.error(message))
        }
    }

    fn value(&mut self, depth: usize) -> Result<Value, ParseError> {
        if depth > MAX_DEPTH {
            return Err(self.error("the document is nested too deeply"));
        }
        match self.peek() {
            Some('{') => self.object(depth),
            Some('[') => self.array(depth),
            Some('"') => self.string().map(Value::String),
            Some('t') => self.literal("true", Value::Bool(true)),
            Some('f') => self.literal("false", Value::Bool(false)),
            Some('n') => self.literal("null", Value::Null),
            Some('-' | '0'..='9') => self.number(),
            Some(_) => Err(self.error("unexpected character")),
            None => Err(self.error("unexpected end of the document")),
        }
    }

    fn literal(&mut self, word: &str, value: Value) -> Result<Value, ParseError> {
        for expected in word.chars() {
            self.expect(expected, "unexpected character")?;
        }
        Ok(value)
    }

    fn object(&mut self, depth: usize) -> Result<Value, ParseError> {
        self.position += 1;
        let mut object = Object::default();
        self.skip_whitespace();
        if self.peek() == Some('}') {
            self.position += 1;
            return Ok(Value::Object(object));
        }
        loop {
            self.skip_whitespace();
            if self.peek() != Some('"') {
                return Err(self.error("expected a property name in double quotes"));
            }
            let key = self.string()?;
            if key.is_empty() {
                return Err(self.error("a property name must not be empty"));
            }
            self.skip_whitespace();
            self.expect(':', "expected ':' after a property name")?;
            self.skip_whitespace();
            let value = self.value(depth + 1)?;
            object
                .insert(key, value)
                .map_err(|message| self.error(&message))?;
            self.skip_whitespace();
            match self.bump() {
                Some(',') => {}
                Some('}') => return Ok(Value::Object(object)),
                _ => {
                    self.position = self.position.saturating_sub(1);
                    return Err(self.error("expected ',' or '}'"));
                }
            }
        }
    }

    fn array(&mut self, depth: usize) -> Result<Value, ParseError> {
        self.position += 1;
        let mut items = Vec::new();
        self.skip_whitespace();
        if self.peek() == Some(']') {
            self.position += 1;
            return Ok(Value::Array(items));
        }
        loop {
            self.skip_whitespace();
            items.push(self.value(depth + 1)?);
            self.skip_whitespace();
            match self.bump() {
                Some(',') => {}
                Some(']') => return Ok(Value::Array(items)),
                _ => {
                    self.position = self.position.saturating_sub(1);
                    return Err(self.error("expected ',' or ']'"));
                }
            }
        }
    }

    fn string(&mut self) -> Result<String, ParseError> {
        self.position += 1;
        let mut text = String::new();
        loop {
            match self.bump() {
                None => return Err(self.error("unterminated string")),
                Some('"') => return Ok(text),
                Some('\\') => text.push(self.escape()?),
                Some(c) if c < ' ' => {
                    self.position -= 1;
                    return Err(self.error("a control character in a string must be escaped"));
                }
                Some(c) => text.push(c),
            }
        }
    }

    fn escape(&mut self) -> Result<char, ParseError> {
        let Some(c) = self.bump() else {
            return Err(self.error("unterminated string"));
        };
        Ok(match c {
            '"' => '"',
            '\\' => '\\',
            '/' => '/',
            'b' => '\u{8}',
            'f' => '\u{c}',
            'n' => '\n',
            'r' => '\r',
            't' => '\t',
            'u' => return self.unicode_escape(),
            _ => {
                self.position -= 1;
                return Err(self.error("invalid escape sequence"));
            }
        })
    }

    fn unicode_escape(&mut self) -> Result<char, ParseError> {
        let first = self.hex4()?;
        let code = if (0xD800..0xDC00).contains(&first) {
            if self.bump() != Some('\\') || self.bump() != Some('u') {
                return Err(
                    self.error("a high surrogate must be followed by a low surrogate escape")
                );
            }
            let second = self.hex4()?;
            if !(0xDC00..0xE000).contains(&second) {
                return Err(
                    self.error("a high surrogate must be followed by a low surrogate escape")
                );
            }
            0x10000 + ((first - 0xD800) << 10) + (second - 0xDC00)
        } else {
            first
        };
        char::from_u32(code).ok_or_else(|| self.error("an escape must not name a lone surrogate"))
    }

    fn hex4(&mut self) -> Result<u32, ParseError> {
        let mut code = 0;
        for _ in 0..4 {
            let digit = self
                .bump()
                .and_then(|c| c.to_digit(16))
                .ok_or_else(|| self.error("expected four hexadecimal digits after \\u"))?;
            code = code * 16 + digit;
        }
        Ok(code)
    }

    fn number(&mut self) -> Result<Value, ParseError> {
        let start = self.position;
        if self.peek() == Some('-') {
            self.position += 1;
        }
        match self.peek() {
            Some('0') => {
                self.position += 1;
                if self.peek().is_some_and(|c| c.is_ascii_digit()) {
                    return Err(self.error("a number must not have leading zeros"));
                }
            }
            Some('1'..='9') => self.digits(),
            _ => return Err(self.error("expected a digit")),
        }
        if self.peek() == Some('.') {
            self.position += 1;
            self.required_digits("expected a digit after '.'")?;
        }
        if matches!(self.peek(), Some('e' | 'E')) {
            self.position += 1;
            if matches!(self.peek(), Some('+' | '-')) {
                self.position += 1;
            }
            self.required_digits("expected a digit in the exponent")?;
        }
        let literal: String = self
            .chars
            .get(start..self.position)
            .unwrap_or_default()
            .iter()
            .collect();
        Ok(Value::Number(Number { literal }))
    }

    fn digits(&mut self) {
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.position += 1;
        }
    }

    fn required_digits(&mut self, message: &str) -> Result<(), ParseError> {
        if !self.peek().is_some_and(|c| c.is_ascii_digit()) {
            return Err(self.error(message));
        }
        self.digits();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn object(text: &str) -> Object {
        match parse(text.as_bytes()).unwrap() {
            Value::Object(object) => object,
            other => panic!("not an object: {other:?}"),
        }
    }

    fn keys(object: &Object) -> Vec<&str> {
        object.iter().map(|(key, _)| key).collect()
    }

    #[test]
    fn keeps_document_order_and_number_literals() {
        let parsed = object(r#"{"b": 1e400, "a": 8.0, "c": -0}"#);
        assert_eq!(keys(&parsed), ["b", "a", "c"]);
        assert_eq!(
            parsed.get("b"),
            Some(&Value::Number(Number {
                literal: "1e400".into()
            }))
        );
        assert!(
            parsed
                .get("b")
                .is_some_and(|v| matches!(v, Value::Number(n) if n.as_f64().is_infinite()))
        );
        assert!(matches!(parsed.get("a"), Some(Value::Number(n)) if n.as_integer().is_none()));
        assert!(matches!(parsed.get("c"), Some(Value::Number(n)) if n.as_integer() == Some(0)));
    }

    #[test]
    fn a_repeated_key_keeps_its_last_value_at_its_last_position() {
        let parsed = object(r#"{"x": 1, "y": 2, "x": 3}"#);
        assert_eq!(keys(&parsed), ["y", "x"]);
        assert!(matches!(parsed.get("x"), Some(Value::Number(n)) if n.literal() == "3"));
    }

    #[test]
    fn many_keys_and_repeats_keep_order_without_quadratic_work() {
        // a quadratic insert would take minutes on this much input
        let count = 100_000;
        let mut text = String::from("{");
        for number in 0..count {
            text.push_str(&format!("\"k{number}\": {number}, "));
        }
        text.push_str("\"k0\": -1}");
        let parsed = object(&text);
        let names = keys(&parsed);
        assert_eq!(names.len(), count);
        assert_eq!(names.first().copied(), Some("k1"));
        assert_eq!(names.last().copied(), Some("k0"));
        assert!(matches!(parsed.get("k0"), Some(Value::Number(n)) if n.literal() == "-1"));
    }

    #[test]
    fn keys_that_differ_only_in_case_are_an_error() {
        let error = parse(br#"{"a": 1, "A": 2}"#).unwrap_err();
        assert!(error.message.contains("different casing"), "{error}");
    }

    #[test]
    fn lookup_ignores_ascii_case() {
        let parsed = object(r#"{"Owner": "x"}"#);
        assert!(parsed.get("owner").is_some());
        assert!(parsed.get("OWNER").is_some());
    }

    #[test]
    fn ignores_a_leading_byte_order_mark() {
        assert!(matches!(parse(b"\xef\xbb\xbf{}"), Ok(Value::Object(_))));
    }

    #[test]
    fn empty_and_blank_documents_are_null() {
        assert_eq!(parse(b"").unwrap(), Value::Null);
        assert_eq!(parse(b" \n\t").unwrap(), Value::Null);
    }

    #[test]
    fn rejects_what_strict_json_rejects() {
        for text in [
            "{\"a\": 1,}",
            "[1,]",
            "{\"a\": 1} // c",
            "/* c */ {}",
            "{'a': 1}",
            "{a: 1}",
            "[01]",
            "[1.]",
            "[.5]",
            "[1e]",
            "[+1]",
            "[NaN]",
            "{\"\": 1}",
            "\"a\nb\"",
            "\"\\x\"",
            "\"\\ud800\"",
            "\"\\udc00\"",
            "{} {}",
            "[",
            "{\"a\"",
        ] {
            assert!(parse(text.as_bytes()).is_err(), "accepted {text:?}");
        }
    }

    #[test]
    fn rejects_invalid_utf8() {
        let error = parse(b"{\"a\": \"\xff\"}").unwrap_err();
        assert!(error.message.contains("UTF-8"), "{error}");
    }

    #[test]
    fn decodes_escapes_and_surrogate_pairs() {
        assert_eq!(
            parse(br#""a\n\u00c9\ud83d\ude00\/""#).unwrap(),
            Value::String("a\n\u{c9}\u{1f600}/".into())
        );
    }

    #[test]
    fn rejects_nesting_beyond_the_depth_limit() {
        let deep = "[".repeat(MAX_DEPTH + 2) + &"]".repeat(MAX_DEPTH + 2);
        assert!(parse(deep.as_bytes()).is_err());
        let shallow = "[".repeat(MAX_DEPTH) + &"]".repeat(MAX_DEPTH);
        assert!(parse(shallow.as_bytes()).is_ok());
    }

    #[test]
    fn the_depth_limit_is_128_levels_below_the_root() {
        let nested = |levels: usize| "[".repeat(levels) + &"]".repeat(levels);
        assert!(parse(nested(MAX_DEPTH + 1).as_bytes()).is_ok());
        assert!(parse(nested(MAX_DEPTH + 2).as_bytes()).is_err());
    }

    #[test]
    fn reports_the_position_of_an_error() {
        let error = parse(b"{\n  \"a\": ?\n}").unwrap_err();
        assert_eq!((error.line, error.column), (2, 8));
    }
}
