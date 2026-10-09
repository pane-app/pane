//! The launcher's result row: presentation only.
//!
//! The row is the reference's `.row`: 44px (a floor — a row whose
//! unavailable reason wraps grows taller rather than clipping it), radius
//! 10, 10px side padding and a 12px gap between all of its parts — the
//! icon (Pane's tile, or an extension's icon drawn bare, #139), the
//! 14px/500 title, the 13px subtitle and, on the right, an extension
//! item's accessories (text, a relative date, a coloured tag; #139), the
//! optional alias chip, key sequence and kind. A title, subtitle or
//! accessory with a tooltip shows it while the pointer rests on it. A pale
//! wash marks hover and selection; selection stays visible while hovering:
//! a selected row keeps its wash and inset edge and does not switch to the
//! hover wash. Neither wash fades: the reference's row changes at once.
//!
//! This component owns no identity and no behavior. It returns a plain
//! [`Div`] so the app attaches everything behavioral on top:
//!
//! - `.id(("row", index))` — the stable id (making the row stateful for
//!   scrolling and hit-testing) and `.debug_selector(...)` for the smokes,
//! - the pressed wash ([`pressed_wash`]), attached after the id, because a
//!   press state needs the named (stateful) row,
//! - the accessibility contract — `.role(Role::ListBoxOption)`,
//!   `.aria_selected`, its position in the list and the list's size,
//!   `.aria_disabled` with a description when the reason is present (no
//!   active descendant: the window's announcer says the selection, #132),
//! - `.on_click(...)`, pointer movement, focus and any key handling.
//!
//! The row registers no handlers and no focus of its own, so nothing here
//! swallows events. Layout: a body (flex, min-width 0) holds the title and
//! the subtitle on one line — the title shrinks and ellipsizes under
//! pressure but does not grow, the subtitle takes the leftover and
//! ellipsizes — and the unavailable reason below it, wrapping within the
//! body's width: never truncated, and the row grows past its 44px floor to
//! fit it. The trailing parts never shrink, so the body gives way first.
//! The reason element carries the Pane debug convention
//! `unavailable-reason-<title>` for tests and smokes, the title and
//! subtitle `row-title-<title>` and `row-subtitle-<title>`, and each
//! accessory `accessory-<title>-<n>-<text>`, counting from 0.

use std::ops::Range;

use gpui::prelude::*;
use gpui::{
    BoxShadow, Div, ElementId, HighlightStyle, Hsla, SharedString, Stateful, StyledText, div, px,
};

use crate::ui::extension_icon::{self, DrawnIcon, IconSize, RowIcon};
use crate::ui::icon::TileSize;
use crate::ui::keycap::{self, CapStyle, KeySequence};
use crate::ui::theme::{Theme, pressed};
use crate::ui::tooltip::{TooltipLook, text_tooltip};

/// What a result row shows — plain presentation values, already resolved
/// by the caller from whatever the launcher holds. Nothing here derives
/// presentation from content: the caller maps identities to
/// [`RowIcon`]s (Pane's own tiles, unknown ones the command tile, or an
/// extension's icon).
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
    pub(crate) icon: Option<RowIcon>,
}

/// What a root result row shows beyond its content, when the launcher has
/// it: where the query matched the title, the alias and key sequence the
/// user gave its command, and its kind. Each part is drawn only when
/// present; the default draws none.
#[derive(Clone, Debug, Default)]
pub(crate) struct RowMeta {
    /// Byte ranges of the title the query matched, drawn in the accent.
    pub(crate) matched: Vec<Range<usize>>,
    /// The alias chip (`.alias`).
    pub(crate) alias: Option<SharedString>,
    /// The key sequence (`.keys`).
    pub(crate) keys: Option<KeySequence>,
    /// The right-aligned kind (`.row-kind`).
    pub(crate) kind: Option<SharedString>,
    /// The row's number and its hint's look (0 hidden, 1 shown) while Ctrl
    /// is held: the cap slides in over the row's right end.
    pub(crate) number: Option<(usize, f32)>,
    /// Shown while the pointer rests on the title (#139).
    pub(crate) title_tooltip: Option<SharedString>,
    /// Shown while the pointer rests on the subtitle (#139).
    pub(crate) subtitle_tooltip: Option<SharedString>,
    /// An extension item's accessories, on the right, in order (#139).
    pub(crate) accessories: Vec<AccessoryLook>,
}

