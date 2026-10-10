//! Root search's pinned home: the pins (quick slots) above a blank query's
//! results, as the launcher window draws and drives them.
//!
//! What the pins are, in order, and whether each can run is the core's
//! ([`pane_core::Launcher::quick_slots`]): a list without gaps or length
//! limit. This module draws them with the shared visuals
//! ([`crate::ui::pinned`]) and wires them:
//!
//! - **The home shows** while root search's trimmed query is blank — the
//!   "Pinned" label and the pins above the rows — and a query hides it;
//!   clearing the query brings it back. When root search comes on screen,
//!   the core is asked for the indexed results its home lists below the
//!   pins, if it has not listed them since root search was last shown
//!   ([`pane_core::Launcher::resolve_root_home`]).
//! - **The layout** is the Launcher page's choice: a strip of tiles
//!   (horizontal), five to a row and wrapping onto more, with the pin hint
//!   after the last pin while its cell is in the last row; or result rows
//!   of the pinned results only (vertical), nothing at all while nothing is
//!   pinned.
//! - **A slot is invoked** by a click, by Enter or Space while it has
//!   focus, or — one of the first [`NUMBERED_PINS`] — by its chord: Ctrl
//!   and its place among the pins, local to the root search field and the
//!   slots, never registered with the system; the next digits pick the
//!   first rows below them, up to 9 (see [`crate::features::number_hints::numbered`]), and
//!   holding Ctrl shows each numbered item's number. A pin past them has
//!   no number. A chord acts only on root search, with no overlay (the
//!   Actions panel, the Pane menu) open and no input-method composition in
//!   the query field; the core then runs the slot's target only if it
//!   resolves to one that can run and no action is already running, and
//!   otherwise says why. Each press runs once: a held chord's repeats and a
//!   double click's second click run nothing more. A click leaves focus in
//!   the query field.
//! - **Pinning and arranging** have the launcher's own fixed keys, under
//!   the same conditions as a chord: Ctrl+Shift+F (Command+Shift+F on
//!   macOS) unpins the slot with focus, or else pins root search's
//!   selected result — or unpins it, once it is pinned; Ctrl+Alt
//!   (Command+Option) and Up or Left moves the slot with focus one place
//!   earlier, Down or Right one place later, and focus follows it.
//! - **A slot's own actions** — invoking it, unpinning it, moving it —
//!   open in the Actions panel from a secondary click on it, or from the
//!   Open actions binding while it has focus (see
//!   [`crate::features::actions_panel`]).
//!
//! The slots are tab stops after the query field, so the keyboard reaches
//! every one of them; the pin hint is not one — it only says how to pin.
//!
//! A pin whose result shares its title with another its command lists
//! (two applications of one name) says what tells it apart
//! ([`pane_core::QuickSlot::detail`]): as its tile's tooltip, its row's
//! subtitle, and its accessible description ([`slot_description`]).

use gpui::{
    AnyElement, App, ClickEvent, Context, Div, EntityInputHandler, FocusHandle, Focusable,
    KeyBinding, KeyDownEvent, Keystroke, MouseButton, MouseDownEvent, Role, Stateful, Window,
    actions, prelude::*,
};
use pane_core::{
    KeyboardAction, LauncherView, PinnedLayout, QuickSlot, ResultAction, Screen, SlotChange,
};

use crate::app::{KEY_CONTEXT, LauncherWindow};
use crate::ui::extension_icon::RowIcon;
use crate::ui::pinned::{
    HOME_CHILDREN, PIN_HINT, SlotContent, home, home_rows, pin_hint, pinned_slot, shows_pin_hint,
};
use crate::ui::result_row::{RowContent, RowMeta, result_row_with};
use crate::ui::theme::Theme;
use crate::ui::tooltip::{TooltipLook, text_tooltip};

actions!(
    quick_slots,
    [PressSlot, TogglePin, MovePinEarlier, MovePinLater]
);

/// A focused slot's key context.
const SLOT_CONTEXT: &str = "QuickSlot";

