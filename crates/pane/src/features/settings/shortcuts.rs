//! The Settings window's Shortcuts page: the installed commands grouped
//! by their extensions, searchable, each with its Name, Alias and Hotkey.
//!
//! The page is a renderer over [`Launcher::shortcut_catalog`] — one read
//! of the same records Manage extensions shows — rebuilt on every redraw:
//! a package installed, disabled, enabled, updated or removed, in the
//! launcher window or in the background, is in the next catalog the page
//! draws. The window's watcher (see the Settings window's module docs) is
//! what asks for that redraw while the user does nothing here.
//!
//! The Alias column is editable inline: activating a command's alias cell
//! opens a field in its place, filled with the current alias, that commits
//! with Enter and cancels with Escape. Commits apply through
//! [`Launcher::set_alias`], so the alias form's rules and records are the
//! ones in force: a refusal shows next to the field and keeps the editor
//! open, and the change reaches root search at once and is recorded by
//! the same write the form's submission makes. The Hotkey column records
//! through [`Launcher::set_hotkey`] — the same checks and the same
//! `hotkeys.json` the hotkey screen in Manage extensions writes: the
//! cell's click, Enter or Space starts the recorder, which takes the keys
//! pressed next as the binding being recorded (captured: they do not
//! navigate or act; Escape cancels, changing nothing), and a Clear button
//! beside the cell forgets a recorded hotkey. A refusal shows under the
//! cell and keeps the recorder listening; a change that lands is
//! registered with the system before the binding it replaces is released,
//! and a record that cannot be written puts back what was last recorded,
//! with its registration, as the status line says.
//!
//! Keyboard: the filter field, each group's header (Enter or Space
//! expands or collapses it), each editable alias cell, each recordable
//! hotkey cell and its Clear button are tab stops. The editor's Enter and
//! Escape are bound in its own key context, and the recorder's keys in
//! its own, so they reach those controls and nothing else.
//!
//! Groups disclose on the shared motion policy (see `crate::ui::motion`):
//! one tween per group drives both the header chevron's rotation and the
//! commands' arrival, so the two coordinate on one timeline and retarget
//! together when the user reverses mid-flight. Expanding mounts the rows
//! at once — the real layout, hit targets and all — and fades the whole
//! block in; collapsing unmounts them at once, never drawing the
//! departing rows fading out, while the chevron turns back. Focus inside
//! a collapsing group moves to its controlling header, an open editor in
//! it closes without committing, and a hotkey recorder for one of its
//! commands ends, as Escape ends it. Typing in the filter is a
//! content update: rows appear and vanish with no transition at all.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use gpui::{
    AnyElement, App, Context, Div, Entity, FocusHandle, Focusable, Hsla, KeyBinding, KeyDownEvent,
    MouseDownEvent, Pixels, Role, SharedString, Stateful, Subscription, Window, actions, div,
    prelude::*, px,
};
use gpui_elements::editable_text::actions::DEFAULT_INPUT_CONTEXT;
use gpui_elements::editable_text::{EditableTextState, StringStorage, TextChanged, text_input};
use pane_core::hotkeys::Shortcut;
use pane_core::{
    AliasOutcome, HotkeyOutcome, Launcher, ShortcutCatalog, ShortcutCommand, ShortcutGroup,
};

use super::{Page, SettingsWindow, search};
use crate::ui::controls;
use crate::ui::extension_icon::row_icon_at;
use crate::ui::icon::{Glyph, TileSize, glyph_rotated};
use crate::ui::motion;
use crate::ui::theme::Theme;

/// The page's sidebar title, its identity in the sidebar and the tests'
/// selectors.
pub(crate) const TITLE: &str = "Shortcuts";

/// What the page is, in one line: its sidebar entry's description in
/// the search.
const ABOUT: &str = "Aliases and hotkeys for your commands";

/// The target id of the page's filter field, the control the sidebar's
/// search jumps to (see [`entries`]).
const FILTER: &str = "filter";

/// The page's key context: the filter field and everything below it.
const PAGE: &str = "ShortcutPage";
/// A group header's key context, in which Enter and Space toggle it.
const GROUP: &str = "ShortcutGroup";
/// An alias cell's key context, in which Enter and Space open the editor.
const CELL: &str = "ShortcutAliasCell";
/// The inline alias editor's key context, in which Enter commits and
/// Escape cancels.
const EDITOR: &str = "ShortcutAliasEditor";
/// A hotkey cell's key context, in which Enter and Space start the
/// recorder.
const HOTKEY_CELL: &str = "ShortcutHotkeyCell";
/// A hotkey cell's key context while its recorder listens, in which the
/// keys pressed are the binding being recorded and Escape cancels.
const HOTKEY_RECORDER: &str = "ShortcutHotkeyRecorder";
/// A hotkey cell's Clear button's key context, in which Enter and Space
/// clear the hotkey.
const HOTKEY_CLEAR: &str = "ShortcutHotkeyClear";

/// The filter's placeholder.
const PLACEHOLDER: &str = "Filter commands and extensions";

/// How often the window's watcher looks for a changed catalog while the
/// page is showing; see the module docs.
pub(crate) const WATCH: Duration = Duration::from_millis(500);

/// The Alias column's width; the Name column takes the rest.
const ALIAS_WIDTH: Pixels = px(130.);
/// The Hotkey column's width.
const HOTKEY_WIDTH: Pixels = px(200.);

actions!(
    shortcuts,
    [
        EditAlias,
        CommitAlias,
        CancelAlias,
        ToggleGroup,
        RecordHotkey,
        CancelHotkeyRecording,
        ClearHotkey
    ]
);

/// Registers the page's key bindings: the group headers' and alias cells'
/// activation keys in their own contexts, the inline editor's commit and
/// cancel keys in the editor's context above the editable text element,
/// and the hotkey cells' keys in theirs — all deeper in the focus stack
/// than anything broader, so they never fall through to the sidebar's
/// keys.
pub(crate) fn bind_keys(cx: &mut App) {
    let editor = format!("{EDITOR} > {DEFAULT_INPUT_CONTEXT}");
    cx.bind_keys([
        KeyBinding::new("enter", EditAlias, Some(CELL)),
        KeyBinding::new("space", EditAlias, Some(CELL)),
        KeyBinding::new("enter", ToggleGroup, Some(GROUP)),
        KeyBinding::new("space", ToggleGroup, Some(GROUP)),
        KeyBinding::new("enter", CommitAlias, Some(&editor)),
        KeyBinding::new("escape", CancelAlias, Some(&editor)),
        // The hotkey cell is a button: Enter and Space start its recorder.
        // While the recorder listens, the cell's context is the recorder's
        // instead (see [`hotkey_cell`]), where those keys stay with the
        // recorder's activation — a no-op while it listens, since neither
        // can be part of a binding — Escape and Tab cancel, and the
        // sidebar's navigation keys are bound to [`gpui::NoAction`] so they
        // do nothing instead; everything else reaches the cell's own key
        // handler as the combination being recorded.
        KeyBinding::new("enter", RecordHotkey, Some(HOTKEY_CELL)),
        KeyBinding::new("space", RecordHotkey, Some(HOTKEY_CELL)),
        KeyBinding::new("enter", RecordHotkey, Some(HOTKEY_RECORDER)),
        KeyBinding::new("space", RecordHotkey, Some(HOTKEY_RECORDER)),
        KeyBinding::new("escape", CancelHotkeyRecording, Some(HOTKEY_RECORDER)),
        KeyBinding::new("tab", CancelHotkeyRecording, Some(HOTKEY_RECORDER)),
        KeyBinding::new("shift-tab", CancelHotkeyRecording, Some(HOTKEY_RECORDER)),
        // The Clear button beside a recorded hotkey.
        KeyBinding::new("enter", ClearHotkey, Some(HOTKEY_CLEAR)),
        KeyBinding::new("space", ClearHotkey, Some(HOTKEY_CLEAR)),
    ]);
    cx.bind_keys(super::captured_while_recording(HOTKEY_RECORDER));
}

