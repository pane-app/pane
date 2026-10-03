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
//! the same write the form's submission makes. The Hotkey column is
//! display-only — recording is #76's slice — and shows each command's
//! hotkey as recorded, with why it is not active when it is not.
//!
//! Keyboard: the filter field, each group's header (Enter or Space
//! expands or collapses it) and each editable alias cell are tab stops.
//! The editor's Enter and Escape are bound in its own key context, so
//! they reach the editor and nothing else.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use gpui::{
    AnyElement, App, BoxShadow, Context, Div, Entity, FocusHandle, Focusable, Hsla, KeyBinding,
    Pixels, Role, Stateful, StyleRefinement, Subscription, Window, actions, div, prelude::*, px,
};
use gpui_elements::editable_text::actions::DEFAULT_INPUT_CONTEXT;
use gpui_elements::editable_text::{EditableTextState, StringStorage, TextChanged, text_input};
use pane_core::{AliasOutcome, Launcher, ShortcutCatalog, ShortcutCommand, ShortcutGroup};

use super::{Page, SettingsWindow, search};
use crate::ui::icon::{Glyph, IconTone, glyph};
use crate::ui::theme::Theme;

/// The page's sidebar title, its identity in the sidebar and the tests'
/// selectors.
pub(crate) const TITLE: &str = "Shortcuts";

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

/// The filter's placeholder.
const PLACEHOLDER: &str = "Filter commands and extensions";

/// How often the window's watcher looks for a changed catalog while the
/// page is showing; see the module docs.
pub(crate) const WATCH: Duration = Duration::from_millis(500);

/// The Alias column's width; the Name column takes the rest.
const ALIAS_WIDTH: Pixels = px(240.);
/// The Hotkey column's width.
const HOTKEY_WIDTH: Pixels = px(190.);

actions!(
    shortcuts,
    [EditAlias, CommitAlias, CancelAlias, ToggleGroup]
);

/// Registers the page's key bindings: the group headers' and alias cells'
/// activation keys in their own contexts, and the inline editor's commit
/// and cancel keys in the editor's context above the editable text
/// element, so they are deeper in the focus stack than anything broader
/// and never fall through to the sidebar's keys.
pub(crate) fn bind_keys(cx: &mut App) {
    let editor = format!("{EDITOR} > {DEFAULT_INPUT_CONTEXT}");
    cx.bind_keys([
        KeyBinding::new("enter", EditAlias, Some(CELL)),
        KeyBinding::new("space", EditAlias, Some(CELL)),
        KeyBinding::new("enter", ToggleGroup, Some(GROUP)),
        KeyBinding::new("space", ToggleGroup, Some(GROUP)),
        KeyBinding::new("enter", CommitAlias, Some(&editor)),
        KeyBinding::new("escape", CancelAlias, Some(&editor)),
    ]);
}

