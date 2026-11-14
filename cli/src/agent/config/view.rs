//! the values of `config.toml` that the editor shows: only the keys the file's tables
//! define for the host, each with a value of the type the key takes, in the file's
//! order of pools. an undefined key, a value of another type, `schema_version` and
//! `[lifecycle]` are left out, so a value the rules would not read never travels; the
//! problems say what is wrong with them.

use serde_json::{Map, Value as Json, json};
use toml_edit::{DocumentMut, Item, TableLike, Value};

use crate::profile::Profile;

/// what a key holds, as the editor reads it.
#[derive(Clone, Copy)]
enum Type {
    Text,
    Integer,
    /// an integer or a float, as `cpus` takes.
    Number,
    Flag,
    /// a list of strings, as `labels` takes.
    Texts,
}

const SETTINGS: [(&str, Type); 2] = [("routing_label", Type::Text), ("image_name", Type::Text)];

const BUDGET: [(&str, Type); 3] = [
    ("cpus", Type::Number),
    ("memory_gb", Type::Integer),
    ("disk_gb", Type::Integer),
];

const POOL: [(&str, Type); 11] = [
    ("name", Type::Text),
    ("repository", Type::Text),
    ("runners", Type::Integer),
    ("labels", Type::Texts),
    ("os", Type::Text),
    ("cpus", Type::Number),
    ("memory_gb", Type::Integer),
    ("disk_gb", Type::Integer),
    ("diagnostics", Type::Flag),
    ("diagnostic_directory", Type::Text),
    ("diagnostic_raw_records", Type::Flag),
];

const VOLUME: [(&str, Type); 2] = [("suffix", Type::Text), ("mount_path", Type::Text)];

/// the settings and the pools of `text`, a TOML document without its byte-order mark,
/// for `profile`; `None` when it is not a TOML document.
#[must_use]
pub fn editable(text: &str, profile: Profile) -> Option<(Map<String, Json>, Vec<Json>)> {
    let document: DocumentMut = text.parse().ok()?;
    let root = document.as_table();
    let mut settings = values(root, &SETTINGS);
    if profile == Profile::Axmc
        && let Some(budget) = root.get("budget").and_then(Item::as_table_like)
    {
        settings.insert("budget".to_string(), Json::Object(values(budget, &BUDGET)));
    }
    let pools = tables(root.get("pool"))
        .into_iter()
        .map(|pool| {
            let mut fields = values(pool, &POOL);
            if profile == Profile::Axpc {
                fields.remove("disk_gb");
            }
            if pool.get("volumes").is_some() {
                let volumes = tables(pool.get("volumes"))
                    .into_iter()
                    .map(|volume| Json::Object(values(volume, &VOLUME)))
                    .collect();
                fields.insert("volumes".to_string(), Json::Array(volumes));
            }
            Json::Object(fields)
        })
        .collect();
    Some((settings, pools))
}

/// the tables of an array of tables, written either as `[[name]]` headers or as an
/// inline array of inline tables; anything else holds none.
fn tables(item: Option<&Item>) -> Vec<&dyn TableLike> {
    match item {
        Some(Item::ArrayOfTables(array)) => {
            array.iter().map(|table| table as &dyn TableLike).collect()
        }
        Some(Item::Value(Value::Array(array))) => array
            .iter()
            .filter_map(|value| value.as_inline_table().map(|table| table as &dyn TableLike))
            .collect(),
        _ => Vec::new(),
    }
}

fn values(table: &dyn TableLike, keys: &[(&str, Type)]) -> Map<String, Json> {
    keys.iter()
        .filter_map(|(key, kind)| {
            let value = table.get(key)?.as_value()?;
            Some(((*key).to_string(), typed(value, *kind)?))
        })
        .collect()
}

fn typed(value: &Value, kind: Type) -> Option<Json> {
    match (kind, value) {
        (Type::Text, Value::String(text)) => Some(json!(text.value())),
        (Type::Integer | Type::Number, Value::Integer(number)) => Some(json!(number.value())),
        (Type::Number, Value::Float(number)) if number.value().is_finite() => {
            Some(json!(number.value()))
        }
        (Type::Flag, Value::Boolean(flag)) => Some(json!(flag.value())),
        (Type::Texts, Value::Array(items)) => items
            .iter()
            .map(|item| item.as_str().map(|text| json!(text)))
            .collect::<Option<Vec<_>>>()
            .map(Json::Array),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_editor_gets_each_defined_key_of_the_right_type_in_the_files_order() {
        let text = "\
schema_version = 1
routing_label = \"axpc\" # the host
image_name = \"marshal-runner:latest\"

[lifecycle]
keep_awake = true

[[pool]]
name = \"ci-linux\"
repository = \"axross/app\"
runners = 2
labels = [\"linux\", \"docker\"]
cpus = 1.5
diagnostics = false

[[pool.volumes]]
suffix = \"cache\"
mount_path = \"/cache\"

[[pool]]
name = \"ci-heavy\"
repository = \"axross/app\"
runners = \"two\"
labels = [\"ok\", 3]
tokn = \"ghp_000000000000000000000000000000000000\"
";
        let (settings, pools) = editable(text, Profile::Axpc).unwrap();
        assert_eq!(
            Json::Object(settings),
            json!({"routing_label": "axpc", "image_name": "marshal-runner:latest"})
        );
        assert_eq!(
            pools,
            [
                json!({
                    "name": "ci-linux", "repository": "axross/app", "runners": 2,
                    "labels": ["linux", "docker"], "cpus": 1.5, "diagnostics": false,
                    "volumes": [{"suffix": "cache", "mount_path": "/cache"}]
                }),
                json!({"name": "ci-heavy", "repository": "axross/app"}),
            ]
        );
    }

    #[test]
    fn an_inline_pool_list_reads_as_a_list_of_tables() {
        let text = "pool = [\n  { name = \"a\", runners = 1, disk_gb = 40 },\n]\n";
        let (_, pools) = editable(text, Profile::Axpc).unwrap();
        assert_eq!(pools, [json!({"name": "a", "runners": 1})]);
        let (_, pools) = editable(text, Profile::Axmc).unwrap();
        assert_eq!(pools, [json!({"name": "a", "runners": 1, "disk_gb": 40})]);
    }

    #[test]
    fn the_budget_is_a_mac_setting() {
        let text = "[budget]\ncpus = 12\nmemory_gb = 24\ndisk_gb = \"256\"\n";
        let (settings, _) = editable(text, Profile::Axmc).unwrap();
        assert_eq!(
            Json::Object(settings),
            json!({"budget": {"cpus": 12, "memory_gb": 24}})
        );
        let (settings, _) = editable(text, Profile::Axpc).unwrap();
        assert!(settings.is_empty());
    }

    #[test]
    fn text_that_is_not_toml_has_no_values() {
        assert!(editable("name = ", Profile::Axpc).is_none());
    }
}
