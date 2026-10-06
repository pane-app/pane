//! The contextual Actions panel: the reference's searchable menu of what
//! can be done with root search's selected result, opened over the footer
//! by its Actions button or the Open actions binding (Ctrl+K by default).
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
//! runs the entry clicked, and Escape (or Tab) closes the panel only,
//! giving focus back to the query field. A mouse-down outside the panel
//! closes it and is consumed, so the result it covered is never invoked.
//! The panel opens and closes at once: the reference authors no motion
//! for it.

use gpui::{
    AnyElement, App, ClickEvent, Context, Div, Entity, FocusHandle, Focusable, KeyBinding,
    MouseDownEvent, MouseMoveEvent, Role, SharedString, Stateful, Subscription, Window, actions,
    div, prelude::*, px,
};
use gpui_elements::editable_text::actions::DEFAULT_INPUT_CONTEXT;
use gpui_elements::editable_text::{EditableTextState, StringStorage, TextChanged, text_input};
use pane_core::{
    KeyboardAction, PinnedLayout, ResultAction, ResultActionItem, ResultActions, RowKind, Screen,
    SlotChange,
};

use crate::app::{LauncherWindow, row_icon};
use crate::features::quick_slots;
use crate::ui::icon::{Glyph, IconTone, TileSize, glyph, tile_at};
use crate::ui::input::TextEditingKeys;
use crate::ui::keycap::{CapStyle, KeySequence, key_sequence};
use crate::ui::material::{Material, popover_shadows};
use crate::ui::theme::{Theme, pressed};

actions!(
    actions_panel,
    [NextAction, PreviousAction, ChooseAction, CloseActions]
);

/// The panel's key context.
pub(crate) const CONTEXT: &str = "ActionsPanel";

