//! The compact window's pins: with the Launcher page's "Show pinned in
//! Compact mode" switch on (Raycast's "Show favorites in compact mode"),
//! the collapsed launcher shows its pins as one row of small icon tiles
//! under the search field.
//!
//! - **The row shows** while the launcher is collapsed (see
//!   `LauncherWindow::collapses`), the switch is on and at least one result
//!   is pinned; the window is then the search field's height plus the
//!   row's ([`ROW_HEIGHT`]). Otherwise the collapsed window is the search
//!   field alone, as it always was.
//! - **One tile per pin**, in the pins' order, as many as the window's
//!   width holds: the rest are clipped, never wrapped or scrolled. A pin
//!   whose target cannot run now is drawn at half strength and says why
//!   to assistive technology.
//! - **A click** runs what the pin holds, through the same path as the
//!   pinned home's tiles ([`LauncherWindow::activate_quick_slot`]), and
//!   focus stays in the query field. A double click's second click runs
//!   nothing more, and nothing runs while the Actions panel or the Pane
//!   menu is open.
//! - **While Ctrl is held**, the pins Ctrl and a digit pick (the first
//!   five; see `LauncherWindow::numbered_slots`) show their number in
//!   their corner, as the pinned home's tiles do. The chords themselves
//!   are the launcher's (see [`crate::features::number_hints::numbered`]): they work collapsed
//!   as expanded.
//!
//! The row's geometry is kept here rather than among the theme's pinned
//! tokens: it is this row's own, and no other surface draws it.

use gpui::{App, ClickEvent, Context, Div, Role, Stateful, Window, div, prelude::*, px};
use pane_core::{QuickSlot, Screen};

use crate::app::{LauncherWindow, Spot};
use crate::ui::extension_icon::row_icon_at;
use crate::ui::icon::TileSize;
use crate::ui::keycap::{CapStyle, Key, KeySequence, key_sequence};
use crate::ui::theme::{Theme, faded, pressed};

/// The row's height under the search field, in px: the pins and the space
/// above and below them. The collapsed window grows by this much while
/// the row shows.
pub(crate) const ROW_HEIGHT: f32 = 48.;

/// A pin's square, in px: its hit area and hover wash around its icon tile
/// (the result row's 28px tile).
const PIN_SIZE: f32 = 34.;

/// The space between a pin's square and its icon tile on each side, in px:
/// the row's inset is the search field's less this, so the tiles line up
/// with the field's magnifier.
const PIN_RIM: f32 = 3.;

/// A pin's corner radius, in px.
const PIN_RADIUS: f32 = 9.;

/// The space between two pins, in px.
const PIN_GAP: f32 = 4.;

/// How far a pin's number hint travels as it is revealed, in px.
const HINT_TRAVEL: f32 = 6.;

/// How far a pin's number hint sits past its square's corner, in px.
const HINT_OVERHANG: f32 = 3.;

/// The aria label the row carries.
const ROW_LABEL: &str = "Pinned";

/// A pin's number hint while Ctrl is held: `number`'s compact cap in the
/// pin's bottom right corner, rising into place as `look` goes from 0
/// (hidden) to 1 (shown). The pin is its positioned parent; the row clips
/// what is still below it.
fn number_hint(number: usize, look: f32, theme: &Theme) -> Div {
    let keys = KeySequence {
        keys: vec![Key::new(number.to_string(), format!("Ctrl+{number}"))],
    };
    div()
        .absolute()
        .right(px(-HINT_OVERHANG))
        .bottom(px(-HINT_OVERHANG - HINT_TRAVEL * (1. - look)))
        .opacity(look)
        .child(key_sequence(&keys, CapStyle::Compact, theme))
}

impl LauncherWindow {
    /// Whether the collapsed window shows the pins' row under its search
    /// field: the Launcher page's switch is on and something is pinned.
    /// The pins are resolved only while the switch is on.
    pub(crate) fn shows_compact_pins(&self, cx: &App) -> bool {
        crate::settings::shared(cx).read(cx).compact_pinned()
            && !self.launcher.quick_slots().is_empty()
    }