/// The Shortcuts page, registered in the window's page list.
pub(crate) fn page() -> Page {
    Page {
        title: TITLE,
        about: "Aliases and global hotkeys for installed commands",
        icon: (IconTone::Command, Glyph::Keyboard),
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
    /// The command whose alias is being edited inline, if any.
    editing: Option<Editing>,
    /// Each editable command's alias cell focus, created when the command
    /// first draws and kept so the focus survives redraws — and returns
    /// there when the editor it opened closes. The handles of commands a
    /// package change removed are kept too: they cost nothing, and the
    /// window holds them for as long as it is open.
    alias_cells: HashMap<String, FocusHandle>,
    /// Each group header's focus, kept as the alias cells' are.
    group_cells: HashMap<String, FocusHandle>,
    /// The last catalog this page drew, which the window's watcher
    /// compares a fresh one against.
    drawn: Option<ShortcutCatalog>,
    /// What the last alias change came to, as the page's status line.
    status: Option<StatusLine>,
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

/// The page's status line: what the last alias change came to.
enum StatusLine {
    /// The alias took effect and is being recorded.
    Saving,
    /// The alias was recorded, saying what typing it now finds.
    Saved(String),
    /// The alias could not be recorded; what was last recorded is back,
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
            editing: None,
            alias_cells: HashMap::new(),
            group_cells: HashMap::new(),
            drawn: Some(launcher.shortcut_catalog()),
            status: None,
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

    /// Expands or collapses the group with `key`, from its header's keys
    /// or click.
    fn shortcuts_toggle_group(&mut self, key: &str, cx: &mut Context<Self>) {
        if !self.shortcuts.collapsed.remove(key) {
            self.shortcuts.collapsed.insert(key.to_owned());
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
                self.shortcuts.status = Some(StatusLine::Saving);
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
    _window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    // The theme the host settings resolve to now: the page redraws with
    // them whenever the Appearance page changes the palette.
    let theme = crate::settings::visuals(cx).theme;
    // Read the catalog fresh: any redraw shows the launcher's packages as
    // they are now, whatever made the window redraw.
    let catalog = this.launcher.shortcut_catalog();
    this.shortcuts.drawn = Some(catalog.clone());
    let query = this.shortcuts.query.read(cx).as_str().to_owned();
    let editing = this
        .shortcuts
        .editing
        .as_ref()
        .map(|editing| editing.command.clone());
    let groups: Vec<AnyElement> = catalog
        .groups
        .iter()
        .filter_map(|group| group_element(this, group, &query, &editing, &theme, cx))
        .collect();
    div()
        .id("shortcuts")
        .debug_selector(|| "shortcuts".into())
        .key_context(PAGE)
        .flex()
        .flex_col()
        .gap(px(4.))
        .child(
            div()
                .id("shortcuts-title")
                .debug_selector(|| "shortcuts-title".into())
                .pb(px(8.))
                .text_size(theme.typography.search_size)
                .font_weight(theme.typography.medium)
                .text_color(theme.text_title)
                .child(TITLE),
        )
        .child(filter_field(this, &query, &theme, cx))
        .child(columns_header(
            catalog.hotkeys_unavailable.as_deref(),
            &theme,
        ))
        .when(groups.is_empty(), |page| {
            page.child(
                div()
                    .id("shortcuts-empty")
                    .debug_selector(|| "shortcuts-empty".into())
                    .text_size(theme.typography.row_subtitle_size)
                    .text_color(theme.text_muted)
                    .child(if query.trim().is_empty() {
                        "No extensions are installed.".to_owned()
                    } else {
                        format!("No commands match “{}”", query.trim())
                    }),
            )
        })
        .children(groups)
        .when_some(this.shortcuts.status.as_ref(), |page, status| {
            page.child(status_line(status, &theme))
        })
        .into_any_element()
}

/// The page's filter field: the shared editable text element in a boxed
/// field, with the magnifier the reference's search inputs carry. The
/// wrapper is the field's accessibility node, as a form field's is, and
/// carries the scroll anchor the search's reveal scrolls to.
fn filter_field(this: &mut SettingsWindow, query: &str, theme: &Theme, cx: &App) -> Stateful<Div> {
    let anchor = this.search_anchor(FILTER);
    let input = &this.shortcuts.query;
    div()
        .id("shortcut-filter")
        .debug_selector(|| "shortcut-filter".into())
        .flex()
        .items_center()
        .gap(px(8.))
        .mb(px(8.))
        .child(glyph(Glyph::Search, px(16.), theme.text_muted))
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .id("shortcut-field")
                .anchor_scroll(Some(anchor))
                .track_focus(&input.focus_handle(cx))
                .role(Role::TextInput)
                .aria_label("Filter commands and extensions")
                .aria_value(query)
                .aria_placeholder(PLACEHOLDER)
                .px(px(8.))
                .py(px(5.))
                .rounded_md()
                .border_1()
                .border_color(theme.hairline)
                .bg(theme.tile_background)
                .focus(|field| field.border_color(theme.focus_ring))
                .child(
                    text_input("filter")
                        .state(input.downgrade())
                        .placeholder(PLACEHOLDER)
                        .placeholder_color(theme.text_placeholder)
                        .caret_color(theme.accent_text)
                        .selection_color(theme.row_selected)
                        .marked_color(theme.accent_text)
                        .text_color(theme.text_body)
                        .w_full()
                        .min_w(px(0.))
                        .whitespace_nowrap()
                        .overflow_x_scroll(),
                ),
        )
}

/// The column labels over the rows: Name, Alias and Hotkey, aligned with
/// the columns the rows lay out below. Beside the Hotkey label, why this
/// system has no global hotkeys at all, when it has none.
fn columns_header(hotkeys_unavailable: Option<&str>, theme: &Theme) -> Stateful<Div> {
    div()
        .id("shortcut-columns")
        .debug_selector(|| "shortcut-columns".into())
        .flex()
        .flex_col()
        .gap(px(2.))
        .pb(px(4.))
        .child(
            div()
                .flex()
                .flex_row()
                .text_size(theme.typography.row_kind_size)
                .font_weight(theme.typography.medium)
                .text_color(theme.text_muted)
                .child(div().flex_1().min_w(px(0.)).child("Name"))
                .child(
                    div()
                        .w(ALIAS_WIDTH)
                        .flex_none()
                        .debug_selector(|| "shortcut-column-alias".into())
                        .child("Alias"),
                )
                .child(
                    div()
                        .w(HOTKEY_WIDTH)
                        .flex_none()
                        .debug_selector(|| "shortcut-column-hotkey".into())
                        .child("Hotkey"),
                ),
        )
        .when_some(hotkeys_unavailable, |header, why| {
            header.child(
                div()
                    .id("shortcut-hotkeys-unavailable")
                    .debug_selector(|| "shortcut-hotkeys-unavailable".into())
                    .text_size(theme.typography.row_kind_size)
                    .text_color(theme.warning)
                    .child(format!("Hotkeys are unavailable here: {why}")),
            )
        })
}

/// One group: the header that expands and collapses it, then its commands
/// as the filter and the expanded state leave them. `None` when a filter
/// is on and nothing in the group matches it.
fn group_element(
    this: &mut SettingsWindow,
    group: &ShortcutGroup,
    query: &str,
    editing: &Option<String>,
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
    let handle = this
        .shortcuts
        .group_cells
        .entry(key.clone())
        .or_insert_with(|| cx.focus_handle().tab_stop(true))
        .clone();
    let for_keys = key.clone();
    let for_click = key.clone();
    let header = div()
        .id(key.clone())
        .debug_selector(|| format!("shortcut-group-{key}"))
        .key_context(GROUP)
        .track_focus(&handle)
        .role(Role::Button)
        .aria_label(group_label(group))
        .aria_expanded(!collapsed)
        .on_action(cx.listener(move |this, _: &ToggleGroup, _, cx| {
            this.shortcuts_toggle_group(&for_keys, cx);
        }))
        .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
            this.shortcuts_toggle_group(&for_click, cx);
        }))
        .flex()
        .items_center()
        .gap(px(6.))
        .min_h(theme.geometry.row_min_height)
        .px(theme.geometry.row_padding_x)
        .rounded(theme.geometry.row_radius)
        .cursor_pointer()
        .hover(|header| header.bg(theme.row_hover))
        .focus(|header| focus_ring(header, theme.focus_ring))
        .child(glyph(
            if collapsed {
                Glyph::ChevronRight
            } else {
                Glyph::ChevronDown
            },
            px(14.),
            theme.text_muted,
        ))
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .flex_col()
                .child(
                    div()
                        .truncate()
                        .text_size(theme.typography.row_title_size)
                        .font_weight(theme.typography.medium)
                        .text_color(theme.text_title)
                        .child(group.title.clone()),
                )
                .when_some(
                    group.identity.as_ref().map(|identity| identity.to_string()),
                    |header, identity| {
                        header.child(
                            div()
                                .truncate()
                                .text_size(theme.typography.row_kind_size)
                                .text_color(theme.text_muted)
                                .child(identity),
                        )
                    },
                )
                .when_some(group.inactive.as_ref(), |header, why| {
                    header.child(
                        div()
                            .text_size(theme.typography.row_kind_size)
                            .text_color(theme.warning)
                            .child(format!("Not active: {why}")),
                    )
                }),
        );
    let rows: Vec<AnyElement> = if blank && collapsed {
        Vec::new()
    } else {
        shown
            .into_iter()
            .map(|command| row_element(this, command, editing, theme, cx))
            .collect()
    };
    // The commands the group shows, in one container of their own: the
    // seam #87's group transitions animate later — nothing here animates.
    let commands = div()
        .id(format!("commands-{key}"))
        .flex()
        .flex_col()
        .gap(px(2.))
        .children(rows);
    Some(
        div()
            .flex()
            .flex_col()
            .gap(px(2.))
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

/// One command's row: the Name column, the Alias column (the cell, or the
/// editor in its place) and the Hotkey column, each with the reason its
/// configuration is not active below it.
fn row_element(
    this: &mut SettingsWindow,
    command: &ShortcutCommand,
    editing: &Option<String>,
    theme: &Theme,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let id = command.id.clone();
    let name = div()
        .flex_1()
        .min_w(px(0.))
        .flex()
        .flex_col()
        .justify_center()
        .py(px(6.))
        .child(
            div()
                .truncate()
                .text_size(theme.typography.row_title_size)
                .text_color(theme.text_title)
                .child(command.title.clone()),
        )
        .when_some(command.subtitle.as_ref(), |name, subtitle| {
            name.child(
                div()
                    .truncate()
                    .text_size(theme.typography.row_kind_size)
                    .text_color(theme.text_muted)
                    .child(subtitle.clone()),
            )
        });
    let alias = if editing.as_deref() == Some(id.as_str()) {
        editor_element(this, command, theme, cx)
    } else {
        alias_cell(this, command, theme, cx)
    };
    let hotkey = hotkey_cell(command, theme);
    div()
        .id(format!("row-{id}"))
        .debug_selector(|| format!("shortcut-row-{id}"))
        .flex()
        .flex_row()
        .items_start()
        .gap(px(8.))
        .min_h(theme.geometry.row_min_height)
        .px(theme.geometry.row_padding_x)
        .rounded(theme.geometry.row_radius)
        .child(name)
        .child(alias)
        .child(hotkey)
        .into_any_element()
}

/// The Alias column's cell: the command's alias (or none) as a button
/// that opens the inline editor, with why the alias is not active below
/// it. A command an alias cannot be given to (one that is gone) shows its
/// recorded alias as plain text instead.
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
    let cell = if command.editable {
        div()
            .id(format!("alias-{id}"))
            .debug_selector(|| format!("shortcut-alias-{id}"))
            .key_context(CELL)
            .track_focus(&handle)
            .role(Role::Button)
            .aria_label(label.clone())
            // The reason the alias is not active, if it is not, announced
            // after the cell's name.
            .when_some(command.alias_inactive.as_ref(), |cell, why| {
                cell.aria_description(format!("Not active: {why}"))
            })
            .on_action(cx.listener(move |this, _: &EditAlias, window, cx| {
                this.shortcuts_edit_alias(for_keys.0.clone(), for_keys.1.clone(), window, cx);
            }))
            .on_click(cx.listener(move |this, _: &gpui::ClickEvent, window, cx| {
                this.shortcuts_edit_alias(for_click.0.clone(), for_click.1.clone(), window, cx);
            }))
            .flex()
            .items_center()
            .min_h(px(30.))
            .px(px(8.))
            .rounded_md()
            .border_1()
            .border_color(theme.hairline)
            .bg(theme.tile_background)
            .cursor_pointer()
            .hover(|cell| cell.bg(theme.row_hover))
            .focus(|cell| cell.border_color(theme.focus_ring))
            .text_size(theme.typography.row_subtitle_size)
            .text_color(if current.is_some() {
                theme.text_title
            } else {
                theme.text_muted
            })
            .child(
                current
                    .as_deref()
                    .map(|alias| format!("“{alias}”"))
                    .unwrap_or_else(|| "None".into()),
            )
            .into_any_element()
    } else {
        div()
            .id(format!("alias-{id}"))
            .debug_selector(|| format!("shortcut-alias-{id}"))
            .role(Role::Label)
            .aria_label(label)
            .when_some(command.alias_inactive.as_ref(), |cell, why| {
                cell.aria_description(format!("Not active: {why}"))
            })
            .flex()
            .items_center()
            .min_h(px(30.))
            .px(px(8.))
            .rounded_md()
            .border_1()
            .border_color(theme.hairline)
            .opacity(0.6)
            .text_size(theme.typography.row_subtitle_size)
            .text_color(theme.text_muted)
            .child(
                current
                    .as_deref()
                    .map(|alias| format!("“{alias}”"))
                    .unwrap_or_else(|| "None".into()),
            )
            .into_any_element()
    };
    div()
        .w(ALIAS_WIDTH)
        .flex_none()
        .flex()
        .flex_col()
        .gap(px(2.))
        .py(px(6.))
        .child(cell)
        .when_some(command.alias_inactive.as_ref(), |value, why| {
            value.child(
                div()
                    .id(format!("alias-inactive-{}", command.id))
                    .debug_selector(|| format!("shortcut-alias-inactive-{}", command.id))
                    .text_size(theme.typography.row_kind_size)
                    .text_color(theme.warning)
                    .child(format!("Not active: {why}")),
            )
        })
}