/// The Shortcuts page, registered in the window's page list.
pub(crate) fn page() -> Page {
    Page {
        title: TITLE,
        about: ABOUT,
        // The magnifier: the page is the searchable catalog of the
        // commands' aliases and hotkeys. (The Keyboard page takes the
        // keyboard glyph — the sidebar keeps its entries distinct.)
        icon: Glyph::Search,
        count: None,
        render,
        search: entries,
        focus,
    }
}

/// The control the page offers the sidebar's search: its filter field,
/// which a jump focuses. The commands themselves stay the page's own
/// filter's to find — the Settings search indexes host settings, not
/// the installed commands — and the page's own entry, which the window
/// registers from its title and description, is what a query naming
/// aliases or hotkeys finds.
fn entries(_launcher: &Launcher, _cx: &App) -> Vec<search::Entry> {
    vec![search::Entry {
        control: Some(FILTER.into()),
        title: "Filter commands".into(),
        group: None,
        unavailable: None,
    }]
}

/// The page's filter field takes keyboard focus, so a jump to it focuses
/// the field, ready to filter the commands. No other target is the
/// page's: `false` falls back to the sidebar.
fn focus(
    this: &mut SettingsWindow,
    target: &str,
    window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> bool {
    if target != FILTER {
        return false;
    }
    window.focus(&this.shortcuts.query.focus_handle(cx), cx);
    true
}

/// The Shortcuts page's state, held by the window as a field.
pub(crate) struct State {
    /// The page's filter field; typing in it narrows the groups and their
    /// commands to what matches, by command or extension name as
    /// displayed.
    query: Entity<EditableTextState>,
    _query_changes: Subscription,
    /// The groups the user has collapsed, by the group's stable key (see
    /// [`group_key`]). Groups start expanded.
    collapsed: HashSet<String>,
    /// Each group's disclosure tween, by the group's stable key: the
    /// group's look (0 collapsed, 1 expanded) while a toggle's
    /// transition is in flight, `None` when settled. The chevron's
    /// rotation and the commands' arrival both follow it, so the group
    /// coordinates on one timeline (see `crate::ui::motion`).
    disclosures: HashMap<String, Option<motion::Tween>>,
    /// Each group's disclosure as the last frame drew it, collapsed or
    /// expanded, by key — only a flip here with the filter unchanged is
    /// a real toggle, which transitions; a filter change or a group's
    /// first draw is a content update, which never animates.
    drawn_collapsed: HashMap<String, bool>,
    /// Whether the page is drawing with no filter, as the last frame drew
    /// it: the same comparison's other half.
    drawn_blank: bool,
    /// Whether any group's disclosure was still in flight as the frame
    /// being drawn found it, so the page asks for the one animation
    /// frame that continues them; the frame that settles them all asks
    /// for none, so a settled page is idle.
    disclosing: bool,
    /// The command whose alias is being edited inline, if any.
    editing: Option<Editing>,
    /// The command whose hotkey is being recorded, if any: the keys
    /// pressed next are its new binding, captured — they do not act.
    recording: Option<Recording>,
    /// Each editable command's alias cell focus, created when the command
    /// first draws and kept so the focus survives redraws — and returns
    /// there when the editor it opened closes. The handles of commands a
    /// package change removed are kept too: they cost nothing, and the
    /// window holds them for as long as it is open.
    alias_cells: HashMap<String, FocusHandle>,
    /// Each recordable command's hotkey cell focus, kept as the alias
    /// cells' are: the recorder takes it while it listens, and the focus
    /// returns there when it closes or the change it recorded lands.
    hotkey_cells: HashMap<String, FocusHandle>,
    /// Each recorded hotkey's Clear button focus, kept as the hotkey
    /// cells' are.
    hotkey_clears: HashMap<String, FocusHandle>,
    /// Each group header's focus, kept as the alias cells' are.
    group_cells: HashMap<String, FocusHandle>,
    /// The last catalog this page drew, which the window's watcher
    /// compares a fresh one against.
    drawn: Option<ShortcutCatalog>,
    /// What the last alias or hotkey change came to, as the page's status
    /// line.
    status: Option<StatusLine>,
    /// Each group's disclosure state as the last frame drew it, for
    /// tests (see [`SettingsWindow::group_disclosure`]). Test and debug
    /// builds only.
    #[cfg(any(test, debug_assertions))]
    drawn_looks: HashMap<String, Option<f32>>,
    /// Each group's arriving commands as the last frame drew them — the
    /// block's (offset from rest in px, opacity) — for tests (see
    /// [`SettingsWindow::group_arrival`]). Test and debug builds only.
    #[cfg(any(test, debug_assertions))]
    drawn_arrivals: HashMap<String, Option<(f32, f32)>>,
}

/// The inline alias editor for one command, in the place of its cell.
struct Editing {
    /// The command whose alias is being edited.
    command: String,
    /// The editor's field.
    input: Entity<EditableTextState>,
    /// Why the last commit was refused, shown next to the field; editing
    /// the field clears it, as the alias form's does.
    error: Option<String>,
    _changes: Subscription,
}

/// The hotkey recorder for one command, in its cell: while it listens,
/// the keys pressed next are the binding being recorded.
struct Recording {
    /// The command whose hotkey is being recorded.
    command: String,
    /// Why the last capture was refused, shown under the cell; the
    /// recorder keeps listening for another try, as the General page's
    /// recorder does.
    rejection: Option<String>,
}

/// The page's status line: what the last change the page started came to.
enum StatusLine {
    /// The change took effect and is being recorded.
    Saving(String),
    /// The change was recorded, saying what typing an alias now finds or
    /// what a hotkey now opens.
    Saved(String),
    /// The change could not be recorded; what was last recorded is back,
    /// saying why.
    NotKept(String),
}

impl State {
    /// The page's state over `launcher`: the filter field, every group
    /// expanded, no edit open. The first catalog is the launcher's as it
    /// is now, so the watcher does not ask for a redraw before the page
    /// has drawn anything.
    pub(crate) fn new(launcher: &Launcher, cx: &mut Context<SettingsWindow>) -> State {
        let query = cx.new(|cx| EditableTextState::new(StringStorage::default(), cx));
        query.focus_handle(cx).tab_stop(true);
        let _query_changes = cx.subscribe(&query, |_, _, _: &TextChanged, cx| cx.notify());
        State {
            query,
            _query_changes,
            collapsed: HashSet::new(),
            disclosures: HashMap::new(),
            drawn_collapsed: HashMap::new(),
            drawn_blank: true,
            disclosing: false,
            editing: None,
            recording: None,
            alias_cells: HashMap::new(),
            hotkey_cells: HashMap::new(),
            hotkey_clears: HashMap::new(),
            group_cells: HashMap::new(),
            drawn: Some(launcher.shortcut_catalog()),
            status: None,
            #[cfg(any(test, debug_assertions))]
            drawn_looks: HashMap::new(),
            #[cfg(any(test, debug_assertions))]
            drawn_arrivals: HashMap::new(),
        }
    }
}

impl SettingsWindow {
    /// Test support: the editing state of the open inline alias field, as
    /// [`crate::LauncherWindow::text_field`] does for the launcher's
    /// forms; a platform input method talks to it while composing text.
    /// GPUI CE's test platform cannot reach the window's input handler,
    /// so the window tests read what the field holds through this
    /// instead.
    #[doc(hidden)]
    pub fn alias_field(&self) -> Option<Entity<EditableTextState>> {
        self.shortcuts
            .editing
            .as_ref()
            .map(|editing| editing.input.clone())
    }

    /// Test support: the disclosure state of the group with `key` as the
    /// last frame drew it: its look — 0 collapsed, 1 expanded — while the
    /// disclosure is in flight, which the chevron's angle and the
    /// commands' arrival both follow; `None` when the frame drew it
    /// settled. Test and debug builds only.
    #[cfg(any(test, debug_assertions))]
    #[doc(hidden)]
    pub fn group_disclosure(&self, key: &str) -> Option<f32> {
        self.shortcuts.drawn_looks.get(key).copied().flatten()
    }

    /// Test support: the group with `key`'s arriving commands as the
    /// last frame drew them — the whole block's (offset from rest in px,
    /// opacity) while the disclosure is in flight; `None` when the frame
    /// drew them settled, which is also all reduced motion ever draws.
    /// Test and debug builds only.
    #[cfg(any(test, debug_assertions))]
    #[doc(hidden)]
    pub fn group_arrival(&self, key: &str) -> Option<(f32, f32)> {
        self.shortcuts.drawn_arrivals.get(key).copied().flatten()
    }

    /// Expands or collapses the group with `key`, from its header's keys
    /// or click — `commands` are the ids of the commands the group holds,
    /// as the last frame drew them.
    ///
    /// Collapsing unmounts the group's rows at once — the departing
    /// content is never drawn fading out, so it can expose no hit targets
    /// or active accessibility nodes — which means anything inside them
    /// that holds the focus must give it up first: an open inline editor
    /// for one of the group's commands closes without committing, as
    /// Escape closes it, and the focus — the editor's field or a row's
    /// alias cell — moves to the header that controls the group, which
    /// stays. Expanding needs nothing: the rows mount and arrive on the
    /// disclosure's timeline (see [`group_element`]).
    fn shortcuts_toggle_group(
        &mut self,
        key: &str,
        commands: &[String],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let collapsing = if !self.shortcuts.collapsed.remove(key) {
            self.shortcuts.collapsed.insert(key.to_owned());
            true
        } else {
            false
        };
        if collapsing {
            // The inline editor for a command in the group, if one is
            // open, closes without committing (as Escape closes it), and
            // a hotkey recorder for one of its commands ends (as Escape
            // ends it): their controls unmount with the rows and cannot
            // keep the focus or take further input — the recorder's keys
            // would fall through to the page with no cell listening.
            let editor = self
                .shortcuts
                .editing
                .take_if(|editing| commands.contains(&editing.command));
            let recording = self
                .shortcuts
                .recording
                .take_if(|recording| commands.contains(&recording.command));
            // Focus inside the collapsing group — the open editor's
            // field, a row's alias cell, the hotkey cell listening as its
            // recorder or its Clear button — moves to the header that
            // controls the group. Focus elsewhere stays where it is.
            let inside = editor
                .as_ref()
                .is_some_and(|editing| editing.input.focus_handle(cx).is_focused(window))
                || recording.is_some()
                || commands.iter().any(|id| {
                    self.shortcuts
                        .alias_cells
                        .get(id)
                        .is_some_and(|cell| cell.is_focused(window))
                        || self
                            .shortcuts
                            .hotkey_cells
                            .get(id)
                            .is_some_and(|cell| cell.is_focused(window))
                        || self
                            .shortcuts
                            .hotkey_clears
                            .get(id)
                            .is_some_and(|button| button.is_focused(window))
                });
            if inside && let Some(header) = self.shortcuts.group_cells.get(key) {
                window.focus(header, cx);
            }
        }
        cx.notify();
    }

    /// Opens the inline alias editor for `command`, filled with the alias
    /// it has (`alias`, as the last frame drew the row), taking the focus
    /// the alias cell had. Called by the cell's click and its keys; the
    /// cell only exists for a command an alias can be given to.
    fn shortcuts_edit_alias(
        &mut self,
        command: String,
        alias: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .shortcuts
            .editing
            .as_ref()
            .is_some_and(|editing| editing.command == command)
        {
            return;
        }
        let input = cx.new(|cx| EditableTextState::new(StringStorage::default(), cx));
        input.focus_handle(cx).tab_stop(true);
        if let Some(alias) = alias.as_deref().filter(|alias| !alias.is_empty()) {
            input.update(cx, |input, cx| input.emplace(alias, cx));
        }
        let for_errors = command.clone();
        let _changes = cx.subscribe(&input, move |this, _, _: &TextChanged, cx| {
            // Editing clears the refusal of the last commit, as the alias
            // form's field does.
            if this
                .shortcuts
                .editing
                .as_ref()
                .is_some_and(|editing| editing.command == for_errors && editing.error.is_some())
                && let Some(editing) = this.shortcuts.editing.as_mut()
            {
                editing.error = None;
                cx.notify();
            }
        });
        window.focus(&input.focus_handle(cx), cx);
        self.shortcuts.editing = Some(Editing {
            command,
            input,
            error: None,
            _changes,
        });
        cx.notify();
    }

    /// Commits the inline alias editor: a refused alias shows its reason
    /// beside the field and keeps the editor open; an accepted one takes
    /// effect at once — the page and root search follow it in the next
    /// frame — and is recorded, the status line saying what it came to.
    /// Focus returns to the row's alias cell.
    fn shortcuts_commit_alias(
        &mut self,
        _: &CommitAlias,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(editing) = self.shortcuts.editing.take() else {
            return;
        };
        let value = editing.input.read(cx).as_str().to_owned();
        let command = editing.command.clone();
        match self.launcher.set_alias(&command, &value) {
            Err(refusal) => {
                // Refused: the field keeps what the user typed, shows the
                // reason and stays open for another try.
                let field = editing.input.focus_handle(cx);
                self.shortcuts.editing = Some(Editing {
                    error: Some(refusal),
                    ..editing
                });
                window.focus(&field, cx);
            }
            Ok(pending) => {
                self.shortcuts.status = Some(StatusLine::Saving("Saving the alias…".into()));
                if let Some(cell) = self.shortcuts.alias_cells.get(&command) {
                    window.focus(cell, cx);
                }
                cx.spawn(async move |this, cx| {
                    let outcome = pending.await;
                    this.update(cx, |window, cx| {
                        window.shortcuts_recorded(outcome, cx);
                    })
                    .ok();
                })
                .detach();
            }
        }
        cx.notify();
    }

    /// Records what the alias change the page committed came to, as its
    /// status line; a change that could not be kept also put back what was
    /// last recorded, which the next frame's catalog shows.
    fn shortcuts_recorded(&mut self, outcome: AliasOutcome, cx: &mut Context<Self>) {
        self.shortcuts.status = Some(match outcome {
            AliasOutcome::Saved(done) => StatusLine::Saved(done),
            AliasOutcome::NotKept(problem) => StatusLine::NotKept(problem),
        });
        cx.notify();
    }

    /// Cancels the inline alias editor: nothing changes, and focus returns
    /// to the row's alias cell.
    fn shortcuts_cancel_alias(
        &mut self,
        _: &CancelAlias,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(editing) = self.shortcuts.editing.take() {
            if let Some(cell) = self.shortcuts.alias_cells.get(&editing.command) {
                window.focus(cell, cx);
            }
            cx.notify();
        }
    }

    /// Starts recording the hotkey of `command`: its cell takes focus and
    /// listens, and the keys pressed next are the binding being recorded,
    /// captured — they do not navigate or act. Called by the cell's keys
    /// and click; the cell only exists for a command a hotkey can be
    /// recorded for.
    fn shortcuts_record_hotkey(
        &mut self,
        command: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.stop_propagation();
        if self
            .shortcuts
            .recording
            .as_ref()
            .is_some_and(|recording| recording.command == command)
        {
            // The keys pressed while it listens are the binding, and
            // Enter and Space cannot be part of one: the recorder keeps
            // listening.
            return;
        }
        self.shortcuts.recording = Some(Recording {
            command: command.to_owned(),
            rejection: None,
        });
        if let Some(cell) = self.shortcuts.hotkey_cells.get(command) {
            window.focus(cell, cx);
        }
        cx.notify();
    }

    /// Escape on a hotkey cell while its recorder listens: cancels it,
    /// changing nothing, and focus stays on the cell, a tab stop again.
    fn shortcuts_cancel_hotkey(
        &mut self,
        _: &CancelHotkeyRecording,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.stop_propagation();
        self.shortcuts_cancel_recording(cx);
    }

    /// Cancels the hotkey recorder, if one is listening, changing nothing:
    /// Escape on its cell, or a mouse-down outside it while it listens
    /// (see the cell's `on_mouse_down_out`).
    pub(crate) fn shortcuts_cancel_recording(&mut self, cx: &mut Context<Self>) {
        if self.shortcuts.recording.take().is_some() {
            cx.notify();
        }
    }

    /// A key pressed while a hotkey recorder listens: the combination it
    /// names is the binding to record, as the General page's recorder
    /// takes the Open Pane keys. Keys the window binds (Enter, Space,
    /// Escape, the navigation and traversal keys) never reach here — they
    /// are bound in the recorder's context and handled above.
    fn shortcuts_hotkey_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(recording) = self.shortcuts.recording.as_ref() else {
            return;
        };
        let command = recording.command.clone();
        let keystroke = &event.keystroke;
        let modifiers = keystroke.modifiers;
        cx.stop_propagation();
        match Shortcut::new(
            modifiers.control,
            modifiers.alt,
            modifiers.shift,
            modifiers.platform,
            &keystroke.key,
        ) {
            Ok(shortcut) => self.shortcuts_apply_hotkey(&command, Some(shortcut), window, cx),
            Err(problem) => {
                if let Some(recording) = self.shortcuts.recording.as_mut() {
                    recording.rejection = Some(format!("{problem}."));
                }
                cx.notify();
            }
        }
    }

    /// Applies `shortcut` — or `None`, clearing — as the hotkey of
    /// `command`, as the recorder or the Clear button asked: through the
    /// launcher, which registers the new binding with the system before
    /// the one it replaces is released and only then keeps and records the
    /// choice, so a refusal, a cancellation or a save that fails never
    /// discards the previous working binding. A refusal is shown under
    /// the cell and keeps the recorder listening for another try; a
    /// change that lands is recorded by the returned future, its outcome
    /// the page's status line, and focus returns to the row's hotkey cell.
    fn shortcuts_apply_hotkey(
        &mut self,
        command: &str,
        shortcut: Option<Shortcut>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.launcher.set_hotkey(command, shortcut) {
            Err(reason) => {
                // Refused: the cell keeps listening and shows the reason
                // under itself, for another try.
                if let Some(recording) = self.shortcuts.recording.as_mut() {
                    recording.rejection = Some(reason);
                }
                cx.notify();
            }
            Ok(pending) => {
                self.shortcuts.recording = None;
                self.shortcuts.status = Some(StatusLine::Saving("Saving the hotkey…".into()));
                if let Some(cell) = self.shortcuts.hotkey_cells.get(command) {
                    window.focus(cell, cx);
                }
                cx.spawn(async move |this, cx| {
                    let outcome = pending.await;
                    this.update(cx, |window, cx| {
                        window.shortcuts_hotkey_recorded(outcome, cx);
                    })
                    .ok();
                })
                .detach();
                cx.notify();
            }
        }
    }

    /// Records what the hotkey change the page started came to, as its
    /// status line; a change that could not be kept also put back what was
    /// last recorded — and the registration that follows it — which the
    /// next frame's catalog shows.
    fn shortcuts_hotkey_recorded(&mut self, outcome: HotkeyOutcome, cx: &mut Context<Self>) {
        self.shortcuts.status = Some(match outcome {
            HotkeyOutcome::Saved(done) => StatusLine::Saved(done),
            HotkeyOutcome::NotKept(problem) => StatusLine::NotKept(problem),
        });
        cx.notify();
    }

    /// What the window's watcher does on each of its ticks: nothing unless
    /// this page is showing, in which case a catalog that differs from the
    /// last one drawn asks for a redraw — the next frame reads the catalog
    /// fresh, so it shows the launcher's packages as they are now.
    pub(crate) fn shortcuts_watched(&mut self, cx: &mut Context<Self>) {
        if self.pages[self.selected].title != TITLE {
            return;
        }
        if Some(self.launcher.shortcut_catalog()) != self.shortcuts.drawn {
            cx.notify();
        }
    }
}

