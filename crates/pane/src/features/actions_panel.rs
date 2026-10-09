//! The contextual Actions panel: the reference's searchable menu of what
//! can be done with root search's selected result, or with the selected
//! item of a command's list, opened over the footer by its Actions button
//! or the Open actions binding (Ctrl+K by default).
//!
//! For an item of a command's list (#137) it lists the item's actions
//! ([`pane_core::Launcher::item_actions`]) in their labelled sections, in
//! order: the primary action shows the invoke binding in the accent caps,
//! the second and third their chords (Ctrl+Enter, Ctrl+Shift+Enter), and an
//! action with a shortcut Pane binds shows that shortcut instead; an action
//! whose shortcut Pane does not bind shows none. Destructive actions are
//! drawn in the destructive color. Choosing one runs it on the item the
//! panel opened for, if that is still the selected item.
//!
//! An action that leads to further choices opens its submenu in place (#140,
//! [`pane_core::Launcher::open_submenu`]): it shows a chevron, Enter or a
//! click opens it, and the panel's header then names the submenu. Its
//! entries are drawn as the item's actions are (sections, keycaps, the
//! destructive style; an entry's own shortcut runs it while that submenu is
//! shown). Entries the command gives when the submenu opens show a loading
//! entry until it answers, and an error entry if it fails, keeping the
//! panel open. Typing filters the level shown, and Escape steps back one
//! level, giving back the filter and selection it had; from the item's
//! actions it closes the panel. Enter on an item's action that opens a
//! submenu, its chord or its shortcut opens the panel at that submenu.
//!
//! What it lists is the core's ([`pane_core::Launcher::result_actions`]):
//! the result's primary action — the footer's, the same dispatch — then
//! pinning it (or unpinning it, once it is pinned), then, for an installed
//! command, its hotkey and alias configuration. Nothing is listed without
//! a working operation behind it (#100). Pinning adds the result after the
//! last pin. A quick slot has a panel of its own — opened by a secondary
//! click on it, or the Open actions binding while it has focus — invoking,
//! unpinning and moving it (see [`crate::features::quick_slots`]); while
//! the pins lie side by side on the strip, its moves say left and right
//! rather than up and down. The pin entries show the launcher's own keys
//! for them (Ctrl+Shift+F, Ctrl+Alt and an arrow), as the primary entry
//! shows the invoke binding.
//!
//! The panel holds its target: the row selected when it opened, by its
//! stable id. While it is open the pointer cannot move root search's
//! selection, and an entry runs only if the core still has that target
//! selected with that action ready — a result removed or disabled behind
//! the panel shows its entries unavailable and runs nothing.
//!
//! Its search field holds focus: typing filters the entries by label, the
//! arrows move the selection over what is listed, Enter runs it, a click
//! runs the entry clicked, Escape closes the panel only (or steps back out
//! of a submenu), and Tab closes it from any level, giving focus back to
//! the query field. A mouse-down outside the panel closes it, from any
//! level, and is consumed, so the result it covered is never invoked.
//! Enter chooses once per press, so a held key's repeats never run what a
//! submenu it opened lists, and a double click's second click runs nothing.
//! The panel opens and closes at once: the reference authors no motion for
//! it.

use std::rc::Rc;

use gpui::{
    AnyElement, App, ClickEvent, Context, Div, Entity, FocusHandle, Focusable, KeyBinding,
    KeyDownEvent, MouseDownEvent, MouseMoveEvent, Pixels, Role, SharedString, Stateful,
    Subscription, Window, actions, div, prelude::*, px,
};
use gpui_elements::editable_text::actions::DEFAULT_INPUT_CONTEXT;
use gpui_elements::editable_text::{EditableTextState, StringStorage, TextChanged, text_input};
use pane_core::clipboard_view::{ClipboardAction, ClipboardActionItem};
use pane_core::{
    Icon, ItemActions, KeyboardAction, OpenSubmenu, PinnedLayout, ResultAction, ResultActions,
    RowKind, Screen, SlotChange, SubmenuState,
};

use crate::app::LauncherWindow;
use crate::features::announcer::{Listing, Noun, Opening, Selected, Target};
use crate::features::quick_slots;
use crate::ui::extension_icon::{self, IconSize, RowIcon};
use crate::ui::icon::{Glyph, IconTone, TileSize, glyph, tile_at};
use crate::ui::input::TextEditingKeys;
use crate::ui::keycap::{CapStyle, KeySequence, key_sequence};
use crate::ui::material::{Material, popover_shadows};
use crate::ui::theme::{TERTIARY_STRENGTH, Theme, pressed};
use crate::ui::virtual_list::{self, VirtualList};

actions!(
    actions_panel,
    [NextAction, PreviousAction, StepBack, CloseActions]
);

/// The panel's key context.
pub(crate) const CONTEXT: &str = "ActionsPanel";

/// The search field's placeholder, the reference's.
pub(crate) const PLACEHOLDER: &str = "Search actions…";
/// The search field's accessible name.
pub(crate) const SEARCH_LABEL: &str = "Search actions";
/// What the list says when the filter leaves nothing, the reference's.
pub(crate) const NO_MATCH: &str = "No actions match";
/// What the list says when no result is selected: nothing to act on.
pub(crate) const NOTHING_SELECTED: &str = "Select a result to see its actions";
/// What the list says for an item of a command's list without actions.
pub(crate) const NO_ACTIONS: &str = "This item has no actions";
/// The group label over the command's configuration.
pub(crate) const PANE_GROUP: &str = "Pane";
/// The group label over a quick slot's own operations.
pub(crate) const SLOT_GROUP: &str = "Quick Slot";
/// What a quick slot's earlier move says while the pins lie side by side
/// on the strip (the core's own label says up).
pub(crate) const MOVE_LEFT: &str = "Move Left";
/// What a quick slot's later move says on the strip (the core's own label
/// says down).
pub(crate) const MOVE_RIGHT: &str = "Move Right";
/// What assistive technology hears of a destructive entry, after its label.
pub(crate) const DESTRUCTIVE: &str = "Destructive";
/// What assistive technology hears of an entry that opens a submenu.
pub(crate) const OPENS_SUBMENU: &str = "Opens a submenu";
/// What a submenu says while the command is asked for its entries.
pub(crate) const LOADING: &str = "Loading…";
/// What a submenu without entries says.
pub(crate) const NO_ENTRIES: &str = "Nothing to choose here";

/// The keys the quick slot entries show, the launcher's own fixed keys for
/// them: the pin key on Pin and Unpin, and a move key on each move — the
/// arrow along the pins' layout.
pub(crate) struct SlotKeys {
    pub(crate) toggle_pin: KeySequence,
    pub(crate) earlier: KeySequence,
    pub(crate) later: KeySequence,
}

impl SlotKeys {
    /// The keys for the pins laid out `horizontal`ly (Left and Right) or
    /// down the list (Up and Down).
    pub(crate) fn new(horizontal: bool) -> SlotKeys {
        let arrow = usize::from(horizontal);
        let [earlier, later] = [true, false].map(|earlier| {
            crate::keyboard::binding_keys(&crate::keyboard::move_pin_bindings(earlier)[arrow])
        });
        SlotKeys {
            toggle_pin: crate::keyboard::binding_keys(&crate::keyboard::toggle_pin_binding()),
            earlier,
            later,
        }
    }

    /// The keys entry `action` shows, if it is a quick slot entry.
    fn of(&self, action: ResultAction) -> Option<&KeySequence> {
        match action {
            ResultAction::Pin | ResultAction::Unpin => Some(&self.toggle_pin),
            ResultAction::MovePinUp => Some(&self.earlier),
            ResultAction::MovePinDown => Some(&self.later),
            ResultAction::Invoke
            | ResultAction::Hotkey
            | ResultAction::Alias
            | ResultAction::ConfigureCommand
            | ResultAction::ConfigureExtension
            | ResultAction::DismissNotice => None,
        }
    }
}

