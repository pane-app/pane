//! The split view: the reference's clipboard board (#102) — a list of
//! records beside a preview of the selected one — as presentation only.
//!
//! The view is 940×600 in the reference: a 64px header (the back button,
//! the command's chip, the search field and the capture button), a 46px
//! strip of tabs with a caption on the right, the body — a 360px list with
//! its rule on the right, beside a pane whose 12px padding holds the
//! preview card — and a 52px footer: the clock and the copied line on the
//! left, the buttons on the right. Its rows are the reference's `.row`
//! with a 13.5px title and the time in Geist Mono; its section labels,
//! keycaps and footer buttons are the launcher's own families.
//!
//! Like the rest of this layer it decides nothing: the caller passes the
//! display values and attaches identity, focus and handlers (a tab and a
//! row take their id, so they can keep a press). The launcher's
//! Clipboard History adapter (`crate::features::clipboard_history`)
//! composes the view through [`compose`] and the parts below. The board's
//! source tones and its code, color, link and image previews are not
//! drawn: Pane keeps text alone and guesses no kind of it (#100).
//!
//! A window narrower than the reference keeps the view usable: the list
//! takes at most half the width, the preview the rest, and both scroll.

use gpui::prelude::*;
use gpui::{
    AnyElement, BoxShadow, Div, ElementId, Entity, Hsla, SharedString, Stateful, div, px, relative,
};
use gpui_elements::editable_text::{EditableTextState, text_input};

use crate::ui::icon::{self, Glyph, IconTone};
use crate::ui::theme::{Theme, pressed};

/// The split view's client size in the reference, in logical pixels: the
/// clipboard board's 940×600 (64 header + 46 tabs + 438 body + 52 footer).
pub(crate) const SPLIT_CLIENT: (f32, f32) = (940., 600.);

/// The whole view: `header`, `tabs`, the body — `list` beside the preview
/// pane holding `preview`, if a record is selected — and `footer` (see
/// [`footer`]).
pub(crate) fn compose(
    header: Div,
    tabs: Div,
    list: AnyElement,
    preview: Option<AnyElement>,
    footer: AnyElement,
    theme: &Theme,
) -> Div {
    let split = &theme.split;
    div()
        .size_full()
        .flex()
        .flex_col()
        .child(header)
        .child(tabs)
        .child(
            div().flex_1().min_h(px(0.)).flex().child(list).child(
                div()
                    .debug_selector(|| "clipboard-preview-pane".into())
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .p(split.preview_padding)
                    .when_some(preview, |pane, preview| pane.child(preview)),
            ),
        )
        .child(footer)
}

/// The header: `back`, `chip`, the search `field` taking the room left,
/// and `capture`, 12px apart in a 64px row with its rule below.
pub(crate) fn header(
    back: AnyElement,
    chip: Div,
    field: AnyElement,
    capture: Option<AnyElement>,
    theme: &Theme,
) -> Div {
    let split = &theme.split;
    div()
        .flex_none()
        .h(split.header_height)
        .flex()
        .items_center()
        .gap(split.header_gap)
        .pl(split.header_padding_left)
        .pr(split.header_padding_right)
        .border_b_1()
        .border_color(theme.hairline_soft)
        .child(back)
        .child(chip)
        .child(field)
        .when_some(capture, |header, capture| header.child(capture))
}

/// The back button's chrome: 32 square, radius 8, the left arrow. The
/// caller attaches its identity and click.
pub(crate) fn back_button(theme: &Theme) -> Div {
    let split = &theme.split;
    div()
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .size(split.back_size)
        .rounded(split.back_radius)
        .bg(split.back_fill)
        .cursor_pointer()
        .child(icon::glyph(
            Glyph::ArrowLeft,
            split.back_glyph,
            split.back_text,
        ))
}

/// The neutral tile's inset ring and top highlight (the reference's
/// `.tile`), on a tile of any fill.
fn tile_edges(theme: &Theme) -> Vec<BoxShadow> {
    vec![
        BoxShadow::new(px(0.), px(0.), theme.tile_border)
            .spread_radius(px(1.))
            .inset(),
        BoxShadow::new(px(0.), px(1.), theme.tile_highlight).inset(),
    ]
}