/// Draws the Shortcuts page: the filter, the column labels, the groups
/// and their commands, and the status line.
fn render(
    this: &mut SettingsWindow,
    window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    // The theme the host settings resolve to now: the page redraws with
    // them whenever the Appearance page changes the palette.
    let theme = crate::settings::visuals(cx).theme;
    // Read the catalog fresh: any redraw shows the launcher's packages as
    // they are now, whatever made the window redraw.
    let catalog = this.launcher.shortcut_catalog();
    this.shortcuts.drawn = Some(catalog.clone());
    // A recorder whose command the packages no longer offer ends: its
    // cell is drawn as the plain label of a command that cannot record,
    // so the keys would fall through to the page. The catalog changed
    // under the page between the watcher's ticks.
    if let Some(recording) = &this.shortcuts.recording {
        let offered = catalog.groups.iter().any(|group| {
            group
                .commands
                .iter()
                .any(|command| command.id == recording.command && command.hotkey_editable)
        });
        if !offered {
            this.shortcuts.recording = None;
        }
    }
    let query = this.shortcuts.query.read(cx).as_str().to_owned();
    // A filter change is a content update — rows appear and vanish as
    // the filter narrows or clears, and no disclosure transitions for
    // it — so the page notes the blank state this frame draws, for the
    // groups to compare against.
    let blank = query.trim().is_empty();
    let filtering = this.shortcuts.drawn_blank != blank;
    this.shortcuts.drawn_blank = blank;
    // Whether any group's disclosure is in flight starts false each
    // frame; the groups that draw in flight set it, and the page asks
    // for the one frame that continues them below.
    this.shortcuts.disclosing = false;
    let editing = this
        .shortcuts
        .editing
        .as_ref()
        .map(|editing| editing.command.clone());
    let groups: Vec<AnyElement> = catalog
        .groups
        .iter()
        .filter_map(|group| group_element(this, group, &query, &editing, filtering, &theme, cx))
        .collect();
    // While any group's disclosure is still in flight, keep frames
    // coming; the frame that completes them requests none, so a settled
    // page is idle.
    if this.shortcuts.disclosing {
        window.request_animation_frame();
    }
    // The page is the Settings families' (#99): the heading block, the
    // filter's well, then the catalog — its column captions over the
    // groups, each a list header over its commands' settings rows — and
    // the status line, in the page's column.
    let focused = this.shortcuts.query.focus_handle(cx).is_focused(window);
    let filter = filter_field(this, &query, focused, &theme, cx);
    let empty = groups.is_empty().then(|| {
        controls::field_description(
            if query.trim().is_empty() {
                "No extensions installed yet.".to_owned()
            } else {
                format!("No commands match “{}”", query.trim())
            },
            theme.text_muted,
            &theme,
        )
        .id("shortcuts-empty")
        .debug_selector(|| "shortcuts-empty".into())
    });
    // The catalog's card: the column captions over the groups, each a list
    // header over its commands' rows, padded 4 so the headers' washes sit
    // inside the card's corners.
    let catalog_block = div()
        .id("shortcuts-title")
        .debug_selector(|| "shortcuts-title".into())
        .w_full()
        .flex()
        .flex_col()
        .gap(theme.geometry.controls.list_gap)
        .p(px(4.))
        .rounded(theme.geometry.settings.card_radius)
        .bg(theme.card_fill)
        .child(columns_header(
            catalog.hotkeys_unavailable.as_deref(),
            &theme,
        ))
        .children(empty)
        .children(groups);
    let page = controls::page(&theme)
        .gap(theme.geometry.settings.section_label_gap)
        .child(filter)
        .child(catalog_block)
        .children(
            this.shortcuts
                .status
                .as_ref()
                .map(|status| status_line(status, &theme)),
        );
    div()
        .id("shortcuts")
        .debug_selector(|| "shortcuts".into())
        .key_context(PAGE)
        .child(page)
        .into_any_element()
}