/// How many pins Ctrl and a digit number: the first five, Ctrl+1 to
/// Ctrl+5. The digits after the numbered pins go to the rows below them.
pub(crate) const NUMBERED_PINS: usize = 5;

/// Registers the slots' keys: Enter and Space press a focused slot, above
/// the launcher's confirm; the pin key ([`crate::keyboard::toggle_pin_binding`])
/// anywhere in the launcher, and the move keys
/// ([`crate::keyboard::move_pin_bindings`]) on a focused slot. The slots'
/// chords are read from the key presses themselves (see
/// [`LauncherWindow::on_quick_slot_keys`]), so a held chord's repeats are
/// told from new presses.
pub(crate) fn bind_keys(cx: &mut App) {
    let toggle = crate::keyboard::toggle_pin_binding().id();
    let mut bindings = vec![
        KeyBinding::new("enter", PressSlot, Some(SLOT_CONTEXT)),
        KeyBinding::new("space", PressSlot, Some(SLOT_CONTEXT)),
        KeyBinding::new(&toggle, TogglePin, Some(KEY_CONTEXT)),
    ];
    for binding in crate::keyboard::move_pin_bindings(true) {
        bindings.push(KeyBinding::new(
            &binding.id(),
            MovePinEarlier,
            Some(SLOT_CONTEXT),
        ));
    }
    for binding in crate::keyboard::move_pin_bindings(false) {
        bindings.push(KeyBinding::new(
            &binding.id(),
            MovePinLater,
            Some(SLOT_CONTEXT),
        ));
    }
    cx.bind_keys(bindings);
}

/// The digit whose chord `keystroke` is (Ctrl and a digit from 1 to 9,
/// with no other modifier), if it is one.
fn chord_digit(keystroke: &Keystroke) -> Option<usize> {
    let pressed = crate::keyboard::binding_of(keystroke).ok()?;
    (1..=9).find(|&number| crate::keyboard::quick_slot_binding(number) == pressed)
}

/// The number Ctrl picks the pin at `index` with — its place, from 1 —
/// if it is one of the numbered pins.
fn slot_number(index: usize) -> Option<usize> {
    (index < NUMBERED_PINS).then_some(index + 1)
}

/// The window's own state for the home: each slot's focus, and whether
/// root search was on screen when the screen last changed.
#[derive(Default)]
pub(crate) struct Home {
    /// One focus per pin, each a tab stop: grown as pins are added (see
    /// [`LauncherWindow::ensure_slot_focus`]) and never shrunk, so a slot
    /// keeps its focus as pins come and go; a handle past the pins is
    /// drawn by nothing.
    focus: Vec<FocusHandle>,
    /// Whether root search was on screen at the last sync.
    on_root: bool,
}

/// Whether `view` shows the home: root search with a blank trimmed query.
pub(crate) fn home_shown(view: &LauncherView) -> bool {
    matches!(&view.screen, Screen::Root { query } if query.trim().is_empty())
}

/// What the pin hint tells assistive technology, with the pin key and the
/// Open actions binding in force.
fn pin_hint_description(toggle_pin: &str, open_actions: &str) -> String {
    format!(
        "To pin a search result, select it and press {toggle_pin}, or press {open_actions} and \
         choose Pin"
    )
}

/// The icon `slot` shows: its row's, by its target's identity — an
/// installed command's own icon (#139), else Pane's tile.
fn slot_icon(launcher: &pane_core::Launcher, slot: &QuickSlot, theme: &Theme) -> RowIcon {
    crate::features::icons::row_icon_of(launcher, &slot.target.key(), theme)
}

/// What assistive technology says of `slot` after its name: what tells it
/// apart from a result of the same title (its tooltip), then why it cannot
/// run; `None` when there is neither.
pub(crate) fn slot_description(slot: &QuickSlot) -> Option<String> {
    let parts: Vec<&str> = [slot.detail.as_deref(), slot.unavailable.as_deref()]
        .into_iter()
        .flatten()
        .collect();
    (!parts.is_empty()).then(|| parts.join(". "))
}

