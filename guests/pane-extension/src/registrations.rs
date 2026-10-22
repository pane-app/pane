//! What a command registers at run time and owns (ADR 0041, #158): a
//! dynamic root item under one of its commands, a timer, a folder watcher
//! or a run-time provision, each a handle the extension owns. Dropping
//! the handle undoes it; so does the instance holding it going away, and
//! the generation ending — a disable, a reload, an update, an uninstall
//! or a pause — so an author never writes cleanup code. Nothing here
//! changes a package's `pane.json` beyond opting into the activation
//! entry point (`"activate"`, [`crate::lifecycle`]), which Pane calls
//! when the package's code may run, so registrations are made without
//! waiting for the user.
//!
//! A [`Timer`]` fires by calling a closure Pane hands the event export
//! ([`export!`](Self::export) below): `every(5, || async { ... })`
//! registers a timer that fires every five seconds. A [`Watcher`] reports
//! a folder's changes the same way, coalesced. A dynamic root item is
//! [`Item`]s in the item shape of docs/list-tree.md, whose actions are
//! repeatable closures, delivered as the command's actions are; one that
//! declares a [`Mode`](Item::mode) is a **dynamic command**, launched as
//! its command is with a launch record naming the item's id. A
//! [`Provision`] provides a capability the package's `pane.json` marks
//! `atRunTime`, only while this is held.
//!
//! Every registration is refused, with the reason, when it is beyond
//! Pane's limits (1000 dynamic root items, 64 timers, 16 watchers and 16
//! provisions for one package), when a timer's interval is not between
//! 1 second and 30 days, when a watcher's path cannot be watched, when a
//! provision is not declared and marked, and when this code of the
//! extension was replaced — using a handle after a reload fails the same
//! way.

use core::cell::RefCell;
use core::future::Future;
use core::pin::Pin;

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;

use crate::icon::{Accessory, Icon, Look};
use crate::registrations::exports::pane::extension::events as events_exported;
use crate::registrations::exports::pane::extension::events::{Event, WatcherChanges};

wit_bindgen::generate!({
    path: "wit",
    world: "events-provider",
    pub_export_macro: true,
    default_bindings_module: "pane_extension::registrations",
    generate_unused_types: true,
});

/// The registrations themselves: `pane:extension/registrations`, as the
/// command world's bindings generate them.
use crate::pane::extension::registrations as registering;

pub use registering::{Provision, RootItem, Timer, Watcher};

/// What an action or an event answers: success, or an error shown as a
/// failure toast.
type Answer = Pin<Box<dyn Future<Output = Result<(), String>>>>;

/// What an action runs, again and again while the item is registered,
/// unlike a list item's, which runs once per drawing: shared, so the table
/// can hand the same closure to each firing.
type Run = Arc<dyn Fn() -> Answer>;

/// The callbacks of the timers registered, by their tags.
static TIMERS: Callbacks = Callbacks(RefCell::new(BTreeMap::new()));

/// The callbacks of the folder watchers registered, by their tags.
static WATCHERS: Watchers = Watchers(RefCell::new(BTreeMap::new()));

/// The actions of the dynamic root items registered, by their callback
/// ids: as the list's are (see `list`), but not taken when they run, and
/// not cleared when a list is drawn.
static ITEMS: Callbacks = Callbacks(RefCell::new(BTreeMap::new()));

/// Callbacks by id, kept while the registration that made them lives.
struct Callbacks(RefCell<BTreeMap<String, Run>>);

/// A watcher's callbacks by tag: what a change answers, shared as an
/// action's is.
struct Watchers(RefCell<BTreeMap<String, Arc<dyn Fn(&WatcherChanges) -> Answer>>>);

impl Watchers {
    /// Keeps `watch` under `tag`.
    fn insert(&self, tag: String, watch: Arc<dyn Fn(&WatcherChanges) -> Answer>) {
        self.0.borrow_mut().insert(tag, watch);
    }

    /// The callback `tag` names, run for each of the watcher's events.
    fn of(&self, tag: &str) -> Option<Arc<dyn Fn(&WatcherChanges) -> Answer>> {
        self.0.borrow().get(tag).cloned()
    }
}

// SAFETY: a component's code runs on one thread, and no borrow of the
// maps is held across an `await`.
unsafe impl Sync for Callbacks {}
unsafe impl Sync for Watchers {}

impl Callbacks {
    /// Keeps `run` under `id`.
    fn insert(&self, id: String, run: Run) {
        self.0.borrow_mut().insert(id, run);
    }