/// The page's filter field: a field's well (#99, the sidebar search's
/// family) with the magnifier, holding the shared editable text element,
/// its ring the focus color while it has the keyboard (`focused`). The
/// text's wrapper is the field's accessibility node, as a form field's
/// is, and carries the scroll anchor the search's reveal scrolls to.
fn filter_field(
    this: &mut SettingsWindow,
    query: &str,
    focused: bool,
    theme: &Theme,
    cx: &App,
) -> Stateful<Div> {
    let anchor = this.search_anchor(FILTER);
    let input = &this.shortcuts.query;
    controls::well(false, theme)
        .id("shortcut-filter")
        .debug_selector(|| "shortcut-filter".into())
        .shadow(controls::well_shadows(focused, theme))
        .child(controls::well_glyph(Glyph::Search, theme))
        .child(
            div()
                .id("shortcut-field")
                .flex_1()
                .min_w(px(0.))
                .anchor_scroll(Some(anchor))
                .track_focus(&input.focus_handle(cx))
                .role(Role::TextInput)
                .aria_label("Filter commands and extensions")
                .aria_value(query.to_owned())
                .aria_placeholder(PLACEHOLDER)
                .child(controls::well_input(
                    text_input("filter").state(input.downgrade()),
                    PLACEHOLDER,
                    theme,
                )),
        )
}

