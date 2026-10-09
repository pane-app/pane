//! Pane's quicklinks, a default extension in Raycast's shape: named targets
//! (a link of any scheme, a file, a folder or an application, ADR 0037),
//! each with an optional application to open it with, which root search
//! finds and opens. Six commands share this component:
//!
//! - **Search Quicklinks** (`quicklinks`, view): each quicklink an item with
//!   its favicon (or its file's or application's icon), its name and its
//!   target, and the actions Open, Open With…, Copy Link, Edit, Duplicate and
//!   Delete. It also supplies the quicklinks to root search as indexed
//!   results that Pane opens itself.
//! - **Create Quicklink** (`create`, view): the form. Edit and Duplicate
//!   launch it with the quicklink in their context, so it opens filled in.
//! - **Open in Browser** (`browser`, no-view, `"matches": "url"`, #195):
//!   listed only for a typed web address, which it receives as its fallback
//!   text and opens with the system's handler.
//! - **Create Quicklink** (`save`, view, `"matches": "url"`, #195): the
//!   create form with the typed address filled in as its link, listed only
//!   for a typed web address (the `create` command's form, reached without
//!   typing one).
//! - **Export Quicklinks** (`export`, no-view): copies them as JSON.
//! - **Import Quicklinks** (`import`, no-view): adds those the clipboard's
//!   JSON holds that are new.
//!
//! Quicklinks are kept in the package's content, its durable extension
//! data, so they survive restarts, updates, disabling and clearing its
//! cache (see [`links`]).
#![no_std]

mod links;
mod transfer;

use pane_extension::alloc::{format, string::String, vec, vec::Vec};
use pane_extension::commands::{self, CommandRef, LaunchType};
use pane_extension::feedback::{self, Confirmation, Toast, ToastStyle};
use pane_extension::icon::{self, Accessory, Icon, Tone};
use pane_extension::indexed::{IndexedAction, IndexedResult, OpenTarget};
use pane_extension::system::{self, Clip};
use pane_extension::window;
use pane_extension::{
    Action, Command, Field, FieldKind, FieldValue, Form, FormError, Item, LaunchRecord, List,
    Modifier, NoCustomView, Shortcut, TextField, actions, applications,
};

use links::Quicklink;

struct Quicklinks;
pane_extension::export!(Quicklinks);
pane_extension::indexed::export!(Quicklinks);

/// The commands' ids in `pane.json`.
const SEARCH: &str = "quicklinks";
const CREATE: &str = "create";
const BROWSER: &str = "browser";
const SAVE: &str = "save";
const IMPORT: &str = "import";
const EXPORT: &str = "export";

/// The prefix of the edit form's id, before the quicklink's id.
const EDIT: &str = "edit:";

/// The form's fields.
const NAME: &str = "name";
const LINK: &str = "link";
const OPEN_WITH: &str = "application";

fn text(id: &str, label: &str, placeholder: &str) -> Field {
    Field {
        id: id.into(),
        label: label.into(),
        kind: FieldKind::Text(TextField {
            placeholder: Some(placeholder.into()),
        }),
    }
}

/// The quicklink form, titled `title`, its button `submit`.
fn form(title: String, submit: &str) -> Form {
    Form {
        title,
        fields: vec![
            text(NAME, "Name", "Pane issues"),
            text(
                LINK,
                "Link",
                "https://…, mailto:…, or the path of a file, folder or application",
            ),
            text(OPEN_WITH, "Open With", "Default application"),
        ],
        submit_label: submit.into(),
    }
}