/// One accessory as a row draws it (#139), resolved by the caller.
#[derive(Clone, Debug)]
pub(crate) struct AccessoryLook {
    /// Its text: a count, a date relative to now, a tag's name; empty for
    /// an icon alone.
    pub(crate) text: SharedString,
    /// Whether it is a tag: its text on a wash of its colour, rounded.
    pub(crate) tag: bool,
    /// The colour of its text, already corrected for contrast.
    pub(crate) color: Hsla,
    pub(crate) icon: Option<DrawnIcon>,
    /// Shown while the pointer rests on it.
    pub(crate) tooltip: Option<SharedString>,
}

/// A result row showing `content` with `meta`'s parts. See the module docs
/// for the identity, accessibility and behavior the caller adds to the
/// returned [`Div`].
pub(crate) fn result_row_with(content: RowContent, meta: RowMeta, theme: &Theme) -> Div {
    let geometry = &theme.geometry;
    let typography = &theme.typography;
    let row = row_surface(content.selected, theme).font_family(typography.family.clone());

    let row = match &content.icon {
        Some(icon) => row.child(extension_icon::row_icon_at(
            icon,
            TileSize::Row,
            "row-icon",
            &content.title,
            theme,
        )),
        None => row,
    };
    let tooltip = TooltipLook::of(theme);

    // The title, its matched parts in the accent.
    let title = StyledText::new(content.title.clone()).with_highlights(
        meta.matched
            .iter()
            .filter(|range| {
                range.end <= content.title.len()
                    && content.title.is_char_boundary(range.start)
                    && content.title.is_char_boundary(range.end)
            })
            .map(|range| {
                (
                    range.clone(),
                    HighlightStyle {
                        color: Some(theme.accent_text),
                        ..Default::default()
                    },
                )
            }),
    );
    let title_selector = format!("row-title-{}", content.title);
    let subtitle_selector = format!("row-subtitle-{}", content.title);
    let line = div()
        .flex()
        .min_w(px(0.))
        .gap(geometry.row_gap)
        .child(
            div()
                .id("row-title")
                .debug_selector(move || title_selector)
                .flex_initial()
                .min_w(px(0.))
                .truncate()
                .text_size(typography.row_title_size)
                .font_weight(typography.medium)
                .text_color(theme.text_title)
                .child(title)
                .when_some(meta.title_tooltip.clone(), |title, tip| {
                    title.tooltip(text_tooltip(tip, tooltip.clone()))
                }),
        )
        .when_some(content.subtitle.clone(), |line, subtitle| {
            line.child(
                div()
                    .id("row-subtitle")
                    .debug_selector(move || subtitle_selector)
                    .flex_1()
                    .min_w(px(0.))
                    .truncate()
                    .text_size(typography.row_subtitle_size)
                    .text_color(theme.text_secondary)
                    .child(subtitle)
                    .when_some(meta.subtitle_tooltip.clone(), |subtitle, tip| {
                        subtitle.tooltip(text_tooltip(tip, tooltip.clone()))
                    }),
            )
        });

    let body = div()
        .flex_1()
        .min_w(px(0.))
        .flex()
        .flex_col()
        .child(line)
        // The unavailable reason never truncates: it wraps within the
        // body's width, and the row's min-height floor lets the row grow.
        // Tests and smokes locate it by the Pane debug convention
        // `unavailable-reason-<title>`.
        .when_some(content.unavailable_reason.clone(), |body, reason| {
            let debug = format!("unavailable-reason-{}", content.title);
            body.child(
                div()
                    .id(content.unavailable_id.clone())
                    .pt(px(2.))
                    .text_size(typography.row_kind_size)
                    .text_color(theme.warning)
                    .debug_selector(move || debug)
                    .child(reason),
            )
        });

    let accessories: Vec<Stateful<Div>> = meta
        .accessories
        .iter()
        .enumerate()
        .map(|(index, accessory)| accessory_element(index, accessory, &content.title, theme))
        .collect();
    row.child(body)
        .children(accessories)
        .when_some(meta.alias, |row, alias| row.child(alias_chip(alias, theme)))
        .when_some(meta.keys, |row, keys| {
            // Its own scope: the key sequence's id is fixed.
            row.child(div().id("row-keys").flex_none().child(keycap::key_sequence(
                &keys,
                CapStyle::Regular,
                theme,
            )))
        })
        .when_some(meta.kind, |row, kind| {
            row.child(
                div()
                    .flex_none()
                    .min_w(geometry.row_kind_min_width)
                    .text_right()
                    .text_size(typography.row_kind_size)
                    .text_color(theme.text_secondary)
                    .child(kind),
            )
        })
        .when_some(meta.number, |row, (number, look)| {
            with_number_hint(row, number, look, theme)
        })
}