/// The command's chip: its 20px clipboard tile and `title`.
pub(crate) fn chip(title: impl Into<SharedString>, theme: &Theme) -> Div {
    let split = &theme.split;
    div()
        .debug_selector(|| "clipboard-chip".into())
        .flex_none()
        .flex()
        .items_center()
        .gap(split.chip_gap)
        .h(split.chip_height)
        .pl(split.chip_padding_left)
        .pr(split.chip_padding_right)
        .rounded(split.chip_radius)
        .bg(split.chip_fill)
        .shadow(vec![
            BoxShadow::new(px(0.), px(0.), split.chip_edge)
                .spread_radius(px(1.))
                .inset(),
        ])
        .text_size(split.chip_size)
        .font_weight(theme.typography.medium)
        .text_color(theme.text_title)
        .child(
            div()
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .size(split.chip_tile)
                .rounded(split.chip_tile_radius)
                .bg(split.chip_tile_fill)
                .shadow(tile_edges(theme))
                .child(icon::glyph(
                    Glyph::Clipboard,
                    split.chip_glyph,
                    theme.tile_foreground,
                )),
        )
        .child(div().whitespace_nowrap().child(title.into()))
}

/// The search field: the editable text element `input` (the caller's own
/// entity) in the reference's `.q` — 19px, the query's ink, the accent
/// caret — taking the room the header leaves.
pub(crate) fn search_field(
    input: &Entity<EditableTextState>,
    placeholder: impl Into<SharedString>,
    theme: &Theme,
) -> Div {
    let typography = &theme.typography;
    div().flex_1().min_w(px(0.)).child(
        text_input("clipboard-query")
            .state(input.downgrade())
            .placeholder(placeholder)
            .placeholder_color(theme.text_placeholder)
            .caret_color(theme.accent_text)
            .selection_color(theme.row_selected)
            .marked_color(theme.accent_text)
            .text_size(typography.search_size)
            .text_color(theme.text_query)
            .font_family(typography.family.clone())
            .font_features(typography.features.clone())
            .pl(theme.geometry.search_text_inset)
            .w_full()
            .min_w(px(0.))
            .whitespace_nowrap()
            .overflow_x_scroll(),
    )
}

/// The capture button (the reference's Pause): `glyph` and `label` in a
/// footer button's chrome, in the accent while `pressed` (paused). The
/// caller attaches its click.
pub(crate) fn capture_button(
    label: impl Into<SharedString>,
    glyph: Glyph,
    pressed: bool,
    theme: &Theme,
) -> Stateful<Div> {
    let geometry = &theme.geometry;
    let color = if pressed {
        theme.accent_text
    } else {
        theme.footer_button_text
    };
    div()
        .id("clipboard-capture")
        .debug_selector(|| "clipboard-capture".into())
        .flex_none()
        .h(geometry.action_height)
        .flex()
        .items_center()
        .gap(geometry.action_gap)
        .px(geometry.action_padding_x)
        .rounded(geometry.action_radius)
        .text_size(theme.typography.footer_size)
        .font_weight(theme.typography.medium)
        .text_color(color)
        .hover(|button| button.bg(theme.control_hover))
        .active(|button| button.bg(crate::ui::theme::pressed(theme.control_hover)))
        .cursor_pointer()
        .child(icon::glyph(glyph, theme.split.capture_glyph, color))
        .child(div().whitespace_nowrap().child(label.into()))
}

/// A tab, `id`: `label`, chosen or not (`on`). While held it takes the
/// [`pressed`] wash of its hover, or of its chosen wash, at once. The
/// caller attaches its click.
pub(crate) fn tab(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    on: bool,
    theme: &Theme,
) -> Stateful<Div> {
    let split = &theme.split;
    let tab = div()
        .id(id)
        .flex_none()
        .flex()
        .items_center()
        .h(split.tab_height)
        .px(split.tab_padding_x)
        .rounded(split.tab_radius)
        .text_size(split.tab_size)
        .font_weight(theme.typography.medium)
        .cursor_pointer()
        .child(label.into());
    if on {
        tab.bg(split.tab_on)
            .text_color(split.tab_on_text)
            .shadow(vec![
                BoxShadow::new(px(0.), px(0.), split.tab_on_edge)
                    .spread_radius(px(1.))
                    .inset(),
            ])
            .active(|tab| tab.bg(pressed(split.tab_on)))
    } else {
        tab.text_color(split.tab_text)
            .hover(|tab| tab.bg(split.tab_hover).text_color(split.tab_hover_text))
            .active(|tab| {
                tab.bg(pressed(split.tab_hover))
                    .text_color(split.tab_hover_text)
            })
    }
}

