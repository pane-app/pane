//! Quicklinks as JSON, the form Export Quicklinks copies and Import
//! Quicklinks reads from the clipboard: an array of objects, each with a
//! `name`, a `link` and, when it opens with an application, `openWith`
//! (Raycast's export names its fields the same way).
//!
//! ```json
//! [{"name": "Pane issues", "link": "https://github.com/hoangvu12/pane/issues"},
//!  {"name": "Notes", "link": "C:\\Notes", "openWith": "app:Zed"}]
//! ```

use pane_guest::alloc::{
    borrow::ToOwned,
    format,
    string::{String, ToString},
    vec::Vec,
};
use serde_json::{Map, Value};

use crate::links::Quicklink;

/// One quicklink as the JSON gives it, before it is checked.
#[derive(Debug, PartialEq, Eq)]
pub struct Given {
    pub name: String,
    pub link: String,
    pub open_with: Option<String>,
}

/// `links` as the JSON Export Quicklinks copies.
pub fn export(links: &[Quicklink]) -> String {
    let entries: Vec<Value> = links
        .iter()
        .map(|link| {
            let mut entry = Map::new();
            entry.insert("name".to_owned(), Value::String(link.name.clone()));
            entry.insert("link".to_owned(), Value::String(link.target.clone()));
            if let Some(application) = &link.application {
                entry.insert("openWith".to_owned(), Value::String(application.id.clone()));
            }
            Value::Object(entry)
        })
        .collect();
    // A `Value` always serializes.
    serde_json::to_string_pretty(&Value::Array(entries)).unwrap_or_default()
}

/// The quicklinks `text` holds, each `None` when its entry is not a
/// quicklink (not an object, or without a text `name` and `link`): those
/// are skipped. `Err` with why when `text` is not such JSON at all.
pub fn read(text: &str) -> Result<Vec<Option<Given>>, String> {
    let value: Value = serde_json::from_str(text.trim())
        .map_err(|error| format!("The clipboard does not hold quicklinks as JSON: {error}"))?;
    let Value::Array(entries) = value else {
        return Err(
            "The clipboard does not hold quicklinks as JSON: it is not a list of quicklinks".into(),
        );
    };
    Ok(entries.iter().map(given).collect())
}

/// The quicklink one entry gives, if it gives one.
fn given(entry: &Value) -> Option<Given> {
    let entry = entry.as_object()?;
    let open_with = match entry.get("openWith") {
        None | Some(Value::Null) => None,
        Some(Value::String(application)) => {
            Some(application.trim().to_string()).filter(|application| !application.is_empty())
        }
        Some(_) => return None,
    };
    Some(Given {
        name: text(entry, "name")?.to_owned(),
        link: text(entry, "link")?.to_owned(),
        open_with,
    })
}

/// The text `entry` holds under `key`, trimmed, if it holds text there.
fn text<'a>(entry: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    entry.get(key).and_then(Value::as_str).map(str::trim)
}

/// The JSON for a launch context naming one quicklink for `purpose`
/// ("edit", "duplicate"): `{"edit": "3"}`.
pub fn context(purpose: &str, id: &str) -> String {
    let mut context = Map::new();
    context.insert(purpose.to_owned(), Value::String(id.to_owned()));
    Value::Object(context).to_string()
}

/// What a launch context asks the form for: the purpose ("edit",
/// "duplicate") and the quicklink's id; `None` for a new quicklink.
pub fn purpose(context: Option<&str>) -> Option<(String, String)> {
    let value: Value = serde_json::from_str(context?).ok()?;
    let object = value.as_object()?;
    ["edit", "duplicate"].iter().find_map(|purpose| {
        let id = object.get(*purpose)?.as_str()?;
        Some(((*purpose).to_owned(), id.to_owned()))
    })
}
