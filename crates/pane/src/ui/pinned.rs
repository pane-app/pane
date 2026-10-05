//! The pinned home's visuals: the reference's "Pinned" label, its strip of
//! quick slots and each slot (`.slot`) — presentation only.
//!
//! - **The label** is the list's `.label` (see [`super::shell`]) with the
//!   slots' chord on its right: the regular keycaps "Ctrl" and "1–5".
//! - **The strip** is a grid of equal columns, 8px apart, with 2px above
//!   the slots and 6px below them.
//! - **A slot** is 100 high, radius 12, padded 14 above, 8 either side and
//!   10 below. Its 42px tile and its title (12.5/500, #D9DADD, one line
//!   with an ellipsis) are centered down it, 9px apart, and its compact
//!   key hint sits 8px in from its top right. It is white 3.5% with a
//!   white 5% inset edge at rest and white 7% under the pointer; keyboard
//!   focus draws the reference's 2px focus outline inside it.
//!
//! Two states are Pane's own, since the reference authors neither: a slot
//! whose target cannot run now keeps its tile (at half strength) and title,
//! with the reason below the title in the warning color; an empty slot is
//! a dashed outline saying it is empty, with no key hint — pressing its
//! chord does nothing.
//!
//! The caller decides what each slot shows ([`SlotContent`]) and attaches
//! its identity, accessibility and behavior to the returned elements. The
//! launcher and the visual workbench's fixture both draw the home through
//! these functions.

use gpui::prelude::*;
use gpui::{AnyElement, BoxShadow, Div, Role, SharedString, Stateful, div, px, relative};

use crate::ui::icon::{Glyph, IconTone, TileSize, tile_at};
use crate::ui::keycap::{CapStyle, KeySequence, key_sequence};
use crate::ui::shell::section_label;
use crate::ui::theme::Theme;

/// The label over the strip.
pub(crate) const PINNED_LABEL: &str = "Pinned";

/// What an empty slot says.
pub(crate) const EMPTY_SLOT: &str = "Empty";

/// How many of the result list's children the home puts above the rows:
/// the label and the strip.
pub(crate) const HOME_CHILDREN: usize = 2;

/// What one slot shows, resolved by the caller.
#[derive(Clone, Debug)]
pub(crate) struct SlotContent {
    /// The slot's place, from 0.
    pub(crate) index: usize,
    /// What it holds; `None` for an empty slot.
    pub(crate) title: Option<SharedString>,
    /// The tile of what it holds.
    pub(crate) icon: (IconTone, Glyph),
    /// The chord that invokes it, drawn in its corner; `None` draws none.
    pub(crate) keys: Option<KeySequence>,
    /// Why what it holds cannot run now, if it cannot.
    pub(crate) unavailable: Option<SharedString>,
}

/// The "Pinned" label, with `keys` — the slots' chord — on its right.
/// Carries the debug selector `section-Pinned`, as the list's other labels
/// carry theirs.
pub(crate) fn pinned_label(keys: &KeySequence, theme: &Theme) -> Div {
    section_label(PINNED_LABEL.into(), None, theme)
        .debug_selector(|| format!("section-{PINNED_LABEL}"))
        .child(
            // Its own scope: the key sequence's id is fixed.
            div()
                .id("pinned-keys")
                .flex_none()
                .child(key_sequence(keys, CapStyle::Regular, theme)),
        )
}

/// The home's children of the result list, above its rows: the "Pinned"
/// label showing `keys`, then the strip of `slots` ([`HOME_CHILDREN`] of
/// them).
pub(crate) fn home(keys: &KeySequence, slots: Vec<AnyElement>, theme: &Theme) -> Vec<AnyElement> {
    vec![
        pinned_label(keys, theme).into_any_element(),
        pinned_strip(slots, theme).into_any_element(),
    ]
}

/// The strip: `slots` in equal columns.
pub(crate) fn pinned_strip(slots: Vec<AnyElement>, theme: &Theme) -> Stateful<Div> {
    let geometry = &theme.geometry.pinned;
    let columns = slots.len().max(1) as u16;
    div()
        .id("pinned-strip")
        .debug_selector(|| "pinned-strip".into())
        .role(Role::List)
        .aria_label(PINNED_LABEL)
        .flex_none()
        .grid()
        .grid_cols(columns)
        .gap(geometry.columns_gap)
        .pt(geometry.strip_padding_top)
        .pb(geometry.strip_padding_bottom)
        .children(slots)
}

/// One slot showing `content` (see the module docs). Its id is
/// `("slot", index)` and its debug selector `slot-<n>`, counting from 1.
pub(crate) fn pinned_slot(content: SlotContent, theme: &Theme) -> Stateful<Div> {
    let geometry = &theme.geometry.pinned;
    let typography = &theme.typography;
    let number = content.index + 1;
    let slot = div()
        .id(("slot", content.index))
        .debug_selector(move || format!("slot-{number}"))
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
        // The reference's focus outline (`:focus-visible`): 2px of white
        // 50%, inside, while the keyboard moved focus here.
        .focus_visible(|slot| {
            slot.shadow(vec![
                BoxShadow::new(px(0.), px(0.), theme.focus_ring)
                    .spread_radius(geometry.focus_width)
                    .inset(),
            ])
        });
    let Some(title) = content.title else {
        return slot
            .border_1()
            .border_dashed()
            .border_color(theme.slot_empty_edge)
            .font_weight(typography.regular)
            .text_color(theme.text_muted)
            .child(EMPTY_SLOT);
    };
    let (tone, glyph) = content.icon;
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
        .child(title);
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
        .bg(theme.slot_background)
        .shadow(vec![
            BoxShadow::new(px(0.), px(0.), theme.slot_edge)
                .spread_radius(geometry.edge_width)
                .inset(),
        ])
        .hover(|slot| slot.bg(theme.slot_hover))
        .child(
            tile_at(TileSize::Slot, tone, glyph, theme).when(unavailable, |tile| tile.opacity(0.5)),
        )
        .child(text)
        .when_some(content.keys, |slot, keys| {
            slot.child(
                div()
                    .id("slot-keys")
                    .absolute()
                    .top(geometry.keys_inset)
                    .right(geometry.keys_inset)
                    .child(key_sequence(&keys, CapStyle::Compact, theme)),
            )
        })
}