    /// The callback `id` names, run for the `count`th time.
    fn run(&self, id: &str) -> Option<Run> {
        self.0.borrow().get(id).cloned()
    }
}

/// The event export, as the SDK implements it: what the timers and
/// watchers registered call back into. Export it beside the command:
///
/// ```ignore
/// pane_extension::export!(Registrations);
/// pane_extension::registrations::export_events!();
/// ```
///
/// A component that registers timers or watchers exports the events
/// interface (its `pane.json` needs no field for it); one that registers
/// neither is unchanged.
pub struct Events;

impl events_exported::Guest for Events {
    async fn handle_event(event: Event) -> Result<(), String> {
        match event {
            Event::Timer(tag) => {
                let run = TIMERS.run(&tag).ok_or_else(|| {
                    format!("this timer's tag “{tag}” was not registered by this component")
                })?;
                run().await
            }
            Event::Watcher(watched) => {
                let tag = watched.tag;
                let watch = WATCHERS.of(&tag).ok_or_else(|| {
                    format!("this watcher's tag “{tag}” was not registered by this component")
                })?;
                watch(&watched.changes).await
            }
        }
    }
}

/// One change a folder watcher reports, as the callback receives it.
pub enum Change {
    /// The paths that changed, relative to the watched folder.
    Paths(Vec<String>),
    /// Too many changes were lost: the extension should rescan the
    /// folder itself.
    Rescan,
}

/// Registers a dynamic root item under the command with manifest id
/// `command` (its id in `pane.json`, served by this component), from
/// `item`: a row of root search, matched and ranked like an indexed
/// result. An item that declares a [`mode`](Item::mode) is a dynamic
/// command; invoking one launches its command with a launch record
/// naming the item's id. Invoking one without a mode runs its first
/// action's closure, as often as the user chooses it. Dropping the
/// handle removes the row.
pub fn root_item(command: &str, item: Item) -> Result<RootItem, String> {
    let (json, _) = item.written();
    registering::add_root_item(command, &json)
}

/// Registers a timer that fires once, `seconds` from now (1 to 2592000),
/// calling `run`. Firings that fall due while one is pending, or while
/// the package waits for what it needs, are one firing, delivered when
/// they can be. Dropping the handle stops the timer.
pub fn after<F, A>(seconds: u64, run: F) -> Result<Timer, String>
where
    F: Fn() -> A + 'static,
    A: Future<Output = Result<(), String>> + 'static,
{
    timer(false, seconds, run)
}

/// Registers a timer that fires every `seconds` (1 to 2592000) until it
/// is dropped or the generation ends, calling `run`, as [`after`]'s
/// firings are coalesced.
pub fn every<F, A>(seconds: u64, run: F) -> Result<Timer, String>
where
    F: Fn() -> A + 'static,
    A: Future<Output = Result<(), String>> + 'static,
{
    timer(true, seconds, run)
}

/// Registers a watcher of `path` (a folder the system can watch),
/// recursive or not, calling `on_change` with what changed, coalesced for
/// half a second, or asking it to rescan after an overflow. Dropping the
/// handle stops the watcher.
pub fn watch_folder<F, A>(path: &str, recursive: bool, on_change: F) -> Result<Watcher, String>
where
    F: Fn(Change) -> A + 'static,
    A: Future<Output = Result<(), String>> + 'static,
{
    let tag = tag("watcher");
    WATCHERS.insert(
        tag.clone(),
        Arc::new(move |changes: &WatcherChanges| {
            let change = match changes {
                WatcherChanges::Paths(paths) => Change::Paths(paths.clone()),
                WatcherChanges::Rescan => Change::Rescan,
            };
            Box::pin(on_change(change)) as Answer
        }),
    );
    registering::watch_folder(path, recursive, &tag)
}

/// Registers a provision of the capability `capability`, such as
/// "acme:translate@1": the package provides it while this is held,
/// served by this instance. The package's `pane.json` must declare it
/// under `provides`, marked `atRunTime`.
pub fn provide(capability: &str) -> Result<Provision, String> {
    registering::provide_capability(capability)
}

/// A unique tag for a registration's callbacks, never reused.
fn tag(what: &str) -> String {
    static NEXT: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
    let next = NEXT.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    format!("{what}:{next}")
}

/// A timer, registered with `run` under a fresh tag.
fn timer<F, A>(every: bool, seconds: u64, run: F) -> Result<Timer, String>
where
    F: Fn() -> A + 'static,
    A: Future<Output = Result<(), String>> + 'static,
{
    let tag = tag("timer");
    TIMERS.insert(tag.clone(), Arc::new(move || Box::pin(run()) as Answer));
    if every {
        registering::timer_every(seconds, &tag)
    } else {
        registering::timer_after(seconds, &tag)
    }
}

