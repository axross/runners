//! applies edits to the parsed document. a changed value takes over its old value's
//! decoration, so a comment on its line stays. a removed value or entry is marked in
//! the document and its lines are cut from the printed text, so what stood beside
//! them stays exactly as it was.

use toml_edit::{Array, DocumentMut, Item, Table, TableLike, Value as TomlValue};

use crate::validate::{Key, KeyPath, Segment, toml_key_name, toml_path_text};

use super::{Edit, Value};

/// the text of the marker that stands for a removed value or entry while the document
/// is printed. an edit refuses a file that already holds it.
const MARKER: &str = "marshal-edit-removed-9f4c1e7a2b6d8035";

/// why an edit did not produce a document.
pub(super) enum Refusal {
    /// the text is not a TOML document.
    Syntax,
    /// the document has no place for the edit; the text says which edit.
    Inapplicable(String),
}

/// the text after the edits, each addressed against the document as the edits before
/// it left it.
pub(super) fn apply(text: &str, edits: &[Edit]) -> Result<String, Refusal> {
    if text.contains(MARKER) {
        return Err(Refusal::Inapplicable(
            "the file holds the text that marks a removal".to_string(),
        ));
    }
    let mut document: DocumentMut = text.parse().map_err(|_| Refusal::Syntax)?;
    for edit in edits {
        let marked = apply_one(&mut document, edit)
            .map_err(|reason| Refusal::Inapplicable(format!("{}: {reason}", describe(edit))))?;
        if let Some(marked) = marked {
            document = cut_marked(&document.to_string(), marked)
                .and_then(|text| text.parse().ok())
                .ok_or_else(|| {
                    Refusal::Inapplicable(format!("{}: the removal found no line", describe(edit)))
                })?;
        }
    }
    Ok(document.to_string())
}

fn describe(edit: &Edit) -> String {
    let (what, path) = match edit {
        Edit::Set { path, .. } => ("set", path),
        Edit::Remove { path } => ("remove", path),
        Edit::CreateTable { path } => ("create", path),
        Edit::AddEntry { array, .. } => ("add an entry to", array),
        Edit::RemoveEntry { path } => ("remove", path),
    };
    format!("cannot {what} {}", toml_path_text(path))
}

/// what an edit left in the document to be cut from its printed text.
#[derive(Clone, Copy)]
enum Marked {
    /// a key whose value is the marker: its line goes.
    Value,
    /// a table made of a marker key: its header line, the marker line and the comment
    /// lines directly above the header go.
    Entry,
}

type Reason = &'static str;

fn apply_one(document: &mut DocumentMut, edit: &Edit) -> Result<Option<Marked>, Reason> {
    let root = document.as_table_mut();
    match edit {
        Edit::Set { path, value } => set(root, path, value).map(|()| None),
        Edit::Remove { path } => remove(root, path),
        Edit::CreateTable { path } => create_table(root, path).map(|()| None),
        Edit::AddEntry { array, values } => add_entry(root, array, values).map(|()| None),
        Edit::RemoveEntry { path } => remove_entry(root, path),
    }
}

/// a table found by its path, and whether it is written inline, where one line holds
/// the whole of it.
struct Found<'a> {
    table: &'a mut dyn TableLike,
    inline: bool,
}

fn descend<'a>(table: &'a mut dyn TableLike, segments: &[Segment]) -> Result<Found<'a>, Reason> {
    descend_from(table, segments, false)
}

fn descend_from<'a>(
    table: &'a mut dyn TableLike,
    segments: &[Segment],
    inline: bool,
) -> Result<Found<'a>, Reason> {
    match segments {
        [] => Ok(Found { table, inline }),
        [Segment::Key(key), Segment::Index(index), rest @ ..] => {
            let item = table
                .get_mut(toml_key_name(*key))
                .ok_or("a list on the way is missing")?;
            let (entry, entry_inline) =
                entry_at(item, *index).ok_or("an entry on the way is missing")?;
            descend_from(entry, rest, inline || entry_inline)
        }
        [Segment::Key(key), rest @ ..] => {
            let item = table
                .get_mut(toml_key_name(*key))
                .ok_or("a table on the way is missing")?;
            let item_inline = item.is_inline_table();
            let next = item
                .as_table_like_mut()
                .ok_or("a key on the way is not a table")?;
            descend_from(next, rest, inline || item_inline)
        }
        [Segment::Index(_), ..] => Err("a position has no key before it"),
    }
}