/// `element`, the slot at `index` showing `slot`, as assistive technology
/// sees it, tile or row alike: a button named "Pinned N: <title>", N its
/// place among the pins from 1, saying what tells it apart from a result of
/// the same title and why it cannot run, with its chord while it is a
/// numbered pin.
fn slot_accessibility(index: usize, slot: &QuickSlot, element: Stateful<Div>) -> Stateful<Div> {
    let shortcut = slot_number(index).map(|number| crate::keyboard::quick_slot_keys(number).name());
    element
        .role(Role::Button)
        .aria_label(format!("Pinned {}: {}", index + 1, slot.title))
        .when_some(slot_description(slot), |element, description| {
            element.aria_description(description)
        })
        .when_some(shortcut, |element, shortcut| {
            element.aria_keyshortcuts(shortcut)
        })
        .when(!slot.ready(), |element| element.aria_disabled(true))
}

impl LauncherWindow {
    /// Follows the launcher's screen: every pin gets its focus, and when
    /// root search comes on screen, the results its home lists below the
    /// pins — the indexed results, asked for if they have not been since
    /// root search was last shown (#199) — are asked for, and the window
    /// redraws once they are listed.
    pub(crate) fn sync_home(&mut self, cx: &mut Context<Self>) {
        let pins = self.launcher.quick_slots().len();
        self.ensure_slot_focus(pins, cx);
        let on_root = matches!(self.launcher.screen(), Screen::Root { .. });
        let was = std::mem::replace(&mut self.home.on_root, on_root);
        if on_root && !was {
            let resolving = self.launcher.resolve_root_home();
            cx.spawn(async move |this, cx| {
                resolving.await;
                this.update(cx, |_, cx| cx.notify()).ok();
            })
            .detach();
        }
    }

    /// Gives each of `pins` pins its focus, a tab stop, creating the ones
    /// the pins added since outnumber.
    pub(crate) fn ensure_slot_focus(&mut self, pins: usize, cx: &App) {
        while self.home.focus.len() < pins {
            self.home.focus.push(cx.focus_handle().tab_stop(true));
        }
    }

    /// The slot with keyboard focus, if one has it.
    pub(crate) fn focused_slot(&self, window: &Window) -> Option<usize> {
        self.home
            .focus
            .iter()
            .position(|handle| handle.is_focused(window))
    }

    /// Moves keyboard focus to the slot at `index`.
    pub(crate) fn focus_slot(&self, index: usize, window: &mut Window, cx: &mut App) {
        if let Some(focus) = self.home.focus.get(index) {
            window.focus(focus, cx);
        }
    }

    /// Invokes the slot at `index` through the core, which resolves it
    /// again and runs it only if it can (see the module docs). An index
    /// past the pins is a true no-op: nothing is dispatched and nothing
    /// redraws.
    pub(crate) fn activate_quick_slot(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if index >= self.launcher.quick_slots().len() {
            return;
        }
        let pending = self.launcher.activate_quick_slot(index);
        self.navigate_forward(window, cx);
        self.show_until_done(pending, window, cx);
    }

    /// Whether the slots' keys act now — a chord, the pin key, the move
    /// keys: only on root search, with no overlay open and no composition
    /// in the query field.
    fn slot_keys_act(&self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if !matches!(self.launcher.screen(), Screen::Root { .. })
            || self.actions.is_some()
            || self.menu.is_some()
        {
            return false;
        }
        let query = self.query_field();
        let composing = query.update(cx, |query, cx| query.marked_text_range(window, cx));
        composing.is_none()
    }

    /// A slot's chord: it acts only when the slots' keys do (see
    /// `slot_keys_act`).
    pub(crate) fn press_quick_slot(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.slot_keys_act(window, cx) {
            self.activate_quick_slot(index, window, cx);
        }
    }

