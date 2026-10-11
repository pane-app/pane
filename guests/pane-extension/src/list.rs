//! A command's list, as an author writes it: a [`List`] of [`Item`]s whose
//! actions are closures. The command implements [`Command`] and calls
//! [`export!`](crate::export); the SDK answers Pane's `render` with the
//! list as the versioned JSON tree of `docs/list-tree.md`, names each
//! action by a callback id, and runs the closure when Pane hands that id
//! to `handle-event`. Authors never see the JSON or the ids.
//!
//! An item has any number of [`Action`]s, in order: Enter runs the first
//! (its primary action), Ctrl+Enter the second, Ctrl+Shift+Enter the third,
//! and Ctrl+K lists them all in Pane's Actions panel, in their sections, each
//! with its [`Shortcut`] if Pane binds it.
//!
//! An action may open a [`Submenu`] instead of running a closure
//! ([`Action::submenu`]): further choices the panel lists in place, each an
//! action of its own. Its entries are given with the list
//! ([`Submenu::new`]), or by a closure Pane calls each time the submenu
//! opens ([`Submenu::lazy`]). A submenu's entries are named after the action
//! that opens it and their place (`<callback>/0`, `<callback>/1`, ...); the
//! closure of a lazy submenu is named as an action would be, and Pane hands
//! that name to `handle-event` to ask for the entries, which the SDK answers
//! as `{"entries": [...]}`.
//!
//! An item's first action is named by the item's id and its later ones by
//! the id and their place (`<id>#1`, `<id>#2`, ...), so the same action has
//! the same callback in every drawing of the list. Pane draws the list again
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
//!
//! An action, a run and a search result answer success or an error. Pane
//! shows nothing of a success: the command tells the user what happened
//! with a toast or a HUD ([`crate::feedback`]), or closes the window
//! ([`crate::window`]). An error is shown as a failure toast.

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
use wit::Platform;

/// The version of the tree this SDK writes (`docs/list-tree.md`).
const TREE_VERSION: u32 = 1;

/// What an action answers: success, or an error shown as a failure toast.
pub(crate) type Answer = Pin<Box<dyn Future<Output = Result<(), String>>>>;

/// What an action runs, once, when the user chooses it.
type Run = Box<dyn FnOnce() -> Answer>;

/// What a lazy submenu's closure answers: its entries, or an error Pane
/// shows as the submenu's one entry.
type Entries = Pin<Box<dyn Future<Output = Result<Vec<Action>, String>>>>;

/// What a lazy submenu runs, once, when it opens.
type Load = Box<dyn FnOnce() -> Entries>;

/// A callback of the list drawn last, by its id: an action's closure, or a
/// lazy submenu's.
enum Callback {
    Run(Run),
    Open(Load),
}

/// A modifier key held with a [`Shortcut`]'s key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Modifier {
    /// Ctrl (Control on macOS).
    Ctrl,
    /// Alt (Option on macOS).
    Alt,
    /// Shift.
    Shift,
    /// The system key: Command on macOS, the Windows key on Windows, Super
    /// on Linux.
    Cmd,
}

impl Modifier {
    fn name(self) -> &'static str {
        match self {
            Modifier::Ctrl => "ctrl",
            Modifier::Alt => "alt",
            Modifier::Shift => "shift",
            Modifier::Cmd => "cmd",
        }
    }
}

/// One key with the modifiers held with it.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Keys {
    modifiers: Vec<Modifier>,
    key: String,
}

impl Keys {
    fn new(modifiers: impl IntoIterator<Item = Modifier>, key: impl Into<String>) -> Keys {
        Keys {
            modifiers: modifiers.into_iter().collect(),
            key: key.into(),
        }
    }
}

/// The keys that run an action from the list without opening the Actions
/// panel: one key with its modifiers on every system, or one per system.
/// Keys are named as Pane names them: a letter or digit, a character such
/// as `,`, or `enter`, `delete`, `backspace`, `up`, `f5` and the like.
///
/// Pane matches the modifiers exactly, and never binds a shortcut that is
/// one of its own keys (Escape, Ctrl+K, the arrows, Ctrl and a digit, the
/// keys the user gave Pane's actions): the action then stays in the panel
/// without it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Shortcut {
    every: Option<Keys>,
    windows: Option<Keys>,
    macos: Option<Keys>,
    linux: Option<Keys>,
}

