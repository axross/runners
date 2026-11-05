//! reads TOML bytes into the tree the rule set reads, with the line of every key
//! beside it. a problem with the bytes is described by fixed text, a line and a
//! column, never by the parser's display text, which quotes the file.

use std::collections::HashMap;
use std::ops::Range;

use toml_edit::{Document, Item, Table as TomlTable, TomlError, Value as TomlValue};

use crate::command::MAX_CONFIG_BYTES;
use crate::json::{Number, Object, Value};

use super::json_reader::key_name as tree_name;
use super::toml_schema::{Child, Table};

/// stands in the tree for a date or time, which no key of the schema holds: it fits
/// no pattern, so the rules report a value of the wrong type.
const DATE_OR_TIME: &str = "\u{0}";

/// a file read into the tree, and where its keys are.
pub(super) struct Layout {
    pub tree: Value,
    pub root: Spot,
}

/// where the file holds a key and the keys and list items under it.
#[derive(Debug, Default)]
pub(super) struct Spot {
    pub line: usize,
    pub items: Vec<Spot>,
    members: Vec<(String, Spot)>,
    /// the position of each member by name, so that finding one stays cheap in a
    /// table of many keys.
    positions: HashMap<String, usize>,
}

impl Spot {
    fn at(line: usize) -> Self {
        Self {
            line,
            ..Self::default()
        }
    }

    fn add_member(&mut self, name: &str, spot: Spot) {
        self.positions.insert(name.to_string(), self.members.len());
        self.members.push((name.to_string(), spot));
    }

    pub fn member(&self, name: &str) -> Option<&Spot> {
        let position = *self.positions.get(name)?;
        self.members.get(position).map(|(_, spot)| spot)
    }

    /// the names of the members, in the order the file gives them.
    pub fn member_names(&self) -> impl Iterator<Item = &str> {
        self.members.iter().map(|(name, _)| name.as_str())
    }
}

/// reads the bytes, or describes in one line why they are not a TOML document.
pub(super) fn read(bytes: &[u8]) -> Result<Layout, String> {
    if bytes.len() as u64 > MAX_CONFIG_BYTES {
        return Err(format!(
            "line 1: the file is larger than {MAX_CONFIG_BYTES} bytes (1 MiB)"
        ));
    }
    let text = match std::str::from_utf8(bytes) {
        Ok(text) => text,
        Err(error) => {
            let valid = bytes
                .get(..error.valid_up_to())
                .and_then(|prefix| std::str::from_utf8(prefix).ok())
                .unwrap_or_default();
            let valid = valid.strip_prefix('\u{feff}').unwrap_or(valid);
            let lines = Lines::new(valid);
            let (line, column) = lines.position(valid.len());
            return Err(format!(
                "line {line}, column {column}: the file is not valid UTF-8"
            ));
        }
    };
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let lines = Lines::new(text);
    let document = Document::parse(text).map_err(|error| syntax_error(&lines, &error))?;
    let builder = Builder { lines: &lines };
    let (tree, root) = builder.table(
        document
            .as_table()
            .iter()
            .map(|(name, item)| (name, Node::Item(item))),
        Some(Table::Root),
        1,
    );
    Ok(Layout { tree, root })
}

/// the fixed text of a syntax error. the parser gives a key or a table header nested
/// past its limit no span, and no position is better than a false one.
fn syntax_error(lines: &Lines, error: &TomlError) -> String {
    match error.span() {
        Some(span) => {
            let (line, column) = lines.position(span.start);
            format!(
                "line {line}, column {column}: syntax error: {}",
                error.message()
            )
        }
        None => format!("syntax error: {}", error.message()),
    }
}

/// the byte offset where each line of the text starts.
struct Lines<'a> {
    text_starts: Vec<usize>,
    text: &'a str,
}

impl<'a> Lines<'a> {
    fn new(text: &'a str) -> Self {
        let mut text_starts = vec![0];
        text_starts.extend(text.match_indices('\n').map(|(at, _)| at + 1));
        Self { text_starts, text }
    }

    /// the line of the offset, counted from 1.
    fn line_of(&self, offset: usize) -> usize {
        self.text_starts
            .partition_point(|start| *start <= offset)
            .max(1)
    }

    /// the line and the column in characters, both counted from 1. a line break is one
    /// position, so a CRLF file and its LF copy give the same pair.
    fn position(&self, offset: usize) -> (usize, usize) {
        let offset = self.break_start(offset);
        let line = self.line_of(offset);
        let start = line
            .checked_sub(1)
            .and_then(|index| self.text_starts.get(index))
            .copied()
            .unwrap_or(0);
        let column = self
            .text
            .get(start..offset)
            .map_or(offset.saturating_sub(start), |before| {
                before.chars().count()
            });
        (line, column + 1)
    }