fn entry_at(item: &mut Item, index: usize) -> Option<(&mut dyn TableLike, bool)> {
    match item {
        Item::ArrayOfTables(tables) => tables
            .get_mut(index)
            .map(|table| (table as &mut dyn TableLike, false)),
        Item::Value(TomlValue::Array(values)) => values
            .get_mut(index)?
            .as_inline_table_mut()
            .map(|table| (table as &mut dyn TableLike, true)),
        _ => None,
    }
}

/// the path without its last key, and that key's name.
fn split_key(path: &KeyPath) -> Result<(&[Segment], &'static str), Reason> {
    match path.segments().split_last() {
        Some((Segment::Key(key), parent)) => Ok((parent, toml_key_name(*key))),
        _ => Err("the path does not end in a key"),
    }
}

/// replaces the value at `path`, or adds it to the table that holds the path. the new
/// value takes over the old one's decoration, which carries the comment on its line.
fn set(root: &mut Table, path: &KeyPath, value: &Value) -> Result<(), Reason> {
    let (parent, name) = split_key(path)?;
    let parent = descend(root, parent)?.table;
    let mut new = value.to_toml();
    match parent.get_mut(name) {
        Some(Item::Value(old)) => {
            *new.decor_mut() = old.decor().clone();
            *old = new;
            Ok(())
        }
        Some(_) => Err("it holds a table, not a value"),
        None => {
            parent.insert(name, Item::Value(new));
            Ok(())
        }
    }
}

/// marks the value at `path` for its line to be cut, or removes it from an inline
/// table directly. a value that is not there stays absent.
fn remove(root: &mut Table, path: &KeyPath) -> Result<Option<Marked>, Reason> {
    let (parent, name) = split_key(path)?;
    let Found { table, inline } = descend(root, parent)?;
    let Some(item) = table.get_mut(name) else {
        return Ok(None);
    };
    if !item.is_value() {
        return Err("it holds a table, not a value");
    }
    if inline {
        remove_inline(table, name);
        return Ok(None);
    }
    *item = Item::Value(TomlValue::from(MARKER));
    Ok(Some(Marked::Value))
}

/// removes the member `name` of an inline table. when it was the last one, the space
/// before the closing brace moves to the member that is last now.
fn remove_inline(table: &mut dyn TableLike, name: &str) {
    let was_last = table.iter().last().is_some_and(|(last, _)| last == name);
    let suffix = table
        .get(name)
        .and_then(Item::as_value)
        .and_then(|value| value.decor().suffix().cloned());
    table.remove(name);
    let (true, Some(suffix)) = (was_last, suffix) else {
        return;
    };
    if let Some(value) = table
        .iter_mut()
        .last()
        .and_then(|(_, item)| item.as_value_mut())
    {
        value.decor_mut().set_suffix(suffix);
    }
}

/// creates the table at `path` when the key is free; a table that exists stays as it is.
fn create_table(root: &mut Table, path: &KeyPath) -> Result<(), Reason> {
    let (parent, name) = split_key(path)?;
    let at_root = parent.is_empty();
    let parent = descend(root, parent)?.table;
    match parent.get_mut(name) {
        Some(item) if item.is_table_like() => Ok(()),
        Some(_) => Err("the key holds something else"),
        None => {
            parent.insert(name, Item::Table(Table::new()));
            if at_root {
                place_before_first_pool(root, name);
            }
            Ok(())
        }
    }
}

/// puts the root table `name` just before the first array of tables, so that a new
/// `[lifecycle]` reads before the pools. the document prints its tables in the order
/// of their positions, so the new table takes the first one's and every table from
/// there on moves up by one.
fn place_before_first_pool(root: &mut Table, name: &str) {
    let first = root
        .iter()
        .filter_map(|(_, item)| item.as_array_of_tables())
        .filter_map(|tables| tables.get(0)?.position())
        .min();
    let Some(first) = first else {
        return;
    };
    shift_from(root, first);
    if let Some(table) = root.get_mut(name).and_then(Item::as_table_mut) {
        table.set_position(Some(first));
    }
}

fn shift_from(table: &mut Table, first: isize) {
    for (_, item) in table.iter_mut() {
        match item {
            Item::Table(table) => shift_table(table, first),
            Item::ArrayOfTables(tables) => {
                for table in tables.iter_mut() {
                    shift_table(table, first);
                }
            }
            _ => {}
        }
    }
}