impl Shortcut {
    /// `key` with `modifiers` held, on every system.
    pub fn new(modifiers: impl IntoIterator<Item = Modifier>, key: impl Into<String>) -> Shortcut {
        Shortcut {
            every: Some(Keys::new(modifiers, key)),
            ..Shortcut::default()
        }
    }

    /// A shortcut per system, with none yet: add each system's with
    /// [`Shortcut::windows`], [`Shortcut::macos`] and [`Shortcut::linux`].
    /// A system it gives none for binds nothing.
    pub fn per_platform() -> Shortcut {
        Shortcut::default()
    }

    /// This shortcut with `key` and `modifiers` on Windows.
    pub fn windows(
        mut self,
        modifiers: impl IntoIterator<Item = Modifier>,
        key: impl Into<String>,
    ) -> Shortcut {
        self.every = None;
        self.windows = Some(Keys::new(modifiers, key));
        self
    }

    /// This shortcut with `key` and `modifiers` on macOS.
    pub fn macos(
        mut self,
        modifiers: impl IntoIterator<Item = Modifier>,
        key: impl Into<String>,
    ) -> Shortcut {
        self.every = None;
        self.macos = Some(Keys::new(modifiers, key));
        self
    }

    /// This shortcut with `key` and `modifiers` on Linux.
    pub fn linux(
        mut self,
        modifiers: impl IntoIterator<Item = Modifier>,
        key: impl Into<String>,
    ) -> Shortcut {
        self.every = None;
        self.linux = Some(Keys::new(modifiers, key));
        self
    }
}

/// One of an item's actions, or an entry of a [`Submenu`]: what it is
/// called, where the Actions panel lists it, how it is drawn (its icon, its
/// style), its shortcut, and the closure it runs or the submenu it opens.
pub struct Action {
    title: Option<String>,
    section: Option<String>,
    destructive: bool,
    shortcut: Option<Shortcut>,
    icon: Option<crate::icon::Icon>,
    does: Does,
}

/// What choosing an action does.
enum Does {
    Run(Run),
    Open(Submenu),
}

/// Further choices an action opens in place in the Actions panel, such as
/// "Open With…" or "Move to List…": a title, which the panel shows while it
/// is open, and entries, each an [`Action`] of its own (a closure, or a
/// further submenu). The panel filters them as the user types, and an
/// entry's shortcut works while its submenu is shown.
pub struct Submenu {
    title: String,
    entries: SubmenuEntries,
}

/// Where a submenu's entries come from.
enum SubmenuEntries {
    Given(Vec<Action>),
    Asked(Load),
}

impl Submenu {
    /// A submenu titled `title` whose entries are given with the list: add
    /// them with [`Submenu::entry`] and [`Submenu::entries`].
    pub fn new(title: impl Into<String>) -> Submenu {
        Submenu {
            title: title.into(),
            entries: SubmenuEntries::Given(Vec::new()),
        }
    }

    /// A submenu titled `title` whose entries `load` gives when the user
    /// opens it, each time: Pane shows it loading until `load` answers, and
    /// an error as its one entry.
    pub fn lazy<F, A>(title: impl Into<String>, load: F) -> Submenu
    where
        F: FnOnce() -> A + 'static,
        A: Future<Output = Result<Vec<Action>, String>> + 'static,
    {
        Submenu {
            title: title.into(),
            entries: SubmenuEntries::Asked(Box::new(move || Box::pin(load()) as Entries)),
        }
    }

    /// This submenu with `entry` after its entries (a lazy one becomes one
    /// whose entries are given).
    pub fn entry(mut self, entry: Action) -> Submenu {
        if let SubmenuEntries::Given(entries) = &mut self.entries {
            entries.push(entry);
        } else {
            self.entries = SubmenuEntries::Given(alloc::vec![entry]);
        }
        self
    }