/// `actions` as the panel lists them with the pins laid out `horizontal`ly:
/// the moves say left and right there, so the filter matches what is
/// shown.
fn laid_out(mut actions: ResultActions, horizontal: bool) -> ResultActions {
    if horizontal {
        for item in &mut actions.items {
            match item.action {
                ResultAction::MovePinUp => item.label = MOVE_LEFT.to_owned(),
                ResultAction::MovePinDown => item.label = MOVE_RIGHT.to_owned(),
                _ => {}
            }
        }
    }
    actions
}

/// Whether the Launcher page lays the pins out side by side, on the strip.
fn pins_horizontal(cx: &App) -> bool {
    crate::settings::shared(cx).read(cx).pinned_layout() == PinnedLayout::Horizontal
}

/// Registers the panel's keys: Up and Down in its search field, above the
/// field's own caret keys, and Escape and Tab in the panel, above the
/// launcher's back and focus traversal. Enter is not bound: the panel takes
/// it from the key press itself ([`LauncherWindow::panel_keys`]), which says
/// whether it is a held key's repeat, and the launcher's confirm hands it on
/// while the panel is open.
pub(crate) fn bind_keys(cx: &mut App, _: &TextEditingKeys) {
    let field = format!("{CONTEXT} > {DEFAULT_INPUT_CONTEXT}");
    cx.bind_keys([
        KeyBinding::new("down", NextAction, Some(&field)),
        KeyBinding::new("up", PreviousAction, Some(&field)),
        KeyBinding::new("escape", StepBack, Some(CONTEXT)),
        KeyBinding::new("tab", CloseActions, Some(CONTEXT)),
        KeyBinding::new("shift-tab", CloseActions, Some(CONTEXT)),
    ]);
}

/// The open Actions panel. Owned by the launcher window for exactly as
/// long as it is open.
pub(crate) struct ActionsPanel {
    /// The actions as they were when the panel opened: its target, header
    /// and entries; `None` when nothing was selected.
    opened: Option<Opened>,
    /// The search field's text.
    filter: Entity<EditableTextState>,
    /// The selected entry, an index into what the filter lists.
    selected: usize,
    /// The filter's text and the selection of each level above the one
    /// shown, outermost first: what stepping back out of a submenu gives
    /// back.
    above: Vec<(String, usize)>,
    /// The selection to keep when the filter's text is next replaced (by
    /// stepping into or out of a submenu), instead of the first entry.
    keep_selection: Option<usize>,
    /// What had focus when the panel opened, restored when it closes.
    restore: Option<FocusHandle>,
    /// Whether it opened over root search rather than a command's list:
    /// the screen it belongs to (see [`LauncherWindow::actions_belong_to`]).
    on_root: bool,
    /// The entries' list, drawn virtually (#165): only the entries in view
    /// are laid out and painted, however many a command gives.
    list: VirtualList,
    /// What the list's children are drawn from, as the last frame read it.
    frame: Option<Rc<PanelFrame>>,
    _filtering: Subscription,
    /// Closes the panel when the window loses activation.
    _deactivation: Subscription,
}

/// What a frame draws in the panel's list: its children are drawn from it
/// as the list lays them out.
struct PanelFrame {
    /// What the filter lists.
    listed: Vec<PanelEntry>,
    /// The list's children: the entries, with their sections' rules and
    /// labels.
    children: Vec<PanelChild>,
    /// The selected entry, an index into `listed`.
    selected: usize,
}

struct Opened {
    /// The row the panel opened for, by its stable id: a result's, or the
    /// item's.
    target: String,
    /// The target's title, as the header names it.
    title: String,
    /// The target's kind: an application's primary action opens it.
    kind: Option<RowKind>,
    /// What the entries act on.
    subject: Subject,
    /// The entries as they were when the panel opened.
    entries: Vec<PanelEntry>,
}

/// What the panel's entries act on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Subject {
    /// Root search's selected result.
    Result,
    /// The quick slot holding the target.
    Slot,
    /// The selected item of a command's list.
    Item,
    /// Pane's own Clipboard History view: its selected record, and the
    /// history (#166).
    Clipboard,
}

impl Subject {
    /// The label over a result's or a slot's entries after the primary
    /// action.
    fn group(self) -> &'static str {
        match self {
            Subject::Result | Subject::Item | Subject::Clipboard => PANE_GROUP,
            Subject::Slot => SLOT_GROUP,
        }
    }
}

/// What an entry runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EntryKind {
    /// One of Pane's actions on a result or a slot.
    Result(ResultAction),
    /// The action at this index of the item's actions.
    Item(usize),
    /// The entry at this index of the submenu shown (#140).
    Entry(usize),
    /// One of the Clipboard History view's actions (#166).
    Clipboard(ClipboardAction),
    /// What a submenu says instead of entries: that it is loading, or why
    /// the command could not give them. It runs nothing, and the filter
    /// keeps it.
    Note,
}

/// One entry the panel lists.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PanelEntry {
    pub(crate) kind: EntryKind,
    /// What the entry says, and what the filter matches.
    pub(crate) label: String,
    /// Whether it can run now.
    pub(crate) available: bool,
    /// Whether it is drawn in the destructive style: a destructive action,
    /// or a submenu's error.
    pub(crate) destructive: bool,
    /// Whether choosing it opens a submenu: a chevron follows it.
    pub(crate) submenu: bool,
    /// The label of its section; entries of one section follow each other,
    /// and an untitled section has none.
    pub(crate) section: Option<SharedString>,
    /// Its glyph.
    pub(crate) glyph: Glyph,
    /// The icon an item's action or a submenu's entry gives (#139), drawn
    /// in its glyph's place; `None` keeps the glyph.
    pub(crate) icon: Option<Icon>,
    /// The keys shown at its right, in their caps' style.
    pub(crate) keys: Option<(KeySequence, CapStyle)>,
}

/// `actions`, a result's or a slot's, as the panel's entries: the primary
/// action with the invoke binding's accent caps, then the rest under
/// `group`, the slot entries with their keys.
fn result_entries(
    actions: &ResultActions,
    kind: Option<RowKind>,
    group: &'static str,
    invoke: &KeySequence,
    slot_keys: &SlotKeys,
) -> Vec<PanelEntry> {
    let primary = primary_glyph(kind);
    actions
        .items
        .iter()
        .map(|item| {
            let invoking = item.action == ResultAction::Invoke;
            PanelEntry {
                kind: EntryKind::Result(item.action),
                label: item.label.clone(),
                available: item.available,
                destructive: false,
                submenu: false,
                section: (!invoking).then(|| group.into()),
                glyph: action_glyph(item.action, primary),
                icon: None,
                keys: if invoking {
                    Some((invoke.clone(), CapStyle::Accent))
                } else {
                    slot_keys
                        .of(item.action)
                        .map(|keys| (keys.clone(), CapStyle::Regular))
                },
            }
        })
        .collect()
}

/// `actions`, an item's, as the panel's entries: each with its own
/// shortcut's caps when Pane binds it, else the primary action with the
/// invoke binding's accent caps and the next two with their chords.
fn item_entries(actions: &ItemActions, invoke: &KeySequence) -> Vec<PanelEntry> {
    actions
        .actions
        .iter()
        .enumerate()
        .map(|(index, action)| {
            let keys = match (&action.shortcut, index) {
                (Some(shortcut), _) => {
                    Some((crate::keyboard::binding_keys(shortcut), CapStyle::Regular))
                }
                (None, 0) => Some((invoke.clone(), CapStyle::Accent)),
                (None, index) => pane_core::keyboard::action_key(index)
                    .map(|chord| (crate::keyboard::binding_keys(&chord), CapStyle::Regular)),
            };
            PanelEntry {
                kind: EntryKind::Item(index),
                label: action.title.clone(),
                available: true,
                destructive: action.destructive,
                submenu: action.submenu,
                section: action.section.clone().map(SharedString::from),
                glyph: Glyph::ActionRun,
                icon: action.icon.clone(),
                keys,
            }
        })
        .collect()
}