/// The Alias column's inline editor, in the place of the cell: the shared
/// editable text element filled with the current alias, the hint the alias
/// form gives, and why the last commit was refused. Enter commits and
/// Escape cancels (see the module docs).
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
    let hint = format!(
        "Alias: one word that finds {} in root search; empty for none",
        command.title
    );
    div()
        .w(ALIAS_WIDTH)
        .flex_none()
        .flex()
        .flex_col()
        .gap(px(2.))
        .py(px(6.))
        .key_context(EDITOR)
        .on_action(cx.listener(SettingsWindow::shortcuts_commit_alias))
        .on_action(cx.listener(SettingsWindow::shortcuts_cancel_alias))
        .child(
            div()
                .id("shortcut-editor")
                .debug_selector(|| "shortcut-editor".into())
                .track_focus(&input.focus_handle(cx))
                .role(Role::TextInput)
                .aria_label(hint.clone())
                .when_some(error.as_ref(), |field, error| {
                    field.aria_description(error.clone())
                })
                .px(px(8.))
                .py(px(5.))
                .rounded_md()
                .border_1()
                .border_color(theme.hairline)
                .bg(theme.tile_background)
                .focus(|field| field.border_color(theme.focus_ring))
                .child(
                    text_input("alias")
                        .state(input.downgrade())
                        .placeholder("such as ec")
                        .placeholder_color(theme.text_placeholder)
                        .caret_color(theme.accent_text)
                        .selection_color(theme.row_selected)
                        .marked_color(theme.accent_text)
                        .text_color(theme.text_title)
                        .w_full()
                        .min_w(px(0.))
                        .whitespace_nowrap()
                        .overflow_x_scroll(),
                ),
        )
        .child(
            div()
                .text_size(theme.typography.row_kind_size)
                .text_color(theme.text_muted)
                .child(hint),
        )
        .when_some(error.as_ref(), |editor, error| {
            editor.child(
                div()
                    .id("shortcut-alias-error")
                    .debug_selector(|| "shortcut-alias-error".into())
                    .text_size(theme.typography.row_kind_size)
                    .text_color(theme.danger)
                    .child(error.clone()),
            )
        })
        .when_some(command.alias_inactive.as_ref(), |editor, why| {
            editor.child(
                div()
                    .id(format!("alias-inactive-{}", command.id))
                    .debug_selector(|| format!("shortcut-alias-inactive-{}", command.id))
                    .text_size(theme.typography.row_kind_size)
                    .text_color(theme.warning)
                    .child(format!("Not active: {why}")),
            )
        })
}