    /// This submenu with `entries` after its entries (see
    /// [`Submenu::entry`]).
    pub fn entries(self, entries: impl IntoIterator<Item = Action>) -> Submenu {
        entries.into_iter().fold(self, Submenu::entry)
    }
}

impl Action {
    /// An action titled `title` that runs `run` when the user chooses it.
    /// An error it answers is shown as a failure toast; on success it
    /// tells the user what happened itself ([`crate::feedback`]). Pane
    /// draws the list again afterwards.
    pub fn new<F, A>(title: impl Into<String>, run: F) -> Action
    where
        F: FnOnce() -> A + 'static,
        A: Future<Output = Result<(), String>> + 'static,
    {
        Action {
            title: Some(title.into()),
            section: None,
            destructive: false,
            shortcut: None,
            icon: None,
            does: Does::Run(Box::new(move || Box::pin(run()) as Answer)),
        }
    }

    /// An action titled `title` that opens `submenu` in the Actions panel
    /// when the user chooses it ("Open With…"), instead of running a
    /// closure. Enter, a chord or its shortcut open the panel at it when it
    /// is one of an item's actions.
    pub fn submenu(title: impl Into<String>, submenu: Submenu) -> Action {
        Action {
            title: Some(title.into()),
            section: None,
            destructive: false,
            shortcut: None,
            icon: None,
            does: Does::Open(submenu),
        }
    }

    /// This action in the section titled `title` of the Actions panel.
    /// Consecutive actions with the same section are one section.
    pub fn section(mut self, title: impl Into<String>) -> Action {
        self.section = Some(title.into());
        self
    }

    /// This action drawn in the destructive style: it deletes or removes
    /// something.
    pub fn destructive(mut self) -> Action {
        self.destructive = true;
        self
    }

    /// This action run by `shortcut` from the list.
    pub fn shortcut(mut self, shortcut: Shortcut) -> Action {
        self.shortcut = Some(shortcut);
        self
    }

    /// This action with `icon` beside it in the Actions panel (#139), in
    /// place of Pane's glyph: drawn as an item's icon is, a web image or a
    /// system icon showing its fallback until it loaded.
    pub fn icon(mut self, icon: crate::icon::Icon) -> Action {
        self.icon = Some(icon);
        self
    }
}

/// A command's screen: a list view, its title and items in order. A form
/// is a designed view now (#241, `crate::view::form`), not a screen the
/// list tree names.
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

/// One entry in a command's list. Choosing it runs its primary action
/// (its first); an item with none cannot be activated, and Pane says so.
pub struct Item {
    id: String,
    title: String,
    subtitle: Option<String>,
    actions: Vec<Action>,
    platforms: Option<Vec<Platform>>,
    /// Its icon, tooltips and accessories (`crate::icon`, #139).
    pub(crate) look: crate::icon::Look,
}

impl Item {
    /// An item titled `title`. `id` identifies it among the list's items:
    /// Pane keeps the selection on it when the list is drawn again, and
    /// passes it to [`Command::open_view`].
    pub fn new(id: impl Into<String>, title: impl Into<String>) -> Item {
        Item {
            id: id.into(),
            title: title.into(),
            subtitle: None,
            actions: Vec::new(),
            platforms: None,
            look: crate::icon::Look::default(),
        }
    }

    /// This item with a second line under its title.
    pub fn subtitle(mut self, subtitle: impl Into<String>) -> Item {
        self.subtitle = Some(subtitle.into());
        self
    }

    /// This item with an untitled action after its actions, which runs
    /// when the user chooses it: the item's primary action when it is the
    /// first, which Pane names "Run item". An error it answers is shown as
    /// a failure toast; on success it tells the user what happened itself
    /// ([`crate::feedback`]). Pane draws the list again afterwards.
    /// [`Item::action`] gives an action a title, a section, a style and a
    /// shortcut.
    pub fn on_action<F, A>(mut self, action: F) -> Item
    where
        F: FnOnce() -> A + 'static,
        A: Future<Output = Result<(), String>> + 'static,
    {
        self.actions.push(Action {
            title: None,
            section: None,
            destructive: false,
            shortcut: None,
            icon: None,
            does: Does::Run(Box::new(move || Box::pin(action()) as Answer)),
        });
        self
    }