/// The column labels over the rows: Name, Alias and Hotkey in the
/// caption type (#99), aligned with the columns the rows lay out below
/// (the commands' own inset, see [`group_element`]). Beside the Hotkey
/// label, why this system has no global hotkeys at all, when it has none.
fn columns_header(hotkeys_unavailable: Option<&str>, theme: &Theme) -> Stateful<Div> {
    div()
        .id("shortcut-columns")
        .debug_selector(|| "shortcut-columns".into())
        .flex()
        .flex_col()
        .gap(theme.geometry.controls.list_gap)
        .pb(theme.geometry.controls.field_gap)
        .child(
            div()
                .flex()
                .flex_row()
                .gap(theme.geometry.controls.row_gap)
                // Over the rows' own text, past their padding.
                .pl(commands_inset(theme) + theme.geometry.settings.card_padding_x)
                .pr(theme.geometry.settings.card_padding_x)
                .pt(px(8.))
                .child(controls::caption("Name", theme).flex_1().min_w(px(0.)))
                .child(
                    controls::caption("Alias", theme)
                        .w(ALIAS_WIDTH)
                        .flex_none()
                        .debug_selector(|| "shortcut-column-alias".into()),
                )
                .child(
                    controls::caption("Hotkey", theme)
                        .w(HOTKEY_WIDTH)
                        .flex_none()
                        .debug_selector(|| "shortcut-column-hotkey".into()),
                ),
        )
        .when_some(hotkeys_unavailable, |header, why| {
            header.child(
                controls::field_description(
                    format!("Hotkeys are unavailable here: {why}"),
                    theme.warning,
                    theme,
                )
                .id("shortcut-hotkeys-unavailable")
                .debug_selector(|| "shortcut-hotkeys-unavailable".into()),
            )
        })
}

/// Where a group's commands begin: under the group's title, past its
/// header's padding, chevron and gap.
fn commands_inset(theme: &Theme) -> Pixels {
    let settings = &theme.geometry.settings;
    settings.item_padding_x + theme.geometry.controls.chevron + settings.item_gap
}

