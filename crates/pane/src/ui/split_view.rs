//! The split view: the reference's clipboard board (#102) — a list of
//! records beside the selected one's detail — as presentation only, as
//! Raycast's Clipboard History draws it (#166).
//!
//! The view is 940×600: a 64px header (the back button, the search field
//! and the type dropdown at its right), the body — a 360px list with its
//! rule on the right, beside the detail pane, whose 12px padding holds the
//! preview card over the record's Information — and a 52px footer: the
//! open command's icon and title on the left (#162; or the launcher's
//! status), the buttons on
//! the right. Its rows are the reference's `.row` with a 13.5px title and
//! the time in Geist Mono; its section labels, keycaps and footer buttons
//! are the launcher's own families. A copied image's row shows its
//! thumbnail and its preview the image; copied files' row shows the first
//! one's system icon and its preview lists them (#167).
//!
//! Like the rest of this layer it decides nothing: the caller passes the
//! display values and attaches identity, focus and handlers (a row takes
//! its id, so it can keep a press). The launcher's Clipboard History
//! adapter (`crate::features::clipboard_history`) composes the view
//! through [`compose`] and the parts below.
//!
//! Search Files (#177) is the view's second user
//! (`crate::features::search_files`): its rows are files ([`file_row`]),
//! with the system's icon and the folder; its detail previews an image
//! ([`file_image_preview`]) over the file's Metadata ([`info_section`]).
//!
//! A window narrower than the reference keeps the view usable: the list
//! takes at most half the width, the detail the rest, and both scroll.

use std::path::PathBuf;

use gpui::prelude::*;
use gpui::{
    AnyElement, BoxShadow, Div, ElementId, Entity, Hsla, ObjectFit, SharedString, Stateful, div,
    img, px, relative,
};
use gpui_elements::editable_text::{EditableTextState, text_input};

use crate::ui::icon::{self, Glyph, IconTone};
use crate::ui::theme::{Theme, faded, pressed};

/// The split view's client size in the reference, in logical pixels: the
/// clipboard board's 940×600.
pub(crate) const SPLIT_CLIENT: (f32, f32) = (940., 600.);

/// The whole view: `header`, the body — `list` beside the detail pane
/// holding `detail`, if a record is selected — and `footer` (see
/// [`footer`]).
pub(crate) fn compose(
    header: Div,
    list: AnyElement,
    detail: Option<AnyElement>,
    footer: AnyElement,
) -> Div {
    div()
        .size_full()
        .flex()
        .flex_col()
        .child(header)
        .child(
            div().flex_1().min_h(px(0.)).flex().child(list).child(
                div()
                    .debug_selector(|| "clipboard-preview-pane".into())
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .when_some(detail, |pane, detail| pane.child(detail)),
            ),
        )
        .child(footer)
}