    /// This item with `action` after its actions. The first is the item's
    /// primary action (Enter), the second its secondary action
    /// (Ctrl+Enter), the third runs with Ctrl+Shift+Enter, and the Actions
    /// panel (Ctrl+K) lists them all.
    pub fn action(mut self, action: Action) -> Item {
        self.actions.push(action);
        self
    }

    /// This item with `actions` after its actions (see [`Item::action`]).
    pub fn actions(mut self, actions: impl IntoIterator<Item = Action>) -> Item {
        self.actions.extend(actions);
        self
    }

    /// This item's action working on `platforms` only. Elsewhere Pane
    /// still lists the item but shows it as unavailable with the reason,
    /// and never runs its action.
    pub fn platforms(mut self, platforms: impl IntoIterator<Item = Platform>) -> Item {
        self.platforms = Some(platforms.into_iter().collect());
        self
    }
}

/// An extension command: one whose screen is a list (a view command), one
/// that runs without a screen (a no-view command), one whose screen is a
/// designed view (a `"mode": "designed"` command), or a component serving
/// several commands of any mode. Implement it and call
/// [`export!`](crate::export):
///
/// ```ignore
/// struct Hello;
/// pane_extension::export!(Hello);
///
/// impl pane_extension::Command for Hello {
///     type DesignedView = pane_extension::view::NoDesignedView;
///
///     async fn render() -> Result<List, String> {
///         Ok(List::new("Hello").item(Item::new("greet", "Say hello").on_action(|| async {
///             pane_extension::feedback::show_toast(Toast::success("Hello"));
///             Ok(())
///         })))
///     }
/// }
/// ```
///
/// A root provider (`"mode": "provider"`, a command that only answers root
/// search through [`crate::root`] or [`crate::indexed`]) implements neither:
/// Pane never opens or runs it, so `type DesignedView` is all it needs.
///
/// A no-view command implements [`Command::run`] instead of `render`:
///
/// ```ignore
/// impl pane_extension::Command for Toggle {
///     type DesignedView = pane_extension::view::NoDesignedView;
///
///     async fn run(command: String, launch: LaunchRecord) -> Result<(), String> {
///         pane_extension::feedback::show_hud("Toggled", ToastStyle::Success);
///         Ok(())
///     }
/// }
/// ```
///
/// A designed command implements [`view::View`](crate::view::View) for its
/// screen's state and opens it:
///
/// ```ignore
/// impl pane_extension::view::View for Counter {
///     fn render(&mut self, cx: &mut Cx<Self>) -> impl IntoAnswer {
///         column()
///             .gap(Space::M)
///             .child(text("Count").style(TextStyle::Title))
///             .child(button("Increment").on_click(cx.listener(|this: &mut Self| {
///                 this.count += 1;
///             })))
///     }
/// }
///
/// impl pane_extension::Command for Sample {
///     type DesignedView = Counter;
///
///     async fn open_designed_view(
///         command: String,
///         launch: LaunchRecord,
///     ) -> Result<Counter, String> {
///         Ok(Counter { count: 0 })
///     }
/// }
/// ```
pub trait Command: 'static {
    /// The state of the designed view the command opens
    /// ([`view::NoDesignedView`](crate::view::NoDesignedView) for none):
    /// a [`view::View`](crate::view::View) whose `render` answers the
    /// tree its screen draws.
    type DesignedView: crate::view::View;

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
    /// another command passed. Pane shows nothing of a success: the command
    /// tells the user what happened with a toast or a HUD
    /// ([`crate::feedback`]). An error is shown as a failure toast with a
    /// "Copy Error" action, and a toast left in the animated style is
    /// hidden once the run ends.
    /// Pane calls it only for a command whose `pane.json` entry says
    /// `"mode": "no-view"`; without it, that is an error.
    fn run(command: String, launch: LaunchRecord) -> impl Future<Output = Result<(), String>> {
        let _ = launch;
        async move {
            Err(format!(
                "`{command}` opens a screen; it has no run entry point"
            ))
        }
    }

    /// Runs the search result with `id` the user chose — an id no item of
    /// the command's list names, as an older contract's command search
    /// answered through `run-search` (retired, #240: a command's search is
    /// the designed List's search-text event now). An error is shown as a
    /// failure toast. Without it, choosing an id no item names is an error.
    fn run_search_result(id: String) -> impl Future<Output = Result<(), String>> {
        async move { Err(format!("unknown action: {id}")) }
    }

    /// Opens the designed view of the command `command` (its id in
    /// `pane.json`, so one component can serve several commands), launched
    /// as `launch` says: the screen a `"mode": "designed"` command opens,
    /// a tree of layout nodes and UI components that Pane renders
    /// (`docs/designed-tree.md`). Each call opens a new view with its own
    /// state. Without it, opening one is an error.
    fn open_designed_view(
        command: String,
        launch: LaunchRecord,
    ) -> impl Future<Output = Result<Self::DesignedView, String>> {
        let _ = (command, launch);
        async { Err("this command opens no designed view".into()) }
    }
}

