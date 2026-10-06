//! The Settings window's shell, as the reference's Settings board composes
//! it (#97): presentation only, shared by the Settings window and the
//! visual workbench's fixture, so a change here reaches both.
//!
//! The board is a 1120×720 panel: a 48px titlebar over a 232px sidebar
//! (12px by 10px of padding, a 2px gap) holding the 34px search field and
//! the 36px section items, and the page beside it, padded 26px above, 32px
//! either side and 24px below. A page opens with its heading block — the
//! 22px/600 heading over its 13px subtitle — and the board's Appearance
//! page lays its controls and its preview out in two columns, 388px and
//! 400px with 36px between them, which fill the canonical page exactly.
//! The workbench's fixture draws that board.
//!
//! Pane's own window keeps the board's titlebar and sidebar, and draws its
//! pages its own way ([`content_viewport`], `ui::controls::page`): no
//! heading block (the titlebar names the page), and sections of rows in
//! raised cards, padded 20 above and 24 either side and below. It has no
//! Appearance page and no preview: the theme and material are a section
//! of the General page.
//!
//! The pieces return plain [`Div`]s, but for the sidebar item, which takes
//! its id so it can keep its press. Accessibility, focus, scrolling and
//! every handler stay with the caller (the Settings window's
//! feature module owns its sections, its search and its pages); nothing
//! here imports launcher state.
//!
//! ## Window size
//!
//! - The window opens at 860×600 ([`WINDOW_CLIENT`]), or, on a work area
//!   too small for it, at the work area less a margin on each side, never
//!   below the window's minimum ([`opening_size`]).
//! - The minimum is 560×400 ([`SETTINGS_MINIMUM`]). The user can resize
//!   and maximize; past 680px the page's content stops growing and
//!   centers.
//! - The titlebar keeps its 48px and the sidebar its 232px at every size.
//!   The sections scroll inside the sidebar when the window is short.
//! - The page scrolls on its own, vertically, independent of the sidebar.
//!
//! ## Pointer feedback
//!
//! A sidebar item's washes change at once, as the reference's `.nav`
//! does: there is no fade. Every item carries a hover style, the selected
//! one's being its own selected look, rather than attaching one only while
//! unselected: GPUI updates an element's remembered hover state only while
//! a hover style is attached, so a style that comes and goes leaves the
//! state stale, and a stale state laid out against the pointer the paint
//! sees never settles. While held, an item takes the stronger
//! [`crate::ui::theme::pressed`] wash of its hover, at once too — an
//! adaptation: the board authors no pressed state.

use gpui::prelude::*;
use gpui::{BoxShadow, Div, ElementId, Entity, Hsla, Pixels, SharedString, Stateful, div, px};
use gpui_elements::editable_text::{EditableTextState, text_input};

use crate::ui::icon::{Glyph, glyph};
use crate::ui::theme::{Theme, pressed};

/// The reference Settings board's 1120×720 panel (48 titlebar + 672
/// body), in logical pixels: the size the workbench's fixture draws the
/// board's scenarios at.
pub(crate) const SETTINGS_CLIENT: (f32, f32) = (1120., 720.);

/// The size Pane's Settings window opens at, in logical pixels: narrower
/// than the board, as a list of settings rows needs no more.
pub(crate) const WINDOW_CLIENT: (f32, f32) = (860., 600.);

/// The smallest the Settings window can be made, in logical pixels.
pub(crate) const SETTINGS_MINIMUM: (f32, f32) = (560., 400.);

/// The sidebar search field's placeholder, and its accessible name.
pub(crate) const SEARCH_PLACEHOLDER: &str = "Search settings";

/// How far a window opened on a work area too small for the canonical
/// size keeps from the work area's edges, in logical pixels.
const WORK_AREA_MARGIN: f32 = 24.;

/// The size the Settings window opens at on a work area of `work_area`
/// (logical pixels; `None` when the platform names none): 860×600, or the
/// work area less [`WORK_AREA_MARGIN`] on each side where that is smaller,
/// never below [`SETTINGS_MINIMUM`].
pub(crate) fn opening_size(work_area: Option<(f32, f32)>) -> (f32, f32) {
    let Some((width, height)) = work_area else {
        return WINDOW_CLIENT;
    };
    let fit = |canonical: f32, room: f32, floor: f32| {
        canonical.min(room - 2. * WORK_AREA_MARGIN).max(floor)
    };
    (
        fit(WINDOW_CLIENT.0, width, SETTINGS_MINIMUM.0),
        fit(WINDOW_CLIENT.1, height, SETTINGS_MINIMUM.1),
    )
}

