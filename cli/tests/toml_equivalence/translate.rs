//! turns a JSON fixture into a TOML document by its structure: it renames the keys,
//! joins `owner` and `repository`, drops `tokenPath`, and adds `schema_version` and the
//! profile's `routing_label`. a shape the translator keeps as it finds it, such as a
//! string where a number belongs, stays that shape in the document.

use marshal_cli::json::{self, Object, Value};
use marshal_cli::profile::Profile;

/// why a fixture has no TOML document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotTranslated {
    /// the bytes are not a JSON document the reader accepts.
    NotJson,
    /// the document's root is not an object, which TOML's root always is.
    RootIsNotAnObject,
    /// the document holds a `null`, which TOML has no value for.
    Null,
}

/// the TOML document for a JSON fixture of `profile`.
pub fn translate(bytes: &[u8], profile: Profile) -> Result<String, NotTranslated> {
    let Value::Object(root) = json::parse(bytes).map_err(|_| NotTranslated::NotJson)? else {
        return Err(NotTranslated::RootIsNotAnObject);
    };
    let mut scalars = vec![
        "schema_version = 1".to_string(),
        format!("routing_label = {}", quote(profile.routing_label())),
    ];
    let mut tables = Vec::new();
    for (name, value) in root.iter() {
        match (name, value) {
            ("budget", Value::Object(budget)) => {
                tables.push(table_text("[budget]", budget, Table::Budget)?);
            }
            ("repositories", Value::Array(pools)) if !pools.is_empty() && all_objects(pools) => {
                for pool in objects(pools) {
                    tables.push(pool_table(pool)?);
                }
            }
            _ => scalars.push(format!(
                "{} = {}",
                key(renamed(Table::Root, name)),
                inline(value)?
            )),
        }
    }
    let mut lines = scalars;
    for table in tables {
        lines.push(String::new());
        lines.push(table);
    }
    Ok(lines.join("\n") + "\n")
}

/// a table of the JSON document, which decides the names its keys get.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Table {
    Root,
    Budget,
    Pool,
    Volume,
}

const ROOT_KEYS: &[(&str, &str)] = &[("imageName", "image_name"), ("repositories", "pool")];
const BUDGET_KEYS: &[(&str, &str)] = &[("memoryGb", "memory_gb"), ("diskGb", "disk_gb")];
const POOL_KEYS: &[(&str, &str)] = &[
    ("slots", "runners"),
    ("memoryGb", "memory_gb"),
    ("diskGb", "disk_gb"),
    ("diagnosticDirectory", "diagnostic_directory"),
    ("diagnosticRawRecords", "diagnostic_raw_records"),
];
const VOLUME_KEYS: &[(&str, &str)] = &[("mountPath", "mount_path")];

/// the name the translator gives the key `name` of `table`, which is `name` for a key
/// the schema does not define.
pub fn renamed(table: Table, name: &str) -> &str {
    let names = match table {
        Table::Root => ROOT_KEYS,
        Table::Budget => BUDGET_KEYS,
        Table::Pool => POOL_KEYS,
        Table::Volume => VOLUME_KEYS,
    };
    names
        .iter()
        .find_map(|(from, to)| (*from == name).then_some(*to))
        .unwrap_or(name)
}

fn all_objects(values: &[Value]) -> bool {
    values.iter().all(|value| matches!(value, Value::Object(_)))
}

fn objects(values: &[Value]) -> impl Iterator<Item = &Object> {
    values.iter().filter_map(|value| match value {
        Value::Object(object) => Some(object),
        _ => None,
    })
}

/// a table: `header`, then each member of `object` under its TOML name.
fn table_text(header: &str, object: &Object, table: Table) -> Result<String, NotTranslated> {
    let mut lines = vec![header.to_string()];
    for (name, value) in object.iter() {
        lines.push(format!(
            "{} = {}",
            key(renamed(table, name)),
            inline(value)?
        ));
    }
    Ok(lines.join("\n"))
}

/// a pool: its scalars first, then its volumes as tables when they are a list of
/// objects. `owner` and `repository` become the one `repository` key, at the place of
/// `repository`.
fn pool_table(pool: &Object) -> Result<String, NotTranslated> {
    let mut lines = vec!["[[pool]]".to_string()];
    let mut volumes = Vec::new();
    let owner = pool.iter().find(|(name, _)| *name == "owner");
    let repository = pool.iter().find(|(name, _)| *name == "repository");
    let joined = match (owner, repository) {
        (Some((_, Value::String(owner))), Some((_, Value::String(name)))) => {
            Some(quote(&format!("{owner}/{name}")))
        }
        (Some((_, value)), None) | (None, Some((_, value))) => Some(inline(value)?),
        (Some((_, owner)), Some((_, name))) => Some(inline(if matches!(name, Value::String(_)) {
            owner
        } else {
            name
        })?),
        (None, None) => None,
    };
    for (name, value) in pool.iter() {
        match (name, value) {
            ("tokenPath", _) | ("owner", _) => {}
            ("repository", _) => {
                if let Some(joined) = &joined {
                    lines.push(format!("repository = {joined}"));
                }
            }
            ("volumes", Value::Array(items)) if !items.is_empty() && all_objects(items) => {
                for volume in objects(items) {
                    volumes.push(table_text("[[pool.volumes]]", volume, Table::Volume)?);
                }
            }
            _ => lines.push(format!(
                "{} = {}",
                key(renamed(Table::Pool, name)),
                inline(value)?
            )),
        }
    }
    // an owner without a repository still reports on `repository`
    if repository.is_none()
        && let Some(joined) = joined
    {
        lines.push(format!("repository = {joined}"));
    }
    for volume in volumes {
        lines.push(String::new());
        lines.push(volume);
    }
    Ok(lines.join("\n"))
}

/// a value written inline; a number keeps its literal, which JSON and TOML spell alike.
fn inline(value: &Value) -> Result<String, NotTranslated> {
    Ok(match value {
        Value::Null => return Err(NotTranslated::Null),
        Value::Bool(flag) => flag.to_string(),
        Value::Number(number) => number.literal().to_string(),
        Value::String(text) => quote(text),
        Value::Array(items) => {
            let items: Result<Vec<_>, _> = items.iter().map(inline).collect();
            format!("[{}]", items?.join(", "))
        }
        Value::Object(object) => {
            let members: Result<Vec<_>, _> = object
                .iter()
                .map(|(name, value)| Ok(format!("{} = {}", key(name), inline(value)?)))
                .collect();
            let members = members?;
            if members.is_empty() {
                "{}".to_string()
            } else {
                format!("{{ {} }}", members.join(", "))
            }
        }
    })
}

fn key(name: &str) -> String {
    let bare = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if bare { name.to_string() } else { quote(name) }
}

fn quote(text: &str) -> String {
    let mut quoted = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            c if c.is_control() => quoted.push_str(&format!("\\u{:04X}", u32::from(c))),
            c => quoted.push(c),
        }
    }
    quoted.push('"');
    quoted
}
