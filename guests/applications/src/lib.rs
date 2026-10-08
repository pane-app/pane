//! Pane's application launcher, a default extension: the applications
//! installed on the system are found by name in root search, each its own
//! result, and invoking one opens it. Pane's host finds and opens them
//! ([`pane_guest::applications`]), since a WASI guest cannot; this
//! extension supplies them to root search, so disabling the package removes
//! them and stops Pane looking for them. Its command is a root provider
//! (`"mode": "provider"` in `pane.json`, #164): it has no row and no screen
//! of its own.
#![no_std]

use pane_guest::alloc::{string::String, vec::Vec};
use pane_guest::applications::{self, Application};
use pane_guest::indexed::{IndexedAction, IndexedResult};
use pane_guest::{Command, NoCustomView};

struct Applications;
pane_guest::export!(Applications);
pane_guest::indexed::export!(Applications);

/// The installed applications, by name.
fn installed() -> Result<Vec<Application>, String> {
    let mut found = applications::installed()?;
    found.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(found)
}

/// A root provider: Pane never opens or runs it, so the command keeps the
/// defaults (opening it is an error).
impl Command for Applications {
    type CustomView = NoCustomView;
}

impl pane_guest::indexed::Guest for Applications {
    /// One result per installed application: its name, found in root search
    /// like a command's title, as are its other names (untranslated, its
    /// program's) and its keywords; subtitled "Application", or with what
    /// tells it apart from another application of its name; opening it when
    /// invoked.
    async fn results() -> Result<Vec<IndexedResult>, String> {
        Ok(installed()?
            .into_iter()
            .map(|application| IndexedResult {
                action: IndexedAction::OpenApplication(application.id.clone()),
                id: application.id,
                title: application.name,
                subtitle: Some(
                    application
                        .distinction
                        .unwrap_or_else(|| "Application".into()),
                ),
                alternate_titles: application.alternate_titles,
                keywords: application.keywords,
            })
            .collect())
    }
}