impl RootItem {
    /// Replaces the item this handle owns with `item`: the row shows what
    /// the new item says, keeping its id. (The bindings' own `update`
    /// takes the item's JSON; this takes the item.)
    pub fn replace(&self, item: Item) -> Result<(), String> {
        let (json, _) = item.written();
        self.update(&json)
    }
}

impl Item {
    /// This item's JSON, as `add-root-item` takes it, keeping its
    /// actions' closures under their callback ids.
    fn written(self) -> (String, usize) {
        let actions = self.actions.len();
        let mut tree = String::from("{\"id\":");
        string(&mut tree, &self.id);
        tree.push_str(",\"title\":");
        string(&mut tree, &self.title);
        if let Some(subtitle) = &self.subtitle {
            tree.push_str(",\"subtitle\":");
            string(&mut tree, subtitle);
        }
        if !self.actions.is_empty() {
            tree.push_str(",\"actions\":");
            // The item's id names its first action's callback, and the id
            // and their place its later ones', as a list item's are.
            let id = self.id.clone();
            write_actions(&mut tree, self.actions, &|index| {
                if index == 0 {
                    id.clone()
                } else {
                    format!("{id}#{index}")
                }
            });
        }
        if let Some(mode) = self.mode {
            tree.push_str(",\"mode\":");
            string(
                &mut tree,
                match mode {
                    Mode::View => "view",
                    Mode::NoView => "no-view",
                },
            );
        }
        crate::icon::write_look(&mut tree, &self.look);
        tree.push('}');
        (tree, actions)
    }
}

/// Writes `actions` as the item's JSON array, naming each one's callback
/// `name(place)` and keeping its closure under that name.
fn write_actions(tree: &mut String, actions: Vec<Action>, name: &dyn Fn(usize) -> String) {
    tree.push('[');
    for (index, action) in actions.into_iter().enumerate() {
        if index > 0 {
            tree.push(',');
        }
        let callback = name(index);
        tree.push('{');
        tree.push_str("\"onAction\":");
        string(tree, &callback);
        ITEMS.insert(callback, action.run);
        if let Some(title) = action.title {
            tree.push_str(",\"title\":");
            string(tree, &title);
        }
        if let Some(section) = action.section {
            tree.push_str(",\"section\":");
            string(tree, &section);
        }
        if action.destructive {
            tree.push_str(",\"style\":\"destructive\"");
        }
        if let Some(icon) = action.icon {
            tree.push_str(",\"icon\":");
            crate::icon::write_icon(tree, &icon);
        }
        tree.push('}');
    }
    tree.push(']');
}

/// Writes `text` as a JSON string, as the tree's writer does.
fn string(tree: &mut String, text: &str) {
    tree.push('"');
    for character in text.chars() {
        match character {
            '"' => tree.push_str("\\\""),
            '\\' => tree.push_str("\\\\"),
            '\n' => tree.push_str("\\n"),
            '\r' => tree.push_str("\\r"),
            '\t' => tree.push_str("\\t"),
            character if (character as u32) < 0x20 => {
                tree.push_str(&format!("\\u{:04x}", character as u32));
            }
            character => tree.push(character),
        }
    }
    tree.push('"');
}

/// A dynamic root item, in the item shape of docs/list-tree.md: a row of
/// root search, under one of the package's commands.
pub struct Item {
    id: String,
    title: String,
    subtitle: Option<String>,
    actions: Vec<Action>,
    mode: Option<Mode>,
    look: Look,
}

/// What a dynamic command declares ([`Item::mode`]): invoking the item
/// launches its command, with a launch record naming the item's id.
pub enum Mode {
    /// Invoking the item opens its command's screen.
    View,
    /// Invoking the item runs its command.
    NoView,
}

impl Item {
    /// An item titled `title`. `id` identifies it among this component's
    /// registered items; a quick slot, an alias or a hotkey holds the
    /// item by its command and this id.
    pub fn new(id: impl Into<String>, title: impl Into<String>) -> Item {
        Item {
            id: id.into(),
            title: title.into(),
            subtitle: None,
            actions: Vec::new(),
            mode: None,
            look: Look::default(),
        }
    }

    /// This item with a second line under its title, which root search
    /// matches as it matches the title.
    pub fn subtitle(mut self, subtitle: impl Into<String>) -> Item {
        self.subtitle = Some(subtitle.into());
        self
    }

