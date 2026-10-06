//! A command's list, as an author writes it: a [`List`] of [`Item`]s whose
//! action is a closure. The command implements [`Command`] and calls
//! [`export!`](crate::export); the SDK answers Pane's `render` with the
//! list as the versioned JSON tree of `docs/list-tree.md`, names each
//! action by a callback id, and runs the closure when Pane hands that id
//! to `handle-event`. Authors never see the JSON or the ids.
//!
//! An item's action is named by the item's id, so the same item has the
//! same callback in every drawing of the list. Pane draws the list again
//! after each action, which hands the SDK the closures of the new drawing.
//! An instance that has not drawn the list yet (a fresh one, after a crash
//! or a search Pane stopped) draws it first when it is handed an id it does
//! not know; an id the list still does not name is a search result's
//! ([`Command::run_search_result`]).
//!
//! A no-view command (`"mode": "no-view"` in `pane.json`) has no list:
//! Pane calls [`Command::run`] each time it is launched. Both receive the
//! command's launch record: `run` as its argument, `render` through
//! [`commands::current`](crate::commands::current).

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::RefCell;
use core::fmt::Write;
use core::future::Future;
use core::pin::Pin;

use crate::exports::pane::extension::command as wit;
use crate::pane::extension::commands::LaunchRecord;
use wit::{
    CustomView, CustomViewInfo, CustomViewRole, FieldKind, FieldValue, Form, FormError,
    GuestCustomView, Platform,
};

/// The version of the tree this SDK writes (`docs/list-tree.md`).
const TREE_VERSION: u32 = 1;

/// What an action answers: text shown to the user as the result, or an
/// error shown as the failure.
type Answer = Pin<Box<dyn Future<Output = Result<String, String>>>>;

/// An item's action, run once when the user chooses it.
type Action = Box<dyn FnOnce() -> Answer>;

/// A command's list view: its title and items, in order.
pub struct List {
    title: String,
    items: Vec<Item>,
}

impl List {
    /// An empty list titled `title`.
    pub fn new(title: impl Into<String>) -> List {
        List {
            title: title.into(),
            items: Vec::new(),
        }
    }

    /// This list with `item` after its items.
    pub fn item(mut self, item: Item) -> List {
        self.items.push(item);
        self
    }

    /// This list with `items` after its items.
    pub fn items(mut self, items: impl IntoIterator<Item = Item>) -> List {
        self.items.extend(items);
        self
    }
}

/// One entry in a command's list. Choosing it opens its form, else its
/// custom view, else runs its action; an item with none of them does
/// nothing and says so.
pub struct Item {
    id: String,
    title: String,
    subtitle: Option<String>,
    action: Option<Action>,
    form: Option<Form>,
    platforms: Option<Vec<Platform>>,
    custom_view: Option<CustomViewInfo>,
}

impl Item {
    /// An item titled `title`. `id` identifies it among the list's items:
    /// Pane keeps the selection on it when the list is drawn again, and
    /// passes it to [`Command::submit_form`] and [`Command::open_view`].
    pub fn new(id: impl Into<String>, title: impl Into<String>) -> Item {
        Item {
            id: id.into(),
            title: title.into(),
            subtitle: None,
            action: None,
            form: None,
            platforms: None,
            custom_view: None,
        }
    }

    /// This item with a second line under its title.
    pub fn subtitle(mut self, subtitle: impl Into<String>) -> Item {
        self.subtitle = Some(subtitle.into());
        self
    }

    /// This item with `action`, which runs when the user chooses it. The
    /// text it answers is shown as the result; an error is shown as the
    /// failure. Pane draws the list again afterwards.
    pub fn on_action<F, A>(mut self, action: F) -> Item
    where
        F: FnOnce() -> A + 'static,
        A: Future<Output = Result<String, String>> + 'static,
    {
        self.action = Some(Box::new(move || Box::pin(action()) as Answer));
        self
    }

    /// This item opening `form` when chosen, instead of running its action;
    /// submitting it calls [`Command::submit_form`] with the item's id.
    pub fn form(mut self, form: Form) -> Item {
        self.form = Some(form);
        self
    }

    /// This item's action (or form) working on `platforms` only. Elsewhere
    /// Pane still lists the item but shows it as unavailable with the
    /// reason, and never runs its action or opens its form.
    pub fn platforms(mut self, platforms: impl IntoIterator<Item = Platform>) -> Item {
        self.platforms = Some(platforms.into_iter().collect());
        self
    }

    /// This item opening a custom view when chosen, instead of running its
    /// action: Pane calls [`Command::open_view`] with the item's id. Ignored
    /// when the item has a form.
    pub fn custom_view(mut self, info: CustomViewInfo) -> Item {
        self.custom_view = Some(info);
        self
    }
}

