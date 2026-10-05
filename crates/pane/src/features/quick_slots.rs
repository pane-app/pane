//! Root search's pinned home: the five quick slots above a blank query's
//! results, as the launcher window draws and drives them.
//!
//! What the slots hold and whether each can run is the core's
//! ([`pane_core::Launcher::quick_slots`]); this module draws them with the
//! shared visuals ([`crate::ui::pinned`]) and wires them:
//!
//! - **The home shows** while root search's trimmed query is blank — the
//!   "Pinned" label and the strip above the rows — and a query hides it;
//!   clearing the query brings it back. When root search comes on screen,
//!   the core is asked for the indexed results the slots pin, if it never
//!   listed them ([`pane_core::Launcher::resolve_quick_slots`]).
//! - **A slot is invoked** by a click, by Enter or Space while it has
//!   focus, or by its chord — Ctrl+1 to Ctrl+5, local to the root search
//!   field and the slots, never registered with the system. A chord acts
//!   only on root search, with no overlay (the Actions panel, the Pane
//!   menu) open and no input-method composition in the query field; the
//!   core then runs the slot's target only if it resolves to one that can
//!   run and no action is already running, and otherwise says why. Each
//!   press runs once: a held chord's repeats and a double click's second
//!   click run nothing more, and an empty slot does nothing at all. A click
//!   leaves focus in the query field.
//! - **A slot's own actions** — invoking it, removing it, moving it left or
//!   right — open in the Actions panel from a secondary click on it, or
//!   from the Open actions binding while it has focus (see
//!   [`crate::features::actions_panel`]).
//!
//! The slots are tab stops after the query field, so the keyboard reaches
//! every one of them, empty ones included, which say how to fill them.

use gpui::{
    AnyElement, App, ClickEvent, Context, Div, EntityInputHandler, FocusHandle, Focusable,
    KeyBinding, KeyDownEvent, Keystroke, MouseButton, MouseDownEvent, Role, Stateful, Window,
    actions, prelude::*,
};
use pane_core::{KeyboardAction, LauncherView, QUICK_SLOTS, QuickSlot, Screen};

use crate::app::{LauncherWindow, row_icon};
use crate::ui::icon::{Glyph, IconTone};
use crate::ui::pinned::{SlotContent, home, pinned_slot};
use crate::ui::theme::Theme;

actions!(quick_slots, [PressSlot]);

/// A focused slot's key context.
const SLOT_CONTEXT: &str = "QuickSlot";

/// Registers a focused slot's keys: Enter and Space press it, above the
/// launcher's confirm. The slots' chords are read from the key presses
/// themselves (see [`LauncherWindow::on_quick_slot_keys`]), so a held
/// chord's repeats are told from new presses.
pub(crate) fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("enter", PressSlot, Some(SLOT_CONTEXT)),
        KeyBinding::new("space", PressSlot, Some(SLOT_CONTEXT)),
    ]);
}

/// The slot whose chord `keystroke` is (Ctrl and the slot's digit, with no
/// other modifier), if it is one.
fn chord_slot(keystroke: &Keystroke) -> Option<usize> {
    let pressed = crate::keyboard::binding_of(keystroke).ok()?;
    (1..=QUICK_SLOTS)
        .find(|&number| crate::keyboard::quick_slot_binding(number) == pressed)
        .map(|number| number - 1)
}

/// The window's own state for the home: each slot's focus, and whether
/// root search was on screen when the screen last changed.
pub(crate) struct Home {
    /// One focus per slot, each a tab stop.
    focus: Vec<FocusHandle>,
    /// Whether root search was on screen at the last sync.
    on_root: bool,
}

impl Home {
    pub(crate) fn new(cx: &mut App) -> Home {
        Home {
            focus: (0..QUICK_SLOTS)
                .map(|_| cx.focus_handle().tab_stop(true))
                .collect(),
            on_root: false,
        }
    }
}

/// Whether `view` shows the home: root search with a blank trimmed query.
pub(crate) fn home_shown(view: &LauncherView) -> bool {
    matches!(&view.screen, Screen::Root { query } if query.trim().is_empty())
}

/// What an empty slot tells assistive technology, with the Open actions
/// binding in force.
fn empty_hint(open_actions: &str) -> String {
    format!(
        "Empty. To pin a search result here, select it, press {open_actions} and choose Pin to \
         Quick Slot"
    )
}