    /// This item with an untitled action, which runs when the user
    /// invokes the row: as often as they choose it.
    pub fn on_action<F, A>(mut self, action: F) -> Item
    where
        F: Fn() -> A + 'static,
        A: Future<Output = Result<(), String>> + 'static,
    {
        self.actions.push(Action {
            title: None,
            section: None,
            destructive: false,
            icon: None,
            run: Arc::new(move || Box::pin(action()) as Answer),
        });
        self
    }

    /// This item with `action` after its actions. The first is the row's
    /// primary action (Enter).
    pub fn action(mut self, action: Action) -> Item {
        self.actions.push(action);
        self
    }

    /// This item with `actions` after its actions.
    pub fn actions(mut self, actions: impl IntoIterator<Item = Action>) -> Item {
        self.actions.extend(actions);
        self
    }

    /// This item as a dynamic command: invoking it launches its command,
    /// with a launch record naming the item's id. Without a mode,
    /// invoking the row runs its first action.
    pub fn mode(mut self, mode: Mode) -> Item {
        self.mode = Some(mode);
        self
    }

    /// This item with `icon` before its title (see [`crate::icon`]).
    pub fn icon(mut self, icon: Icon) -> Item {
        self.look.icon = Some(icon);
        self
    }

    /// This item with a tooltip on its title.
    pub fn title_tooltip(mut self, tooltip: impl Into<String>) -> Item {
        self.look.title_tooltip = Some(tooltip.into());
        self
    }

    /// This item with a tooltip on its subtitle.
    pub fn subtitle_tooltip(mut self, tooltip: impl Into<String>) -> Item {
        self.look.subtitle_tooltip = Some(tooltip.into());
        self
    }

    /// This item with `accessory` after its accessories. A row shows the
    /// first three.
    pub fn accessory(mut self, accessory: Accessory) -> Item {
        self.look.accessories.push(accessory);
        self
    }

    /// This item with `accessories` after its accessories.
    pub fn accessories(mut self, accessories: impl IntoIterator<Item = Accessory>) -> Item {
        self.look.accessories.extend(accessories);
        self
    }
}

/// An action of a dynamic root item: repeatable, as the item is
/// registered, unlike a list item's once-per-drawing action.
pub struct Action {
    title: Option<String>,
    section: Option<String>,
    destructive: bool,
    icon: Option<Icon>,
    run: Run,
}

impl Action {
    /// An action titled `title`, running `run` each time the user chooses
    /// it. An error it answers is shown as a failure toast; on success it
    /// tells the user what happened itself ([`crate::feedback`]).
    pub fn new<F, A>(title: impl Into<String>, run: F) -> Action
    where
        F: Fn() -> A + 'static,
        A: Future<Output = Result<(), String>> + 'static,
    {
        Action {
            title: Some(title.into()),
            section: None,
            destructive: false,
            icon: None,
            run: Arc::new(move || Box::pin(run()) as Answer),
        }
    }

    /// This action in the destructive style, drawn red.
    pub fn destructive(mut self) -> Action {
        self.destructive = true;
        self
    }

    /// This action in `section` of the Actions panel.
    pub fn section(mut self, section: impl Into<String>) -> Action {
        self.section = Some(section.into());
        self
    }

    /// This action with `icon` in the Actions panel (see [`crate::icon`]).
    pub fn icon(mut self, icon: Icon) -> Action {
        self.icon = Some(icon);
        self
    }
}

/// Runs the action of a dynamic root item whose callback `callback`
/// names, as the command's `handle-event` is asked to, answering whether
/// it ran. Called by `list`'s handling of an id the drawn list does not
/// name: a dynamic item's action is one of those.
pub(crate) async fn run_item_action(callback: &str) -> Result<bool, String> {
    match ITEMS.run(callback) {
        Some(run) => {
            run().await?;
            Ok(true)
        }
        None => Ok(false),
    }
}

/// Exports the events entry point ([`Events`]), as the timers and
/// watchers registered are delivered to. Beside
/// [`pane_extension::export!`](crate::export!) the command's own export
/// is, importing the entry point beside:
///
/// ```ignore
/// pane_extension::export!(Registrations);
/// use pane_extension::registrations::Events;
/// pane_extension::registrations::export_events!(Events);
/// ```
#[macro_export]
macro_rules! export_events {
    ($ty:ident) => {
        $crate::registrations::export!($ty);
    };
}

pub use crate::export_events;