/// An extension command: one whose screen is a list (a view command), one
/// that runs without a screen (a no-view command), or a component serving
/// several commands of either mode. Implement it and call
/// [`export!`](crate::export):
///
/// ```ignore
/// struct Hello;
/// pane_guest::export!(Hello);
///
/// impl pane_guest::Command for Hello {
///     type CustomView = pane_guest::NoCustomView;
///
///     async fn render() -> Result<List, String> {
///         Ok(List::new("Hello").item(
///             Item::new("greet", "Say hello").on_action(|| async { Ok("Hello".into()) }),
///         ))
///     }
///     // submit_form, open_view ...
/// }
/// ```
///
/// A no-view command implements [`Command::run`] instead of `render`:
///
/// ```ignore
/// impl pane_guest::Command for Toggle {
///     type CustomView = pane_guest::NoCustomView;
///
///     async fn run(command: String, launch: LaunchRecord) -> Result<String, String> {
///         Ok("Toggled".into())
///     }
/// }
/// ```
pub trait Command: 'static {
    /// The custom view the command opens ([`NoCustomView`](crate::NoCustomView)
    /// for none).
    type CustomView: GuestCustomView;

    /// The command's list, as it is now. Pane asks for it when the command
    /// opens and again after each action. An error is shown to the user.
    /// [`commands::current`](crate::commands::current) is the launch record
    /// the screen was opened with. Without it, opening the command is an
    /// error: a no-view command has no list.
    fn render() -> impl Future<Output = Result<List, String>> {
        async { Err("this command opens no screen".into()) }
    }

    /// Runs the no-view command `command` (its id in `pane.json`, so one
    /// component can serve several commands), launched as `launch` says:
    /// how (by the user or in the background, and from where), with any
    /// text sent through its alias or as a fallback, and any context
    /// another command passed. The text it answers is shown as the result,
    /// and an error is shown as the failure.
    /// Pane calls it only for a command whose `pane.json` entry says
    /// `"mode": "no-view"`; without it, that is an error.
    fn run(command: String, launch: LaunchRecord) -> impl Future<Output = Result<String, String>> {
        let _ = launch;
        async move {
            Err(format!(
                "`{command}` opens a screen; it has no run entry point"
            ))
        }
    }

    /// Runs the search result with `id` the user chose, for a command that
    /// searches as the user types (`pane_guest::search`): its id is the
    /// callback Pane hands back. The text is shown as the result. Without
    /// it, choosing an id no item names is an error.
    fn run_search_result(id: String) -> impl Future<Output = Result<String, String>> {
        async move { Err(format!("unknown action: {id}")) }
    }

    /// Handles the submitted form of the item with `item_id`. `values`
    /// holds every field of the form, in order. The text is shown as the
    /// result; an error is shown next to its field. Without it, a submitted
    /// form is refused.
    fn submit_form(
        item_id: String,
        values: Vec<FieldValue>,
    ) -> impl Future<Output = Result<String, FormError>> {
        let _ = (item_id, values);
        async {
            Err(FormError {
                field: None,
                message: "this command has no forms".into(),
            })
        }
    }

    /// Opens the custom view of the item with `item_id`. Each call opens a
    /// new view with its own state. Without it, opening one is an error.
    fn open_view(item_id: String) -> impl Future<Output = Result<CustomView, String>> {
        let _ = item_id;
        async { Err("this command has no custom views".into()) }
    }
}

impl<T: Command> wit::Guest for T {
    type CustomView = <T as Command>::CustomView;

    async fn render(launch: LaunchRecord) -> Result<String, String> {
        crate::commands::set_current(launch);
        let list = <T as Command>::render().await?;
        Ok(remember(list))
    }

    async fn run(command: String, launch: LaunchRecord) -> Result<String, String> {
        crate::commands::set_current(launch.clone());
        let status = <T as Command>::run(command, launch).await?;
        Ok(answer(&status))
    }

    async fn handle_event(callback: String, _details: String) -> Result<String, String> {
        let action = match take(&callback) {
            Some(action) => Some(action),
            None => {
                // A fresh instance: the list names its actions once drawn.
                remember(<T as Command>::render().await?);
                take(&callback)
            }
        };
        let status = match action {
            Some(action) => action().await?,
            None => <T as Command>::run_search_result(callback).await?,
        };
        Ok(answer(&status))
    }

    async fn submit_form(item_id: String, values: Vec<FieldValue>) -> Result<String, FormError> {
        <T as Command>::submit_form(item_id, values).await
    }

    async fn open_view(item_id: String) -> Result<CustomView, String> {
        <T as Command>::open_view(item_id).await
    }
}