impl<T: Command> wit::Guest for T {
    type View = crate::view::Open<<T as Command>::DesignedView>;

    async fn render(launch: LaunchRecord) -> Result<String, String> {
        crate::commands::set_current(launch);
        let list = <T as Command>::render().await?;
        Ok(remember(list))
    }

    async fn run(command: String, launch: LaunchRecord) -> Result<String, String> {
        crate::commands::set_current(launch.clone());
        <T as Command>::run(command, launch).await?;
        Ok(ANSWER.into())
    }

    async fn handle_event(callback: String, _details: String) -> Result<String, String> {
        // A toast's action, which stays the toast's while it shows.
        if let Some(action) = crate::feedback::toast_action(&callback) {
            action().await?;
            return Ok(ANSWER.into());
        }
        let found = match take(&callback) {
            Some(found) => Some(found),
            None => {
                // A fresh instance, or a lazy submenu opened again: the list
                // names its callbacks once drawn.
                remember(<T as Command>::render().await?);
                take(&callback)
            }
        };
        // An action of a dynamic root item this component registered
        // (#158): kept while the item is, not taken when it runs, and not
        // cleared when the list is drawn.
        if found.is_none() && crate::registrations::run_item_action(&callback).await? {
            return Ok(ANSWER.into());
        }
        match found {
            Some(Callback::Run(action)) => {
                action().await?;
                Ok(ANSWER.into())
            }
            Some(Callback::Open(load)) => {
                let entries = load().await?;
                Ok(entries_answer(&callback, entries))
            }
            None => {
                <T as Command>::run_search_result(callback).await?;
                Ok(ANSWER.into())
            }
        }
    }

    async fn open_view(command: String, launch: LaunchRecord) -> Result<wit::View, String> {
        crate::commands::set_current(launch.clone());
        let state = <T as Command>::open_designed_view(command, launch).await?;
        Ok(wit::View::new(crate::view::Open::new(state)))
    }
}

/// The answer object of `handle-event` and `run`: empty, since Pane shows
/// nothing of an answer.
const ANSWER: &str = "{}";

/// The answer of `handle-event` for the lazy submenu `callback` opened:
/// `{"entries": [...]}`, its entries named `<callback>/<n>` and kept beside
/// the list's callbacks until the list is drawn again.
fn entries_answer(callback: &str, entries: Vec<Action>) -> String {
    let mut callbacks = ACTIONS.0.borrow_mut();
    let mut answer = String::from("{\"entries\":");
    write_actions(
        &mut answer,
        entries,
        &|index| format!("{callback}/{index}"),
        &mut callbacks,
    );
    answer.push('}');
    answer
}

/// The callbacks of the list the instance drew last (and of the lazy
/// submenus opened since), by callback id.
struct Actions(RefCell<BTreeMap<String, Callback>>);

// SAFETY: a component's code runs on one thread, and no borrow of the map
// is held across an `await`.
unsafe impl Sync for Actions {}

static ACTIONS: Actions = Actions(RefCell::new(BTreeMap::new()));

