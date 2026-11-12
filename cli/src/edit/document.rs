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
/// the whole of it: a path into an inline table written over several lines, which
/// TOML 1.1 allows, is refused, since its members' comments sit where an edit would
/// move them.
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
            let entry = item.as_array().and_then(|list| list.get(*index));
            if entry
                .and_then(TomlValue::as_inline_table)
                .is_some_and(written_over_lines)
            {
                return Err(OVER_LINES);
            }
            let (entry, entry_inline) =
                entry_at(item, *index).ok_or("an entry on the way is missing")?;
            descend_from(entry, rest, inline || entry_inline)
        }
        [Segment::Key(key), rest @ ..] => {
            let item = table
                .get_mut(toml_key_name(*key))
                .ok_or("a table on the way is missing")?;
            if item.as_inline_table().is_some_and(written_over_lines) {
                return Err(OVER_LINES);
            }
            let item_inline = item.is_inline_table();
            let next = item
                .as_table_like_mut()
                .ok_or("a key on the way is not a table")?;
            descend_from(next, rest, inline || item_inline)
        }
        [Segment::Index(_), ..] => Err("a position has no key before it"),
    }
}

const OVER_LINES: Reason = "an inline table on the way is written over several lines";

/// whether the inside of `table`, between its braces, spans more than one line.
fn written_over_lines(table: &toml_edit::InlineTable) -> bool {
    let mut inside = table.clone();
    inside.decor_mut().clear();
    inside.to_string().contains('\n')
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
    let Found {
        table: parent,
        inline,
    } = descend(root, parent)?;
    let mut new = value.to_toml();
    match parent.get_mut(name) {
        Some(Item::Value(old)) => {
            *new.decor_mut() = old.decor().clone();
            *old = new;
            Ok(())
        }
        Some(_) => Err("it holds a table, not a value"),
        None => {
            insert_member(parent, inline, name, Item::Value(new));
            Ok(())
        }
    }
}

