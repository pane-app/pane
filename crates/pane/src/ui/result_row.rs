//! The launcher's result row: presentation only.
//!
//! The row is the reference's `.row`: 44px (a floor — a row whose
//! unavailable reason wraps grows taller rather than clipping it), radius
//! 10, a 12px content gap, the icon tile, the 14px/500 title and the 13px
//! subtitle inline, and a pale wash for hover and selection. Selection
//! stays visible while hovering: a selected row keeps its wash and inset
//! edge and does not switch to the hover wash.
//!
//! This component owns no identity and no behavior. It returns a plain
//! [`Div`] so the app attaches everything behavioral on top:
//!
//! - `.id(("row", index))` — the stable id (making the row stateful for
//!   scrolling and hit-testing) and `.debug_selector(...)` for the smokes,
//! - the pressed feedback — the wash strengthening to the selected one
//!   while the row is held, fading on the shared pointer span beside the
//!   hover wash this component carries — attached after the id, because a
//!   press state needs the named (stateful) row,
//! - the accessibility contract — `.role(Role::ListBoxOption)`,
//!   `.aria_selected`, `.aria_active_descendant` when selected,
//!   `.aria_disabled` with a description when the reason is present,
//! - `.on_click(...)`, focus and any key handling.
//!
//! The row registers no handlers and no focus of its own, so nothing here
//! swallows events. Layout: a text group (flex, min-width 0) holds the
//! 14px/500 title and the 13px subtitle on one line — the title shrinks
//! and ellipsizes under pressure but does not grow, the subtitle takes the
//! leftover and ellipsizes — and the unavailable reason below it, wrapping
//! within the group's width: never truncated, and the row grows past its
//! 44px floor to fit it. The reason element carries the Pane debug
//! convention `unavailable-reason-<title>` for tests and smokes.

use gpui::prelude::*;
use gpui::{BoxShadow, Div, ElementId, SharedString, div, px};

use crate::ui::icon::{self, Glyph, IconTone};
use crate::ui::theme::Theme;

/// What a result row shows — plain presentation values, already resolved
/// by the caller from whatever the launcher holds. Nothing here derives
/// presentation from content: the caller maps identities to
/// [`IconTone`]s (unknown ones use [`IconTone::Command`]).
#[derive(Clone, Debug)]
pub(crate) struct RowContent {
    /// The row's title.
    pub(crate) title: SharedString,
    /// The row's subtitle, if it has one.
    pub(crate) subtitle: Option<SharedString>,
    /// Why the row cannot run here, if it cannot; never truncated.
    pub(crate) unavailable_reason: Option<SharedString>,
    /// The existing reason-node identity, supplied by the application.
    pub(crate) unavailable_id: ElementId,
    /// Whether the row is selected. Selection styling wins over hover.
    pub(crate) selected: bool,
    /// The row's icon presentation.
    pub(crate) icon: Option<(IconTone, Glyph)>,
}

/// A result row showing `content`. See the module docs for the identity,
/// accessibility and behavior the caller adds to the returned [`Div`].
pub(crate) fn result_row(content: RowContent, theme: &Theme) -> Div {
    let geometry = &theme.geometry;
    let typography = &theme.typography;
    let row = div()
        .flex_none()
        .w_full()
        .flex()
        .items_center()
        .gap(geometry.row_gap)
        .min_h(geometry.row_min_height)
        .px(geometry.row_padding_x)
        .rounded(geometry.row_radius)
        .cursor_pointer()
        .font_family(typography.family.clone())
        // A row under the pointer (unselected only: selection stays
        // visible while hovering) takes the pale hover wash.
        .when(!content.selected, |row| {
            row.hover(|row| row.bg(theme.row_hover))
        })
        // The selected row: its wash and its 1px inset edge.
        .when(content.selected, |row| {
            row.bg(theme.row_selected).shadow(vec![
                BoxShadow::new(px(0.), px(0.), theme.row_selected_border)
                    .spread_radius(px(1.))
                    .inset(),
            ])
        });

    let row = match content.icon {
        Some((tone, glyph)) => row.child(icon::tile(tone, glyph, theme)),
        None => row,
    };

    // The text group takes every remaining pixel (flex, min-width 0), so
    // the row works at any window width: the tile, the gaps and the
    // paddings are the only fixed claim, and the title and subtitle share
    // the group's one line. The title does not grow (a short subtitle
    // stays next to a short title) but shrinks and ellipsizes under
    // pressure; the subtitle takes the leftover and ellipsizes. The
    // unavailable reason sits below, wrapping within the group's width —
    // never truncated, never pushing anything out of the row.
    let line = div()
        .flex()
        .min_w(px(0.))
        .gap(px(6.))
        .child(
            div()
                .flex_initial()
                .min_w(px(0.))
                .truncate()
                .text_size(typography.row_title_size)
                .font_weight(typography.medium)
                .text_color(theme.text_title)
                .child(content.title.clone()),
        )
        .when_some(content.subtitle.clone(), |line, subtitle| {
            line.child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .truncate()
                    .text_size(typography.row_subtitle_size)
                    .text_color(theme.text_muted)
                    .child(subtitle),
            )
        });

    let group = div()
        .flex_1()
        .min_w(px(0.))
        .flex()
        .flex_col()
        .child(line)
        // The unavailable reason never truncates: it wraps within the
        // group's width, and the row's min-height floor lets the row grow.
        // Tests and smokes locate it by the Pane debug convention
        // `unavailable-reason-<title>`.
        .when_some(content.unavailable_reason.clone(), |group, reason| {
            let debug = format!("unavailable-reason-{}", content.title);
            group.child(
                div()
                    .id(content.unavailable_id.clone())
                    .pt(px(2.))
                    .text_size(typography.row_kind_size)
                    .text_color(theme.warning)
                    .debug_selector(move || debug)
                    .child(reason),
            )
        });

    row.child(group)
}