/// `submenu`'s entries as the panel lists them (#140): each with its own
/// shortcut's caps when Pane binds it (the action chords are the item's
/// actions'), or the one entry saying it is loading or why it failed.
fn submenu_entries(submenu: &OpenSubmenu) -> Vec<PanelEntry> {
    let note = |label: String, glyph, destructive| PanelEntry {
        kind: EntryKind::Note,
        label,
        available: false,
        destructive,
        submenu: false,
        section: None,
        glyph,
        icon: None,
        keys: None,
    };
    match &submenu.state {
        SubmenuState::Loading => vec![note(LOADING.to_owned(), Glyph::Clock, false)],
        SubmenuState::Failed(why) => vec![note(why.clone(), Glyph::SearchNone, true)],
        SubmenuState::Listed(entries) => entries
            .iter()
            .enumerate()
            .map(|(index, entry)| PanelEntry {
                kind: EntryKind::Entry(index),
                label: entry.title.clone(),
                available: true,
                destructive: entry.destructive,
                submenu: entry.submenu,
                section: entry.section.clone().map(SharedString::from),
                glyph: Glyph::ActionRun,
                icon: entry.icon.clone(),
                keys: entry
                    .shortcut
                    .as_ref()
                    .map(|shortcut| (crate::keyboard::binding_keys(shortcut), CapStyle::Regular)),
            })
            .collect(),
    }
}

/// The Clipboard History view's `actions` as the panel's entries (#166):
/// Paste with the invoke binding's accent caps, Copy and Delete with the
/// view's own keys, the rest under their sections.
pub(crate) fn clipboard_entries(
    actions: &[ClipboardActionItem],
    invoke: &KeySequence,
) -> Vec<PanelEntry> {
    actions
        .iter()
        .map(|item| {
            let (glyph, keys) = match item.action {
                ClipboardAction::Paste => {
                    (Glyph::ActionRun, Some((invoke.clone(), CapStyle::Accent)))
                }
                ClipboardAction::Copy => (
                    Glyph::Clipboard,
                    Some((
                        crate::features::clipboard_history::copy_keys(),
                        CapStyle::Regular,
                    )),
                ),
                ClipboardAction::Delete => (
                    Glyph::Delete,
                    Some((
                        crate::features::clipboard_history::delete_keys(),
                        CapStyle::Regular,
                    )),
                ),
                ClipboardAction::PauseRecording => (Glyph::Pause, None),
                ClipboardAction::ResumeRecording => (Glyph::Record, None),
                ClipboardAction::ClearHistory => (Glyph::Reset, None),
                ClipboardAction::KeepFor(_) => (Glyph::Clock, None),
                ClipboardAction::DisabledApplications => (Glyph::Shield, None),
            };
            PanelEntry {
                kind: EntryKind::Clipboard(item.action),
                label: item.label.clone(),
                available: item.available,
                destructive: item.destructive,
                submenu: false,
                section: item.section.clone().map(SharedString::from),
                glyph,
                icon: None,
                keys,
            }
        })
        .collect()
}

/// The panel's accessible name, which the announcer says as it opens (#132):
/// "Actions for <target>", or "<submenu>, actions for <target>" while a
/// submenu is shown.
fn panel_label(opened: Option<&Opened>, submenu: Option<&OpenSubmenu>) -> String {
    let Some(opened) = opened else {
        return "Actions".to_owned();
    };
    match submenu {
        Some(submenu) => format!("{}, actions for {}", submenu.title, opened.title),
        None => format!("Actions for {}", opened.title),
    }
}

/// The entries whose label holds `query`, ignoring case and the spaces
/// around it; all of them for a blank one. Filtering flattens the
/// sections (see [`panel_children`]). A submenu's note stays.
fn matching(entries: Vec<PanelEntry>, query: &str) -> Vec<PanelEntry> {
    let query = query.trim().to_lowercase();
    entries
        .into_iter()
        .filter(|entry| {
            entry.kind == EntryKind::Note || entry.label.to_lowercase().contains(&query)
        })
        .collect()
}

impl ActionsPanel {
    /// The filter's text.
    fn query(&self, cx: &App) -> String {
        self.filter.read(cx).as_str().to_owned()
    }
}

impl LauncherWindow {
    /// Opens or closes the Actions panel: the Open actions binding and the
    /// footer's Actions button.
    pub(crate) fn toggle_actions(
        &mut self,
        _: &crate::OpenActions,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.actions.is_some() {
            self.close_actions(window, cx);
        } else {
            self.open_actions(window, cx);
        }
    }

    /// Opens the Actions panel for root search's selected result — or, while
    /// a quick slot has focus, for that slot — or for the selected item of
    /// a command's list, with focus in its search field. Root search and
    /// commands' lists have Actions.
    pub(crate) fn open_actions(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(slot) = self.focused_slot(window) {
            self.open_slot_actions(slot, window, cx);
            return;
        }
        // A command's list, or a row of root search whose actions Pane
        // performs itself (a file, a computed answer: #150).
        let screen = self.launcher.screen();
        let own_row =
            matches!(screen, Screen::Root { .. }) && self.launcher.item_actions().is_some();
        if commands_list(&screen) || own_row {
            let invoke = invoke_keys(cx);
            let opened = self.launcher.item_actions().map(|actions| Opened {
                target: actions.target.clone(),
                title: actions.title.clone(),
                kind: None,
                subject: Subject::Item,
                entries: item_entries(&actions, &invoke),
            });
            // A row without actions of its own still opens the panel, which
            // says so.
            let opened = opened.or_else(|| {
                let view = self.launcher.view();
                let row = view.rows.get(view.selected?)?;
                Some(Opened {
                    target: row.id.clone(),
                    title: row.title.clone(),
                    kind: None,
                    subject: Subject::Item,
                    entries: Vec::new(),
                })
            });
            self.open_panel(opened, window, cx);
            return;
        }
        let opened = self.launcher.result_actions().map(|actions| {
            let presentation = self.launcher.presentation();
            let kind = self
                .launcher
                .selected()
                .and_then(|index| presentation.rows.get(index))
                .and_then(|row| row.kind);
            Opened {
                target: actions.target.clone(),
                title: actions.title.clone(),
                kind,
                subject: Subject::Result,
                entries: result_entries(
                    &actions,
                    kind,
                    Subject::Result.group(),
                    &invoke_keys(cx),
                    &SlotKeys::new(pins_horizontal(cx)),
                ),
            }
        });
        self.open_panel(opened, window, cx);
    }

    /// Opens the Actions panel for the quick slot at `index`: invoking,
    /// unpinning and moving it.
    pub(crate) fn open_slot_actions(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(slot) = self.launcher.quick_slots().into_iter().nth(index) else {
            return;
        };
        let target = slot.target.key();
        let opened = self.launcher.quick_slot_actions(&target).map(|actions| {
            let actions = laid_out(actions, pins_horizontal(cx));
            Opened {
                target: actions.target.clone(),
                title: actions.title.clone(),
                kind: slot.kind,
                subject: Subject::Slot,
                entries: result_entries(
                    &actions,
                    slot.kind,
                    Subject::Slot.group(),
                    &invoke_keys(cx),
                    &SlotKeys::new(pins_horizontal(cx)),
                ),
            }
        });
        if opened.is_some() {
            self.open_panel(opened, window, cx);
        }
    }

    /// Opens the Actions panel of Pane's own Clipboard History view (#166)
    /// for the record selected there (`selected`, its id and title), or
    /// for the history with none: the record's actions and the history's.
    pub(crate) fn open_clipboard_actions(
        &mut self,
        selected: Option<(String, String)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(view) = self.launcher.clipboard_history() else {
            return;
        };
        let (target, title) = match selected {
            Some((id, title)) => (id, title),
            None => (String::new(), view.title.clone()),
        };
        let record = Some(target.as_str()).filter(|id| !id.is_empty());
        let entries = clipboard_entries(&view.actions(record), &invoke_keys(cx));
        self.open_panel(
            Some(Opened {
                target,
                title,
                kind: None,
                subject: Subject::Clipboard,
                entries,
            }),
            window,
            cx,
        );
    }