/// inserts `name` into `parent`. in an inline table the space before the closing
/// brace is the last member's suffix, so it moves to the new last member.
fn insert_member(parent: &mut dyn TableLike, inline: bool, name: &str, item: Item) {
    let closing = inline
        .then(|| {
            let last = parent.iter_mut().last()?.1.as_value_mut()?;
            let suffix = last.decor().suffix().cloned()?;
            last.decor_mut().set_suffix("");
            Some(suffix)
        })
        .flatten();
    parent.insert(name, item);
    if let Some(closing) = closing
        && let Some(value) = parent.get_mut(name).and_then(Item::as_value_mut)
    {
        value.decor_mut().set_suffix(closing);
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
    let Found {
        table: parent,
        inline,
    } = descend(root, parent)?;
    let mut entry = Table::new();
    for (key, value) in values {
        entry.insert(toml_key_name(*key), Item::Value(value.to_toml()));
    }
    match parent.get_mut(name) {
        Some(Item::ArrayOfTables(tables)) => tables.push(entry),
        Some(Item::Value(TomlValue::Array(list))) => push_to_list(list, entry.into_inline_table()),
        Some(_) => return Err("the key does not hold a list of tables"),
        None => {
            let mut tables = toml_edit::ArrayOfTables::new();
            tables.push(entry);
            insert_member(parent, inline, name, Item::ArrayOfTables(tables));
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
            remove_from_list(list, *index);
            Ok(None)
        }
        _ => Err("the entry does not exist"),
    }
}

/// removes the entry at `index` of an inline list with its own comments, and keeps
/// the comments of the others. a list keeps the text before an entry in that entry's
/// prefix, and the text after the last entry in its trailing text, or in that entry's
/// suffix when no comma follows it. up to the first line break, that text ends the
/// line before: the comment after the previous entry's comma, or after the `[`. after
/// the line break come the entry's own comment lines and its indentation; comment
/// lines that a blank line sets apart from the entry are not its own and stay.
fn remove_from_list(list: &mut Array, index: usize) {
    let removed = list.get(index).and_then(prefix_of);
    let removed_suffix = (index + 1 == list.len())
        .then(|| list.get(index).and_then(suffix_of))
        .flatten();
    list.remove(index);
    let Some(removed) = removed else {
        return;
    };
    let (line_end, own) = split_at_line_break(&removed);
    let line_end = line_end.trim_end();
    let kept = own.map_or("", set_apart);
    match list.get_mut(index) {
        Some(next) => {
            let Some(after) = prefix_of(next) else {
                return;
            };
            let prefix = match (own, split_at_line_break(&after).1) {
                (_, Some(next_own)) => format!("{line_end}\n{kept}{next_own}"),
                (Some(own), None) => {
                    let indentation = own.rsplit('\n').next().unwrap_or(own);
                    format!("{line_end}\n{kept}{indentation}")
                }
                (None, None) => removed.clone(),
            };
            next.decor_mut().set_prefix(prefix);
        }
        None => {
            let Some(trailing) = list.trailing().as_str().map(str::to_string) else {
                return;
            };
            let closing = split_at_line_break(&trailing).1.or_else(|| {
                removed_suffix
                    .as_deref()
                    .and_then(|s| split_at_line_break(s).1)
            });
            let trailing = match (own, closing) {
                (_, Some(closing)) => format!("{line_end}\n{kept}{closing}"),
                (Some(_), None) => format!("{line_end}\n{kept}"),
                (None, None) => return,
            };
            list.set_trailing(trailing);
        }
    }
}

/// the part of an entry's own text that is not the entry's: everything up to and
/// including its last blank line.
fn set_apart(own: &str) -> &str {
    let mut end = 0;
    let mut at = 0;
    for line in own.split_inclusive('\n') {
        at += line.len();
        if line.ends_with('\n') && line.trim().is_empty() {
            end = at;
        }
    }
    own.get(..end).unwrap_or_default()
}

/// appends `entry` to an inline list. in a list written over several lines it goes on
/// a line of its own with the last entry's indentation, and the comment after the
/// last entry stays on that entry's line.
fn push_to_list(list: &mut Array, entry: toml_edit::InlineTable) {
    let indentation = list
        .iter()
        .last()
        .and_then(prefix_of)
        .and_then(|prefix| prefix.rsplit_once('\n').map(|(_, rest)| rest.to_string()));
    let Some(indentation) = indentation else {
        list.push(entry);
        return;
    };
    let mut value = TomlValue::InlineTable(entry);
    let last_suffix = list.iter().last().and_then(suffix_of);
    match last_suffix.as_deref().map(split_at_line_break) {
        // no comma follows the last entry, so its line ends in its suffix: the comma
        // goes before that line's comment, and the line break moves to the new entry.
        Some((line_end, Some(rest))) => {
            value
                .decor_mut()
                .set_prefix(format!("{}\n{indentation}", line_end.trim_end()));
            value.decor_mut().set_suffix(format!("\n{rest}"));
            if let Some(last) = list.iter_mut().last() {
                last.decor_mut().set_suffix("");
            }
        }
        _ => {
            let trailing = list.trailing().as_str().unwrap_or_default().to_string();
            let (line_end, rest) = split_at_line_break(&trailing);
            value
                .decor_mut()
                .set_prefix(format!("{}\n{indentation}", line_end.trim_end()));
            list.set_trailing(rest.map_or_else(String::new, |rest| format!("\n{rest}")));
        }
    }
    list.push_formatted(value);
}

/// the text before `value` in its list, when it is held as text.
fn prefix_of(value: &TomlValue) -> Option<String> {
    value.decor().prefix()?.as_str().map(str::to_string)
}

/// the text after `value` in its list, before any comma, when it is held as text.
fn suffix_of(value: &TomlValue) -> Option<String> {
    value.decor().suffix()?.as_str().map(str::to_string)
}

/// `text` up to its first line break, and what follows the break when it has one.
fn split_at_line_break(text: &str) -> (&str, Option<&str>) {
    match text.split_once('\n') {
        Some((line_end, rest)) => (line_end, Some(rest)),
        None => (text, None),
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
