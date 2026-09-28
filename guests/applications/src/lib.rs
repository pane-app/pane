//! Pane's application launcher, a default extension: the applications
//! installed on the system are found by name in root search, and invoking
//! one opens it. Its command lists them all. Pane's host finds and opens
//! them ([`pane_guest::applications`]), since a WASI guest cannot; this
//! extension supplies them to root search, so disabling the package removes
//! them and stops Pane looking for them.
#![no_std]

use pane_guest::alloc::{format, string::String, vec::Vec};
use pane_guest::applications::{self, Application};
use pane_guest::indexed::{IndexedAction, IndexedResult};
use pane_guest::{CustomView, FieldValue, FormError, Guest, Item, NoCustomView, View};

struct Applications;
pane_guest::export!(Applications);
pane_guest::indexed::export!(Applications);

/// The installed applications, by name.
fn installed() -> Result<Vec<Application>, String> {
    let mut found = applications::installed()?;
    found.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(found)
}

impl Guest for Applications {
    type CustomView = NoCustomView;

    async fn get_view() -> Result<View, String> {
        let items = installed()?
            .into_iter()
            .map(|application| Item {
                id: application.id,
                title: application.name,
                subtitle: Some(application.location),
                form: None,
                platforms: None,
                custom_view: None,
            })
            .collect();
        Ok(View {
            title: "Applications: type a name in root search".into(),
            items,
        })
    }

    async fn run_action(item_id: String) -> Result<String, String> {
        applications::open(&item_id)?;
        let name = installed()?
            .into_iter()
            .find(|application| application.id == item_id)
            .map_or(item_id, |application| application.name);
        Ok(format!("Opened {name}"))
    }

    async fn submit_form(_item_id: String, _values: Vec<FieldValue>) -> Result<String, FormError> {
        Err(FormError {
            field: None,
            message: "Applications has no forms".into(),
        })
    }

    async fn open_view(_item_id: String) -> Result<CustomView, String> {
        Err("Applications has no custom views".into())
    }
}

impl pane_guest::indexed::Guest for Applications {
    /// One result per installed application: its name, found in root search
    /// like a command's title, and opening it when invoked.
    async fn results() -> Result<Vec<IndexedResult>, String> {
        Ok(installed()?
            .into_iter()
            .map(|application| IndexedResult {
                action: IndexedAction::OpenApplication(application.id.clone()),
                id: application.id,
                title: application.name,
                subtitle: Some("Application".into()),
            })
            .collect())
    }
}