    /// Opens the panel over `opened`, with focus in its search field.
    fn open_panel(&mut self, opened: Option<Opened>, window: &mut Window, cx: &mut Context<Self>) {
        let screen = self.launcher.screen();
        let has_actions = matches!(screen, Screen::Root { .. }) || commands_list(&screen);
        if self.actions.is_some() || !has_actions {
            return;
        }
        self.close_open_menu(window, cx);
        // A panel opens on the item's own actions, with no submenu open.
        self.launcher.close_submenus();
        let filter = cx.new(|cx| EditableTextState::new(StringStorage::default(), cx));
        let filtering = cx.subscribe(&filter, |this, _, _: &TextChanged, cx| {
            if let Some(panel) = this.actions.as_mut() {
                panel.selected = panel.keep_selection.take().unwrap_or(0);
            }
            cx.notify();
        });
        let deactivation = cx.observe_window_activation(window, |this, window, cx| {
            if !window.is_window_active() {
                this.close_actions(window, cx);
            }
        });
        let restore = window.focused(cx);
        window.focus(&filter.focus_handle(cx), cx);
        self.actions = Some(ActionsPanel {
            opened,
            filter,
            selected: 0,
            above: Vec::new(),
            keep_selection: None,
            restore,
            on_root: matches!(screen, Screen::Root { .. }),
            list: VirtualList::new(entry_height(&crate::settings::launcher_visuals(cx).theme)),
            frame: None,
            _filtering: filtering,
            _deactivation: deactivation,
        });
        cx.notify();
    }

    /// Whether an open Actions panel still belongs on `screen`: the kind of
    /// screen it opened over (root search, or a command's list) is still
    /// shown. A background change that leaves it there (a web image
    /// arriving, #142) keeps the panel open; a screen that replaced it (a
    /// hotkey pressed, a change from Settings) does not.
    pub(crate) fn actions_belong_to(&self, screen: &Screen) -> bool {
        self.actions.as_ref().is_some_and(|panel| {
            item_list(screen) && panel.on_root == matches!(screen, Screen::Root { .. })
        })
    }

    /// Closes the Actions panel, if it is open, from whatever level it
    /// shows, giving focus back to what had it. Whether it was open.
    pub(crate) fn close_actions(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(panel) = self.actions.take() else {
            return false;
        };
        // Its submenus close with it: an answer still on its way is
        // discarded.
        self.launcher.close_submenus();
        if let Some(restore) = panel.restore {
            window.focus(&restore, cx);
        }
        cx.notify();
        true
    }

    /// The panel's entries as they stand now: the core's, while it still
    /// has the target selected, else the ones the panel opened with, all
    /// unavailable — a target removed or disabled behind the panel runs
    /// nothing. The moves are labelled for the pins' layout (see
    /// `laid_out`).
    fn live_entries(&self, cx: &App) -> Option<Vec<PanelEntry>> {
        let opened = self.actions.as_ref()?.opened.as_ref()?;
        let horizontal = pins_horizontal(cx);
        let invoke = invoke_keys(cx);
        let slot_keys = SlotKeys::new(horizontal);
        let live = match opened.subject {
            Subject::Result => self
                .launcher
                .result_actions()
                .filter(|live| live.target == opened.target)
                .map(|live| {
                    result_entries(
                        &laid_out(live, horizontal),
                        opened.kind,
                        Subject::Result.group(),
                        &invoke,
                        &slot_keys,
                    )
                }),
            Subject::Slot => self
                .launcher
                .quick_slot_actions(&opened.target)
                .map(|live| {
                    result_entries(
                        &laid_out(live, horizontal),
                        opened.kind,
                        Subject::Slot.group(),
                        &invoke,
                        &slot_keys,
                    )
                }),
            Subject::Item => match self.shown_submenu() {
                Some(submenu) => Some(submenu_entries(&submenu)),
                None => self
                    .launcher
                    .item_actions()
                    .filter(|live| live.target == opened.target)
                    .map(|live| item_entries(&live, &invoke)),
            },
            // The history as it is now: a record deleted or expired behind
            // the panel loses its own actions.
            Subject::Clipboard => self.launcher.clipboard_history().map(|view| {
                let record = Some(opened.target.as_str()).filter(|id| !id.is_empty());
                clipboard_entries(&view.actions(record), &invoke)
            }),
        };
        Some(live.unwrap_or_else(|| {
            opened
                .entries
                .iter()
                .map(|entry| PanelEntry {
                    available: false,
                    ..entry.clone()
                })
                .collect()
        }))
    }

    /// The submenu the panel shows over its item, if one is open (#140).
    fn shown_submenu(&self) -> Option<OpenSubmenu> {
        let opened = self.actions.as_ref()?.opened.as_ref()?;
        if opened.subject != Subject::Item {
            return None;
        }
        self.launcher
            .submenu()
            .filter(|submenu| submenu.target == opened.target)
    }

    /// What the filter lists now.
    fn listed(&self, cx: &App) -> Vec<PanelEntry> {
        let (Some(panel), Some(entries)) = (self.actions.as_ref(), self.live_entries(cx)) else {
            return Vec::new();
        };
        matching(entries, &panel.query(cx))
    }

    fn actions_next(&mut self, _: &NextAction, _: &mut Window, cx: &mut Context<Self>) {
        self.move_action(true, cx);
    }

    fn actions_previous(&mut self, _: &PreviousAction, _: &mut Window, cx: &mut Context<Self>) {
        self.move_action(false, cx);
    }

    /// Moves the selection to the next (or previous) entry that can run,
    /// staying put at the ends.
    fn move_action(&mut self, forward: bool, cx: &mut Context<Self>) {
        let listed = self.listed(cx);
        if let Some(panel) = self.actions.as_mut()
            && let Some(next) = next_available(&listed, panel.selected, forward)
        {
            panel.selected = next;
            self.announcer.user_moved();
            cx.notify();
        }
    }

    /// The open panel as the window's announcer follows it (#132): over
    /// the screen, opening with its name and how many entries it lists,
    /// its search field's text the typing, and each level of a submenu a
    /// list of its own.
    pub(crate) fn panel_listing(&self, cx: &App) -> Option<Listing> {
        let panel = self.actions.as_ref()?;
        let listed = self.listed(cx);
        let submenu = self.shown_submenu();
        let label = panel_label(panel.opened.as_ref(), submenu.as_ref());
        let target = match listed.get(panel.selected) {
            Some(entry) => Target::Row(Selected {
                id: entry.label.clone(),
                title: entry.label.clone(),
                position: panel.selected + 1,
                unavailable: !entry.available && entry.kind != EntryKind::Note,
                section: entry.section.as_ref().map(SharedString::to_string),
            }),
            None if listed.is_empty() => Target::NoResults,
            None => Target::Nothing,
        };
        Some(Listing {
            over: true,
            key: label.clone(),
            opening: Opening::Named(label, Noun::Commands),
            count: listed.len(),
            target,
            query: Some(panel.query(cx)),
            settled: true,
        })
    }

    /// A key pressed while the panel is open, before its search field sees
    /// it: Enter chooses the selected entry, once per press, so a held
    /// Enter's repeats never run what the submenu it opened lists; and while
    /// a submenu is shown, an entry's own shortcut runs that entry (or opens
    /// its submenu), whether or not the filter lists it (#140).
    pub(crate) fn panel_keys(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(selected) = self.actions.as_ref().map(|panel| panel.selected) else {
            return;
        };
        let Ok(pressed) = crate::keyboard::binding_of(&event.keystroke) else {
            return;
        };
        if pressed.id() == "enter" {
            cx.stop_propagation();
            if !event.is_held {
                self.run_action(selected, window, cx);
            }
            return;
        }
        let Some(submenu) = self.shown_submenu() else {
            return;
        };
        let Some(entry) = submenu
            .bound_to(&pressed)
            .and_then(|index| submenu_entries(&submenu).into_iter().nth(index))
        else {
            return;
        };
        cx.stop_propagation();
        if !event.is_held {
            self.choose_item_entry(&submenu.target, &entry, window, cx);
        }
    }