/// The accessory at `index` of the row titled `row`: its icon and text,
/// a tag's on a wash of its colour, with its tooltip on hover (#139).
fn accessory_element(
    index: usize,
    accessory: &AccessoryLook,
    row: &str,
    theme: &Theme,
) -> Stateful<Div> {
    let selector = format!("accessory-{row}-{index}-{}", accessory.text);
    let scope = format!("{row}-accessory-{index}");
    let mut element = div()
        .id(("accessory", index))
        .debug_selector(move || selector)
        .flex_none()
        .flex()
        .items_center()
        .gap(px(4.))
        .text_size(theme.typography.row_kind_size)
        .text_color(accessory.color);
    if let Some(icon) = &accessory.icon {
        element = element.child(extension_icon::draw(
            icon,
            IconSize::small(px(14.)),
            ("accessory-icon", index),
            &scope,
            theme,
        ));
    }
    if !accessory.text.is_empty() {
        element = element.child(accessory.text.clone());
    }
    if accessory.tag {
        element = element.px(px(6.)).py(px(1.)).rounded(px(5.)).bg(Hsla {
            alpha: 0.16,
            ..accessory.color
        });
    }
    element.when_some(accessory.tooltip.clone(), |element, tip| {
        element.tooltip(text_tooltip(tip, TooltipLook::of(theme)))
    })
}

/// `row` with `number`'s hint over its right end at `look` (see
/// [`keycap::row_number_hint`]): the row positions and clips it, and the
/// hint keeps the row's rounded right corners.
pub(crate) fn with_number_hint<E: ParentElement + Styled>(
    row: E,
    number: usize,
    look: f32,
    theme: &Theme,
) -> E {
    let radius = theme.geometry.row_radius;
    row.relative().overflow_hidden().child(
        keycap::row_number_hint(number, look, theme)
            .rounded_tr(radius)
            .rounded_br(radius),
    )
}

/// The wash a row takes while pressed: the [`pressed`] wash of its hover,
/// or of its selected wash while `selected`. The app attaches it after the
/// row's id (see the module docs) — an adaptation: the reference authors
/// no press for its rows.
pub(crate) fn pressed_wash(selected: bool, theme: &Theme) -> Hsla {
    pressed(if selected {
        theme.row_selected
    } else {
        theme.row_hover
    })
}

/// A row's surface (`.row`): at least 44 high, radius 10, 10px either side
/// and 12 between its parts, with the pale hover wash while unselected,
/// and the selected wash and its 1px inset edge while `selected` (a
/// selected row keeps them under the pointer): the result row's.
fn row_surface(selected: bool, theme: &Theme) -> Div {
    let geometry = &theme.geometry;
    div()
        .flex_none()
        .w_full()
        .flex()
        .items_center()
        .gap(geometry.row_gap)
        .min_h(geometry.row_min_height)
        .px(geometry.row_padding_x)
        .rounded(geometry.row_radius)
        .cursor_pointer()
        // A row under the pointer (unselected only: selection stays
        // visible while hovering) takes the pale hover wash.
        .when(!selected, |row| row.hover(|row| row.bg(theme.row_hover)))
        // The selected row: its wash and its 1px inset edge — frosted over
        // a background image (ADR 0028).
        .when(selected, |row| {
            row.when_some(theme.frost, |row, frost| row.backdrop_blur(frost.blur))
                .bg(theme.row_selected)
                .shadow(vec![
                    BoxShadow::new(px(0.), px(0.), theme.row_selected_border)
                        .spread_radius(px(1.))
                        .inset(),
                ])
        })
}

/// The reference's `.alias`: the alias in Geist Mono 11 inside a 1px
/// ring, 2px by 6px of padding, radius 5.
fn alias_chip(alias: SharedString, theme: &Theme) -> Div {
    let geometry = &theme.geometry;
    let typography = &theme.typography;
    div()
        .flex_none()
        .py(geometry.alias_padding_y)
        .px(geometry.alias_padding_x)
        .rounded(geometry.alias_radius)
        .shadow(vec![
            BoxShadow::new(px(0.), px(0.), theme.alias_edge)
                .spread_radius(px(1.))
                .inset(),
        ])
        .font_family(typography.mono_family.clone())
        .text_size(typography.alias_size)
        // CSS's `normal` line height for Geist Mono: its ascent and
        // descent, 1.3 em — the reference's chip is 18px tall.
        .line_height(typography.alias_size * typography.mono_line_height)
        .text_color(theme.alias_text)
        .child(alias)
}