/// One group: the header that expands and collapses it, then its commands
/// as the filter and the expanded state leave them. `None` when a filter
/// is on and nothing in the group matches it.
///
/// `filtering` says the filter changed since the last drawn frame, which
/// makes whatever rows appear or vanish here a content update — those
/// never animate.
fn group_element(
    this: &mut SettingsWindow,
    group: &ShortcutGroup,
    query: &str,
    editing: &Option<String>,
    filtering: bool,
    theme: &Theme,
    cx: &mut Context<SettingsWindow>,
) -> Option<AnyElement> {
    let blank = query.trim().is_empty();
    let key = group_key(group);
    let collapsed = blank && this.shortcuts.collapsed.contains(&key);
    // A filter shows the commands that match it, whatever the expanded
    // state; without one, the expanded state decides. A group the filter
    // matches by its own name shows all its commands.
    let shown: Vec<&ShortcutCommand> = if blank || group_matches(query, group) {
        group.commands.iter().collect()
    } else {
        group
            .commands
            .iter()
            .filter(|command| command_matches(query, group, command))
            .collect()
    };
    if !blank && shown.is_empty() {
        return None;
    }
    // The group's disclosure: its look — 0 collapsed, 1 expanded — while
    // a toggle's transition is in flight. Only a real toggle of this
    // group starts or retargets it (the drawn state flipped with the
    // filter unchanged); a filter change and the group's first draw are
    // content updates, which never animate. The look is the one number
    // the chevron's rotation and the commands' arrival below derive
    // from, so the group coordinates on one timeline and retargets
    // together; reduced motion settles it at once. See
    // `crate::ui::motion` for the whole policy.
    let drawn = this
        .shortcuts
        .drawn_collapsed
        .insert(key.clone(), collapsed);
    let toggled = !filtering && drawn == Some(!collapsed);
    let look = motion::advance_disclosure(
        this.shortcuts.disclosures.entry(key.clone()).or_default(),
        !collapsed,
        toggled,
        cx.reduce_motion(),
        cx.background_executor().now(),
    );
    let in_flight = look.is_some();
    if in_flight {
        this.shortcuts.disclosing = true;
    }
    #[cfg(any(test, debug_assertions))]
    {
        this.shortcuts.drawn_looks.insert(key.clone(), look);
    }
    let handle = this
        .shortcuts
        .group_cells
        .entry(key.clone())
        .or_insert_with(|| cx.focus_handle().tab_stop(true))
        .clone();
    // The group's commands, for the header's toggle to act on — the ids
    // as this frame draws them.
    let commands: Vec<String> = group
        .commands
        .iter()
        .map(|command| command.id.clone())
        .collect();
    let for_keys = (key.clone(), commands.clone());
    let for_click = (key.clone(), commands);
    // The chevron: one glyph, rotated from pointing right (collapsed, at
    // 0) to pointing down (expanded, at a quarter turn) by the look's
    // angle — so its turn runs on the disclosure's own timeline and
    // retargets with it, never snapping ahead or behind the content.
    // Paint only: the element's layout and hit target are the unrotated
    // box's.
    let angle = std::f32::consts::FRAC_PI_2 * look.unwrap_or(if collapsed { 0. } else { 1. });
    // The header: a list header (#99) — the chevron, the group's title
    // over its source and why it is not active — with the sidebar item's
    // hover, at once, and Pane's focus ring.
    let mut lines = Vec::new();
    if let Some(identity) = group.identity.as_ref() {
        lines.push(
            controls::field_description(identity.to_string(), theme.text_muted, theme)
                .truncate()
                .into_any_element(),
        );
    }
    if let Some(why) = group.inactive.as_ref() {
        lines.push(
            controls::field_description(format!("Not active: {why}"), theme.warning, theme)
                .into_any_element(),
        );
    }
    let chevron = glyph_rotated(
        Glyph::ChevronRight,
        theme.geometry.controls.chevron,
        theme.nav_icon,
        gpui::radians(angle),
    );
    // The extension's own icon after the chevron (#139).
    let icon = group.identity.as_ref().map(|_| {
        row_icon_at(
            &crate::features::icons::row_icon_of(&this.launcher, &key, theme),
            TileSize::Mini,
            "group-icon",
            &format!("shortcut-group-{key}"),
            theme,
        )
    });
    let chevron = div()
        .flex_none()
        .flex()
        .items_center()
        .gap(theme.geometry.settings.item_gap)
        .child(chevron)
        .children(icon);
    let ring = controls::focus_ring(theme);
    let header = controls::group_header(key.clone(), chevron, group.title.clone(), lines, theme)
        .debug_selector(|| format!("shortcut-group-{key}"))
        .key_context(GROUP)
        .track_focus(&handle)
        .role(Role::Button)
        .aria_label(group_label(group))
        .aria_expanded(!collapsed)
        .focus(move |header| header.shadow(ring))
        .on_action(cx.listener(move |this, _: &ToggleGroup, window, cx| {
            this.shortcuts_toggle_group(&for_keys.0, &for_keys.1, window, cx);
        }))
        .on_click(cx.listener(move |this, _: &gpui::ClickEvent, window, cx| {
            this.shortcuts_toggle_group(&for_click.0, &for_click.1, window, cx);
        }));
    let rows: Vec<AnyElement> = if blank && collapsed {
        // Collapsed: the rows are unmounted — the departing content is
        // never drawn fading out, so it can expose no hit targets or
        // active accessibility nodes. The chevron's turn above is the
        // collapse's transition.
        Vec::new()
    } else {
        shown
            .into_iter()
            .map(|command| row_element(this, command, editing, theme, cx))
            .collect()
    };
    // The commands the group shows, in one container of their own, which
    // arrives with the disclosure while it is in flight: the whole block
    // of rows fades in over the tiny shift from below — one arrival for
    // the group, never a stagger of rows — mounted and interactive from
    // the first frame, because the state has already flipped. The offset
    // is a relative `top` inset, applied after layout, so the real
    // layout (the page's actual height, the scroll range, the rows'
    // hit targets) is the expanded one from the first frame too, and
    // nothing resizes the window. Collapsing, or settled, draws plain.
    let (offset, opacity) = look
        .filter(|_| !collapsed)
        .map(|look| {
            (
                motion::VIEW_SHIFT * (1. - look),
                motion::VIEW_OPACITY_FLOOR + (1. - motion::VIEW_OPACITY_FLOOR) * look,
            )
        })
        .unwrap_or((0., 1.));
    // The commands' arrival this frame draws, for the tests (see
    // [`SettingsWindow::group_arrival`]): the block's (offset, opacity)
    // while the disclosure is in flight, `None` when settled. Test and
    // debug builds only.
    #[cfg(any(test, debug_assertions))]
    {
        this.shortcuts.drawn_arrivals.insert(
            key.clone(),
            look.filter(|_| !collapsed).map(|_| (offset, opacity)),
        );
    }
    let commands = div()
        .id(format!("commands-{key}"))
        .debug_selector(|| format!("commands-{key}"))
        .flex()
        .flex_col()
        .pl(commands_inset(theme))
        .relative()
        .top(px(offset))
        .when(opacity < 1., |commands| commands.opacity(opacity))
        .children(rows);
    Some(
        div()
            .flex()
            .flex_col()
            .gap(theme.geometry.controls.list_gap)
            .child(header)
            .child(commands)
            .into_any_element(),
    )
}

/// The group's stable key: its package identity's key, or "not installed"
/// for the group of recorded choices whose package is gone.
fn group_key(group: &ShortcutGroup) -> String {
    group
        .identity
        .as_ref()
        .map(|identity| identity.key())
        .unwrap_or_else(|| "not-installed".into())
}

/// The group header's accessible name: its title and its source.
fn group_label(group: &ShortcutGroup) -> String {
    match group.identity.as_ref() {
        Some(identity) => format!("{}, {identity}", group.title),
        None => group.title.clone(),
    }
}