    /// Escape: steps back out of the submenu shown to the level above it,
    /// giving back the filter's text and the selection it had there; from
    /// the item's actions (or a result's), closes the panel.
    fn actions_back(&mut self, _: &StepBack, window: &mut Window, cx: &mut Context<Self>) {
        let above = self.actions.as_mut().and_then(|panel| panel.above.pop());
        let Some((query, selected)) = above else {
            self.close_actions(window, cx);
            return;
        };
        self.launcher.close_submenu();
        if let Some(panel) = self.actions.as_mut() {
            panel.selected = selected;
            panel.keep_selection = Some(selected);
            let filter = panel.filter.clone();
            filter.update(cx, |filter, cx| filter.emplace(&query, cx));
        }
        cx.notify();
    }

    fn actions_close(&mut self, _: &CloseActions, window: &mut Window, cx: &mut Context<Self>) {
        self.close_actions(window, cx);
    }

    /// Opens the submenu of the action at `index` of the level the panel
    /// shows for the item `target` (#140): its entries replace the level
    /// shown, with an empty filter and the first entry selected, and the
    /// level shown is kept for Escape to give back. For entries the command
    /// gives when the submenu opens, the panel shows the loading entry
    /// until it answers, then draws the answer. Nothing happens if the
    /// action opens no submenu, or the item is no longer selected.
    pub(crate) fn enter_submenu(
        &mut self,
        target: &str,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let depth =
            |launcher: &pane_core::Launcher| launcher.submenu().map_or(0, |open| open.depth);
        let before = depth(&self.launcher);
        let pending = self.launcher.open_submenu(target, index);
        if depth(&self.launcher) <= before {
            return;
        }
        let Some(panel) = self.actions.as_mut() else {
            return;
        };
        let query = panel.query(cx);
        panel.above.push((query, panel.selected));
        panel.selected = 0;
        panel.keep_selection = Some(0);
        let filter = panel.filter.clone();
        filter.update(cx, |filter, cx| filter.emplace("", cx));
        // An answer the command gives later is drawn when it arrives.
        cx.spawn_in(window, async move |this, cx| {
            pending.await;
            this.update_in(cx, |_, _, cx| cx.notify()).ok();
        })
        .detach();
        cx.notify();
    }

    /// Opens the Actions panel at the submenu of the selected item's action
    /// at `index`: what Enter, an action chord or the action's shortcut does
    /// from the list for an action that opens a submenu (#140). Escape then
    /// steps back to the item's actions.
    pub(crate) fn open_item_submenu(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.actions.is_none() {
            self.open_actions(window, cx);
        }
        let target = self
            .actions
            .as_ref()
            .and_then(|panel| panel.opened.as_ref())
            .filter(|opened| opened.subject == Subject::Item)
            .map(|opened| opened.target.clone());
        if let Some(target) = target {
            self.enter_submenu(&target, index, window, cx);
        }
    }

    /// Chooses `entry`, one of the item `target`'s actions or an entry of
    /// the submenu shown: opens its submenu, or runs it once on the item the
    /// panel opened for, if that is still selected, closing the panel. A
    /// note, or an entry that cannot run, does nothing.
    fn choose_item_entry(
        &mut self,
        target: &str,
        entry: &PanelEntry,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !entry.available {
            return;
        }
        match entry.kind {
            EntryKind::Item(index) | EntryKind::Entry(index) if entry.submenu => {
                self.enter_submenu(target, index, window, cx);
            }
            // On root search, a row's first action is Enter's: the window
            // copies a computed answer itself (#150), as Enter does.
            EntryKind::Item(0) if matches!(self.launcher.screen(), Screen::Root { .. }) => {
                self.close_actions(window, cx);
                self.press_primary_action(window, cx);
            }
            EntryKind::Item(index) => {
                // The core runs it only on the item the panel opened for,
                // still selected.
                self.close_actions(window, cx);
                let pending = self.launcher.run_item_action(target, index);
                self.show_until_done(pending, window, cx);
            }
            EntryKind::Entry(index) => {
                // Started before the panel closes, which closes the
                // submenus: the core closes them itself as the entry starts,
                // so a repeat of the choice runs nothing.
                let pending = self.launcher.run_submenu_entry(target, index);
                self.close_actions(window, cx);
                self.show_until_done(pending, window, cx);
            }
            EntryKind::Result(_) | EntryKind::Clipboard(_) | EntryKind::Note => {}
        }
    }

    /// Runs the listed entry `index`, once, if the core still has the
    /// panel's target — the selected row, or the slot holding it — with
    /// that action ready; the panel closes, but an entry that cannot run
    /// does nothing.
    fn run_action(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let opened = self
            .actions
            .as_ref()
            .and_then(|panel| panel.opened.as_ref())
            .map(|opened| (opened.target.clone(), opened.subject));
        let entry = self.listed(cx).get(index).cloned();
        let (Some((target, subject)), Some(entry)) = (opened, entry) else {
            return;
        };
        let action = match entry.kind {
            EntryKind::Result(action) => action,
            // The Clipboard History view's own (#166): run on the record the
            // panel opened for, which the core revalidates.
            EntryKind::Clipboard(action) => {
                if entry.available {
                    self.close_actions(window, cx);
                    let record = Some(target).filter(|id| !id.is_empty());
                    self.run_clipboard_action(action, record, window, cx);
                }
                return;
            }
            EntryKind::Item(_) | EntryKind::Entry(_) | EntryKind::Note => {
                self.choose_item_entry(&target, &entry, window, cx);
                return;
            }
        };
        let ready = match subject {
            Subject::Result => self.launcher.result_action_ready(&target, action),
            Subject::Slot => self.launcher.quick_slot_action_ready(&target, action),
            Subject::Item | Subject::Clipboard => false,
        };
        if !ready {
            return;
        }
        match action {
            ResultAction::Invoke => {
                self.close_actions(window, cx);
                match self.launcher.quick_slot_of(&target) {
                    Some(slot) if subject == Subject::Slot => {
                        self.activate_quick_slot(slot, window, cx)
                    }
                    _ => self.press_primary_action(window, cx),
                }
            }
            ResultAction::Hotkey | ResultAction::Alias => {
                self.close_actions(window, cx);
                if self.launcher.open_result_action(&target, action) {
                    self.navigate_forward(window, cx);
                }
            }
            // The extension's card in Settings › Extensions, at the
            // command's preferences or the package's (#143).
            ResultAction::ConfigureCommand | ResultAction::ConfigureExtension => {
                self.close_actions(window, cx);
                if let Some(pane_core::PreferencesTarget { identity, command }) =
                    self.launcher.preferences_target(&target)
                {
                    let anchor = match action {
                        ResultAction::ConfigureCommand => {
                            crate::features::settings::extensions::command_preferences_anchor(
                                &identity.key(),
                                &command,
                            )
                        }
                        _ => identity.key(),
                    };
                    crate::features::settings::open_at(
                        &self.launcher,
                        crate::features::settings::extensions::TITLE,
                        &anchor,
                        cx,
                    );
                }
            }
            ResultAction::Pin
            | ResultAction::Unpin
            | ResultAction::MovePinUp
            | ResultAction::MovePinDown => {
                // Closed first, so the focus it gives back — a slot's, for
                // the panel its Open actions binding opened — is the one
                // the change makes follow a moved or unpinned slot.
                self.close_actions(window, cx);
                let change = self.change_quick_slot(&target, action, window, cx);
                // Pinning what a slot holds already moves focus to that
                // slot: a typed query is cleared first, so the home and
                // the slot show (the status keeps naming the slot).
                if let SlotChange::AlreadyPinned(slot) = change {
                    if !quick_slots::home_shown(&self.launcher.view()) {
                        let cleared = self.launcher.set_query("");
                        self.show_until_done(cleared, window, cx);
                    }
                    self.focus_slot(slot, window, cx);
                }
            }
            // The notice that Pane quit unexpectedly last time (#133): its
            // row leaves root search.
            ResultAction::DismissNotice => {
                self.close_actions(window, cx);
                self.launcher.dismiss_crash_notice();
                self.show_until_done(std::future::ready(()), window, cx);
            }
        }
    }

    /// Test support: whether the Actions panel is open.
    #[doc(hidden)]
    pub fn actions_open(&self) -> bool {
        self.actions.is_some()
    }

