//! The pinned home's visuals: the reference's "Pinned" label, its strip of
//! quick slots and each slot (`.slot`) — presentation only.
//!
//! - **The label** is the list's `.label` (see [`super::shell`]).
//! - **The strip** (the horizontal layout) is a grid of
//!   [`PINNED_COLUMNS`] equal columns, 8px apart, with 2px above the slots
//!   and 6px below them. The pins fill it in order and wrap onto more
//!   rows, 8px apart; there are as many tiles as pins, never an empty one.
//! - **A slot** is 80 high, radius 12, padded 8 all round — Pane's compact
//!   strip, lower than the reference's. Its 30px tile and its title
//!   (12.5/500, #D9DADD, one line with an ellipsis) are centered down it,
//!   7px apart. It is white 3.5% with a white 5% inset edge at rest and
//!   white 7% under the pointer; keyboard focus draws the reference's 2px
//!   focus outline inside it.
//! - **The vertical layout** lists the pinned results as result rows under
//!   the label instead; the launcher draws those rows.
//!
//! No chord is drawn at rest: while Ctrl is held, each numbered slot's
//! number slides down into its top right corner ([`slot_number_hint`]).
//!
//! Two things are Pane's own, since the reference authors neither: a slot
//! whose target cannot run now keeps its tile (at half strength) and title,
//! with the reason below the title in the warning color; and the pin hint
//! ([`pin_hint`]), a dashed outline with a plus and "Pin" in the cell after
//! the last pin, shown only while that cell is in the strip's last row
//! ([`shows_pin_hint`]) — it never starts a row of its own, and with
//! nothing pinned it is the strip's one tile. It says how to pin; it is not
//! a slot, and pressing it does nothing.
//!
//! The caller decides what each slot shows ([`SlotContent`]) and attaches
//! its identity, accessibility and behavior to the returned elements. The
//! launcher draws the home through these functions.

use gpui::prelude::*;
use gpui::{AnyElement, BoxShadow, Div, Role, SharedString, Stateful, div, px, relative};

use crate::ui::extension_icon::{RowIcon, row_icon_at};
use crate::ui::icon::{Glyph, TileSize, glyph};
use crate::ui::keycap::slot_number_hint;
use crate::ui::shell::section_label;
use crate::ui::theme::{Theme, pressed};

/// The label over the strip.
pub(crate) const PINNED_LABEL: &str = "Pinned";

/// What the pin hint says, under its plus.
pub(crate) const PIN_HINT: &str = "Pin";

/// How many equal columns the strip has: each of its rows holds this many
/// pins.
pub(crate) const PINNED_COLUMNS: usize = 5;

/// How many of the result list's children the home puts above the rows:
/// the label and the strip.
pub(crate) const HOME_CHILDREN: usize = 2;

/// What one slot shows, resolved by the caller.
#[derive(Clone, Debug)]
pub(crate) struct SlotContent {
    /// The slot's place among the pins, from 0.
    pub(crate) index: usize,
    /// What it holds.
    pub(crate) title: SharedString,
    /// The icon of what it holds: Pane's tile, or an extension's icon.
    pub(crate) icon: RowIcon,
    /// Its number and the number hint's look (0 hidden, 1 shown) while Ctrl
    /// is held; `None` draws none (a pin past the numbered ones has none).
    pub(crate) number: Option<(usize, f32)>,
    /// Why what it holds cannot run now, if it cannot.
    pub(crate) unavailable: Option<SharedString>,
}

/// Whether the strip shows the pin hint after `pins` pins: when the cell
/// after the last pin is in the strip's last row, or nothing is pinned —
/// never when the pins fill their last row, so the hint never adds a row.
pub(crate) fn shows_pin_hint(pins: usize) -> bool {
    pins == 0 || !pins.is_multiple_of(PINNED_COLUMNS)
}

/// The "Pinned" label. Carries the debug selector `section-Pinned`, as the
/// list's other labels carry theirs.
pub(crate) fn pinned_label(theme: &Theme) -> Div {
    section_label(PINNED_LABEL.into(), None, theme)
        .debug_selector(|| format!("section-{PINNED_LABEL}"))
}

/// The horizontal home's children of the result list, above its rows: the
/// "Pinned" label, then the strip of `tiles` — the slots and, when it
/// shows, the pin hint after them ([`HOME_CHILDREN`] children).
pub(crate) fn home(tiles: Vec<AnyElement>, theme: &Theme) -> Vec<AnyElement> {
    vec![
        pinned_label(theme).into_any_element(),
        pinned_strip(tiles, theme).into_any_element(),
    ]
}

/// The vertical home's children of the result list, above its rows: the
/// "Pinned" label over `rows`, the pinned results as result rows — or
/// nothing at all while nothing is pinned.
pub(crate) fn home_rows(rows: Vec<AnyElement>, theme: &Theme) -> Vec<AnyElement> {
    if rows.is_empty() {
        return rows;
    }
    std::iter::once(pinned_label(theme).into_any_element())
        .chain(rows)
        .collect()
}