/// The form `launch` asks for: a new quicklink's, one filled in to edit the
/// quicklink its context names, or a new one filled in as a copy of it.
fn form_screen(launch: &LaunchRecord) -> Result<List, String> {
    let Some((purpose, id)) = transfer::purpose(launch.context.as_deref()) else {
        return Ok(List::form(
            CREATE,
            form("Create Quicklink".into(), "Create Quicklink"),
        ));
    };
    let saved = links::load()?;
    let link = links::with_id(&saved, &id)
        .map(|index| &saved[index])
        .ok_or("That quicklink no longer exists")?;
    // An installed application by its name, one named by its path by the
    // path: what the field takes back.
    let application = link.application.as_ref().map_or("", |application| {
        if links::is_path(&application.id) {
            application.id.as_str()
        } else {
            application.name.as_str()
        }
    });
    let screen = if purpose == "edit" {
        List::form(
            format!("{EDIT}{}", link.id),
            form(format!("Edit “{}”", link.name), "Save Quicklink"),
        )
        .value(NAME, link.name.clone())
    } else {
        List::form(
            CREATE,
            form("Duplicate Quicklink".into(), "Create Quicklink"),
        )
        .value(NAME, links::copy_name(&saved, &link.name))
    };
    Ok(screen
        .value(LINK, link.target.clone())
        .value(OPEN_WITH, application))
}

/// The form the `save` command shows for the address the user typed
/// (#195): the create form with the address, received as its launch
/// record's fallback text, filled in as its link and waiting for a name.
fn save_screen(launch: &LaunchRecord) -> List {
    let address = launch.fallback_text.clone().unwrap_or_default();
    List::form(CREATE, form("Create Quicklink".into(), "Create Quicklink")).value(LINK, address)
}

/// Open in Browser (#195): opens the address the user typed, received as
/// its launch record's fallback text, with the system's handler (their
/// browser, for an `https:` address), then closes the window, as the SDK's
/// standard Open action does.
fn open_browser(launch: &LaunchRecord) -> Result<(), String> {
    let Some(address) = launch.fallback_text.as_deref() else {
        return Err("Open in Browser needs a web address typed in root search".into());
    };
    system::open(address, None)?;
    window::close(false, window::PopToRootType::Default);
    Ok(())
}

/// Launches Create Quicklink with `context`, as the user would.
fn open_form(context: Option<&str>) -> Result<(), String> {
    let create = CommandRef {
        source: None,
        command: CREATE.into(),
    };
    commands::launch(&create, LaunchType::UserInitiated, &[], context)
}

/// The icon of `link`'s row: its site's favicon, the system's icon of its
/// file, folder or application, or a link.
fn icon_of(link: &Quicklink) -> Icon {
    if links::is_web(&link.target) {
        icon::favicon(&link.target)
    } else if links::is_path(&link.target) {
        icon::file_icon(&link.target)
    } else {
        Icon::builtin("link").tint(Tone::Secondary)
    }
}

/// `key` with Ctrl held, and Shift with `shift`.
fn ctrl(shift: bool, key: &str) -> Shortcut {
    if shift {
        Shortcut::new([Modifier::Ctrl, Modifier::Shift], key)
    } else {
        Shortcut::new([Modifier::Ctrl], key)
    }
}

/// `link` as an item of Search Quicklinks.
fn item(link: &Quicklink) -> Item {
    let mut open = actions::open(link.target.clone());
    if let Some(application) = &link.application {
        open = open.application(application.id.clone());
    }
    let copy = Action::from(actions::copy(Clip::Text(link.target.clone())).title("Copy Link"))
        .shortcut(ctrl(true, "c"));
    let edit = {
        let id = link.id.clone();
        Action::new("Edit", move || async move {
            open_form(Some(&transfer::context("edit", &id)))
        })
        .shortcut(ctrl(false, "e"))
    };
    let duplicate = {
        let id = link.id.clone();
        Action::new("Duplicate", move || async move {
            open_form(Some(&transfer::context("duplicate", &id)))
        })
    };
    let delete = {
        let (id, name) = (link.id.clone(), link.name.clone());
        Action::new("Delete", move || async move { delete(&id, &name).await }).destructive()
    };
    let mut item = Item::new(link.id.clone(), link.name.clone())
        .subtitle(link.target.clone())
        .icon(icon_of(link))
        .action(open.into())
        .action(actions::open_with(link.target.clone()).into())
        .action(copy)
        .action(edit)
        .action(duplicate)
        .action(delete);
    if let Some(application) = &link.application {
        item = item.accessory(
            Accessory::text(application.name.clone())
                .tooltip(format!("Opens with {}", application.name)),
        );
    }
    item
}

