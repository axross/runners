//! the edits a client asks for, read into the write path's operations: each is
//! addressed by a TOML key path of the host's tables, such as `pool[1].labels`, and
//! none may touch `schema_version` or `[lifecycle]`, which the editor does not change.

use serde::Deserialize;
use serde_json::Value as Json;

use crate::edit::{Edit, Value};
use crate::profile::Profile;
use crate::validate::{Key, KeyPath, Segment, parse_toml_path};

/// why the edits were not read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditsError {
    /// the request is malformed; the text says how, without repeating a value.
    Invalid(String),
    /// an edit touches a key the editor does not change.
    Refused(String),
}

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum Wire {
    Set {
        path: String,
        value: Json,
    },
    Remove {
        path: String,
    },
    AddEntry {
        array: String,
        values: serde_json::Map<String, Json>,
    },
    RemoveEntry {
        path: String,
    },
}

/// reads `edits`, a JSON array, for `profile`.
pub fn read_edits(edits: Json, profile: Profile) -> Result<Vec<Edit>, EditsError> {
    let wire: Vec<Wire> = serde_json::from_value(edits)
        .map_err(|_| EditsError::Invalid("the edits are not what config_edit takes".into()))?;
    wire.into_iter()
        .map(|edit| match edit {
            Wire::Set { path, value } => Ok(Edit::Set {
                path: path_of(&path, profile)?,
                value: value_of(value, &path)?,
            }),
            Wire::Remove { path } => Ok(Edit::Remove {
                path: path_of(&path, profile)?,
            }),
            Wire::AddEntry { array, values } => {
                let path = path_of(&array, profile)?;
                let values = values
                    .into_iter()
                    .map(|(name, value)| {
                        let member = path_of(&format!("{array}[0].{name}"), profile)?;
                        let Some(Segment::Key(key)) = member.segments().last() else {
                            return Err(unknown(&name));
                        };
                        Ok((*key, value_of(value, &name)?))
                    })
                    .collect::<Result<_, _>>()?;
                Ok(Edit::AddEntry {
                    array: path,
                    values,
                })
            }
            Wire::RemoveEntry { path } => Ok(Edit::RemoveEntry {
                path: path_of(&path, profile)?,
            }),
        })
        .collect()
}

fn unknown(path: &str) -> EditsError {
    EditsError::Invalid(format!("{path} is not a key of this host's configuration"))
}

fn path_of(text: &str, profile: Profile) -> Result<KeyPath, EditsError> {
    let path = parse_toml_path(text, profile).ok_or_else(|| unknown(text))?;
    if matches!(
        path.segments().first(),
        Some(Segment::Key(Key::SchemaVersion | Key::Lifecycle))
    ) {
        return Err(EditsError::Refused(format!(
            "{text}: the editor does not change schema_version or [lifecycle]; edit them in a \
             text editor"
        )));
    }
    Ok(path)
}

fn value_of(value: Json, path: &str) -> Result<Value, EditsError> {
    let wrong = || EditsError::Invalid(format!("{path}: the value is not one a key takes"));
    Ok(match value {
        Json::Bool(flag) => Value::Bool(flag),
        Json::Number(number) => match number.as_i64() {
            Some(integer) => Value::Integer(integer),
            None => Value::Float(
                number
                    .as_f64()
                    .filter(|float| float.is_finite())
                    .ok_or_else(wrong)?,
            ),
        },
        Json::String(text) => Value::String(text),
        Json::Array(items) => Value::Strings(
            items
                .into_iter()
                .map(|item| match item {
                    Json::String(text) => Ok(text),
                    _ => Err(wrong()),
                })
                .collect::<Result<_, _>>()?,
        ),
        Json::Null | Json::Object(_) => return Err(wrong()),
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn path(text: &str) -> KeyPath {
        parse_toml_path(text, Profile::Axpc).unwrap()
    }

    #[test]
    fn each_operation_reads_into_the_write_paths() {
        let edits = read_edits(
            json!([
                {"op": "set", "path": "pool[0].runners", "value": 3},
                {"op": "set", "path": "pool[0].cpus", "value": 1.5},
                {"op": "set", "path": "pool[0].labels", "value": ["linux", "large"]},
                {"op": "set", "path": "pool[0].diagnostics", "value": true},
                {"op": "remove", "path": "pool[0].labels"},
                {"op": "add_entry", "array": "pool", "values": {"name": "b", "runners": 1}},
                {"op": "add_entry", "array": "pool[0].volumes", "values": {"suffix": "c"}},
                {"op": "remove_entry", "path": "pool[1]"}
            ]),
            Profile::Axpc,
        )
        .unwrap();
        assert_eq!(
            edits,
            [
                Edit::Set {
                    path: path("pool[0].runners"),
                    value: Value::Integer(3)
                },
                Edit::Set {
                    path: path("pool[0].cpus"),
                    value: Value::Float(1.5)
                },
                Edit::Set {
                    path: path("pool[0].labels"),
                    value: Value::Strings(vec!["linux".into(), "large".into()])
                },
                Edit::Set {
                    path: path("pool[0].diagnostics"),
                    value: Value::Bool(true)
                },
                Edit::Remove {
                    path: path("pool[0].labels")
                },
                Edit::AddEntry {
                    array: path("pool"),
                    values: vec![
                        (Key::Name, Value::String("b".into())),
                        (Key::Slots, Value::Integer(1))
                    ]
                },
                Edit::AddEntry {
                    array: path("pool[0].volumes"),
                    values: vec![(Key::Suffix, Value::String("c".into()))]
                },
                Edit::RemoveEntry {
                    path: path("pool[1]")
                },
            ]
        );
    }

    #[test]
    fn schema_version_and_lifecycle_are_refused() {
        for edit in [
            json!({"op": "set", "path": "schema_version", "value": 2}),
            json!({"op": "remove", "path": "lifecycle.keep_awake"}),
            json!({"op": "remove", "path": "lifecycle"}),
        ] {
            assert!(
                matches!(
                    read_edits(json!([edit]), Profile::Axpc),
                    Err(EditsError::Refused(_))
                ),
                "{edit}"
            );
        }
    }

    #[test]
    fn a_malformed_edit_is_invalid_and_repeats_no_value() {
        for edit in [
            json!({"op": "set", "path": "pool[0].tokn", "value": "ghp_secret"}),
            json!({"op": "set", "path": "budget.cpus", "value": 4}),
            json!({"op": "set", "path": "pool[0].labels", "value": ["ok", 3]}),
            json!({"op": "set", "path": "pool[0].name", "value": null}),
            json!({"op": "set", "path": "pool[0].name", "value": {"a": "ghp_secret"}}),
            json!({"op": "add_entry", "array": "pool", "values": {"tokn": "ghp_secret"}}),
            json!({"op": "rename", "path": "pool[0]"}),
            json!({"op": "set", "path": "pool[0].name", "value": "a", "more": 1}),
        ] {
            match read_edits(json!([edit]), Profile::Axpc) {
                Err(EditsError::Invalid(message)) => {
                    assert!(!message.contains("ghp_secret"), "{message}");
                }
                other => panic!("{edit}: {other:?}"),
            }
        }
    }
}