fn shift_table(table: &mut Table, first: isize) {
    if let Some(position) = table.position().filter(|position| *position >= first) {
        table.set_position(Some(position + 1));
    }
    shift_from(table, first);
}

/// appends an entry to the list of tables at `array` as that list is written: a table,
/// or an inline table in an inline list. a missing list is created.
fn add_entry(root: &mut Table, array: &KeyPath, values: &[(Key, Value)]) -> Result<(), Reason> {
    let (parent, name) = split_key(array)?;
    let parent = descend(root, parent)?.table;
    let mut entry = Table::new();
    for (key, value) in values {
        entry.insert(toml_key_name(*key), Item::Value(value.to_toml()));
    }
    match parent.get_mut(name) {
        Some(Item::ArrayOfTables(tables)) => tables.push(entry),
        Some(Item::Value(TomlValue::Array(list))) => list.push(entry.into_inline_table()),
        Some(_) => return Err("the key does not hold a list of tables"),
        None => {
            let mut tables = toml_edit::ArrayOfTables::new();
            tables.push(entry);
            parent.insert(name, Item::ArrayOfTables(tables));
        }
    }
    Ok(())
}

/// marks the entry of an array of tables for its lines to be cut, or removes an entry
/// of an inline list directly.
fn remove_entry(root: &mut Table, path: &KeyPath) -> Result<Option<Marked>, Reason> {
    let Some((Segment::Index(index), before)) = path.segments().split_last() else {
        return Err("the path does not end in a position");
    };
    let (parent, name) = match before.split_last() {
        Some((Segment::Key(key), parent)) => (parent, toml_key_name(*key)),
        _ => return Err("the position has no key before it"),
    };
    let parent = descend(root, parent)?.table;
    match parent.get_mut(name) {
        Some(Item::ArrayOfTables(tables)) => {
            let entry = tables.get_mut(*index).ok_or("the entry does not exist")?;
            entry.clear();
            entry.insert(MARKER, Item::Value(TomlValue::from(MARKER)));
            Ok(Some(Marked::Entry))
        }
        Some(Item::Value(TomlValue::Array(list))) if *index < list.len() => {
            list.remove(*index);
            Ok(None)
        }
        _ => Err("the entry does not exist"),
    }
}

/// `text` without the lines of the one marker it holds, or `None` when that is not
/// one marker on a line of its own.
fn cut_marked(text: &str, marked: Marked) -> Option<String> {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let mut at = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| line.contains(MARKER))
        .map(|(index, _)| index);
    let marker = at.next()?;
    if at.next().is_some() {
        return None;
    }
    let first = match marked {
        Marked::Value => marker,
        Marked::Entry => entry_start(&lines, marker)?,
    };
    let kept = lines.get(..first)?.iter().chain(lines.get(marker + 1..)?);
    Some(kept.copied().collect())
}

/// the first line of the entry whose marker is on line `marker`: the comment lines
/// directly above its header go with it. the blank lines above, which set the entry
/// apart, go too unless another entry follows closely.
fn entry_start(lines: &[&str], marker: usize) -> Option<usize> {
    let header = marker.checked_sub(1)?;
    if !lines.get(header)?.trim_start().starts_with("[[") {
        return None;
    }
    let mut first = header;
    while first > 0 && lines.get(first - 1).is_some_and(|line| is_comment(line)) {
        first -= 1;
    }
    let followed_by_gap = lines
        .get(marker + 1)
        .is_none_or(|line| line.trim().is_empty());
    if followed_by_gap {
        while first > 0
            && lines
                .get(first - 1)
                .is_some_and(|line| line.trim().is_empty())
        {
            first -= 1;
        }
    }
    Some(first)
}

fn is_comment(line: &str) -> bool {
    line.trim_start().starts_with('#')
}

impl Value {
    fn to_toml(&self) -> TomlValue {
        match self {
            Self::Bool(flag) => TomlValue::from(*flag),
            Self::Integer(number) => TomlValue::from(*number),
            Self::Float(number) => TomlValue::from(*number),
            Self::String(text) => TomlValue::from(text.as_str()),
            Self::Strings(texts) => {
                let mut array = Array::new();
                for text in texts {
                    array.push(text.as_str());
                }
                TomlValue::Array(array)
            }
        }
    }
}