/// The callback named `callback` in the list drawn last, taken out: Pane
/// draws the list again after an action runs, and a lazy submenu opened
/// again draws it first to find its closure.
fn take(callback: &str) -> Option<Callback> {
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
        if !item.actions.is_empty() {
            tree.push_str(",\"actions\":");
            // The item's id names its first action's callback, and the id
            // and their place its later ones'.
            let id = item.id.clone();
            write_actions(
                &mut tree,
                item.actions,
                &|index| {
                    if index == 0 {
                        id.clone()
                    } else {
                        format!("{id}#{index}")
                    }
                },
                &mut actions,
            );
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
        crate::icon::write_look(&mut tree, &item.look);
        tree.push('}');
    }
    tree.push_str("]}}");
    tree
}

/// Writes `actions` as the tree's JSON array, naming each one's callback
/// `name(place)` and keeping its closure (or its lazy submenu's) in
/// `callbacks` by that name. A submenu given with the list names its
/// entries after the action that opens it: `<name>/0`, `<name>/1`, ...
fn write_actions(
    tree: &mut String,
    actions: Vec<Action>,
    name: &dyn Fn(usize) -> String,
    callbacks: &mut BTreeMap<String, Callback>,
) {
    tree.push('[');
    for (index, action) in actions.into_iter().enumerate() {
        if index > 0 {
            tree.push(',');
        }
        let callback = name(index);
        let Action {
            title,
            section,
            destructive,
            shortcut,
            icon,
            does,
        } = action;
        tree.push('{');
        match does {
            Does::Run(run) => {
                tree.push_str("\"onAction\":");
                string(tree, &callback);
                callbacks.insert(callback.clone(), Callback::Run(run));
            }
            Does::Open(submenu) => {
                tree.push_str("\"submenu\":{\"title\":");
                string(tree, &submenu.title);
                match submenu.entries {
                    SubmenuEntries::Given(entries) => {
                        tree.push_str(",\"entries\":");
                        let opener = callback.clone();
                        write_actions(
                            tree,
                            entries,
                            &|place| format!("{opener}/{place}"),
                            callbacks,
                        );
                    }
                    SubmenuEntries::Asked(load) => {
                        tree.push_str(",\"onOpen\":");
                        string(tree, &callback);
                        callbacks.insert(callback.clone(), Callback::Open(load));
                    }
                }
                tree.push('}');
            }
        }
        if let Some(title) = &title {
            tree.push_str(",\"title\":");
            string(tree, title);
        }
        if let Some(section) = &section {
            tree.push_str(",\"section\":");
            string(tree, section);
        }
        if destructive {
            tree.push_str(",\"style\":\"destructive\"");
        }
        if let Some(shortcut) = &shortcut {
            tree.push_str(",\"shortcut\":");
            write_shortcut(tree, shortcut);
        }
        if let Some(icon) = &icon {
            tree.push_str(",\"icon\":");
            crate::icon::write_icon(tree, icon);
        }
        tree.push('}');
    }
    tree.push(']');
}

/// Writes `shortcut` as the tree's JSON: `{"modifiers": [...], "key": ...}`
/// for every system, or such an object per system.
pub(crate) fn write_shortcut(tree: &mut String, shortcut: &Shortcut) {
    if let Some(keys) = &shortcut.every {
        write_keys(tree, keys);
        return;
    }
    tree.push('{');
    let mut first = true;
    for (name, keys) in [
        ("windows", &shortcut.windows),
        ("macos", &shortcut.macos),
        ("linux", &shortcut.linux),
    ] {
        let Some(keys) = keys else {
            continue;
        };
        if !first {
            tree.push(',');
        }
        first = false;
        string(tree, name);
        tree.push(':');
        write_keys(tree, keys);
    }
    tree.push('}');
}

/// Writes one key with its modifiers as the tree's JSON.
fn write_keys(tree: &mut String, keys: &Keys) {
    tree.push_str("{\"modifiers\":[");
    for (index, modifier) in keys.modifiers.iter().enumerate() {
        if index > 0 {
            tree.push(',');
        }
        string(tree, modifier.name());
    }
    tree.push_str("],\"key\":");
    string(tree, &keys.key);
    tree.push('}');
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