/// Deletes the quicklink `id`, named `name`, once the user confirms it.
async fn delete(id: &str, name: &str) -> Result<(), String> {
    let asked = Confirmation::new(format!("Delete “{name}”?"))
        .message("The quicklink is deleted for good.")
        .primary("Delete")
        .destructive();
    if !feedback::confirm(asked).await? {
        return Ok(());
    }
    let mut saved = links::load()?;
    if let Some(index) = links::with_id(&saved, id) {
        saved.remove(index);
        links::save(&saved)?;
    }
    feedback::show_toast(Toast::success(format!("Deleted “{name}”")));
    Ok(())
}

fn value<'a>(values: &'a [FieldValue], id: &str) -> &'a str {
    values
        .iter()
        .find(|value| value.id == id)
        .map_or("", |value| value.value.trim())
}

fn field_error(field: &str, message: String) -> FormError {
    FormError {
        field: Some(field.into()),
        message,
    }
}

fn form_error(message: String) -> FormError {
    FormError {
        field: None,
        message,
    }
}

/// Checks `name` and `target` for the quicklink at `index` (a new one when
/// `None`) among `saved`.
fn check(
    saved: &[Quicklink],
    index: Option<usize>,
    name: &str,
    target: &str,
) -> Result<(), FormError> {
    if let Some(problem) = links::name_problem(name) {
        return Err(field_error(NAME, problem));
    }
    if let Some(other) = links::find(saved, name).filter(|&other| Some(other) != index) {
        let existing = &saved[other].name;
        return Err(field_error(
            NAME,
            format!("A quicklink named “{existing}” already exists"),
        ));
    }
    if let Some(problem) = links::target_problem(target) {
        return Err(field_error(LINK, problem));
    }
    Ok(())
}

/// The installed applications, for naming one to open with; none when Pane
/// cannot list them.
fn installed() -> Vec<applications::Application> {
    applications::installed().unwrap_or_default()
}

/// The plural of "quicklink" for `count`.
fn quicklinks(count: usize) -> String {
    match count {
        1 => "1 quicklink".into(),
        count => format!("{count} quicklinks"),
    }
}

/// Export Quicklinks: copies them as JSON and says how many in a HUD.
fn export() -> Result<(), String> {
    let saved = links::load()?;
    system::copy(&Clip::Text(transfer::export(&saved)), false)?;
    feedback::show_hud(
        &format!("Copied {} as JSON", quicklinks(saved.len())),
        ToastStyle::Success,
    );
    Ok(())
}

/// Import Quicklinks: adds those the clipboard's JSON holds whose name no
/// quicklink has yet, and says how many it added and skipped in a toast; a
/// failure toast says why when the clipboard holds no such JSON.
fn import() -> Result<(), String> {
    let text = match system::read_clipboard()? {
        Some(Clip::Text(text)) => text,
        _ => {
            feedback::show_toast(
                Toast::failure("Could not import quicklinks")
                    .message("The clipboard holds no text: copy the quicklinks' JSON first"),
            );
            return Ok(());
        }
    };
    let given = match transfer::read(&text) {
        Ok(given) => given,
        Err(why) => {
            feedback::show_toast(Toast::failure("Could not import quicklinks").message(why));
            return Ok(());
        }
    };
    let mut saved = links::load()?;
    let installed = installed();
    let (mut added, mut skipped) = (0, 0);
    for given in given {
        match given.and_then(|given| new_quicklink(&saved, given, &installed)) {
            Some(link) => {
                saved.push(link);
                added += 1;
            }
            None => skipped += 1,
        }
    }
    if added > 0 {
        links::save(&saved)?;
    }
    feedback::show_toast(Toast::success(format!(
        "Added {}, skipped {skipped}",
        quicklinks(added)
    )));
    Ok(())
}