/// The Hotkey column's cell: the command's hotkey as text in the keycap
/// chrome (display-only until #76 lands recording), or none, with why the
/// hotkey is not active below it.
fn hotkey_cell(command: &ShortcutCommand, theme: &Theme) -> Div {
    let shown = command.hotkey.as_ref().map(|shortcut| shortcut.to_string());
    let label = format!(
        "Hotkey for {}: {}",
        command.title,
        shown.as_deref().unwrap_or("none")
    );
    let inactive = command
        .hotkey_inactive
        .as_ref()
        .map(|why| format!("Not active: {why}"));
    div()
        .w(HOTKEY_WIDTH)
        .flex_none()
        .flex()
        .flex_col()
        .gap(px(2.))
        .py(px(6.))
        .child(
            div()
                .id(format!("hotkey-{}", command.id))
                .debug_selector(|| format!("shortcut-hotkey-{}", command.id))
                .role(Role::Label)
                .aria_label(label)
                .when_some(inactive, |cell, why| cell.aria_description(why))
                .flex()
                .items_center()
                .min_h(px(30.))
                .child(match shown {
                    Some(shortcut) => hotkey_chip(&shortcut, theme).into_any_element(),
                    None => div()
                        .text_size(theme.typography.row_subtitle_size)
                        .text_color(theme.text_muted)
                        .child("None")
                        .into_any_element(),
                }),
        )
        .when_some(command.hotkey_inactive.as_ref(), |cell, why| {
            cell.child(
                div()
                    .id(format!("hotkey-inactive-{}", command.id))
                    .debug_selector(|| format!("shortcut-hotkey-inactive-{}", command.id))
                    .text_size(theme.typography.row_kind_size)
                    .text_color(theme.warning)
                    .child(format!("Not active: {why}")),
            )
        })
}