    /// Test support: the Actions panel's search field.
    #[doc(hidden)]
    pub fn actions_filter(&self) -> Option<Entity<EditableTextState>> {
        self.actions.as_ref().map(|panel| panel.filter.clone())
    }

    /// The open panel over the footer strip, if it is open: anchored to
    /// the strip's top edge with the reference's 8px between, and its
    /// right edge 10px in from the window's. Its entries are drawn
    /// virtually (#165), the list as high as they are up to the room the
    /// window leaves it, the selected entry kept in view.
    pub(crate) fn render_actions_layer(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let listed = self.listed(cx);
        let submenu = self.shown_submenu();
        let panel = self.actions.as_mut()?;
        let visuals = crate::settings::launcher_visuals(cx);
        let theme = &visuals.theme;
        let opened = panel.opened.as_ref();
        let filtering = !panel.query(cx).trim().is_empty();
        let item_without_actions = opened
            .is_some_and(|opened| opened.subject == Subject::Item && opened.entries.is_empty());
        // A submenu names itself in the header, as the panel's context.
        let empty_note = match (&submenu, filtering) {
            (Some(_), false) => NO_ENTRIES,
            (None, false) if item_without_actions => NO_ACTIONS,
            _ => NO_MATCH,
        };
        let title = submenu
            .as_ref()
            .map(|submenu| submenu.title.clone())
            .or_else(|| opened.map(|opened| opened.title.clone()));
        // The target's icon as its row and its slot draw it: an extension's
        // or an application's own, else Pane's tile (#163).
        let icon = opened
            .filter(|opened| matches!(opened.subject, Subject::Result | Subject::Slot))
            .map(|opened| {
                crate::features::icons::row_icon_of(&self.launcher, &opened.target, theme)
            });
        let label = panel_label(opened, submenu.as_ref());
        // The list's frame. It is measured again, from its top, only when
        // what it lists changed — not as an icon arrives — and keeps the
        // selected entry in view.
        let children = panel_children(&listed, filtering);
        let changed = panel.frame.as_ref().is_none_or(|last| {
            last.children != children
                || last.listed.len() != listed.len()
                || last
                    .listed
                    .iter()
                    .zip(&listed)
                    .any(|(last, now)| last.label != now.label || last.section != now.section)
        });
        let moved = panel
            .frame
            .as_ref()
            .is_none_or(|last| last.selected != panel.selected);
        if changed || panel.list.count() != children.len() {
            panel.list.reset(children.len());
        }
        if (changed || moved)
            && let Some(child) = children
                .iter()
                .position(|child| *child == PanelChild::Entry(panel.selected))
        {
            panel.list.reveal(child);
        }
        let room = list_room(window.viewport_size().height, title.is_some(), theme);
        let height = list_height(&children, theme).min(room);
        let empty = listed.is_empty();
        panel.frame = Some(Rc::new(PanelFrame {
            listed,
            children,
            selected: panel.selected,
        }));
        let list = (!empty).then(|| {
            gpui::list(
                panel.list.state().clone(),
                cx.processor(|this, index, _: &mut Window, cx| this.render_panel_child(index, cx)),
            )
            .w_full()
            .h(height)
            .py(theme.geometry.actions.list_padding)
            .into_any_element()
        });
        let surface = compose(
            PanelView {
                title: title.as_deref(),
                icon,
                empty,
                empty_note,
                filter: &panel.filter,
                focus: panel.filter.focus_handle(cx),
                query: panel.query(cx),
            },
            list,
            theme,
            visuals.material,
        );
        let surface = surface
            .key_context(CONTEXT)
            .capture_key_down(cx.listener(Self::panel_keys))
            .on_action(cx.listener(Self::actions_next))
            .on_action(cx.listener(Self::actions_previous))
            .on_action(cx.listener(Self::actions_back))
            .on_action(cx.listener(Self::actions_close))
            .on_mouse_down_out(cx.listener(|this, _: &MouseDownEvent, window, cx| {
                this.close_actions(window, cx);
                cx.stop_propagation();
            }))
            .role(Role::Dialog)
            .aria_label(label);
        Some(anchored(surface, theme).into_any_element())
    }

    /// The panel's list child at `index` of the frame laid out, as the
    /// list draws it: a rule, a section's label or an entry — an available
    /// one selecting under the moving pointer and running on a click —
    /// with the gap after it and the list's side padding.
    fn render_panel_child(&mut self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(frame) = self.actions.as_ref().and_then(|panel| panel.frame.clone()) else {
            return div().into_any_element();
        };
        let theme = crate::settings::launcher_visuals(cx).theme;
        let child = match frame.children.get(index) {
            Some(&child) => panel_child(child, &frame.listed, frame.selected, &theme, |row, at| {
                row.on_mouse_move(cx.listener(move |this, _: &MouseMoveEvent, _, cx| {
                    if let Some(panel) = this.actions.as_mut()
                        && panel.selected != at
                    {
                        panel.selected = at;
                        this.announcer.user_moved();
                        cx.notify();
                    }
                }))
                .on_click(cx.listener(
                    move |this, event: &ClickEvent, window, cx| {
                        // A double click's second click runs nothing more:
                        // not the entry of the submenu its first one opened.
                        if event.click_count() <= 1 {
                            this.run_action(at, window, cx);
                        }
                    },
                ))
            }),
            None => div().into_any_element(),
        };
        let geometry = &theme.geometry.actions;
        let last = index + 1 >= frame.children.len();
        virtual_list::item(child, last, geometry.list_gap, geometry.list_padding).into_any_element()
    }
}

/// What the panel shows: what [`compose`] draws.
pub(crate) struct PanelView<'a> {
    /// The target's title; `None` with nothing selected.
    pub(crate) title: Option<&'a str>,
    /// The target's icon, as its row draws it; the command glyph without.
    pub(crate) icon: Option<RowIcon>,
    /// Whether the filter lists nothing.
    pub(crate) empty: bool,
    /// What the list says when nothing is listed for a target.
    pub(crate) empty_note: &'static str,
    /// The search field's text.
    pub(crate) filter: &'a Entity<EditableTextState>,
    /// The search field's focus, which its accessibility node tracks.
    pub(crate) focus: FocusHandle,
    /// The search field's text, as its accessibility node's value.
    pub(crate) query: String,
}

/// The panel as `view` describes it: the header (the target's tile and
/// title), the entries' `list` — or the note saying why there are none —
/// and the search row, in the L2 popover.
pub(crate) fn compose(
    view: PanelView,
    list: Option<AnyElement>,
    theme: &Theme,
    material: Material,
) -> Stateful<Div> {
    let empty = match (view.title, view.empty) {
        (None, _) => Some(NOTHING_SELECTED),
        (Some(_), true) => Some(view.empty_note),
        (Some(_), false) => None,
    };
    let header = view
        .title
        .map(|title| header(title, view.icon.as_ref(), theme));
    popup(
        header,
        list.filter(|_| empty.is_none()),
        empty,
        search_field(view.filter, &view.focus, view.query, theme),
        theme,
        material,
    )
}

/// The height the list's children are taken to have until drawn: an
/// entry's, with the gap after it.
fn entry_height(theme: &Theme) -> Pixels {
    let geometry = &theme.geometry.actions;
    geometry.row_height + geometry.list_gap
}

/// The list's height with `children`: each child's own — an entry's, a
/// section label's and a rule's are fixed — with the gaps between them and
/// the list's padding above and below.
fn list_height(children: &[PanelChild], theme: &Theme) -> Pixels {
    let geometry = &theme.geometry.actions;
    let own: Pixels = children
        .iter()
        .map(|child| match child {
            PanelChild::Rule => px(1.) + geometry.rule_margin_y * 2.,
            PanelChild::Group(_) => geometry.group_height,
            PanelChild::Entry(_) => geometry.row_height,
        })
        .fold(px(0.), |total, height| total + height);
    let gaps = geometry.list_gap * children.len().saturating_sub(1) as f32;
    own + gaps + geometry.list_padding * 2.
}

