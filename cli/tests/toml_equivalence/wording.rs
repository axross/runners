//! puts a JSON problem into the words and path form the TOML reader uses, so that the
//! two readers' problems can be compared line by line. the translation covers the
//! wording the readers write twice: key names, paths, the host's name and the few
//! messages worded for the format.

use marshal_cli::profile::Profile;
use regex::Regex;

use crate::translate::{Table, renamed};

/// the TOML name of each JSON key whose name differs.
const KEY_NAMES: &[(&str, &str)] = &[
    ("repositories", "pool"),
    ("imageName", "image_name"),
    ("slots", "runners"),
    ("memoryGb", "memory_gb"),
    ("diskGb", "disk_gb"),
    ("diagnosticDirectory", "diagnostic_directory"),
    ("diagnosticRawRecords", "diagnostic_raw_records"),
    ("mountPath", "mount_path"),
    ("owner", "repository"),
];

/// message text that the TOML reader words another way.
const MESSAGES: &[(&str, &str)] = &[
    ("required field is missing", "required key is missing"),
    ("must be a list of labels", "must be an array of labels"),
    ("must be a list", "must be an array of tables"),
    ("must be an object", "must be a table"),
    (
        "must list at least one repository",
        "must list at least one pool",
    ),
    (
        "set a distinct name on one entry",
        "set a distinct name on one pool",
    ),
    (
        "is a platform label, so list only non-platform labels",
        "is a platform label, so use a label that is not one",
    ),
];

fn key_name(name: &str) -> &str {
    KEY_NAMES
        .iter()
        .find_map(|(from, to)| (*from == name).then_some(*to))
        .unwrap_or(name)
}

/// a JSON path such as `repositories[0].volumes[1].mountPath` as `pool[0].volumes[1].mount_path`;
/// the root, `configuration`, has no path.
fn path(json: &str) -> String {
    if json == "configuration" {
        return String::new();
    }
    let mut text = String::new();
    for part in json.split('.') {
        let (name, index) = part.split_once('[').map_or((part, ""), |(name, rest)| {
            (name, rest.strip_suffix(']').unwrap_or(rest))
        });
        if !text.is_empty() {
            text.push('.');
        }
        text.push_str(key_name(name));
        if !index.is_empty() {
            text.push_str(&format!("[{index}]"));
        }
    }
    text
}

/// the words of a message outside its quoted parts, which are the file's own text.
fn message(json: &str, profile: Profile) -> String {
    let host = match profile {
        Profile::Axpc => ("axpc", "Windows"),
        Profile::Axmc => ("axmc", "macOS"),
    };
    let entry = Regex::new(r"repositories\[(\d+)\]").unwrap();
    let word = |from: &str, to: &str, text: String| {
        Regex::new(&format!(r"\b{from}\b"))
            .unwrap()
            .replace_all(&text, to)
            .into_owned()
    };
    json.split('\'')
        .enumerate()
        .map(|(index, piece)| {
            if index % 2 == 1 {
                return piece.to_string();
            }
            let mut text = piece.to_string();
            for (from, to) in MESSAGES {
                text = text.replace(from, to);
            }
            text = entry.replace_all(&text, "pool[$1]").into_owned();
            for (from, to) in KEY_NAMES {
                if *from != "repositories" && *from != "owner" {
                    text = word(from, to, text);
                }
            }
            word(host.0, host.1, text)
        })
        .collect::<Vec<_>>()
        .join("'")
}

/// the problem as the TOML reader words it, without its leading line number.
pub fn toml_problem(json: &str, profile: Profile) -> String {
    let unknown = Regex::new(
        r"^(configuration|budget|repositories\[\d+\](?:\.volumes\[\d+\])?)\.(.*): unknown field$",
    )
    .unwrap();
    if let Some(found) = unknown.captures(json) {
        let parent = path(&found[1]);
        let table = match &found[1] {
            "configuration" => Table::Root,
            "budget" => Table::Budget,
            parent if parent.ends_with(']') && parent.contains("volumes") => Table::Volume,
            _ => Table::Pool,
        };
        let name = renamed(table, &found[2]);
        return if parent.is_empty() {
            format!("{name}: unknown key")
        } else {
            format!("{parent}.{name}: unknown key")
        };
    }
    match json.split_once(": ") {
        Some((location, text)) => format!("{}: {}", path(location), message(text, profile)),
        None => message(json, profile),
    }
}
