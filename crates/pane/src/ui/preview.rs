//! The Appearance page's live preview (#98), as the reference Settings
//! board draws it: a 400×520 stage, radius 16 under a white 8% ring, with
//! a miniature launcher 56px below its top — 340 wide, radius 14: a 46px
//! search line, a pinned strip and 38px rows, and a 38px footer with its
//! action and the accent key.
//!
//! The miniature is a distinct recipe, not the launcher at a smaller
//! scale: its rows are 38 high (root search's are 44), its tiles 22 and
//! 24. What it shares with the launcher is what the user chooses — it is
//! drawn with the theme and the material in effect
//! ([`Material::preview_panel`]: the glass tint where glass stands, the
//! solid surface where it does not) and with the effective bindings the
//! caller passes, so a choice shows in it as the windows show it.
//!
//! The board's stage holds a miniature wallpaper (`.wp-mini`) for its
//! glass to blur. Pane paints no wallpaper (#100, the research's material
//! rule: a painted backdrop would fake the translucency it previews), so
//! the stage has no fill of its own: the miniature's glass lies over the
//! Settings window's own surface, and through it over whatever the
//! window's glass shows.
//!
//! Presentation only: the caller passes plain values ([`PreviewContent`])
//! and attaches identity; nothing here imports launcher state.

use gpui::prelude::*;
use gpui::{BoxShadow, Div, SharedString, div, px};

use crate::ui::icon::{Glyph, IconTone, TileSize, glyph, tile_at};
use crate::ui::keycap::{self, CapStyle, KeySequence};
use crate::ui::material::Material;
use crate::ui::theme::Theme;

/// One row of the miniature: its title, its kind, its tile and whether it
/// is the selected one.
#[derive(Clone, Debug)]
pub(crate) struct PreviewRow {
    pub(crate) title: SharedString,
    pub(crate) kind: SharedString,
    pub(crate) tone: IconTone,
    pub(crate) glyph: Glyph,
    pub(crate) selected: bool,
}

/// What the miniature shows: the query in its search line, its pinned
/// slots' tiles (none hides the strip), its rows, and its footer — the
/// tip at the left (if any), the action and its keys at the right.
#[derive(Clone, Debug)]
pub(crate) struct PreviewContent {
    pub(crate) query: SharedString,
    pub(crate) pins: Vec<(IconTone, Glyph)>,
    pub(crate) rows: Vec<PreviewRow>,
    pub(crate) tip: Option<SharedString>,
    pub(crate) action: SharedString,
    pub(crate) keys: KeySequence,
}

/// The stage: as wide as its column, 520 high, radius 16 under a white 8%
/// inset ring, `miniature` centered 56px below its top. No fill of its
/// own (see the module docs).
pub(crate) fn stage(miniature: impl IntoElement, theme: &Theme) -> Div {
    let preview = &theme.geometry.preview;
    let ring = BoxShadow::new(px(0.), px(0.), theme.controls.preview_stage_edge)
        .spread_radius(px(1.))
        .inset();
    div()
        .relative()
        .flex_none()
        .w_full()
        .h(preview.stage_height)
        .rounded(preview.stage_radius)
        .overflow_hidden()
        .shadow(vec![ring])
        .flex()
        .justify_center()
        .items_start()
        .pt(preview.stage_padding_top)
        .child(miniature)
}

/// The miniature launcher showing `content`, on `material`'s preview
/// surface, under its drop shadow (down 30, blur 70, spread -20, black
/// 70%).
pub(crate) fn miniature(content: &PreviewContent, material: Material, theme: &Theme) -> Div {
    let preview = &theme.geometry.preview;
    let column = div()
        .flex()
        .flex_col()
        .child(search_line(content, theme))
        .child(body(content, theme))
        .child(footer(content, theme));
    let drop = BoxShadow::new(px(0.), preview.shadow_offset, theme.controls.preview_shadow)
        .blur_radius(preview.shadow_blur)
        .spread_radius(preview.shadow_spread);
    div()
        .relative()
        .flex_none()
        .flex()
        .flex_col()
        .w(preview.panel_width)
        .rounded(preview.panel_radius)
        .shadow(vec![drop])
        .child(material.preview_panel(theme, column))
}

/// The search line: 46 high over its 1px rule, the 15px magnifier, then
/// the query in 14px with the accent caret 1px after it.
fn search_line(content: &PreviewContent, theme: &Theme) -> Div {
    let preview = &theme.geometry.preview;
    let query = theme.typography.settings.preview_query;
    div()
        .debug_selector(|| "preview-search".into())
        .flex_none()
        .h(preview.search_height)
        .flex()
        .items_center()
        .gap(preview.search_gap)
        .px(preview.search_padding_x)
        .border_b_1()
        .border_color(theme.hairline_soft)
        .child(glyph(Glyph::Search, preview.search_glyph, theme.text_muted).flex_none())
        .child(
            div()
                .flex()
                .items_center()
                // The board's caret follows the query's 10px gap, pulled
                // back 9px.
                .gap(preview.search_gap - preview.caret_pull)
                .child(
                    div()
                        .text_size(query.size)
                        .line_height(query.line_height)
                        .text_color(theme.text_query)
                        .child(content.query.clone()),
                )
                .child(
                    div()
                        .flex_none()
                        .w(preview.caret_width)
                        .h(preview.caret_height)
                        .bg(theme.accent_text),
                ),
        )
}

