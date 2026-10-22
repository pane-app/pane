//! The launcher shell's layout: the reference root's panel size, the
//! result list that fills the space between the search header and the
//! footer, and the section labels over its rows.
//!
//! The reference fixes the panel at 760×518 — a 64px search header, a
//! 404px list and a 50px footer — and paints the panel's inner edge as an
//! inset ring, so none of the three loses a pixel to a border (see
//! [`super::material::Material::panel`]). The list here is the one the
//! launcher's search and list screens lay their rows out in.

use gpui::prelude::*;
use gpui::{Div, Role, SharedString, Stateful, WindowControlArea, div};

use crate::ui::theme::Theme;

/// The launcher window's client size, in logical pixels: the reference
/// root panel's 760×518 (64 search header + 404 list + 50 footer). The
/// window opens at this size; the user may resize it.
pub(crate) const LAUNCHER_CLIENT: (f32, f32) = (760., 518.);

/// The result list: the rows' column, filling the height the header and
/// footer leave. The caller names it for assistive technology and adds
/// the virtualized list that scrolls inside it (#165; see
/// [`super::virtual_list`]), inset by the reference root body's paddings
/// (4 above, 10 at the sides and below) with its 2px gap between rows.
pub(crate) fn result_list(_theme: &Theme) -> Stateful<Div> {
    div()
        .id("rows")
        .debug_selector(|| "rows".into())
        .role(Role::ListBox)
        .flex_1()
        .min_h(gpui::px(0.))
        .flex()
        .flex_col()
}

/// A section label over the result list's rows (see [`section_label`]): its
/// title and note, and the index of its first row.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SectionLabel {
    pub(crate) first: usize,
    pub(crate) label: SharedString,
    pub(crate) note: Option<SharedString>,
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
        // The tertiary level (ADR 0035): the ink at 40% wherever it reads,
        // the accepted colour wherever it would not.
        .text_color(theme.text_tertiary)
        .child(div().child(label))
        .when_some(note, |label, note| {
            label.child(div().font_weight(typography.regular).child(note))
        })
}

/// A launcher screen's heading (the core's own screens: a package's
/// preview, a confirmation, the details and hotkey screens; root search
/// and an extension's views have none, #162): the
/// screen's `title` in the row title's 14/500, 20px in from either side
/// and 12px above and below, truncating rather than eating the screen. It
/// is also the screen's drag region: with the native title bar hidden, it
/// is a place outside an editable field to grab the window by (#99).
pub(crate) fn screen_heading(title: impl Into<SharedString>, theme: &Theme) -> Div {
    div()
        .debug_selector(|| "screen-heading".into())
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