/// A 1px ring inset along a box's edge, as the reference's `box-shadow:
/// inset 0 0 0 1px` draws it: it takes no layout space.
fn inset_ring(color: Hsla) -> Vec<BoxShadow> {
    vec![
        BoxShadow::new(px(0.), px(0.), color)
            .spread_radius(px(1.))
            .inset(),
    ]
}

/// The titlebar: 48px with a 1px rule along its bottom. The caller fills
/// it with the drag region (holding [`titlebar_label`]) and, on Windows,
/// the caption buttons, each its height. Drawn only where the platform's
/// own titlebar is hidden (see `features::settings::titlebar`).
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(crate) fn titlebar(theme: &Theme) -> Div {
    div()
        .flex_none()
        .flex()
        .h(theme.geometry.settings.titlebar_height)
        .border_b_1()
        .border_color(theme.hairline_soft)
}

/// The titlebar's label: 13px/500 in the body color. Pane's window names
/// the page showing; the board's says "Settings".
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(crate) fn titlebar_label(label: impl Into<SharedString>, theme: &Theme) -> Div {
    let typography = &theme.typography;
    div()
        .min_w(px(0.))
        .truncate()
        .text_size(typography.settings_text_size)
        .line_height(typography.settings_text_size * typography.line_height)
        .font_weight(typography.medium)
        .text_color(theme.text_body)
        .child(label.into())
}

/// The body under the titlebar: the sidebar, then the page beside it.
pub(crate) fn body(sidebar: impl IntoElement, page: impl IntoElement) -> Div {
    div()
        .flex_1()
        .min_h(px(0.))
        .flex()
        .flex_row()
        .child(sidebar)
        .child(page)
}

/// The sidebar: 232px wide (its rule included), padded 12px by 10px, its
/// children 2px apart, over the black 10% fill with a 1px rule along its
/// right edge — `search` (see [`search_field`]) above `sections` (see
/// [`section_list`]).
pub(crate) fn sidebar(search: impl IntoElement, sections: impl IntoElement, theme: &Theme) -> Div {
    let settings = &theme.geometry.settings;
    div()
        .flex_none()
        .w(settings.sidebar_width)
        .h_full()
        .flex()
        .flex_col()
        .gap(settings.sidebar_gap)
        .py(settings.sidebar_padding_y)
        .px(settings.sidebar_padding_x)
        .bg(theme.sidebar_fill)
        .border_r_1()
        .border_color(theme.hairline_soft)
        .child(search)
        .child(sections)
}

/// The sidebar's sections: a column of [`sidebar_item`]s 2px apart that
/// takes the sidebar's height below the search field. The caller names
/// it, makes it scroll when the window is short and adds the items.
pub(crate) fn section_list(theme: &Theme) -> Div {
    div()
        .flex_1()
        .min_h(px(0.))
        .flex()
        .flex_col()
        .gap(theme.geometry.settings.sidebar_gap)
}

/// The sidebar's search field: the 34px well (black 24% under a white 6%
/// inset ring, radius 8, with an 8px margin below it that the sidebar's
/// 2px gap follows) with the 14px magnifier
/// (which keeps its 14px: the field never squeezes it) and `input`'s
/// editable text in 13px, inset by the reference's `<input>` padding
/// (`search_text_inset`). The ring takes the focus ring's
/// color while the field has the keyboard focus — an adaptation: the
/// reference draws no focus state for it. The caller tracks the input's
/// focus and attaches the field's identity and keys.
pub(crate) fn search_field(
    input: &Entity<EditableTextState>,
    placeholder: &'static str,
    theme: &Theme,
) -> Div {
    let settings = &theme.geometry.settings;
    let typography = &theme.typography;
    let focus_ring = theme.focus_ring;
    div()
        .flex_none()
        .flex()
        .items_center()
        .gap(settings.search_gap)
        .h(settings.search_height)
        .px(settings.search_padding_x)
        .mb(settings.search_margin_bottom)
        .rounded(settings.search_radius)
        .bg(theme.field_fill)
        .shadow(inset_ring(theme.field_edge))
        .focus(move |field| field.shadow(inset_ring(focus_ring)))
        .child(glyph(Glyph::Search, settings.search_glyph, theme.nav_icon).flex_none())
        .child(
            text_input("settings-search")
                .state(input.downgrade())
                .placeholder(placeholder)
                .placeholder_color(theme.text_placeholder)
                .caret_color(theme.accent_text)
                .selection_color(theme.row_selected)
                .marked_color(theme.accent_text)
                .text_size(typography.settings_text_size)
                .text_color(theme.text_title)
                .font_family(typography.family.clone())
                .font_features(typography.features.clone())
                .pl(theme.geometry.search_text_inset)
                .w_full()
                .min_w(px(0.))
                .whitespace_nowrap()
                .overflow_x_scroll(),
        )
}