/// The search field's placeholder, the reference's.
pub(crate) const PLACEHOLDER: &str = "Search actions…";
/// What the list says when the filter leaves nothing, the reference's.
pub(crate) const NO_MATCH: &str = "No actions match";
/// What the list says when no result is selected: nothing to act on.
pub(crate) const NOTHING_SELECTED: &str = "Select a result to see its actions";
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
            ResultAction::Invoke | ResultAction::Hotkey | ResultAction::Alias => None,
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
/// field's own caret keys, and Enter, Escape and Tab in the panel, above
/// the launcher's confirm, back and focus traversal.
pub(crate) fn bind_keys(cx: &mut App, _: &TextEditingKeys) {
    let field = format!("{CONTEXT} > {DEFAULT_INPUT_CONTEXT}");
    cx.bind_keys([
        KeyBinding::new("down", NextAction, Some(&field)),
        KeyBinding::new("up", PreviousAction, Some(&field)),
        KeyBinding::new("enter", ChooseAction, Some(CONTEXT)),
        KeyBinding::new("escape", CloseActions, Some(CONTEXT)),
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
    /// What had focus when the panel opened, restored when it closes.
    restore: Option<FocusHandle>,
    _filtering: Subscription,
    /// Closes the panel when the window loses activation.
    _deactivation: Subscription,
}

struct Opened {
    actions: ResultActions,
    /// The target's kind: an application's primary action opens it.
    kind: Option<RowKind>,
    /// What the entries act on.
    subject: Subject,
}

/// What the panel's entries act on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Subject {
    /// Root search's selected result.
    Result,
    /// The quick slot holding the target.
    Slot,
}

impl Subject {
    /// The label over the entries after the primary action.
    fn group(self) -> &'static str {
        match self {
            Subject::Result => PANE_GROUP,
            Subject::Slot => SLOT_GROUP,
        }
    }
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
    /// a quick slot has focus, for that slot — with focus in its search
    /// field. Only root search has Actions.
    pub(crate) fn open_actions(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(slot) = self.focused_slot(window) {
            self.open_slot_actions(slot, window, cx);
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
                actions,
                kind,
                subject: Subject::Result,
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
        let opened = self
            .launcher
            .quick_slot_actions(&target)
            .map(|actions| Opened {
                actions,
                kind: slot.kind,
                subject: Subject::Slot,
            });
        if opened.is_some() {
            self.open_panel(opened, window, cx);
        }
    }

    /// Opens the panel over `opened`, with focus in its search field.
    fn open_panel(&mut self, opened: Option<Opened>, window: &mut Window, cx: &mut Context<Self>) {
        if self.actions.is_some() || !matches!(self.launcher.view().screen, Screen::Root { .. }) {
            return;
        }
        self.close_open_menu(window, cx);
        let filter = cx.new(|cx| EditableTextState::new(StringStorage::default(), cx));
        let filtering = cx.subscribe(&filter, |this, _, _: &TextChanged, cx| {
            if let Some(panel) = this.actions.as_mut() {
                panel.selected = 0;
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
            restore,
            _filtering: filtering,
            _deactivation: deactivation,
        });
        cx.notify();
    }

    /// Closes the Actions panel, if it is open, giving focus back to what
    /// had it. Whether it was open.
    pub(crate) fn close_actions(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(panel) = self.actions.take() else {
            return false;
        };
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
    fn live_actions(&self, cx: &App) -> Option<ResultActions> {
        let panel = self.actions.as_ref()?.opened.as_ref()?;
        let opened = &panel.actions;
        let live = match panel.subject {
            Subject::Result => self.launcher.result_actions(),
            Subject::Slot => self.launcher.quick_slot_actions(&opened.target),
        };
        let actions = match live {
            Some(live) if live.target == opened.target => live,
            _ => ResultActions {
                items: opened
                    .items
                    .iter()
                    .map(|item| ResultActionItem {
                        available: false,
                        ..item.clone()
                    })
                    .collect(),
                ..opened.clone()
            },
        };
        Some(laid_out(actions, pins_horizontal(cx)))
    }

    /// What the filter lists now.
    fn listed(&self, cx: &App) -> Vec<ResultActionItem> {
        let (Some(panel), Some(actions)) = (self.actions.as_ref(), self.live_actions(cx)) else {
            return Vec::new();
        };
        actions
            .matching(&panel.query(cx))
            .into_iter()
            .cloned()
            .collect()
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
            cx.notify();
        }
    }

    fn actions_choose(&mut self, _: &ChooseAction, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(index) = self.actions.as_ref().map(|panel| panel.selected) {
            self.run_action(index, window, cx);
        }
    }

    fn actions_close(&mut self, _: &CloseActions, window: &mut Window, cx: &mut Context<Self>) {
        self.close_actions(window, cx);
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
            .map(|opened| (opened.actions.target.clone(), opened.subject));
        let action = self.listed(cx).get(index).map(|item| item.action);
        let (Some((target, subject)), Some(action)) = (opened, action) else {
            return;
        };
        let ready = match subject {
            Subject::Result => self.launcher.result_action_ready(&target, action),
            Subject::Slot => self.launcher.quick_slot_action_ready(&target, action),
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
    /// right edge 10px in from the window's.
    pub(crate) fn render_actions_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let panel = self.actions.as_ref()?;
        let visuals = crate::settings::launcher_visuals(cx);
        let theme = &visuals.theme;
        let opened = panel.opened.as_ref();
        let listed = self.listed(cx);
        let filtering = !panel.query(cx).trim().is_empty();
        let invoke = crate::settings::shared(cx)
            .read(cx)
            .keyboard()
            .binding(KeyboardAction::InvokeSelectedAction)
            .clone();
        let invoke = crate::keyboard::binding_keys(&invoke);
        let slot_keys = SlotKeys::new(pins_horizontal(cx));
        let surface = compose(
            PanelView {
                target: opened.map(|opened| (&opened.actions, opened.kind)),
                icon: opened.map(|opened| row_icon(&opened.actions.target)),
                listed: &listed,
                group: opened.map_or(PANE_GROUP, |opened| opened.subject.group()),
                filtering,
                selected: panel.selected,
                invoke: &invoke,
                slot_keys: Some(&slot_keys),
                filter: &panel.filter,
            },
            theme,
            visuals.material,
            |row, index| {
                row.on_mouse_move(cx.listener(move |this, _: &MouseMoveEvent, _, cx| {
                    if let Some(panel) = this.actions.as_mut()
                        && panel.selected != index
                    {
                        panel.selected = index;
                        cx.notify();
                    }
                }))
                .on_click(cx.listener(
                    move |this, _: &ClickEvent, window, cx| {
                        this.run_action(index, window, cx);
                    },
                ))
            },
        );
        let surface = surface
            .key_context(CONTEXT)
            .on_action(cx.listener(Self::actions_next))
            .on_action(cx.listener(Self::actions_previous))
            .on_action(cx.listener(Self::actions_choose))
            .on_action(cx.listener(Self::actions_close))
            .on_mouse_down_out(cx.listener(|this, _: &MouseDownEvent, window, cx| {
                this.close_actions(window, cx);
                cx.stop_propagation();
            }))
            .role(Role::Dialog)
            .aria_label(match opened {
                Some(opened) => format!("Actions for {}", opened.actions.title),
                None => "Actions".to_owned(),
            });
        Some(anchored(surface, theme).into_any_element())
    }
}

/// What the panel shows: what [`compose`] draws.
pub(crate) struct PanelView<'a> {
    /// The target's actions and kind; `None` with nothing selected.
    pub(crate) target: Option<(&'a ResultActions, Option<RowKind>)>,
    /// The target's tile, as its row draws it.
    pub(crate) icon: Option<(IconTone, Glyph)>,
    /// What the filter lists now.
    pub(crate) listed: &'a [ResultActionItem],
    /// The label over the entries after the primary action: "Pane" over
    /// a result's, or the quick slot labels over a slot's.
    pub(crate) group: &'static str,
    /// Whether the filter holds text, which drops the group's separator
    /// and label as the reference's does.
    pub(crate) filtering: bool,
    /// The selected entry, an index into `listed`.
    pub(crate) selected: usize,
    /// The invoke binding's keys, the primary entry's.
    pub(crate) invoke: &'a KeySequence,
    /// The quick slot entries' keys; `None` draws them without.
    pub(crate) slot_keys: Option<&'a SlotKeys>,
    /// The search field's text.
    pub(crate) filter: &'a Entity<EditableTextState>,
}

/// The panel as `view` describes it: the header (the target's tile and
/// title), the entries — or the note saying why there are none — and the
/// search row, in the L2 popover. `attach` gives each available entry its
/// handlers.
pub(crate) fn compose(
    view: PanelView,
    theme: &Theme,
    material: Material,
    attach: impl Fn(Stateful<Div>, usize) -> Stateful<Div>,
) -> Stateful<Div> {
    let rows = list_children(
        view.listed,
        view.group,
        view.filtering,
        view.selected,
        primary_glyph(view.target.and_then(|(_, kind)| kind)),
        view.invoke,
        view.slot_keys,
        theme,
        attach,
    );
    let empty = match (view.target, view.listed.is_empty()) {
        (None, _) => Some(NOTHING_SELECTED),
        (Some(_), true) => Some(NO_MATCH),
        (Some(_), false) => None,
    };
    let header = view
        .target
        .map(|(actions, _)| header(&actions.title, view.icon, theme));
    popup(
        header,
        rows,
        empty,
        search_field(view.filter, theme),
        theme,
        material,
    )
}

/// The entry after (or before) `from` in `listed` that can run, if any.
pub(crate) fn next_available(
    listed: &[ResultActionItem],
    from: usize,
    forward: bool,
) -> Option<usize> {
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
    /// The "Pane" group label.
    Group,
    /// The listed entry at this index.
    Entry(usize),
}

/// The list's children for `listed`: the primary action, then — unless
/// the filter is narrowing them, as the reference drops its separators
/// then — a rule and the "Pane" label over the command's configuration.
pub(crate) fn panel_children(listed: &[ResultActionItem], filtering: bool) -> Vec<PanelChild> {
    let mut children = Vec::new();
    let mut grouped = false;
    for (index, item) in listed.iter().enumerate() {
        if item.action != ResultAction::Invoke && !grouped && !filtering {
            grouped = true;
            if index > 0 {
                children.push(PanelChild::Rule);
            }
            children.push(PanelChild::Group);
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
    }
}

/// The list's children for `listed`: the primary action, then — unless
/// the filter is narrowing them, as the reference drops its separators
/// then — a rule and the "Pane" label over the command's configuration.
/// `selected` indexes `listed`; `attach` gives each row its handlers (the
/// launcher's pointer and click). The primary entry
/// shows the invoke binding in the accent caps, as the footer's button
/// does; the quick slot entries show `slot_keys` in the regular caps,
/// when given; the configuration has no keys of its own.
#[allow(clippy::too_many_arguments)]
pub(crate) fn list_children(
    listed: &[ResultActionItem],
    group: &'static str,
    filtering: bool,
    selected: usize,
    primary: Glyph,
    invoke: &KeySequence,
    slot_keys: Option<&SlotKeys>,
    theme: &Theme,
    attach: impl Fn(Stateful<Div>, usize) -> Stateful<Div>,
) -> Vec<AnyElement> {
    panel_children(listed, filtering)
        .into_iter()
        .map(|child| match child {
            PanelChild::Rule => rule(theme).into_any_element(),
            PanelChild::Group => group_label(group, theme).into_any_element(),
            PanelChild::Entry(index) => {
                let item = &listed[index];
                let keys = match item.action {
                    ResultAction::Invoke => Some((invoke, CapStyle::Accent)),
                    action => slot_keys
                        .and_then(|keys| keys.of(action))
                        .map(|keys| (keys, CapStyle::Regular)),
                };
                let row = action_row(
                    index,
                    action_glyph(item.action, primary),
                    item.label.clone(),
                    keys,
                    index == selected,
                    item.available,
                    theme,
                );
                let row = if item.available {
                    attach(row, index)
                } else {
                    row
                };
                row.into_any_element()
            }
        })
        .collect()
}

/// An entry (`.arow`): 36 high, radius 8, 8px either side, its 16px glyph
/// in the icon gray, its 13px/450 label filling the row, and its keys at
/// the right in their caps' style; the 11% wash when selected, the 6% one
/// on hover.
pub(crate) fn action_row(
    index: usize,
    glyph_of: Glyph,
    label: impl Into<SharedString>,
    keys: Option<(&KeySequence, CapStyle)>,
    selected: bool,
    available: bool,
    theme: &Theme,
) -> Stateful<Div> {
    let geometry = &theme.geometry.actions;
    let label: SharedString = label.into();
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
        .text_color(theme.action_text)
        .role(Role::MenuItem)
        .aria_label(label.clone())
        .aria_selected(selected)
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
        .when(!available, |row| row.opacity(0.5).aria_disabled(true))
        .child(glyph(glyph_of, geometry.glyph_size, theme.action_icon).flex_none())
        .child(div().flex_1().min_w(px(0.)).truncate().child(label))
        .when_some(keys, |row, (keys, style)| {
            row.child(key_sequence(keys, style, theme))
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
        .bg(theme.action_rule)
}

/// A group label (`.alabel`): 26 high, its 11.5px/500 text at the bottom
/// with 8px either side and 4px below.
pub(crate) fn group_label(label: &'static str, theme: &Theme) -> Div {
    let geometry = &theme.geometry.actions;
    div()
        .debug_selector(move || format!("action-group-{label}"))
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
        .text_color(theme.text_muted)
        .child(label)
}

/// The panel's header: the target's 18px tile and its title, 12px/500
/// muted, 30 high with 8px above and 14px either side.
pub(crate) fn header(title: &str, icon: Option<(IconTone, Glyph)>, theme: &Theme) -> Div {
    let geometry = &theme.geometry.actions;
    let (tone, glyph_of) = icon.unwrap_or((IconTone::Command, Glyph::Prompt));
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
        .child(tile_at(TileSize::Mini, tone, glyph_of, theme))
        .child(
            div()
                .min_w(px(0.))
                .truncate()
                .child(SharedString::from(title.to_owned())),
        )
}

/// The panel's search field in its row: 44 high, a rule above, the 15px
/// magnifier and the 13px field 10px after it, centered in the row.
pub(crate) fn search_field(filter: &Entity<EditableTextState>, theme: &Theme) -> Div {
    let geometry = &theme.geometry.actions;
    div()
        .debug_selector(|| "actions-search".into())
        .flex_none()
        .flex()
        .items_center()
        .gap(geometry.search_gap)
        .h(geometry.search_height)
        .px(geometry.search_padding_x)
        .border_t_1()
        .border_color(theme.action_rule)
        .child(glyph(Glyph::Search, geometry.search_glyph_size, theme.text_muted).flex_none())
        .child(
            text_input("actions-filter")
                .state(filter.downgrade())
                .placeholder(PLACEHOLDER)
                .placeholder_color(theme.text_placeholder)
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
    rows: Vec<AnyElement>,
    empty: Option<&'static str>,
    search: Div,
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
        .gap(geometry.list_gap)
        .p(geometry.list_padding)
        .children(rows)
        .when_some(empty, |list, note| {
            list.child(
                div()
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