/// One command's row: a settings row (#99) — the command's name over its
/// subtitle — then the Alias column (the cell, or the editor in its place)
/// and the Hotkey column, each with the reason its configuration is not
/// active below it.
fn row_element(
    this: &mut SettingsWindow,
    command: &ShortcutCommand,
    editing: &Option<String>,
    theme: &Theme,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let id = command.id.clone();
    let lines = command
        .subtitle
        .as_ref()
        .map(|subtitle| {
            controls::field_description(subtitle.clone(), theme.text_muted, theme)
                .truncate()
                .into_any_element()
        })
        .into_iter()
        .collect();
    let alias = if editing.as_deref() == Some(id.as_str()) {
        editor_element(this, command, theme, cx)
    } else {
        alias_cell(this, command, theme, cx)
    };
    let hotkey = hotkey_cell(this, command, theme, cx);
    // The command's own icon, or its extension's, before its name (#139).
    let label = div()
        .flex()
        .items_center()
        .gap(theme.geometry.settings.item_gap)
        .child(row_icon_at(
            &crate::features::icons::row_icon_of(&this.launcher, &id, theme),
            TileSize::Mini,
            "command-icon",
            &format!("shortcut-row-{id}"),
            theme,
        ))
        .child(controls::field_label(command.title.clone(), theme));
    // The row's parts align at their tops, padded as a described row is,
    // so a note growing under a cell never moves the others.
    controls::setting_row_with(label, lines, theme)
        .items_start()
        .py(theme.geometry.controls.row_padding_y)
        .id(format!("row-{id}"))
        .debug_selector(|| format!("shortcut-row-{id}"))
        .child(alias)
        .child(hotkey)
        .into_any_element()
}

/// What a cell shows as its text: the alias, quoted, or "None" muted.
fn cell_text(alias: Option<&str>, theme: &Theme) -> Div {
    div()
        .flex_1()
        .min_w(px(0.))
        .truncate()
        .text_color(if alias.is_some() {
            theme.text_title
        } else {
            theme.text_muted
        })
        .child(
            alias
                .map(|alias| format!("“{alias}”"))
                .unwrap_or_else(|| "None".into()),
        )
}

/// A cell's note under its well: why the configuration is not active, a
/// refusal, a hint — a field's description in `color`.
fn cell_note(selector: String, text: String, color: Hsla, theme: &Theme) -> Stateful<Div> {
    controls::field_description(text, color, theme)
        .id(SharedString::from(selector.clone()))
        .debug_selector(move || selector.clone())
}

/// The Alias column's cell: the command's alias (or none) in an inline
/// field's well (#99) that, as a button, opens the inline editor, with why
/// the alias is not active below it. A command an alias cannot be given to
/// (one that is gone) shows its recorded alias in the well, as a label, at
/// the disabled opacity.
fn alias_cell(
    this: &mut SettingsWindow,
    command: &ShortcutCommand,
    theme: &Theme,
    cx: &mut Context<SettingsWindow>,
) -> Div {
    let current = command.alias.clone();
    let id = command.id.clone();
    let handle = this
        .shortcuts
        .alias_cells
        .entry(id.clone())
        .or_insert_with(|| cx.focus_handle().tab_stop(true))
        .clone();
    let for_keys = (id.clone(), current.clone());
    let for_click = (id.clone(), current.clone());
    let label = format!(
        "Alias for {}: {}",
        command.title,
        current.as_deref().unwrap_or("none")
    );
    let well = controls::well(true, theme)
        .w_full()
        .child(cell_text(current.as_deref(), theme))
        .id(format!("alias-{id}"))
        .debug_selector(|| format!("shortcut-alias-{id}"))
        .aria_label(label)
        // The reason the alias is not active, if it is not, announced
        // after the cell's name.
        .when_some(command.alias_inactive.as_ref(), |cell, why| {
            cell.aria_description(format!("Not active: {why}"))
        });
    let cell = if command.editable {
        let ring = controls::well_shadows(true, theme);
        well.key_context(CELL)
            .track_focus(&handle)
            .role(Role::Button)
            .cursor_pointer()
            .focus(move |cell| cell.shadow(ring))
            .on_action(cx.listener(move |this, _: &EditAlias, window, cx| {
                this.shortcuts_edit_alias(for_keys.0.clone(), for_keys.1.clone(), window, cx);
            }))
            .on_click(cx.listener(move |this, _: &gpui::ClickEvent, window, cx| {
                this.shortcuts_edit_alias(for_click.0.clone(), for_click.1.clone(), window, cx);
            }))
    } else {
        well.role(Role::Label)
            .opacity(theme.geometry.controls.disabled_opacity)
    };
    div()
        .w(ALIAS_WIDTH)
        .flex_none()
        .flex()
        .flex_col()
        .gap(theme.geometry.controls.list_gap)
        .child(cell)
        .when_some(command.alias_inactive.as_ref(), |value, why| {
            value.child(cell_note(
                format!("shortcut-alias-inactive-{}", command.id),
                format!("Not active: {why}"),
                theme.warning,
                theme,
            ))
        })
}

/// The Alias column's inline editor, in the place of the cell: the shared
/// editable text element in the same inline well, filled with the current
/// alias, its ring the focus color while it has the keyboard; under it the
/// hint the alias form gives and why the last commit was refused. Enter
/// commits and Escape cancels (see the module docs).
fn editor_element(
    this: &mut SettingsWindow,
    command: &ShortcutCommand,
    theme: &Theme,
    cx: &mut Context<SettingsWindow>,
) -> Div {
    let editing = this
        .shortcuts
        .editing
        .as_ref()
        .expect("the editor draws only for the command being edited");
    let input = editing.input.clone();
    let error = editing.error.clone();
    let hint = "One word. Leave empty for none.".to_owned();
    let ring = controls::well_shadows(true, theme);
    div()
        .w(ALIAS_WIDTH)
        .flex_none()
        .flex()
        .flex_col()
        .gap(theme.geometry.controls.list_gap)
        .key_context(EDITOR)
        .on_action(cx.listener(SettingsWindow::shortcuts_commit_alias))
        .on_action(cx.listener(SettingsWindow::shortcuts_cancel_alias))
        .child(
            controls::well(true, theme)
                .w_full()
                .id("shortcut-editor")
                .debug_selector(|| "shortcut-editor".into())
                .track_focus(&input.focus_handle(cx))
                .focus(move |field| field.shadow(ring))
                .role(Role::TextInput)
                .aria_label(hint.clone())
                .when_some(error.as_ref(), |field, error| {
                    field.aria_description(error.clone())
                })
                .child(controls::well_input(
                    text_input("alias").state(input.downgrade()),
                    "such as ec",
                    theme,
                )),
        )
        .child(controls::field_description(hint, theme.text_muted, theme))
        .when_some(error.as_ref(), |editor, error| {
            editor.child(cell_note(
                "shortcut-alias-error".into(),
                error.clone(),
                theme.danger,
                theme,
            ))
        })
        .when_some(command.alias_inactive.as_ref(), |editor, why| {
            editor.child(cell_note(
                format!("shortcut-alias-inactive-{}", command.id),
                format!("Not active: {why}"),
                theme.warning,
                theme,
            ))
        })
}