impl LauncherWindow {
    /// Follows the launcher's screen: when root search comes on screen,
    /// the indexed results the slots pin are asked for if they never were,
    /// and the window redraws once they are listed.
    pub(crate) fn sync_home(&mut self, cx: &mut Context<Self>) {
        let on_root = matches!(self.launcher.view().screen, Screen::Root { .. });
        let was = std::mem::replace(&mut self.home.on_root, on_root);
        if on_root && !was {
            let resolving = self.launcher.resolve_quick_slots();
            cx.spawn(async move |this, cx| {
                resolving.await;
                this.update(cx, |_, cx| cx.notify()).ok();
            })
            .detach();
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
    /// again and runs it only if it can (see the module docs). An empty
    /// slot is a true no-op: nothing is dispatched and nothing redraws.
    pub(crate) fn activate_quick_slot(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let holds = self
            .launcher
            .quick_slots()
            .get(index)
            .is_some_and(|slot| !slot.is_empty());
        if !holds {
            return;
        }
        let pending = self.launcher.activate_quick_slot(index);
        self.navigate_forward(window, cx);
        self.show_until_done(pending, window, cx);
    }

    /// A slot's chord: it acts only on root search, with no overlay open
    /// and no composition in the query field.
    fn press_quick_slot(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(self.launcher.view().screen, Screen::Root { .. })
            || self.actions.is_some()
            || self.menu.is_some()
        {
            return;
        }
        let query = self.query_field();
        let composing = query.update(cx, |query, cx| query.marked_text_range(window, cx));
        if composing.is_some() {
            return;
        }
        self.activate_quick_slot(index, window, cx);
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
        if matches!(self.launcher.view().screen, Screen::Root { .. }) {
            self.query.focus(window, cx);
        }
    }

    /// A key pressed in the launcher, before the focused control sees
    /// it: a slot's chord, while the root search field or a slot has focus
    /// (an overlay's own field never does), invokes that slot — once per
    /// press: the system's repeats of a held chord run nothing more.
    fn quick_slot_chord(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = chord_slot(&event.keystroke) else {
            return;
        };
        // The query field is also an opened command's search, which has
        // no slots.
        if !matches!(self.launcher.view().screen, Screen::Root { .. }) {
            return;
        }
        let field = self.query_field().focus_handle(cx).is_focused(window);
        if !field && self.focused_slot(window).is_none() {
            return;
        }
        cx.stop_propagation();
        if !event.is_held {
            self.press_quick_slot(index, window, cx);
        }
    }

    /// `content`, the launcher's root, handling the slots' chords.
    pub(crate) fn on_quick_slot_keys(content: Div, cx: &mut Context<Self>) -> Div {
        content.capture_key_down(cx.listener(Self::quick_slot_chord))
    }

    /// The home's children of the result list — the "Pinned" label and the
    /// strip of five slots — when `view` shows the home; `None` otherwise.
    pub(crate) fn render_home(
        &self,
        view: &LauncherView,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Option<Vec<AnyElement>> {
        if !home_shown(view) {
            return None;
        }
        let open_actions = crate::settings::shared(cx)
            .read(cx)
            .keyboard()
            .binding(KeyboardAction::OpenActions)
            .clone();
        let open_actions = crate::keyboard::binding_keys(&open_actions).name();
        let slots = self
            .launcher
            .quick_slots()
            .into_iter()
            .enumerate()
            .map(|(index, slot)| {
                self.render_slot(index, slot, &open_actions, theme, cx)
                    .into_any_element()
            })
            .collect();
        Some(home(
            &crate::keyboard::quick_slots_keys(QUICK_SLOTS),
            slots,
            theme,
        ))
    }

    /// The slot at `index`, showing `slot`, with its focus, accessibility
    /// and input.
    fn render_slot(
        &self,
        index: usize,
        slot: QuickSlot,
        open_actions: &str,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let number = index + 1;
        let keys = crate::keyboard::quick_slot_keys(number);
        let shortcut = keys.name();
        let empty = slot.is_empty();
        let icon = slot
            .target
            .as_ref()
            .and_then(|target| row_icon(&target.key()))
            .unwrap_or((IconTone::Command, Glyph::Prompt));
        let label = if empty {
            format!("Quick slot {number}")
        } else {
            format!("Quick slot {number}: {}", slot.title)
        };
        let description = if empty {
            Some(empty_hint(open_actions))
        } else {
            slot.unavailable.clone()
        };
        let content = SlotContent {
            index,
            title: (!empty).then(|| slot.title.clone().into()),
            icon,
            keys: (!empty).then_some(keys),
            unavailable: slot.unavailable.clone().map(Into::into),
        };
        pinned_slot(content, theme)
            .key_context(SLOT_CONTEXT)
            .track_focus(&self.home.focus[index])
            .role(Role::Button)
            .aria_label(label)
            .when_some(description, |element, description| {
                element.aria_description(description)
            })
            .when(!empty, |element| element.aria_keyshortcuts(shortcut))
            .when(!slot.ready(), |element| element.aria_disabled(true))
            .on_action(cx.listener(move |this, _: &PressSlot, window, cx| {
                this.activate_quick_slot(index, window, cx);
            }))
            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                this.click_quick_slot(index, event, window, cx);
            }))
            .when(!empty, |element| {
                element.on_mouse_down(
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
            })
    }
}