/// The hotkey as text in the neutral control chrome — the keycap's tile
/// colors at text scale, since a hotkey is more than one key and the
/// keycap shows one glyph.
fn hotkey_chip(text: &str, theme: &Theme) -> Div {
    div()
        .flex_none()
        .flex()
        .items_center()
        .h(theme.geometry.keycap_height)
        .px(theme.geometry.keycap_padding_x)
        .rounded(theme.geometry.keycap_radius)
        .bg(theme.tile_background)
        .shadow(vec![
            BoxShadow::new(px(0.), px(0.), theme.tile_border)
                .spread_radius(px(1.))
                .inset(),
            BoxShadow::new(px(0.), px(1.), theme.tile_highlight).inset(),
        ])
        .text_size(theme.typography.footer_size)
        .text_color(theme.tile_foreground)
        .child(text.to_owned())
}

/// The page's status line: what the last alias change came to, a live
/// region so assistive technology announces it.
fn status_line(status: &StatusLine, theme: &Theme) -> Stateful<Div> {
    let (text, color): (String, Hsla) = match status {
        StatusLine::Saving => ("Saving the alias…".into(), theme.warning),
        StatusLine::Saved(done) => (done.clone(), theme.success),
        StatusLine::NotKept(problem) => (
            format!("Could not keep the change: {problem}"),
            theme.danger,
        ),
    };
    div()
        .id("shortcut-status")
        .debug_selector(|| "shortcut-status".into())
        .pt(px(10.))
        .role(Role::Status)
        .aria_label(text.clone())
        .text_size(theme.typography.row_subtitle_size)
        .text_color(color)
        .child(text)
}

/// The keyboard focus ring, as the sidebar rows' and the menu button's.
fn focus_ring(style: StyleRefinement, color: Hsla) -> StyleRefinement {
    style.shadow(vec![
        BoxShadow::new(px(0.), px(0.), color)
            .spread_radius(px(1.))
            .inset(),
    ])
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