/// The most the list may take in a window `height` high: what is left
/// between the footer, with the panel's space above it, and the same
/// space below the window's top edge, after the header (when `titled`)
/// and the search row. At least one entry's.
fn list_room(height: Pixels, titled: bool, theme: &Theme) -> Pixels {
    let geometry = &theme.geometry.actions;
    let header = if titled {
        geometry.header_height
    } else {
        px(0.)
    };
    let room = height
        - theme.geometry.footer_height
        - geometry.above_footer * 2.
        - header
        - geometry.search_height;
    room.max(geometry.row_height + geometry.list_padding * 2.)
}

/// The entry after (or before) `from` in `listed` that can run, if any.
pub(crate) fn next_available(listed: &[PanelEntry], from: usize, forward: bool) -> Option<usize> {
    let available = |index: &usize| listed.get(*index).is_some_and(|item| item.available);
    if forward {
        (from + 1..listed.len()).find(available)
    } else {
        (0..from).rev().find(available)
    }
}

/// One child of the panel's list, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PanelChild {
    /// The separator's rule.
    Rule,
    /// The label of the section the listed entry at this index begins.
    Group(usize),
    /// The listed entry at this index.
    Entry(usize),
}

/// The list's children for `listed`, section by section: a rule between
/// two sections, and a section's label over its entries when it has one —
/// unless the filter is narrowing them, as the reference drops its
/// separators then and lists what matches as one. For a result that is its
/// primary action, then a rule and the "Pane" label over the command's
/// configuration.
pub(crate) fn panel_children(listed: &[PanelEntry], filtering: bool) -> Vec<PanelChild> {
    let mut children = Vec::new();
    for (index, entry) in listed.iter().enumerate() {
        if !filtering {
            let begins = match index.checked_sub(1) {
                None => true,
                Some(previous) => listed[previous].section != entry.section,
            };
            if begins && index > 0 {
                children.push(PanelChild::Rule);
            }
            if begins && entry.section.is_some() {
                children.push(PanelChild::Group(index));
            }
        }
        children.push(PanelChild::Entry(index));
    }
    children
}

/// The results, under the panel's dimmer while it is `open`: the dimmer
/// lies over the list's area only, between the search header and the
/// footer, and takes no input.
pub(crate) fn dimmed(results: AnyElement, open: bool, theme: &Theme) -> AnyElement {
    if !open {
        return results;
    }
    div()
        .relative()
        .flex_1()
        .min_h(px(0.))
        .flex()
        .flex_col()
        .child(results)
        .child(dimmer(theme))
        .into_any_element()
}

/// The glyph of the primary action: an arrow out for an application (the
/// reference's `A.open`), else the run triangle (`A.run`).
pub(crate) fn primary_glyph(kind: Option<RowKind>) -> Glyph {
    match kind {
        Some(RowKind::Application) => Glyph::ActionOpen,
        _ => Glyph::ActionRun,
    }
}

/// The glyph of an action.
fn action_glyph(action: ResultAction, primary: Glyph) -> Glyph {
    match action {
        ResultAction::Invoke => primary,
        ResultAction::Hotkey => Glyph::ActionHotkey,
        ResultAction::Alias => Glyph::ActionAlias,
        ResultAction::Pin
        | ResultAction::Unpin
        | ResultAction::MovePinUp
        | ResultAction::MovePinDown => Glyph::ActionPin,
        ResultAction::ConfigureCommand | ResultAction::ConfigureExtension => Glyph::Sliders,
        ResultAction::DismissNotice => Glyph::Delete,
    }
}

/// The list's `child` of `listed` (see [`panel_children`]). `selected`
/// indexes `listed`; `attach` gives an available entry its handlers (the
/// launcher's pointer and click). An entry shows its keys in their caps'
/// style: the primary entry the invoke binding in the accent caps, as the
/// footer's button does.
pub(crate) fn panel_child(
    child: PanelChild,
    listed: &[PanelEntry],
    selected: usize,
    theme: &Theme,
    attach: impl FnOnce(Stateful<Div>, usize) -> Stateful<Div>,
) -> AnyElement {
    match child {
        PanelChild::Rule => rule(theme).into_any_element(),
        PanelChild::Group(index) => {
            let label = listed
                .get(index)
                .and_then(|entry| entry.section.clone())
                .unwrap_or_default();
            group_label(label, theme).into_any_element()
        }
        PanelChild::Entry(index) => {
            let Some(entry) = listed.get(index) else {
                return div().into_any_element();
            };
            let row = action_row(index, entry, index == selected, theme)
                .aria_position_in_set(index + 1)
                .aria_size_of_set(listed.len());
            let row = if entry.available {
                attach(row, index)
            } else {
                row
            };
            row.into_any_element()
        }
    }
}

/// An entry (`.arow`): 36 high, radius 8, 8px either side, its 16px glyph
/// in the icon gray, its 13px/450 label filling the row, and its keys at
/// the right in their caps' style; the 11% wash when selected, the 6% one
/// on hover. A destructive entry draws its glyph and label in the
/// destructive color, and says so to assistive technology; the keys are
/// also the row's shortcut there. An entry that opens a submenu ends in a
/// chevron (#140). A submenu's note (loading, or its error) is not dimmed
/// as an unavailable entry is, but runs nothing either.
pub(crate) fn action_row(
    index: usize,
    entry: &PanelEntry,
    selected: bool,
    theme: &Theme,
) -> Stateful<Div> {
    let geometry = &theme.geometry.actions;
    let glyph_of = entry.glyph;
    let label: SharedString = entry.label.clone().into();
    let keys = entry.keys.as_ref().map(|(keys, style)| (keys, *style));
    let (available, destructive) = (entry.available, entry.destructive);
    let note = entry.kind == EntryKind::Note;
    let description = match (destructive && !note, entry.submenu) {
        (true, true) => Some(format!("{DESTRUCTIVE}, {}", OPENS_SUBMENU.to_lowercase())),
        (true, false) => Some(DESTRUCTIVE.to_owned()),
        (false, true) => Some(OPENS_SUBMENU.to_owned()),
        (false, false) => None,
    };
    let debug = format!("action-{label}");
    div()
        .id(("action", index))
        .debug_selector(move || debug)
        .flex_none()
        .flex()
        .items_center()
        .gap(geometry.row_gap)
        .w_full()
        .h(geometry.row_height)
        .px(geometry.row_padding_x)
        .rounded(geometry.row_radius)
        .text_size(theme.typography.action_size)
        .font_weight(theme.typography.action_weight)
        .text_color(if destructive {
            theme.danger
        } else {
            theme.action_text
        })
        .role(Role::MenuItem)
        .aria_label(label.clone())
        .aria_selected(selected)
        .when_some(description, |row, description| {
            row.aria_description(description)
        })
        .when_some(keys, |row, (keys, _)| row.aria_keyshortcuts(keys.name()))
        .when(selected, |row| row.bg(theme.action_selected))
        .when(!selected && available, |row| {
            row.hover(|row| row.bg(theme.control_hover))
        })
        // While held, an available entry takes the stronger wash of its
        // hover, or of its selected wash, at once.
        .when(available, |row| {
            let press = pressed(if selected {
                theme.action_selected
            } else {
                theme.control_hover
            });
            row.active(move |row| row.bg(press))
        })
        // Disabled text is the ink at the tertiary strength (ADR 0035),
        // as the reference's disabled fields are; the whole row dims with
        // it, glyph and keys as one.
        .when(!available && !note, |row| row.opacity(TERTIARY_STRENGTH))
        .when(!available, |row| row.aria_disabled(true))
        .child(match &entry.icon {
            // The action's own icon (#139), at the glyph's size, its web
            // image or system icon as it is now (#142).
            Some(icon) => extension_icon::draw(
                &crate::features::icons::drawn(icon, theme),
                IconSize::small(geometry.glyph_size),
                ("action-icon", index),
                &format!("action-{label}"),
                theme,
            )
            .into_any_element(),
            None => glyph(
                glyph_of,
                geometry.glyph_size,
                if destructive {
                    theme.danger
                } else {
                    theme.action_icon
                },
            )
            .flex_none()
            .into_any_element(),
        })
        .child(div().flex_1().min_w(px(0.)).truncate().child(label))
        .when_some(keys, |row, (keys, style)| {
            row.child(key_sequence(keys, style, theme))
        })
        .when(entry.submenu, |row| {
            row.child(
                glyph(Glyph::ChevronRight, geometry.glyph_size, theme.action_icon).flex_none(),
            )
        })
}