/// The quicklink `given` adds to `saved`: `None` when its name or link is
/// not one a quicklink can have, or a quicklink already has its name. An
/// application this system does not have is kept as given: opening says
/// why it cannot be used.
fn new_quicklink(
    saved: &[Quicklink],
    given: transfer::Given,
    installed: &[applications::Application],
) -> Option<Quicklink> {
    if links::name_problem(&given.name).is_some()
        || links::find(saved, &given.name).is_some()
        || links::target_problem(&given.link).is_some()
    {
        return None;
    }
    let application = match given.open_with {
        None => None,
        Some(open_with) => match links::application(&open_with, installed) {
            Ok(found) => found,
            Err(_) if open_with.chars().any(char::is_control) => return None,
            Err(_) => Some(links::Application {
                id: open_with.clone(),
                name: open_with,
            }),
        },
    };
    Some(Quicklink {
        id: links::next_id(saved),
        name: given.name,
        target: given.link,
        application,
    })
}

impl Command for Quicklinks {
    type CustomView = NoCustomView;

    async fn render() -> Result<List, String> {
        let launch = commands::current();
        if launch.command == CREATE {
            return form_screen(&launch);
        }
        if launch.command == SAVE {
            return Ok(save_screen(&launch));
        }
        let saved = links::load()?;
        if saved.is_empty() {
            let create = Item::new("create", "Create Quicklink")
                .subtitle("No quicklinks yet")
                .icon(Icon::builtin("add").tint(Tone::Secondary))
                .action(Action::new("Create Quicklink", || async {
                    open_form(None)
                }));
            return Ok(List::new("Quicklinks").item(create));
        }
        Ok(List::new("Quicklinks").items(saved.iter().map(item)))
    }

    async fn run(command: String, launch: LaunchRecord) -> Result<(), String> {
        match command.as_str() {
            EXPORT => export(),
            IMPORT => import(),
            BROWSER => open_browser(&launch),
            other => Err(format!(
                "`{other}` opens a screen; it has no run entry point"
            )),
        }
    }

    async fn submit_form(form_id: String, values: Vec<FieldValue>) -> Result<String, FormError> {
        let mut saved = links::load().map_err(form_error)?;
        let (name, target) = (value(&values, NAME), value(&values, LINK));
        let editing = match form_id.strip_prefix(EDIT) {
            Some(id) => Some(
                links::with_id(&saved, id)
                    .ok_or_else(|| form_error("This quicklink no longer exists".into()))?,
            ),
            None if form_id == CREATE => None,
            None => return Err(form_error(format!("unknown form: {form_id}"))),
        };
        check(&saved, editing, name, target)?;
        let application = links::application(value(&values, OPEN_WITH), &installed())
            .map_err(|problem| field_error(OPEN_WITH, problem))?;
        let message = match editing {
            Some(index) => {
                let link = &mut saved[index];
                link.name = name.into();
                link.target = target.into();
                link.application = application;
                format!("Saved “{name}”")
            }
            None => {
                let id = links::next_id(&saved);
                saved.push(Quicklink {
                    id,
                    name: name.into(),
                    target: target.into(),
                    application,
                });
                format!("Created “{name}”")
            }
        };
        links::save(&saved).map_err(form_error)?;
        feedback::show_toast(Toast::success(message.clone()));
        if editing.is_some() {
            // Back to the quicklinks, as the edit came from there.
            let search = CommandRef {
                source: None,
                command: SEARCH.into(),
            };
            commands::launch(&search, LaunchType::UserInitiated, &[], None).map_err(form_error)?;
        } else {
            // Back to root search, where the new quicklink is found.
            window::pop_to_root(false);
        }
        Ok(message)
    }
}

impl pane_extension::indexed::Guest for Quicklinks {
    /// One result per quicklink: its name, found in root search like a
    /// command's title, its target as the subtitle, and Pane opening the
    /// target (with its application, if it has one) when invoked.
    async fn results() -> Result<Vec<IndexedResult>, String> {
        Ok(links::load()?
            .into_iter()
            .map(|link| IndexedResult {
                id: link.id,
                title: link.name,
                subtitle: Some(link.target.clone()),
                alternate_titles: Vec::new(),
                keywords: Vec::new(),
                action: IndexedAction::Open(OpenTarget {
                    target: link.target,
                    application: link.application.map(|application| application.id),
                }),
            })
            .collect())
    }
}