    /// Changes the quick slots as `action` asks for `target` through the
    /// core — root search's selected row, which [`ResultAction::Pin`] pins
    /// and [`ResultAction::Unpin`] unpins, or the slot holding it, which
    /// the moves move — and shows the change recorded. While a slot has
    /// keyboard focus, focus follows what it holds through the change (see
    /// `follow_focused_slot`). What the change did.
    pub(crate) fn change_quick_slot(
        &mut self,
        target: &str,
        action: ResultAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> SlotChange {
        // The slot with focus, by its place and what it holds.
        let focused = self.focused_slot(window).and_then(|index| {
            let slot = self.launcher.quick_slots().into_iter().nth(index)?;
            Some((index, slot.target.key()))
        });
        let (change, recording) = self.launcher.change_quick_slots(target, action);
        if let Some((index, key)) = focused
            && matches!(change, SlotChange::Changed(_))
        {
            self.follow_focused_slot(index, &key, window, cx);
        }
        self.show_until_done(recording, window, cx);
        change
    }

    /// Keeps keyboard focus with the slots after a change, for the slot
    /// that had it at `index`, holding the target with key `key`: focus
    /// follows that target to its place now — a moved slot's new place, or
    /// one place earlier when a pin before it was unpinned; when it was the
    /// one unpinned, focus goes to the slot that took its place, else to
    /// the last pin, else — nothing pinned any more — to the query field.
    fn follow_focused_slot(
        &mut self,
        index: usize,
        key: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(now) = self.launcher.quick_slot_of(key) {
            self.focus_slot(now, window, cx);
            return;
        }
        let pins = self.launcher.quick_slots().len();
        if pins == 0 {
            self.query.focus(window, cx);
        } else {
            self.focus_slot(index.min(pins - 1), window, cx);
        }
    }

    /// The pin key ([`crate::keyboard::toggle_pin_binding`]): it unpins
    /// the slot with focus; with none, it pins root search's selected
    /// result, or unpins it once it is pinned — each only when the core
    /// has that action ready, and never while the window is collapsed,
    /// which hides the rows. It acts only when the slots' keys do.
    fn toggle_pin(&mut self, _: &TogglePin, window: &mut Window, cx: &mut Context<Self>) {
        if !self.slot_keys_act(window, cx) {
            return;
        }
        if let Some(index) = self.focused_slot(window) {
            let Some(slot) = self.launcher.quick_slots().into_iter().nth(index) else {
                return;
            };
            let target = slot.target.key();
            if self
                .launcher
                .quick_slot_action_ready(&target, ResultAction::Unpin)
            {
                self.change_quick_slot(&target, ResultAction::Unpin, window, cx);
            }
            return;
        }
        if self.is_collapsed() {
            return;
        }
        let Some(actions) = self.launcher.result_actions() else {
            return;
        };
        let action = if self.launcher.quick_slot_of(&actions.target).is_some() {
            ResultAction::Unpin
        } else {
            ResultAction::Pin
        };
        if self.launcher.result_action_ready(&actions.target, action) {
            self.change_quick_slot(&actions.target, action, window, cx);
        }
    }

    /// A move key on a focused slot: one place earlier.
    fn move_pin_earlier(
        &mut self,
        _: &MovePinEarlier,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_focused_pin(ResultAction::MovePinUp, window, cx);
    }

    /// A move key on a focused slot: one place later.
    fn move_pin_later(&mut self, _: &MovePinLater, window: &mut Window, cx: &mut Context<Self>) {
        self.move_focused_pin(ResultAction::MovePinDown, window, cx);
    }

    /// Moves the slot with focus as `action` asks, when the core has it
    /// ready (a move past either end is not), focus following it. It acts
    /// only when the slots' keys do.
    fn move_focused_pin(
        &mut self,
        action: ResultAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.slot_keys_act(window, cx) {
            return;
        }
        let Some(index) = self.focused_slot(window) else {
            return;
        };
        let Some(slot) = self.launcher.quick_slots().into_iter().nth(index) else {
            return;
        };
        let target = slot.target.key();
        if self.launcher.quick_slot_action_ready(&target, action) {
            self.change_quick_slot(&target, action, window, cx);
        }
    }

    /// A click on the slot at `index`: it runs what the slot holds, and
    /// focus stays in the query field, as typing expects it. A double
    /// click's second click runs nothing more.
    fn click_quick_slot(
        &mut self,
        index: usize,
        event: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.actions.is_some() || self.menu.is_some() || event.click_count() > 1 {
            return;
        }
        self.activate_quick_slot(index, window, cx);
        self.arm_arrival();
        if matches!(self.launcher.screen(), Screen::Root { .. }) {
            self.query.focus(window, cx);
        }
    }

    /// A key pressed in the launcher, before the focused control sees
    /// it: Ctrl and a digit, while the search field, a slot or a command's
    /// list has focus (an overlay's own field never does), picks what that
    /// number names (see [`crate::features::number_hints::numbered`]) — once per press: the
    /// system's repeats of a held chord run nothing more.
    fn quick_slot_chord(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(digit) = chord_digit(&event.keystroke) else {
            // Another key while Ctrl is held: a chord, not a look at the
            // numbers.
            self.chord_pressed();
            return;
        };
        let field = self.query_field().focus_handle(cx).is_focused(window);
        let list = self.focus_handle.is_focused(window);
        if !field && !list && self.focused_slot(window).is_none() {
            return;
        }
        if self.number_target(digit).is_none() {
            return;
        }
        cx.stop_propagation();
        if event.is_held {
            return;
        }
        // The chord waits for the current query's list to be published
        // (#203), as Enter does, and is replayed through this same path —
        // applied to the row the published list selects.
        if self.hold_key(event.keystroke.clone(), window, cx) {
            return;
        }
        self.pick_number(digit, window, cx);
    }

    /// `content`, the launcher's root, handling the slots' chords, the pin
    /// key and the move keys.
    pub(crate) fn on_quick_slot_keys(content: Div, cx: &mut Context<Self>) -> Div {
        content
            .capture_key_down(cx.listener(Self::quick_slot_chord))
            .on_action(cx.listener(Self::toggle_pin))
            .on_action(cx.listener(Self::move_pin_earlier))
            .on_action(cx.listener(Self::move_pin_later))
    }

    /// The slots Ctrl and a digit number while the home shows, in order:
    /// the first [`NUMBERED_PINS`] pins, on the strip as in the vertical
    /// list. A pin past them has no number; the digits after the numbered
    /// pins go to the rows.
    pub(crate) fn numbered_slots(&self) -> Vec<usize> {
        let pins = self.launcher.quick_slots().len();
        (0..pins.min(NUMBERED_PINS)).collect()
    }

    /// How many of the result list's children the home puts above its
    /// rows as `view` shows it: the label and the strip (one element,
    /// however many rows it wraps onto), or the label and a row per pin
    /// (none while nothing is pinned).
    pub(crate) fn home_children(&self, view: &LauncherView, cx: &App) -> usize {
        if !home_shown(view) {
            return 0;
        }
        match crate::settings::shared(cx).read(cx).pinned_layout() {
            PinnedLayout::Horizontal => HOME_CHILDREN,
            PinnedLayout::Vertical => {
                let pinned = self.launcher.quick_slots().len();
                if pinned == 0 { 0 } else { pinned + 1 }
            }
        }
    }

    /// The home's children of the result list — the "Pinned" label and the
    /// strip of every pin with the pin hint after them when it shows, or a
    /// row per pin — when `view` shows the home; `None` otherwise. Every
    /// pin gets its focus first. `numbers` is the number hints' look.
    pub(crate) fn render_home(
        &mut self,
        view: &LauncherView,
        numbers: f32,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Option<Vec<AnyElement>> {
        if !home_shown(view) {
            return None;
        }
        let pins = self.launcher.quick_slots();
        let count = pins.len();
        self.ensure_slot_focus(count, cx);
        let vertical =
            crate::settings::shared(cx).read(cx).pinned_layout() == PinnedLayout::Vertical;
        let mut tiles: Vec<AnyElement> = pins
            .into_iter()
            .enumerate()
            .map(|(index, slot)| {
                let number = slot_number(index)
                    .filter(|_| numbers > 0.)
                    .map(|number| (number, numbers));
                if vertical {
                    self.render_slot_row(index, slot, number, theme, cx)
                        .into_any_element()
                } else {
                    self.render_slot(index, slot, number, theme, cx)
                        .into_any_element()
                }
            })
            .collect();
        if vertical {
            return Some(home_rows(tiles, theme));
        }
        if shows_pin_hint(count) {
            tiles.push(self.render_pin_hint(theme, cx).into_any_element());
        }
        Some(home(tiles, theme))
    }

    /// The pin hint after the pins on the strip: what it tells assistive
    /// technology — how to pin, with the keys in force — as a note, which
    /// takes no focus and no input.
    fn render_pin_hint(&self, theme: &Theme, cx: &App) -> Stateful<Div> {
        let open_actions = crate::settings::shared(cx)
            .read(cx)
            .keyboard()
            .binding(KeyboardAction::OpenActions)
            .clone();
        let open_actions = crate::keyboard::binding_keys(&open_actions).name();
        let toggle_pin =
            crate::keyboard::binding_keys(&crate::keyboard::toggle_pin_binding()).name();
        pin_hint(theme)
            .role(Role::Note)
            .aria_label(PIN_HINT)
            .aria_description(pin_hint_description(&toggle_pin, &open_actions))
    }

    /// The pin at `index` as a result row, for the vertical layout: its
    /// tile and title, why it cannot run, and its number while Ctrl is
    /// held (`number`, a numbered pin's); the slot's own focus,
    /// accessibility and input, as a tile has them.
    fn render_slot_row(
        &self,
        index: usize,
        slot: QuickSlot,
        number: Option<(usize, f32)>,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let row = result_row_with(
            RowContent {
                title: slot.title.clone().into(),
                // What tells it apart from a result of its title, as its
                // tile's tooltip says.
                subtitle: slot.detail.clone().map(Into::into),
                unavailable_reason: slot.unavailable.clone().map(Into::into),
                unavailable_id: ("slot-unavailable", index).into(),
                selected: false,
                icon: Some(slot_icon(&self.launcher, &slot, theme)),
            },
            RowMeta {
                number,
                ..RowMeta::default()
            },
            theme,
        )
        .focus_visible(|row| row.bg(theme.row_hover));
        let press = crate::ui::result_row::pressed_wash(false, theme);
        let row = row
            .id(("slot", index))
            .active(move |row| row.bg(press))
            .debug_selector(move || format!("slot-{}", index + 1));
        slot_accessibility(index, &slot, self.slot_input(index, row, cx))
    }

    /// The input a slot takes, tile or row: its focus, Enter and Space, a
    /// click and a secondary click for its actions.
    fn slot_input(
        &self,
        index: usize,
        slot: Stateful<Div>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        slot.key_context(SLOT_CONTEXT)
            .track_focus(&self.home.focus[index])
            .on_action(cx.listener(move |this, _: &PressSlot, window, cx| {
                this.activate_quick_slot(index, window, cx);
            }))
            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                this.click_quick_slot(index, event, window, cx);
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, _: &MouseDownEvent, window, cx| {
                    this.open_slot_actions(index, window, cx);
                    // The panel's search field keeps the focus it was
                    // just given: without this, the slot's own focus
                    // tracking takes it on the same press, so typing
                    // would not filter and Enter would press the slot.
                    if this.actions.is_some() {
                        window.prevent_default();
                    }
                }),
            )
    }

    /// The pin at `index` as a strip tile, showing `slot`, with its focus,
    /// accessibility and input; `number`, a numbered pin's, is the digit
    /// Ctrl picks it with and the hint's look.
    fn render_slot(
        &self,
        index: usize,
        slot: QuickSlot,
        number: Option<(usize, f32)>,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let content = SlotContent {
            index,
            title: slot.title.clone().into(),
            icon: slot_icon(&self.launcher, &slot, theme),
            number,
            unavailable: slot.unavailable.clone().map(Into::into),
        };
        let tile = self
            .slot_input(index, pinned_slot(content, theme), cx)
            // What tells it apart from a result of its title, which its
            // tile has no room to show.
            .when_some(slot.detail.clone(), |tile, detail| {
                tile.tooltip(text_tooltip(detail.into(), TooltipLook::of(theme)))
            });
        slot_accessibility(index, &slot, tile)
    }
}