/// The Hotkey column's cell: the shared recorder ([`controls::recorder`])
/// showing the command's hotkey written out — or "None" — with its clear
/// button, enabled while a hotkey is recorded. Clicked or pressed with
/// Enter it listens, ringed red: the keys pressed next are the binding
/// being recorded, captured; Escape, Tab, a click outside or another click
/// on it cancels, and why the last capture was refused shows below. A
/// command a hotkey cannot be recorded for (its package is disabled, or it
/// is unavailable on this system) shows its recorded hotkey at the
/// disabled opacity, taking no input. Whatever the cell shows, why the
/// hotkey is not active is below it.
fn hotkey_cell(
    this: &mut SettingsWindow,
    command: &ShortcutCommand,
    theme: &Theme,
    cx: &mut Context<SettingsWindow>,
) -> Div {
    let id = command.id.clone();
    let shown = command.hotkey.as_ref().map(|shortcut| shortcut.to_string());
    let listening = this
        .shortcuts
        .recording
        .as_ref()
        .is_some_and(|recording| recording.command == id);
    let rejection = this
        .shortcuts
        .recording
        .as_ref()
        .filter(|recording| recording.command == id)
        .and_then(|recording| recording.rejection.clone());
    let label = format!(
        "{}Hotkey for {}: {}",
        if listening { "Recording; " } else { "" },
        command.title,
        shown.as_deref().unwrap_or("none")
    );
    let inactive = command
        .hotkey_inactive
        .as_ref()
        .map(|why| format!("Not active: {why}"));
    // What assistive technology is told after the cell's name: while the
    // recorder listens, why the last capture was refused — as the alias
    // editor's field announces its error — with the reason the hotkey is
    // not active as the fallback while it listens and alone otherwise.
    let description = if listening {
        rejection.clone().or_else(|| inactive.clone())
    } else {
        inactive.clone()
    };

    // The Clear button, inside the recorder of a command that can record
    // and has a hotkey: it forgets the hotkey through the same checks and
    // record the recorder writes.
    let clear = (command.hotkey_editable && command.hotkey.is_some()).then(|| {
        let handle = this
            .shortcuts
            .hotkey_clears
            .entry(id.clone())
            .or_insert_with(|| cx.focus_handle().tab_stop(true))
            .clone();
        let for_keys = id.clone();
        let for_click = id.clone();
        let ring = controls::focus_ring(theme);
        controls::icon_button(format!("hotkey-clear-{id}"), Glyph::Reset, true, theme)
            .debug_selector(|| format!("shortcut-hotkey-clear-{id}"))
            .key_context(HOTKEY_CLEAR)
            .track_focus(&handle)
            .role(Role::Button)
            .aria_label(format!("Clear the hotkey for {}", command.title))
            .focus(move |button| button.shadow(ring))
            .on_action(cx.listener(move |this, _: &ClearHotkey, window, cx| {
                this.shortcuts_apply_hotkey(&for_keys, None, window, cx);
            }))
            .on_click(cx.listener(move |this, _: &gpui::ClickEvent, window, cx| {
                // The button sits inside the recorder: its click is its own.
                cx.stop_propagation();
                this.shortcuts_apply_hotkey(&for_click, None, window, cx);
            }))
            .into_any_element()
    });
    let text = command
        .hotkey
        .as_ref()
        .map(|shortcut| controls::binding_text(&crate::keyboard::hotkey_keys(shortcut)))
        .unwrap_or_else(|| "None".into());
    let recorder = controls::recorder(text, listening, clear, theme)
        .w_full()
        .id(format!("hotkey-{id}"))
        .debug_selector(|| format!("shortcut-hotkey-{id}"))
        .aria_label(label)
        .when_some(description.clone(), |cell, why| cell.aria_description(why));
    let cell = if command.hotkey_editable {
        let handle = this
            .shortcuts
            .hotkey_cells
            .entry(id.clone())
            .or_insert_with(|| cx.focus_handle().tab_stop(true))
            .clone();
        let for_keys = id.clone();
        let for_click = id.clone();
        let for_out = id.clone();
        // While the recorder listens, the cell's context is the recorder's
        // (see [`bind_keys`]): its keys are the binding being recorded.
        recorder
            .key_context(if listening {
                HOTKEY_RECORDER
            } else {
                HOTKEY_CELL
            })
            .track_focus(&handle)
            .role(Role::Button)
            .on_action(cx.listener(move |this, _: &RecordHotkey, window, cx| {
                this.shortcuts_record_hotkey(&for_keys, window, cx);
            }))
            .on_action(cx.listener(SettingsWindow::shortcuts_cancel_hotkey))
            .on_key_down(cx.listener(SettingsWindow::shortcuts_hotkey_key_down))
            // A mouse-down anywhere outside the cell while it listens cancels
            // the recording and is consumed, as the General page's recorder
            // and the footer menu's popup do: the click underneath does not
            // act, and the recorder gives up the keys.
            .on_mouse_down_out(cx.listener(move |this, _: &MouseDownEvent, _window, cx| {
                if this
                    .shortcuts
                    .recording
                    .as_ref()
                    .is_some_and(|recording| recording.command == for_out)
                {
                    this.shortcuts_cancel_recording(cx);
                    cx.stop_propagation();
                }
            }))
            // A click starts recording, or stops it again.
            .on_click(cx.listener(move |this, _: &gpui::ClickEvent, window, cx| {
                let listening = this
                    .shortcuts
                    .recording
                    .as_ref()
                    .is_some_and(|recording| recording.command == for_click);
                if listening {
                    this.shortcuts_cancel_recording(cx);
                } else {
                    this.shortcuts_record_hotkey(&for_click, window, cx);
                }
            }))
    } else {
        recorder
            .role(Role::Label)
            .cursor_default()
            .opacity(theme.geometry.controls.disabled_opacity)
    };

    div()
        .w(HOTKEY_WIDTH)
        .flex_none()
        .flex()
        .flex_col()
        .gap(theme.geometry.controls.list_gap)
        .child(cell)
        .when_some(rejection, |column, why| {
            column.child(cell_note(
                "shortcut-hotkey-error".into(),
                why,
                theme.danger,
                theme,
            ))
        })
        .when_some(inactive, |cell, why| {
            cell.child(cell_note(
                format!("shortcut-hotkey-inactive-{}", command.id),
                why,
                theme.warning,
                theme,
            ))
        })
}

/// The page's status line: what the last change the page started came
/// to, as a field's description in its tone (#99), a live region so
/// assistive technology announces it.
fn status_line(status: &StatusLine, theme: &Theme) -> Stateful<Div> {
    let (text, color): (String, Hsla) = match status {
        StatusLine::Saving(saving) => (saving.clone(), theme.warning),
        StatusLine::Saved(done) => (done.clone(), theme.success),
        StatusLine::NotKept(problem) => (
            format!("Could not keep the change: {problem}"),
            theme.danger,
        ),
    };
    controls::field_description(text.clone(), color, theme)
        .id("shortcut-status")
        .debug_selector(|| "shortcut-status".into())
        .role(Role::Status)
        .aria_label(text)
}

/// Whether the filter matches the group itself: its title or the source
/// of its package, as the page displays them.
fn group_matches(query: &str, group: &ShortcutGroup) -> bool {
    let needle = query.trim().to_lowercase();
    if needle.is_empty() {
        return true;
    }
    group.title.to_lowercase().contains(&needle)
        || group
            .identity
            .as_ref()
            .is_some_and(|identity| identity.to_string().to_lowercase().contains(&needle))
}

/// Whether the filter matches the command: its own name, or its group's
/// as [`group_matches`] accepts it.
fn command_matches(query: &str, group: &ShortcutGroup, command: &ShortcutCommand) -> bool {
    let needle = query.trim().to_lowercase();
    if needle.is_empty() {
        return true;
    }
    let own = format!(
        "{} {}",
        command.title,
        command.subtitle.as_deref().unwrap_or_default()
    );
    own.to_lowercase().contains(&needle) || group_matches(query, group)
}