/// The list: padded 6, 2px apart — the pinned strip (four equal slots on
/// white 5%, each its 24px tile) over the 38px rows.
fn body(content: &PreviewContent, theme: &Theme) -> Div {
    let preview = &theme.geometry.preview;
    let strip = (!content.pins.is_empty()).then(|| {
        div()
            .debug_selector(|| "preview-pins".into())
            .flex()
            .gap(preview.pins_gap)
            .pt(preview.pins_padding_top)
            .px(preview.pins_padding_x)
            .pb(preview.pins_padding_bottom)
            .children(content.pins.iter().map(|&(tone, mark)| {
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .h(preview.pin_height)
                    .rounded(preview.pin_radius)
                    .bg(theme.controls.preview_pin)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(tile_at(TileSize::PreviewPin, tone, mark, theme))
            }))
    });
    div()
        .flex()
        .flex_col()
        .gap(preview.body_gap)
        .p(preview.body_padding)
        .children(strip)
        .children(
            content
                .rows
                .iter()
                .enumerate()
                .map(|(index, row)| preview_row(index, row, theme)),
        )
}

/// Row `index` (`.mrow`): 38 high, radius 8, padded 8, its 22px tile, its
/// title in 12.5/500 and its kind in 11.5 at its right end; the selected
/// one on white 9%.
fn preview_row(index: usize, row: &PreviewRow, theme: &Theme) -> Div {
    let preview = &theme.geometry.preview;
    let lines = &theme.typography.settings;
    div()
        .debug_selector(move || format!("preview-row-{index}"))
        .flex()
        .items_center()
        .gap(preview.row_gap)
        .h(preview.row_height)
        .px(preview.row_padding_x)
        .rounded(preview.row_radius)
        .when(row.selected, |wash| {
            wash.bg(theme.controls.preview_row_selected)
        })
        .child(tile_at(TileSize::PreviewRow, row.tone, row.glyph, theme))
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .truncate()
                .text_size(lines.preview_title.size)
                .line_height(lines.preview_title.line_height)
                .font_weight(theme.typography.medium)
                .text_color(theme.text_title)
                .child(row.title.clone()),
        )
        .child(
            div()
                .flex_none()
                .text_size(lines.preview_small.size)
                .line_height(lines.preview_small.line_height)
                .text_color(theme.text_muted)
                .child(row.kind.clone()),
        )
}

/// The footer: 38 high under its 1px rule, on the launcher footer's black
/// 14%; the tip at the left, and at the right the action in 11.5/500 with
/// its keys in the accent cap, 6px after it.
fn footer(content: &PreviewContent, theme: &Theme) -> Div {
    let preview = &theme.geometry.preview;
    let small = theme.typography.settings.preview_small;
    div()
        .debug_selector(|| "preview-footer".into())
        .flex_none()
        .h(preview.footer_height)
        .flex()
        .items_center()
        .justify_between()
        .gap(preview.footer_gap)
        .pl(preview.footer_padding_left)
        .pr(preview.footer_padding_right)
        .border_t_1()
        .border_color(theme.hairline_soft)
        .bg(theme.footer_tint)
        .text_size(small.size)
        .line_height(small.line_height)
        .text_color(theme.text_muted)
        .child(
            div()
                .min_w(px(0.))
                .truncate()
                .child(content.tip.clone().unwrap_or_default()),
        )
        .child(
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap(preview.open_gap)
                .font_weight(theme.typography.medium)
                .text_color(theme.footer_button_text)
                .child(content.action.clone())
                .child(keycap::key_sequence(&content.keys, CapStyle::Accent, theme)),
        )
}

/// A link line under the stage (the board's "Find themes in the Plugin
/// Store"): its label in 13/500 and the 14px arrow 6px after it. The board
/// links to a store Pane does not have, so only the visual workbench's
/// reference fixture draws one (#100).
pub(crate) fn link(label: impl Into<SharedString>, theme: &Theme) -> Div {
    let preview = &theme.geometry.preview;
    let line = theme.typography.settings.link;
    div()
        .flex()
        .items_center()
        .gap(preview.link_gap)
        .text_size(line.size)
        .line_height(line.line_height)
        .font_weight(theme.typography.medium)
        .text_color(theme.text_title)
        .child(label.into())
        .child(glyph(
            Glyph::ArrowRight,
            preview.link_glyph,
            theme.text_title,
        ))
}