/// What a sidebar item shows: plain presentation values the caller
/// resolved — a section, or a search result.
#[derive(Clone, Debug)]
pub(crate) struct SidebarItem {
    /// The item's label.
    pub(crate) label: SharedString,
    /// The 16px glyph before it.
    pub(crate) glyph: Glyph,
    /// A muted second line under the label (where a search result lives).
    pub(crate) detail: Option<SharedString>,
    /// Why the item cannot be used here, in the warning color; it wraps,
    /// and the item grows past its height to hold it.
    pub(crate) reason: Option<SharedString>,
    /// A count at the item's right end (the reference's installed-plugins
    /// count).
    pub(crate) count: Option<SharedString>,
    /// Whether the item is the selected one.
    pub(crate) selected: bool,
}

/// The reference's sidebar item (`.nav`): 36px (a floor), radius 8, 10px
/// of side padding and a 10px gap, the 16px glyph in the muted color,
/// the 13px/500 label — #B3B4B9 at rest; white 5% and #EDEDEF under the
/// pointer; white 9% and white while selected, which the pointer does not
/// change — and the 12px count at its right end. While held it takes the
/// [`pressed`] wash of its hover (of its selected wash, if selected). No
/// wash fades (see the module docs). The item is `id`; the caller attaches
/// its accessibility and click.
pub(crate) fn sidebar_item(
    id: impl Into<ElementId>,
    item: SidebarItem,
    theme: &Theme,
) -> Stateful<Div> {
    let settings = &theme.geometry.settings;
    let typography = &theme.typography;
    let (hover, hover_text) = if item.selected {
        (theme.nav_selected, theme.nav_selected_text)
    } else {
        (theme.nav_hover, theme.nav_hover_text)
    };
    let line = |size: Pixels| size * typography.line_height;
    let caption = typography.settings_caption_size;
    let text = div()
        .flex_1()
        .min_w(px(0.))
        .flex()
        .flex_col()
        .child(
            div()
                .truncate()
                .line_height(line(typography.settings_text_size))
                .child(item.label),
        )
        .when_some(item.detail, |text, detail| {
            text.child(
                div()
                    .truncate()
                    .text_size(caption)
                    .line_height(line(caption))
                    .font_weight(typography.regular)
                    .text_color(theme.nav_icon)
                    .child(detail),
            )
        })
        .when_some(item.reason, |text, reason| {
            text.child(
                div()
                    .text_size(caption)
                    .line_height(line(caption))
                    .font_weight(typography.regular)
                    .text_color(theme.warning)
                    .child(reason),
            )
        });
    div()
        .flex_none()
        .w_full()
        .flex()
        .items_center()
        .gap(settings.item_gap)
        .min_h(settings.item_height)
        .px(settings.item_padding_x)
        .rounded(settings.item_radius)
        .cursor_pointer()
        .text_size(typography.settings_text_size)
        .font_weight(typography.medium)
        .text_color(if item.selected {
            theme.nav_selected_text
        } else {
            theme.nav_text
        })
        .when(item.selected, |row| row.bg(theme.nav_selected))
        .id(id)
        .hover(move |row| row.bg(hover).text_color(hover_text))
        .active(move |row| row.bg(pressed(hover)).text_color(hover_text))
        .child(glyph(item.glyph, settings.item_glyph, theme.nav_icon).flex_none())
        .child(text)
        .when_some(item.count, |row, count| {
            row.child(
                div()
                    .flex_none()
                    .text_size(caption)
                    .line_height(line(caption))
                    .text_color(theme.nav_icon)
                    .child(count),
            )
        })
}

/// The page's viewport: the area beside the sidebar, padded 26px above,
/// 32px either side and 24px below, its text 13px in the body ink unless a
/// page sets its own. The caller names it, makes it scroll and adds the
/// page.
pub(crate) fn page_viewport(theme: &Theme) -> Div {
    let settings = &theme.geometry.settings;
    div()
        .flex_1()
        .min_w(px(0.))
        .h_full()
        .pt(settings.page_padding_top)
        .px(settings.page_padding_x)
        .pb(settings.page_padding_bottom)
        .text_size(theme.typography.row_subtitle_size)
        .text_color(theme.text_body)
}