/// The tab strip: `tabs` from the left, 4px apart, and the caption — what
/// is kept, as `caption` says it, in its color — on the right.
pub(crate) fn tabs(
    tabs: Vec<AnyElement>,
    caption: Option<(SharedString, Hsla)>,
    theme: &Theme,
) -> Div {
    let split = &theme.split;
    div()
        .flex_none()
        .h(split.tabs_height)
        .flex()
        .items_center()
        .gap(split.tabs_gap)
        .px(split.tabs_padding_x)
        .border_b_1()
        .border_color(theme.hairline_soft)
        .children(tabs)
        .child(div().flex_1().min_w(px(0.)))
        .when_some(caption, |strip, (text, color)| {
            strip.child(
                div()
                    .debug_selector(|| "clipboard-caption".into())
                    .flex_initial()
                    .min_w(px(0.))
                    .flex()
                    .items_center()
                    .gap(split.caption_gap)
                    .text_size(split.caption_size)
                    .text_color(color)
                    .child(icon::glyph(Glyph::Shield, split.caption_glyph, color).flex_none())
                    .child(div().min_w(px(0.)).truncate().child(text)),
            )
        })
}

/// The list column: 360 wide (at most half a narrower window's), its rule
/// on the right, padded 2 above, 8 either side and 10 below, its children
/// 2px apart, scrolling with no scroll bar. The caller adds the section
/// labels and rows, and tracks its scroll.
pub(crate) fn list(theme: &Theme) -> Stateful<Div> {
    let split = &theme.split;
    div()
        .id("clipboard-list")
        .debug_selector(|| "clipboard-list".into())
        .flex_none()
        .w(split.list_width)
        .max_w(relative(split.list_max_share))
        .flex()
        .flex_col()
        .gap(theme.geometry.row_list_gap)
        .pt(split.list_padding_top)
        .px(split.list_padding_x)
        .pb(split.list_padding_bottom)
        .border_r_1()
        .border_color(theme.hairline_soft)
        .overflow_y_scroll()
}

/// What a record's row shows.
#[derive(Clone, Debug)]
pub(crate) struct ClipRow {
    pub(crate) title: SharedString,
    pub(crate) time: SharedString,
    pub(crate) selected: bool,
    /// The glyph on the row's neutral tile.
    pub(crate) glyph: Glyph,
}

/// A record's row (the reference's `.row`): 44 high, radius 10, 10px
/// either side, 12 between its mark, its 13.5px/500 title (truncating)
/// and its time in Geist Mono 11.5. The hover wash shows on an unselected
/// row; the selected one keeps its wash and inset edge. While held, a row
/// takes the [`pressed`] wash of its hover, or of its selected wash, at
/// once. The row is `id`; the caller attaches accessibility and the click,
/// which selects.
pub(crate) fn clip_row(id: impl Into<ElementId>, row: ClipRow, theme: &Theme) -> Stateful<Div> {
    let geometry = &theme.geometry;
    let split = &theme.split;
    let press = pressed(if row.selected {
        theme.row_selected
    } else {
        theme.row_hover
    });
    div()
        .id(id)
        .active(move |line| line.bg(press))
        .flex_none()
        .w_full()
        .h(geometry.row_min_height)
        .flex()
        .items_center()
        .gap(geometry.row_gap)
        .px(geometry.row_padding_x)
        .rounded(geometry.row_radius)
        .cursor_pointer()
        .when(!row.selected, |line| {
            line.hover(|line| line.bg(theme.row_hover))
        })
        .when(row.selected, |line| {
            line.bg(theme.row_selected).shadow(vec![
                BoxShadow::new(px(0.), px(0.), theme.row_selected_border)
                    .spread_radius(px(1.))
                    .inset(),
            ])
        })
        .child(icon::tile(IconTone::Command, row.glyph, theme))
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .truncate()
                .text_size(split.title_size)
                .font_weight(theme.typography.medium)
                .text_color(theme.text_title)
                .child(row.title),
        )
        .child(
            div()
                .flex_none()
                .font_family(theme.typography.mono_family.clone())
                .text_size(split.time_size)
                .text_color(theme.text_muted)
                .child(row.time),
        )
}

