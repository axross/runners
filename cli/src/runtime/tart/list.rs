//! `tart list --format json`, read as untrusted data: an array of objects, of which
//! only the name, the state and the source are kept, and any other shape is refused.

use std::fmt;

use crate::json::{self, Value};

/// the most entries a listing may hold.
pub const MAX_ENTRIES: usize = 1024;

/// the longest name an entry may have, in bytes.
pub const MAX_NAME_BYTES: usize = 512;

/// where Tart keeps a VM.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmSource {
    /// a VM in Tart's local storage, which Marshal's own VMs are.
    Local,
    /// an image in Tart's cache of pulled images.
    Oci,
}

/// a VM's state as Tart reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmState {
    Running,
    Suspended,
    Stopped,
}

/// one line of Tart's listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VmEntry {
    pub name: String,
    pub source: VmSource,
    pub state: VmState,
}

/// why a listing was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListError(String);

impl fmt::Display for ListError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "the tart listing is not as expected: {}", self.0)
    }
}

impl std::error::Error for ListError {}

fn refused(reason: impl Into<String>) -> ListError {
    ListError(reason.into())
}

/// reads a listing; refuses anything but an array of up to [`MAX_ENTRIES`] objects
/// that each carry a `Name`, a `State` and a `Source` string of a known value.
pub fn parse_vm_list(bytes: &[u8]) -> Result<Vec<VmEntry>, ListError> {
    let value = json::parse(bytes).map_err(|error| refused(error.to_string()))?;
    let Value::Array(items) = value else {
        return Err(refused("the document is not an array"));
    };
    if items.len() > MAX_ENTRIES {
        return Err(refused(format!("more than {MAX_ENTRIES} entries")));
    }
    items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            entry(item).map_err(|reason| refused(format!("entry {index}: {reason}")))
        })
        .collect()
}

fn entry(item: &Value) -> Result<VmEntry, String> {
    let Value::Object(object) = item else {
        return Err("not an object".to_string());
    };
    let (mut name, mut state, mut source) = (None, None, None);
    for (key, value) in object.iter() {
        match key {
            "Name" => name = Some(text(value, "Name")?),
            "State" => state = Some(text(value, "State")?),
            "Source" => source = Some(text(value, "Source")?),
            _ => {}
        }
    }
    let name = name.ok_or("no Name")?;
    if name.is_empty() || name.len() > MAX_NAME_BYTES || name.chars().any(char::is_control) {
        return Err("a Name that is empty, too long or holds a control character".to_string());
    }
    let state = match state.ok_or("no State")? {
        "running" => VmState::Running,
        "suspended" => VmState::Suspended,
        "stopped" => VmState::Stopped,
        _ => return Err("an unknown State".to_string()),
    };
    let source = match source.ok_or("no Source")? {
        "local" => VmSource::Local,
        "OCI" => VmSource::Oci,
        _ => return Err("an unknown Source".to_string()),
    };
    Ok(VmEntry {
        name: name.to_string(),
        source,
        state,
    })
}

fn text<'a>(value: &'a Value, field: &str) -> Result<&'a str, String> {
    match value {
        Value::String(text) => Ok(text),
        _ => Err(format!("{field} is not a string")),
    }
}