/// A separator (`.sep`): a 1px rule with 4px above and below and 6px in
/// from either side.
pub(crate) fn rule(theme: &Theme) -> Div {
    let geometry = &theme.geometry.actions;
    div()
        .flex_none()
        .h(px(1.))
        .my(geometry.rule_margin_y)
        .mx(geometry.rule_margin_x)
        .bg(theme.separator)
}

/// A group label (`.alabel`): 26 high, its 11.5px/500 text at the bottom
/// with 8px either side and 4px below.
pub(crate) fn group_label(label: impl Into<SharedString>, theme: &Theme) -> Div {
    let geometry = &theme.geometry.actions;
    let label: SharedString = label.into();
    let debug = format!("action-group-{label}");
    div()
        .debug_selector(move || debug)
        .flex_none()
        .flex()
        .items_end()
        .h(geometry.group_height)
        .px(geometry.group_padding_x)
        .pb(geometry.group_padding_bottom)
        .text_size(theme.typography.action_group_size)
        // CSS's `normal` line for Geist: the text's bottom sits on the
        // label's bottom padding, as the reference's does.
        .line_height(theme.typography.action_group_size * theme.typography.line_height)
        .font_weight(theme.typography.medium)
        // The tertiary level: the Actions panel's groups are sections.
        .text_color(theme.text_tertiary)
        .child(label)
}

/// The panel's header: the target's icon in an 18px box (Pane's tile, or
/// an extension's or application's own icon drawn bare) and its title,
/// 12px/500 muted, 30 high with 8px above and 14px either side.
pub(crate) fn header(title: &str, icon: Option<&RowIcon>, theme: &Theme) -> Div {
    let geometry = &theme.geometry.actions;
    let icon = match icon {
        Some(icon) => extension_icon::row_icon_at(
            icon,
            TileSize::Mini,
            "actions-header-icon",
            "actions-header",
            theme,
        ),
        None => tile_at(TileSize::Mini, IconTone::Command, Glyph::Prompt, theme),
    };
    div()
        .debug_selector(|| "actions-header".into())
        .flex_none()
        .flex()
        .items_center()
        .gap(geometry.header_gap)
        .h(geometry.header_height)
        .pt(geometry.header_padding_top)
        .px(geometry.header_padding_x)
        .text_size(theme.typography.actions_header_size)
        .font_weight(theme.typography.medium)
        .text_color(theme.text_muted)
        .child(icon)
        .child(
            div()
                .min_w(px(0.))
                .truncate()
                .child(SharedString::from(title.to_owned())),
        )
}

/// The panel's search field in its row: 44 high, a rule above, the 15px
/// magnifier and the 13px field 10px after it, centered in the row.
///
/// For assistive technology the row is the field's node, as root search's
/// is: an editable combo box tracking `focus` (the filter's), with `query`
/// as its value. The focus stays there while the selection moves, and the
/// window's announcer says the selected entry (#132).
pub(crate) fn search_field(
    filter: &Entity<EditableTextState>,
    focus: &FocusHandle,
    query: String,
    theme: &Theme,
) -> Stateful<Div> {
    let geometry = &theme.geometry.actions;
    div()
        .id("actions-search")
        .debug_selector(|| "actions-search".into())
        .track_focus(focus)
        .role(Role::EditableComboBox)
        .aria_label(SEARCH_LABEL)
        .aria_value(query)
        .aria_placeholder(PLACEHOLDER)
        .flex_none()
        .flex()
        .items_center()
        .gap(geometry.search_gap)
        .h(geometry.search_height)
        .px(geometry.search_padding_x)
        .border_t_1()
        .border_color(theme.separator)
        .child(glyph(Glyph::Search, geometry.search_glyph_size, theme.text_muted).flex_none())
        .child(
            text_input("actions-filter")
                .state(filter.downgrade())
                .placeholder(PLACEHOLDER)
                .placeholder_color(theme.query_placeholder)
                .caret_color(theme.accent_text)
                .selection_color(theme.row_selected)
                .marked_color(theme.accent_text)
                .text_size(theme.typography.action_size)
                .text_color(theme.text_title)
                .font_family(theme.typography.family.clone())
                .font_features(theme.typography.features.clone())
                // Its own line's height, centered in the row: the
                // reference's 40px input centers its text the same way.
                // The input's own 2px inset, as root search's field has.
                .pl(theme.geometry.search_text_inset)
                .w_full()
                .min_w(px(0.))
                .whitespace_nowrap()
                .overflow_x_scroll(),
        )
}

/// The panel (`.pop`): the L2 popover, 320 wide, its header, its list (6px
/// padding, 1px between rows) — or `empty`'s note — and its search row,
/// with the reference's outer shadows: a 0.5px dark outline and the long
/// soft drop (`0 28px 70px -14px`), which darkens the footer under it.
pub(crate) fn popup(
    header: Option<Div>,
    rows: Option<AnyElement>,
    empty: Option<&'static str>,
    search: Stateful<Div>,
    theme: &Theme,
    material: Material,
) -> Stateful<Div> {
    let geometry = &theme.geometry.actions;
    let list = div()
        .id("actions-list")
        .debug_selector(|| "actions-list".into())
        .role(Role::Menu)
        .flex()
        .flex_col()
        // The entries' virtualized list pads itself (#165).
        .children(rows)
        .when_some(empty, |list, note| {
            list.child(
                div()
                    .m(geometry.list_padding)
                    .debug_selector(|| "actions-empty".into())
                    .py(geometry.empty_padding_y)
                    .px(geometry.empty_padding_x)
                    .text_size(theme.typography.action_size)
                    .line_height(theme.typography.action_size * theme.typography.line_height)
                    .text_color(theme.text_muted)
                    .child(note),
            )
        });
    let content = div()
        .flex()
        .flex_col()
        .when_some(header, |content, header| content.child(header))
        .child(list)
        .child(search);
    div()
        .id("actions-panel")
        .debug_selector(|| "actions-panel".into())
        .w(geometry.width)
        .occlude()
        .rounded(theme.geometry.popover_radius)
        .shadow(popover_shadows(theme))
        .child(material.popover(theme, content))
}

/// Places the panel over the footer strip, as the reference does: its
/// bottom 8px above the strip's top edge, its right edge 10px in.
pub(crate) fn anchored(panel: Stateful<Div>, theme: &Theme) -> Div {
    let geometry = &theme.geometry.actions;
    div()
        .absolute()
        .right(geometry.inset)
        .bottom(gpui::relative(1.))
        .pb(geometry.above_footer)
        .child(panel)
}

/// The dimmer over the results while the panel is open: the list's area
/// only, between the search header and the footer, never taking input.
pub(crate) fn dimmer(theme: &Theme) -> Div {
    div()
        .debug_selector(|| "actions-dimmer".into())
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .bg(theme.actions_dimmer)
}

/// Whether `screen` is an open command's list, whose items have actions.
pub(crate) fn commands_list(screen: &Screen) -> bool {
    matches!(screen, Screen::Command | Screen::CommandSearch { .. })
}

/// Whether the selected row of `screen` may have actions of its own, which
/// Enter, the action chords and shortcuts run: an open command's list, or
/// root search, whose files and computed answers have Pane's own (#150).
pub(crate) fn item_list(screen: &Screen) -> bool {
    commands_list(screen) || matches!(screen, Screen::Root { .. })
}

/// The invoke binding's keys as they are bound now: the primary action's.
fn invoke_keys(cx: &App) -> KeySequence {
    let invoke = crate::settings::shared(cx)
        .read(cx)
        .keyboard()
        .binding(KeyboardAction::InvokeSelectedAction)
        .clone();
    crate::keyboard::binding_keys(&invoke)
}