    /// The collapsed window's row of pins, under the search field, when it
    /// shows (see [`LauncherWindow::shows_compact_pins`]); `None`
    /// otherwise. `numbers` is the number hints' look, 0 hidden and 1
    /// shown.
    pub(crate) fn render_compact_pins(
        &self,
        numbers: f32,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Option<Stateful<Div>> {
        if !crate::settings::shared(cx).read(cx).compact_pinned() {
            return None;
        }
        let pins = self.launcher.quick_slots();
        if pins.is_empty() {
            return None;
        }
        // The pins Ctrl and a digit pick, in order: a pin's number is its
        // place among them.
        let numbered = self.numbered_slots();
        let pins: Vec<_> = pins
            .into_iter()
            .enumerate()
            .map(|(index, pin)| {
                let number = numbered
                    .iter()
                    .position(|&slot| slot == index)
                    .map(|place| place + 1);
                self.render_compact_pin(index, pin, number, numbers, theme, cx)
            })
            .collect();
        Some(
            div()
                .id("compact-pins")
                .debug_selector(|| "compact-pins".into())
                .role(Role::List)
                .aria_label(ROW_LABEL)
                .flex_none()
                .flex()
                .items_center()
                .gap(px(PIN_GAP))
                .h(px(ROW_HEIGHT))
                .px(theme.geometry.search_padding_x - px(PIN_RIM))
                // As many as the width holds; the rest are clipped.
                .overflow_hidden()
                .children(pins),
        )
    }

    /// The pin at `index`, showing `pin`: its tile, its identity and
    /// accessibility, its click, and — while Ctrl is held and Ctrl picks
    /// it — its `number`, at the hints' `look`.
    fn render_compact_pin(
        &self,
        index: usize,
        pin: QuickSlot,
        number: Option<usize>,
        look: f32,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let place = index + 1;
        // An installed command's own icon (#139), else Pane's tile.
        let icon = crate::features::icons::row_icon_of(&self.launcher, &pin.target.key(), theme);
        let ready = pin.ready();
        // Hovering a pin moves no selection: the fainter wash, fading out
        // once the pointer leaves (#245).
        let look = self
            .motion
            .hover
            .look(Spot::Pin(index), cx.background_executor().now());
        div()
            .id(("compact-pin", index))
            .debug_selector(move || format!("compact-pin-{place}"))
            .relative()
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .size(px(PIN_SIZE))
            .rounded(px(PIN_RADIUS))
            .cursor_pointer()
            .when(look > 0., |style| style.bg(faded(theme.hover_wash, look)))
            .active(|style| style.bg(pressed(theme.hover_wash)))
            .on_hover(cx.listener(move |this, over: &bool, _, cx| {
                this.motion.hover.set(Spot::Pin(index), *over, cx);
            }))
            .role(Role::Button)
            .aria_label(format!("Pinned {place}: {}", pin.title))
            .when_some(
                crate::features::quick_slots::slot_description(&pin),
                |element, description| element.aria_description(description),
            )
            // What tells it apart from a result of its title, which the
            // compact pin has no room to show.
            .when_some(pin.detail.clone(), |element, detail| {
                element.tooltip(crate::ui::tooltip::text_tooltip(
                    detail.into(),
                    crate::ui::tooltip::TooltipLook::of(theme),
                ))
            })
            .when_some(number, |element, number| {
                element.aria_keyshortcuts(crate::keyboard::quick_slot_keys(number).name())
            })
            .when(!ready, |element| element.aria_disabled(true))
            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                this.click_compact_pin(index, event, window, cx);
            }))
            .child(
                row_icon_at(
                    &icon,
                    TileSize::Row,
                    "pin-icon",
                    &format!("compact-pin-{place}"),
                    theme,
                )
                .when(!ready, |tile| tile.opacity(0.5)),
            )
            .when_some(number.filter(|_| look > 0.), |element, number| {
                element.child(number_hint(number, look, theme))
            })
    }

    /// A click on the pin at `index`: it runs what the pin holds, and focus
    /// stays in the query field, as typing expects it. A double click's
    /// second click runs nothing more.
    fn click_compact_pin(
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
}