/// The answer object of `handle-event` and `run` for `status`, the text
/// shown as the result: `{"status": ...}`.
fn answer(status: &str) -> String {
    let mut answer = String::from("{\"status\":");
    string(&mut answer, status);
    answer.push('}');
    answer
}

/// The actions of the list the instance drew last, by callback id.
struct Actions(RefCell<BTreeMap<String, Action>>);

// SAFETY: a component's code runs on one thread, and no borrow of the map
// is held across an `await`.
unsafe impl Sync for Actions {}

static ACTIONS: Actions = Actions(RefCell::new(BTreeMap::new()));

/// The action named `callback` in the list drawn last, taken out: Pane
/// draws the list again after it runs.
fn take(callback: &str) -> Option<Action> {
    ACTIONS.0.borrow_mut().remove(callback)
}

/// `list` as its tree, keeping its actions by their callback ids in place
/// of those of the list drawn before.
fn remember(list: List) -> String {
    let mut actions = ACTIONS.0.borrow_mut();
    actions.clear();
    let mut tree = format!("{{\"version\":{TREE_VERSION},\"view\":{{\"type\":\"list\",\"title\":");
    string(&mut tree, &list.title);
    tree.push_str(",\"items\":[");
    for (index, item) in list.items.into_iter().enumerate() {
        if index > 0 {
            tree.push(',');
        }
        tree.push_str("{\"id\":");
        string(&mut tree, &item.id);
        tree.push_str(",\"title\":");
        string(&mut tree, &item.title);
        if let Some(subtitle) = &item.subtitle {
            tree.push_str(",\"subtitle\":");
            string(&mut tree, subtitle);
        }
        if let Some(action) = item.action {
            // The item's id names its action's callback.
            tree.push_str(",\"actions\":[{\"onAction\":");
            string(&mut tree, &item.id);
            tree.push_str("}]");
            actions.insert(item.id.clone(), action);
        }
        if let Some(form) = &item.form {
            tree.push_str(",\"form\":");
            write_form(&mut tree, form);
        }
        if let Some(platforms) = &item.platforms {
            tree.push_str(",\"platforms\":[");
            for (index, platform) in platforms.iter().enumerate() {
                if index > 0 {
                    tree.push(',');
                }
                string(
                    &mut tree,
                    match platform {
                        Platform::Windows => "windows",
                        Platform::Macos => "macos",
                        Platform::Linux => "linux",
                    },
                );
            }
            tree.push(']');
        }
        if let Some(view) = &item.custom_view {
            tree.push_str(",\"customView\":{\"title\":");
            string(&mut tree, &view.title);
            tree.push_str(",\"label\":");
            string(&mut tree, &view.label);
            tree.push_str(",\"role\":");
            string(
                &mut tree,
                match view.role {
                    CustomViewRole::ColorWell => "color-well",
                },
            );
            tree.push('}');
        }
        tree.push('}');
    }
    tree.push_str("]}}");
    tree
}

/// Writes `form` as the tree's JSON.
fn write_form(tree: &mut String, form: &Form) {
    tree.push_str("{\"title\":");
    string(tree, &form.title);
    tree.push_str(",\"submitLabel\":");
    string(tree, &form.submit_label);
    tree.push_str(",\"fields\":[");
    for (index, field) in form.fields.iter().enumerate() {
        if index > 0 {
            tree.push(',');
        }
        tree.push_str("{\"id\":");
        string(tree, &field.id);
        tree.push_str(",\"label\":");
        string(tree, &field.label);
        match &field.kind {
            FieldKind::Text(text) => {
                tree.push_str(",\"kind\":\"text\"");
                if let Some(placeholder) = &text.placeholder {
                    tree.push_str(",\"placeholder\":");
                    string(tree, placeholder);
                }
            }
            FieldKind::Choice(choices) => {
                tree.push_str(",\"kind\":\"choice\",\"choices\":[");
                for (index, choice) in choices.iter().enumerate() {
                    if index > 0 {
                        tree.push(',');
                    }
                    tree.push_str("{\"id\":");
                    string(tree, &choice.id);
                    tree.push_str(",\"label\":");
                    string(tree, &choice.label);
                    tree.push('}');
                }
                tree.push(']');
            }
        }
        tree.push('}');
    }
    tree.push_str("]}");
}

/// Writes `text` as a JSON string.
fn string(json: &mut String, text: &str) {
    json.push('"');
    for character in text.chars() {
        match character {
            '"' => json.push_str("\\\""),
            '\\' => json.push_str("\\\\"),
            '\n' => json.push_str("\\n"),
            '\r' => json.push_str("\\r"),
            '\t' => json.push_str("\\t"),
            control if u32::from(control) < 0x20 => {
                let _ = write!(json, "\\u{:04x}", u32::from(control));
            }
            other => json.push(other),
        }
    }
    json.push('"');
}