    /// the offset of the `\r` when `offset` is at the `\n` of a CRLF break, and `offset`
    /// otherwise.
    fn break_start(&self, offset: usize) -> usize {
        let bytes = self.text.as_bytes();
        match offset.checked_sub(1) {
            Some(before)
                if bytes.get(offset) == Some(&b'\n') && bytes.get(before) == Some(&b'\r') =>
            {
                before
            }
            _ => offset,
        }
    }

    fn line(&self, span: Option<Range<usize>>, fallback: usize) -> usize {
        span.map_or(fallback, |span| self.line_of(span.start))
    }
}

/// a node of the parsed document: tables and values are different types, in a table
/// and in an inline one.
#[derive(Clone, Copy)]
enum Node<'a> {
    Item(&'a Item),
    Table(&'a TomlTable),
    Value(&'a TomlValue),
}

struct Builder<'a> {
    lines: &'a Lines<'a>,
}

impl Builder<'_> {
    fn node(&self, node: Node, child: Child, fallback: usize) -> (Value, Spot) {
        match node {
            Node::Item(Item::None) => (Value::Null, Spot::at(fallback)),
            Node::Item(Item::Value(value)) => self.node(Node::Value(value), child, fallback),
            Node::Item(Item::Table(table)) => self.node(Node::Table(table), child, fallback),
            Node::Item(Item::ArrayOfTables(tables)) => {
                let line = self
                    .lines
                    .line(tables.iter().next().and_then(TomlTable::span), fallback);
                self.list(tables.iter().map(Node::Table), child, line)
            }
            Node::Table(table) => {
                let line = self.lines.line(table.span(), fallback);
                self.table(
                    table.iter().map(|(name, item)| (name, Node::Item(item))),
                    child_table(child),
                    line,
                )
            }
            Node::Value(value) => self.value(value, child, fallback),
        }
    }

    fn value(&self, value: &TomlValue, child: Child, fallback: usize) -> (Value, Spot) {
        let line = self.lines.line(value.span(), fallback);
        let scalar = match value {
            TomlValue::String(text) => Value::String(text.value().clone()),
            TomlValue::Integer(number) => {
                Value::Number(Number::from_literal(number.value().to_string()))
            }
            // the debug form always shows a fraction or an exponent, so a float
            // never reads as an integer.
            TomlValue::Float(number) => {
                Value::Number(Number::from_literal(format!("{:?}", number.value())))
            }
            TomlValue::Boolean(flag) => Value::Bool(*flag.value()),
            TomlValue::Datetime(_) => Value::String(DATE_OR_TIME.to_string()),
            TomlValue::Array(array) => {
                return self.list(array.iter().map(Node::Value), child, line);
            }
            TomlValue::InlineTable(table) => {
                return self.table(
                    table.iter().map(|(name, value)| (name, Node::Value(value))),
                    child_table(child),
                    line,
                );
            }
        };
        (scalar, Spot::at(line))
    }

    fn list<'n>(
        &self,
        items: impl Iterator<Item = Node<'n>>,
        child: Child,
        line: usize,
    ) -> (Value, Spot) {
        let element = match child {
            Child::Tables(table) => Child::Table(table),
            _ => Child::Leaf,
        };
        let (values, items) = items.map(|item| self.node(item, element, line)).unzip();
        (
            Value::Array(values),
            Spot {
                line,
                items,
                ..Spot::default()
            },
        )
    }

    /// reads a table: the keys `kind` defines go into the tree under the names the
    /// rules read, every key is placed, and an undefined one is placed without being
    /// read. a table the schema has no kind for reads as an empty object.
    fn table<'n>(
        &self,
        entries: impl Iterator<Item = (&'n str, Node<'n>)>,
        kind: Option<Table>,
        line: usize,
    ) -> (Value, Spot) {
        let mut object = Object::default();
        let mut spot = Spot::at(line);
        let Some(kind) = kind else {
            return (Value::Object(object), spot);
        };
        for (name, node) in entries {
            let Some(member) = kind.named(name) else {
                spot.add_member(name, Spot::at(self.node_line(node, line)));
                continue;
            };
            let (value, placed) = self.node(node, member.child, line);
            // the keys of a table are distinct, so an insert never meets a clash.
            let _ = object.insert(tree_name(member.key).to_string(), value);
            spot.add_member(name, placed);
        }
        (Value::Object(object), spot)
    }

    fn node_line(&self, node: Node, fallback: usize) -> usize {
        let span = match node {
            Node::Item(item) => item.span(),
            Node::Table(table) => table.span(),
            Node::Value(value) => value.span(),
        };
        self.lines.line(span, fallback)
    }
}

fn child_table(child: Child) -> Option<Table> {
    match child {
        Child::Table(table) => Some(table),
        _ => None,
    }
}