/// The header: `back`, the search `field` taking the room left, and the
/// type `dropdown` at its right, 12px apart in a 64px row with its rule
/// below. No badge or chip sits on the field: the command shows in the
/// footer.
pub(crate) fn header(
    back: AnyElement,
    field: AnyElement,
    dropdown: AnyElement,
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
        .border_color(theme.separator)
        .child(back)
        .child(field)
        .child(
            div()
                .debug_selector(|| "clipboard-type-dropdown".into())
                .flex_none()
                .child(dropdown),
        )
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
            .placeholder_color(theme.query_placeholder)
            .caret_color(theme.accent_text)
            .selection_color(theme.selection_wash)
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

/// The list column: 360 wide (at most half a narrower window's), its rule
/// on the right. The caller adds the virtualized list that scrolls inside
/// it, with no scroll bar (#165; see [`super::virtual_list`]), padded 2
/// above, 8 either side and 10 below, its children 2px apart: the section
/// labels and rows.
pub(crate) fn list(theme: &Theme) -> Stateful<Div> {
    let split = &theme.split;
    div()
        .id("clipboard-list")
        .debug_selector(|| "clipboard-list".into())
        .flex_none()
        .w(split.list_width)
        .max_w(relative(split.list_max_share))
        .min_h(px(0.))
        .flex()
        .flex_col()
        .border_r_1()
        .border_color(theme.separator)
}

/// What a record's row shows.
#[derive(Clone, Debug)]
pub(crate) struct ClipRow {
    pub(crate) title: SharedString,
    pub(crate) time: SharedString,
    pub(crate) selected: bool,
    /// The hover wash's strength, 0 to 1 (see `crate::app::hover_wash`):
    /// full while the pointer rests on the row, fading once it has left.
    pub(crate) hover: f32,
}

/// A text record's mark: `glyph` (its kind) on the row's neutral tile.
pub(crate) fn kind_mark(glyph: Glyph, theme: &Theme) -> AnyElement {
    icon::tile(IconTone::Command, glyph, theme).into_any_element()
}

/// A copied image's mark (#167): the image at `path` (the PNG the history
/// keeps), filling the row's tile box, its corners the tile's, over the
/// preview's surface while it loads.
pub(crate) fn thumbnail_mark(path: PathBuf, theme: &Theme) -> AnyElement {
    let tile = theme.geometry.tile;
    div()
        .debug_selector(|| "clip-thumbnail".into())
        .flex_none()
        .size(tile.size)
        .rounded(tile.radius)
        .overflow_hidden()
        .bg(theme.split.preview_fill)
        .child(
            img(path)
                .size(tile.size)
                .rounded(tile.radius)
                .object_fit(ObjectFit::Cover),
        )
        .into_any_element()
}

/// A record's row (the reference's `.row`): 44 high, radius 10, 10px
/// either side, 12 between its `mark` (its kind's tile, an image's
/// thumbnail or a file's system icon), its 13.5px/500 title (truncating)
/// and its time in Geist Mono 11.5. The hover wash shows on an unselected
/// row at `hover`'s strength, fading out once the pointer has left (#245);
/// the selected one keeps its wash, with no inset edge (ADR 0035). While
/// held, a row takes the [`pressed`] wash of its hover, or of its
/// selected wash, at once. The row is `id`; the caller attaches its
/// hover report, accessibility and the click, which selects.
pub(crate) fn clip_row(
    id: impl Into<ElementId>,
    row: ClipRow,
    mark: AnyElement,
    theme: &Theme,
) -> Stateful<Div> {
    let geometry = &theme.geometry;
    let split = &theme.split;
    let press = pressed(if row.selected {
        theme.selection_wash
    } else {
        theme.hover_wash
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
        .when(!row.selected && row.hover > 0., |line| {
            line.bg(faded(theme.hover_wash, row.hover))
        })
        .when(row.selected, |line| line.bg(theme.selection_wash))
        .child(mark)
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

/// What a found file's row shows (Search Files, #177).
pub(crate) struct FoundFile {
    pub(crate) title: SharedString,
    /// Its folder below the home folder (`~/…`).
    pub(crate) subtitle: SharedString,
    pub(crate) selected: bool,
    /// The hover wash's strength, 0 to 1 (see `crate::app::hover_wash`):
    /// full while the pointer rests on the row, fading once it has left.
    pub(crate) hover: f32,
    /// The system's icon for the file, drawn bare.
    pub(crate) icon: AnyElement,
}

/// A file's row: a record's row (see [`clip_row`]) with the file's own
/// icon in the tile's place and its folder, muted and truncating first,
/// after its title. The caller attaches identity, accessibility, its
/// hover report and the click.
pub(crate) fn file_row(id: impl Into<ElementId>, row: FoundFile, theme: &Theme) -> Stateful<Div> {
    let geometry = &theme.geometry;
    let split = &theme.split;
    let press = pressed(if row.selected {
        theme.selection_wash
    } else {
        theme.hover_wash
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
        .when(!row.selected && row.hover > 0., |line| {
            line.bg(faded(theme.hover_wash, row.hover))
        })
        .when(row.selected, |line| line.bg(theme.selection_wash))
        .child(div().flex_none().child(row.icon))
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .items_baseline()
                .gap(split.info_gap)
                .child(
                    div()
                        .flex_shrink_0()
                        .max_w(relative(0.7))
                        .truncate()
                        .text_size(split.title_size)
                        .font_weight(theme.typography.medium)
                        .text_color(theme.text_title)
                        .child(row.title),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .truncate()
                        .text_size(split.time_size)
                        .text_color(theme.text_muted)
                        .child(row.subtitle),
                ),
        )
}

/// An image file, previewed whole in the preview card (Search Files,
/// #177): the file at `path`, scaled down to fit the card with its
/// proportions kept, centered, padded as text is. A copied image's
/// preview is [`image_preview`] (#167).
pub(crate) fn file_image_preview(path: PathBuf, theme: &Theme) -> Div {
    let split = &theme.split;
    div()
        .debug_selector(|| "files-preview-image".into())
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .p(split.text_padding_y)
        .child(img(path).size_full().object_fit(ObjectFit::Contain))
}

/// A file that is no image, in the preview card: its `icon`, large and
/// centered.
pub(crate) fn icon_preview(icon: AnyElement) -> Div {
    div()
        .debug_selector(|| "files-preview-icon".into())
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .child(icon)
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

/// The detail pane's content: `preview` (the card) filling what
/// `information` leaves, padded 12.
pub(crate) fn detail(preview: AnyElement, information: Div, theme: &Theme) -> Div {
    div()
        .flex_1()
        .min_h(px(0.))
        .flex()
        .flex_col()
        .p(theme.split.preview_padding)
        .child(preview)
        .child(information)
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

/// A copied image, previewed (#167): the image at `path` (the PNG the
/// history keeps) fit inside the card, centred, padded as text is.
pub(crate) fn image_preview(path: PathBuf, theme: &Theme) -> Div {
    let split = &theme.split;
    div()
        .debug_selector(|| "clipboard-preview-image".into())
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .px(split.text_padding_x)
        .py(split.text_padding_y)
        .child(img(path).size_full().object_fit(ObjectFit::Contain))
}

/// One copied file in a files record's preview: its system `icon`, its
/// name and the folder it is in.
pub(crate) struct FileLine {
    pub(crate) icon: AnyElement,
    pub(crate) name: SharedString,
    pub(crate) folder: SharedString,
}

/// Copied files, previewed (#167): one line each, in the order copied,
/// with its icon, its name and, muted, the folder it is in; padded as
/// text is.
pub(crate) fn files_preview(files: Vec<FileLine>, theme: &Theme) -> Div {
    let split = &theme.split;
    let tile = theme.geometry.tile;
    div()
        .debug_selector(|| "clipboard-preview-files".into())
        .flex()
        .flex_col()
        .gap(split.info_gap)
        .px(split.text_padding_x)
        .py(split.text_padding_y)
        .children(files.into_iter().map(|file| {
            let selector = format!("clipboard-preview-file-{}", file.name);
            div()
                .debug_selector(move || selector)
                .flex_none()
                .flex()
                .items_center()
                .gap(theme.geometry.row_gap)
                .child(div().flex_none().size(tile.size).child(file.icon))
                .child(
                    div()
                        .min_w(px(0.))
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .truncate()
                                .text_size(split.title_size)
                                .font_weight(theme.typography.medium)
                                .text_color(theme.text_title)
                                .child(file.name),
                        )
                        .child(
                            div()
                                .truncate()
                                .text_size(split.info_size)
                                .text_color(theme.text_muted)
                                .child(file.folder),
                        ),
                )
        }))
}

/// One row of the Information: its `label` on the left, muted, and its
/// `value` on the right, after `icon` if it has one.
pub(crate) struct InfoRow {
    pub(crate) label: &'static str,
    pub(crate) value: SharedString,
    pub(crate) icon: Option<AnyElement>,
}

/// The Information under the preview (#166), as Raycast's detail has it:
/// "Information" over its rows, each a hairline apart, 12px.
pub(crate) fn information(rows: Vec<InfoRow>, theme: &Theme) -> Div {
    info_section("Information", "clipboard", rows, theme)
}

/// A section of facts under the preview: `heading` ("Information",
/// "Metadata") over its rows, each a hairline apart, 12px. The section is
/// `<scope>-information` to tests, and each row `<scope>-info-<label>`.
pub(crate) fn info_section(
    heading: &'static str,
    scope: &'static str,
    rows: Vec<InfoRow>,
    theme: &Theme,
) -> Div {
    let split = &theme.split;
    div()
        .debug_selector(move || format!("{scope}-information"))
        .flex_none()
        .flex()
        .flex_col()
        .mt(split.info_margin_top)
        .text_size(split.info_size)
        .child(
            div()
                .flex_none()
                .h(split.info_row_height)
                .flex()
                .items_center()
                .font_weight(theme.typography.medium)
                .text_color(theme.text_muted)
                .child(heading),
        )
        .children(rows.into_iter().map(|row| {
            let selector = format!("{scope}-info-{}", row.label);
            div()
                .debug_selector(move || selector)
                .flex_none()
                .h(split.info_row_height)
                .flex()
                .items_center()
                .justify_between()
                .gap(split.info_gap)
                .border_t_1()
                .border_color(theme.separator)
                .child(
                    div()
                        .flex_none()
                        .text_color(theme.text_muted)
                        .child(row.label),
                )
                .child(
                    div()
                        .min_w(px(0.))
                        .flex()
                        .items_center()
                        .gap(split.info_gap)
                        .text_color(theme.text_title)
                        .when_some(row.icon, |value, icon| {
                            value.child(div().flex_none().size(split.info_icon).child(icon))
                        })
                        .child(div().min_w(px(0.)).truncate().child(row.value)),
                )
        }))
}

/// The launcher's status — the outcome of what was just done — in `color`,
/// on one line: what the footer's left side shows in place of the open
/// command's icon and title until the user moves on.
pub(crate) fn footer_status(text: impl Into<SharedString>, color: Hsla, theme: &Theme) -> Div {
    div()
        .debug_selector(|| "clipboard-status".into())
        .flex_initial()
        .min_w(px(0.))
        .whitespace_nowrap()
        .text_size(theme.typography.footer_size)
        .text_color(color)
        .child(div().min_w(px(0.)).truncate().child(text.into()))
}

/// The footer's buttons, left to right, as the reference orders them:
/// `primary` (the selected record's, with the accent key), the rule, then
/// `more` (Actions); the rule only with a button before it.
pub(crate) fn footer_buttons(
    primary: Option<AnyElement>,
    more: AnyElement,
    theme: &Theme,
) -> Vec<AnyElement> {
    let rule = primary
        .is_some()
        .then(|| crate::ui::footer::divider(theme).into_any_element());
    primary
        .into_iter()
        .chain(rule)
        .chain(std::iter::once(more))
        .collect()
}

/// The footer: 52 high with its rule above and the footer's wash, `lead`
/// on the left and `buttons` (the launcher's footer buttons and rule) on
/// the right, 4px apart. It holds the Actions panel, anchored above it,
/// when the caller adds it.
pub(crate) fn footer(lead: Div, buttons: Vec<AnyElement>, theme: &Theme) -> Div {
    let split = &theme.split;
    let geometry = &theme.geometry;
    div()
        .relative()
        .flex_none()
        .h(split.footer_height)
        .flex()
        .items_center()
        .justify_between()
        .gap(split.footer_gap)
        .pl(geometry.footer_padding_left)
        .pr(geometry.footer_padding_right)
        .border_t_1()
        .border_color(theme.separator)
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