/// Pane's page area: the area beside the sidebar, padded 20 above, 24
/// either side and below, its text 13px in the body ink unless a page
/// sets its own. The caller names it, makes it scroll and adds the page
/// (see `ui::controls::page`). The board's own page is
/// [`page_viewport`], which the workbench's fixture draws.
pub(crate) fn content_viewport(theme: &Theme) -> Div {
    let settings = &theme.geometry.settings;
    div()
        .flex_1()
        .min_w(px(0.))
        .h_full()
        .pt(settings.content_padding_top)
        .px(settings.content_padding_x)
        .pb(settings.content_padding_bottom)
        .text_size(theme.typography.row_subtitle_size)
        .text_color(theme.text_body)
}

/// A page's heading block: the 22px/600 heading (-.01em of tracking) in
/// the heading color (the dark palette's white), over its 13px muted
/// subtitle, 4px apart, with 4px below the block. Each line box is the
/// reference's own (28 and 17: see `theme::SettingsType`), so the fields
/// below the block start where the board's do.
pub(crate) fn page_header(
    title: impl Into<SharedString>,
    subtitle: Option<SharedString>,
    theme: &Theme,
) -> Div {
    let settings = &theme.geometry.settings;
    let typography = &theme.typography;
    let lines = &typography.settings;
    div()
        .flex_none()
        .flex()
        .flex_col()
        .gap(settings.header_gap)
        .mb(settings.header_margin_bottom)
        .child(
            div()
                .text_size(lines.heading.size)
                .line_height(lines.heading.line_height)
                .font_weight(typography.heading_weight)
                .letter_spacing(lines.heading.size * typography.heading_tracking)
                .text_color(theme.heading_text)
                .child(title.into()),
        )
        .when_some(subtitle, |header, subtitle| {
            header.child(
                div()
                    .text_size(lines.subtitle.size)
                    .line_height(lines.subtitle.line_height)
                    .font_weight(typography.regular)
                    .text_color(theme.text_muted)
                    .child(subtitle),
            )
        })
}

/// A page's two columns: `controls` in the 388px column and `aside` (see
/// [`aside`]) in the 400px one, 36px apart — together exactly the
/// canonical page's width. Narrower, they collapse: the aside wraps below
/// the controls (36px under them), at most its 400px and never wider than
/// the page, while the controls take the page's width.
pub(crate) fn page_columns(
    controls: impl IntoElement,
    aside: impl IntoElement,
    theme: &Theme,
) -> Div {
    let settings = &theme.geometry.settings;
    div()
        .w_full()
        .flex()
        .flex_row()
        .flex_wrap()
        .items_start()
        .gap(settings.column_gap)
        .child(
            div()
                .flex_grow(1.)
                .flex_shrink(1.)
                .flex_basis(settings.controls_width)
                .min_w(px(0.))
                .flex()
                .flex_col()
                .child(controls),
        )
        .child(
            div()
                .flex_none()
                .w_full()
                .max_w(settings.aside_width)
                .flex()
                .flex_col()
                .child(aside),
        )
}

/// An aside column's content: its 12px/500 muted caption ("Preview", in
/// the reference's 16px line), 10px above whatever the caller adds.
pub(crate) fn aside(caption: impl Into<SharedString>, theme: &Theme) -> Div {
    let typography = &theme.typography;
    let caption_line = typography.settings.caption;
    div()
        .flex()
        .flex_col()
        .gap(theme.geometry.settings.aside_gap)
        .child(
            div()
                .text_size(caption_line.size)
                .line_height(caption_line.line_height)
                .font_weight(typography.medium)
                .text_color(theme.text_muted)
                .child(caption.into()),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_roomy_work_area_opens_the_window_size() {
        assert_eq!(opening_size(Some((1920., 1040.))), (860., 600.));
        assert_eq!(opening_size(None), (860., 600.));
    }

    #[test]
    fn a_small_work_area_opens_the_window_inside_it() {
        // 1366×768 at 150%, less its taskbar: 910×488 logical.
        assert_eq!(opening_size(Some((910., 488.))), (860., 440.));
    }

    #[test]
    fn the_window_never_opens_below_its_minimum() {
        assert_eq!(opening_size(Some((500., 300.))), (560., 400.));
    }

    #[test]
    fn the_canonical_page_holds_both_columns_exactly() {
        // The board's page: 1120 less the 232px sidebar and 32px of
        // padding either side is 824 = 388 + 36 + 400, so the columns sit
        // side by side at the canonical size and collapse below it.
        let settings = Theme::dark().geometry.settings;
        let page = SETTINGS_CLIENT.0
            - f32::from(settings.sidebar_width)
            - 2. * f32::from(settings.page_padding_x);
        let columns = settings.controls_width + settings.column_gap + settings.aside_width;
        assert_eq!(page, 824.);
        assert_eq!(f32::from(columns), page);
    }
}
