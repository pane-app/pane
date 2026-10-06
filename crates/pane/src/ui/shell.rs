//! The launcher shell's layout: the reference root's panel size, the
//! result list that fills the space between the search header and the
//! footer, and the section labels over its rows.
//!
//! The reference fixes the panel at 760×518 — a 64px search header, a
//! 404px list and a 50px footer — and paints the panel's inner edge as an
//! inset ring, so none of the three loses a pixel to a border (see
//! [`super::material::Material::panel`]). The list here is the one the
//! launcher's search and list screens and the visual workbench's root
//! fixture (#91) both lay their rows out in, so a change to its paddings
//! reaches both.

use gpui::prelude::*;
use gpui::{AnyElement, Div, Role, SharedString, Stateful, WindowControlArea, div};

use crate::ui::theme::Theme;

/// The launcher window's client size, in logical pixels: the reference
/// root panel's 760×518 (64 search header + 404 list + 50 footer). The
/// window opens at this size; the user may resize it.
pub(crate) const LAUNCHER_CLIENT: (f32, f32) = (760., 518.);

/// The result list: the rows' column, inset by the reference root body's
/// paddings (4 above, 10 at the sides and below) with its 2px gap between
/// rows, filling the height the header and footer leave and scrolling
/// past it. The caller names it for assistive technology, tracks its
/// scroll and adds the rows.
pub(crate) fn result_list(theme: &Theme) -> Stateful<Div> {
    let geometry = &theme.geometry;
    div()
        .id("rows")
        .debug_selector(|| "rows".into())
        .role(Role::ListBox)
        .flex_1()
        .flex()
        .flex_col()
        .gap(geometry.row_list_gap)
        .px(geometry.list_padding_x)
        .pt(geometry.list_padding_top)
        .pb(geometry.list_padding_bottom)
        .overflow_y_scroll()
}

/// A section label over the result list's rows (see [`section_label`]): its
/// title and note, and the index of its first row.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SectionLabel {
    pub(crate) first: usize,
    pub(crate) label: SharedString,
    pub(crate) note: Option<SharedString>,
}

/// The result list's children: `rows` in order, each section's label
/// ahead of its first row. Each label carries the debug selector
/// `section-<label>`.
pub(crate) fn with_section_labels(
    rows: impl IntoIterator<Item = AnyElement>,
    sections: &[SectionLabel],
    theme: &Theme,
) -> Vec<AnyElement> {
    let mut children = Vec::new();
    for (index, row) in rows.into_iter().enumerate() {
        for section in sections.iter().filter(|section| section.first == index) {
            let debug = format!("section-{}", section.label);
            children.push(
                section_label(section.label.clone(), section.note.clone(), theme)
                    .debug_selector(move || debug)
                    .into_any_element(),
            );
        }
        children.push(row);
    }
    children
}

/// The list child that shows row `row`: the row comes after every label
/// at or before it (for scrolling the list to it).
pub(crate) fn child_of_row(sections: &[SectionLabel], row: usize) -> usize {
    row + sections
        .iter()
        .filter(|section| section.first <= row)
        .count()
}

/// The reference's `.label`: a 30px section label over a run of rows —
/// its title on the left, its note (lighter weight) on the right — in
/// 12px/500 with the reference's .01em of tracking, 8px above and 10px
/// either side.
pub(crate) fn section_label(label: SharedString, note: Option<SharedString>, theme: &Theme) -> Div {
    let geometry = &theme.geometry;
    let typography = &theme.typography;
    div()
        .flex_none()
        .flex()
        .items_center()
        .justify_between()
        .gap(geometry.section_gap)
        .h(geometry.section_height)
        .pt(geometry.section_padding_top)
        .px(geometry.section_padding_x)
        .text_size(typography.section_size)
        .font_weight(typography.medium)
        .letter_spacing(typography.section_size * typography.section_tracking)
        // A step brighter over a background image (ADR 0028).
        .text_color(theme.frost.map_or(theme.text_muted, |frost| frost.label))
        .child(div().child(label))
        .when_some(note, |label, note| {
            label.child(div().font_weight(typography.regular).child(note))
        })
}

/// A launcher screen's heading (every screen but the search screens, whose
/// header is the query field): the screen's `title` in the row title's
/// 14/500, 20px in from either side and 12px above and below, truncating
/// rather than eating the screen. It is also the screen's drag region:
/// with the native title bar hidden, it is the one place outside an
/// editable field to grab the window by. Shared by the launcher and the
/// visual workbench's form scenario (#99).
pub(crate) fn screen_heading(title: impl Into<SharedString>, theme: &Theme) -> Div {
    div()
        .flex_none()
        .px(theme.geometry.search_padding_x)
        .py(theme.geometry.screen_padding_y)
        .truncate()
        .text_size(theme.typography.row_title_size)
        .font_weight(theme.typography.medium)
        .text_color(theme.text_title)
        .window_control_area(WindowControlArea::Drag)
        .child(title.into())
}