/// The strip: `tiles` in [`PINNED_COLUMNS`] equal columns, wrapping onto
/// as many rows as they need, the columns' gap between the rows too.
pub(crate) fn pinned_strip(tiles: Vec<AnyElement>, theme: &Theme) -> Stateful<Div> {
    let geometry = &theme.geometry.pinned;
    div()
        .id("pinned-strip")
        .debug_selector(|| "pinned-strip".into())
        .role(Role::List)
        .aria_label(PINNED_LABEL)
        .flex_none()
        .grid()
        .grid_cols(PINNED_COLUMNS as u16)
        .gap(geometry.columns_gap)
        .pt(geometry.strip_padding_top)
        .pb(geometry.strip_padding_bottom)
        .children(tiles)
}

/// A strip cell's box, a slot's or the pin hint's: its height, radius,
/// paddings and type, its content centered down it.
fn cell(theme: &Theme) -> Div {
    let geometry = &theme.geometry.pinned;
    let typography = &theme.typography;
    div()
        .relative()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(geometry.slot_gap)
        .h(geometry.slot_height)
        .min_w(px(0.))
        .pt(geometry.slot_padding_top)
        .px(geometry.slot_padding_x)
        .pb(geometry.slot_padding_bottom)
        .rounded(geometry.slot_radius)
        .font_family(typography.family.clone())
        .text_size(typography.slot_title_size)
        .line_height(typography.slot_title_size * typography.line_height)
}

/// The pin hint: a dashed outline in the cell after the last pin, its
/// plus over "Pin" in the muted text (see the module docs). Its id and
/// debug selector are `pin-hint`; the caller says what it tells assistive
/// technology. It takes no focus and no input.
pub(crate) fn pin_hint(theme: &Theme) -> Stateful<Div> {
    let typography = &theme.typography;
    cell(theme)
        .id("pin-hint")
        .debug_selector(|| "pin-hint".into())
        .border_1()
        .border_dashed()
        .border_color(theme.slot_empty_edge)
        .font_weight(typography.regular)
        .text_color(theme.text_muted)
        .child(
            glyph(
                Glyph::Plus,
                theme.geometry.slot_tile.glyph,
                theme.text_muted,
            )
            .flex_none(),
        )
        .child(PIN_HINT)
}

/// One slot showing `content` (see the module docs). Its id is
/// `("slot", index)` and its debug selector `slot-<n>`, counting from 1.
pub(crate) fn pinned_slot(content: SlotContent, theme: &Theme) -> Stateful<Div> {
    let geometry = &theme.geometry.pinned;
    let typography = &theme.typography;
    let number = content.index + 1;
    let slot = cell(theme)
        .id(("slot", content.index))
        .debug_selector(move || format!("slot-{number}"))
        // The reference's focus outline (`:focus-visible`): 2px of white
        // 50%, inside, while the keyboard moved focus here.
        .focus_visible(|slot| {
            slot.shadow(vec![
                BoxShadow::new(px(0.), px(0.), theme.focus_ring)
                    .spread_radius(geometry.focus_width)
                    .inset(),
            ])
        });
    let unavailable = content.unavailable.is_some();
    let title = div()
        .max_w(relative(1.))
        .min_w(px(0.))
        .truncate()
        .font_weight(typography.medium)
        .text_color(if unavailable {
            theme.text_muted
        } else {
            theme.slot_title
        })
        .child(content.title);
    // An unavailable slot's title and reason are one block, the reason
    // right under the title, the block closer to the tile: with the
    // reference's gaps they would not fit the slot (see
    // `PinnedGeometry::unavailable_gap`).
    let text = match content.unavailable {
        None => title.into_any_element(),
        Some(reason) => div()
            .flex()
            .flex_col()
            .items_center()
            .max_w(relative(1.))
            .min_w(px(0.))
            .child(title)
            .child(
                div()
                    .max_w(relative(1.))
                    .min_w(px(0.))
                    .truncate()
                    .text_size(typography.slot_reason_size)
                    .line_height(typography.slot_reason_size * typography.line_height)
                    .text_color(theme.warning)
                    .child(reason),
            )
            .into_any_element(),
    };
    slot.cursor_pointer()
        .when(unavailable, |slot| slot.gap(geometry.unavailable_gap))
        // Over a background image a slot is frosted (ADR 0028): it blurs
        // the picture behind it, under the frost's edges.
        .when_some(theme.frost, |slot, frost| slot.backdrop_blur(frost.blur))
        .bg(theme.slot_background)
        .shadow(match theme.frost {
            Some(frost) => frost.edges(),
            None => vec![
                BoxShadow::new(px(0.), px(0.), theme.slot_edge)
                    .spread_radius(geometry.edge_width)
                    .inset(),
            ],
        })
        .hover(|slot| slot.bg(theme.slot_hover))
        .active(|slot| slot.bg(pressed(theme.slot_hover)))
        .child(
            row_icon_at(
                &content.icon,
                TileSize::Slot,
                "slot-icon",
                &format!("slot-{number}"),
                theme,
            )
            .when(unavailable, |tile| tile.opacity(0.5)),
        )
        .child(text)
        .when_some(content.number, |slot, (number, look)| {
            slot.child(slot_number_hint(number, look, theme))
        })
}