/// The note in the list's place when it lists nothing: centered 13px muted
/// text, 40 above and below.
pub(crate) fn empty_note(text: impl Into<SharedString>, theme: &Theme) -> Div {
    let split = &theme.split;
    div()
        .debug_selector(|| "clipboard-empty".into())
        .flex_none()
        .py(split.empty_padding_y)
        .px(split.empty_padding_x)
        .text_center()
        .text_size(split.empty_size)
        .text_color(theme.text_muted)
        .child(text.into())
}

/// The preview card around `content`: filling the pane, radius 12, black
/// 24% with a white 7% inset ring, clipping its content and scrolling a
/// long one. `id` names the record previewed, so another record's preview
/// starts at its top.
pub(crate) fn preview_card(id: ElementId, content: AnyElement, theme: &Theme) -> Stateful<Div> {
    let split = &theme.split;
    div()
        .id(id)
        .debug_selector(|| "clipboard-preview".into())
        .relative()
        .flex_1()
        .min_h(px(0.))
        .rounded(split.preview_radius)
        .bg(split.preview_fill)
        .shadow(vec![
            BoxShadow::new(px(0.), px(0.), split.preview_edge)
                .spread_radius(px(1.))
                .inset(),
        ])
        .overflow_y_scroll()
        .child(content)
}

/// Plain text, previewed as it was copied: its lines and spaces kept,
/// wrapping within the card, 20px at line height 1.5, padded 28 by 30.
/// Pane previews every record so: it guesses no other kind.
pub(crate) fn text_preview(text: impl Into<SharedString>, theme: &Theme) -> Div {
    let split = &theme.split;
    div()
        .debug_selector(|| "clipboard-preview-text".into())
        .px(split.text_padding_x)
        .py(split.text_padding_y)
        .text_size(split.text_size)
        .line_height(split.text_size * split.text_line_height)
        .letter_spacing(split.text_size * split.text_tracking)
        .text_color(theme.text_title)
        .child(text.into())
}

/// The footer's left side: the clock and `text` — when and where the
/// selected record was copied, or the launcher's status — in `color`, on
/// one line.
pub(crate) fn footer_lead(text: impl Into<SharedString>, color: Hsla, theme: &Theme) -> Div {
    let split = &theme.split;
    div()
        .debug_selector(|| "clipboard-copied".into())
        .flex_initial()
        .min_w(px(0.))
        .flex()
        .items_center()
        .gap(split.footer_lead_gap)
        .whitespace_nowrap()
        .text_size(theme.typography.footer_size)
        .text_color(color)
        .child(icon::glyph(Glyph::Clock, split.footer_glyph, color).flex_none())
        .child(div().min_w(px(0.)).truncate().child(text.into()))
}

/// The footer's buttons, left to right, as the reference orders them:
/// `primary` (the selected record's, with the accent key) and `secondary`
/// (its other actions, in order), the rule, then `more`; the rule only with
/// a button before it.
pub(crate) fn footer_buttons(
    primary: Option<AnyElement>,
    secondary: Vec<AnyElement>,
    more: AnyElement,
    theme: &Theme,
) -> Vec<AnyElement> {
    let rule = (primary.is_some() || !secondary.is_empty())
        .then(|| crate::ui::footer::divider(theme).into_any_element());
    primary
        .into_iter()
        .chain(secondary)
        .chain(rule)
        .chain(std::iter::once(more))
        .collect()
}

/// The footer: 52 high with its rule above and the footer's wash, `lead`
/// on the left and `buttons` (the launcher's footer buttons and rule) on
/// the right, 4px apart.
pub(crate) fn footer(lead: Div, buttons: Vec<AnyElement>, theme: &Theme) -> Div {
    let split = &theme.split;
    let geometry = &theme.geometry;
    div()
        .flex_none()
        .h(split.footer_height)
        .flex()
        .items_center()
        .justify_between()
        .gap(split.footer_gap)
        .pl(geometry.footer_padding_left)
        .pr(geometry.footer_padding_right)
        .border_t_1()
        .border_color(theme.hairline_soft)
        .bg(theme.footer_tint)
        .child(lead)
        .child(
            div()
                .flex_initial()
                .min_w(px(0.))
                .flex()
                .items_center()
                .gap(geometry.footer_buttons_gap)
                .children(buttons),
        )
}
