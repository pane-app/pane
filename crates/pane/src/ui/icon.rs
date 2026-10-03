//! The reference's icon treatment: stroke glyphs rendered as tinted SVG
//! masks, and the vertical-gradient tiles they sit on.
//!
//! The glyphs are authored SVG assets in `crates/pane/assets/icons/`, path
//! data copied from the reference's own icon set (stroke style: 1.6
//! stroke-width, round caps and joins, 24×24 viewBox). They are embedded at
//! compile time with `include_bytes!` — no runtime file lookup, no
//! `AssetSource` registration; `svg().data(bytes)` renders them directly.
//! GPUI renders an SVG as an alpha mask and tints it with the element's
//! text color, so the glyph's color always comes from the caller's token.
//!
//! The reference's set has no settings, menu, window-control,
//! appearance or general glyph, so those are Pane's own authoring in the
//! same stroke style: the ellipsis and gear (the Settings rows), the
//! globe (the documentation entry), the Windows titlebar's close,
//! minimize and maximize marks (see the Settings window's custom
//! titlebar), the half-and-half circle (the Appearance page) and the
//! three sliders (the General page).
//!
//! Tones are the reference's `appTone` map, exactly: nine vertical
//! gradients with their glyph colors, plus the neutral command tile.
//! Tones carry *presentation* only — mapping a real row's identity to a
//! tone is the caller's job, and unknown identities should use
//! [`IconTone::Command`] rather than inventing app metadata from text.
//! The generic command glyph is the reference's terminal prompt.

use gpui::prelude::*;
use gpui::{Div, Hsla, Svg, div, linear_color_stop, linear_gradient, px, rgb_to_hsla, rgba, svg};

use crate::ui::theme::Theme;

/// A stroke glyph, authored from the reference's path data. Only the
/// glyphs this build's known rows use are kept; the reference's other
/// glyphs (globe, notes, music, pen, chat, calendar) return with the rows
/// that need them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Glyph {
    Search,
    Terminal,
    Prompt,
    Code,
    Folder,
    Blocks,
    /// Three dots: the launcher footer's menu button.
    Ellipsis,
    /// A cog: the Settings root row and the Settings window's sidebar.
    Gear,
    /// A globe: Settings' documentation entry.
    Globe,
    /// A keyboard: the Settings window's Shortcuts section.
    Keyboard,
    /// A chevron pointing down: a group of rows expanded.
    ChevronDown,
    /// A chevron pointing right: a group of rows collapsed.
    ChevronRight,
    /// A circle split down the middle: the Appearance page's sidebar entry
    /// (the two palettes its theme choice stands between).
    Theme,
    /// Three sliders: the General page's sidebar entry (the choices that
    /// govern Pane as a whole).
    Sliders,
    /// The Windows titlebar's close mark.
    #[cfg(target_os = "windows")]
    WindowClose,
    /// The Windows titlebar's minimize mark.
    #[cfg(target_os = "windows")]
    WindowMinimize,
    /// The Windows titlebar's maximize mark.
    #[cfg(target_os = "windows")]
    WindowMaximize,
}

impl Glyph {
    /// The embedded SVG bytes for this glyph.
    pub(crate) fn svg_bytes(self) -> &'static [u8] {
        match self {
            Glyph::Search => include_bytes!("../../assets/icons/search.svg"),
            Glyph::Terminal => include_bytes!("../../assets/icons/terminal.svg"),
            Glyph::Prompt => include_bytes!("../../assets/icons/prompt.svg"),
            Glyph::Code => include_bytes!("../../assets/icons/code.svg"),
            Glyph::Folder => include_bytes!("../../assets/icons/folder.svg"),
            Glyph::Blocks => include_bytes!("../../assets/icons/blocks.svg"),
            Glyph::Ellipsis => include_bytes!("../../assets/icons/ellipsis.svg"),
            Glyph::Gear => include_bytes!("../../assets/icons/gear.svg"),
            Glyph::Globe => include_bytes!("../../assets/icons/globe.svg"),
            Glyph::Keyboard => include_bytes!("../../assets/icons/keyboard.svg"),
            Glyph::ChevronDown => include_bytes!("../../assets/icons/chevron-down.svg"),
            Glyph::ChevronRight => include_bytes!("../../assets/icons/chevron-right.svg"),
            Glyph::Theme => include_bytes!("../../assets/icons/theme.svg"),
            Glyph::Sliders => include_bytes!("../../assets/icons/sliders.svg"),
            #[cfg(target_os = "windows")]
            Glyph::WindowClose => include_bytes!("../../assets/icons/window-close.svg"),
            #[cfg(target_os = "windows")]
            Glyph::WindowMinimize => include_bytes!("../../assets/icons/window-minimize.svg"),
            #[cfg(target_os = "windows")]
            Glyph::WindowMaximize => include_bytes!("../../assets/icons/window-maximize.svg"),
        }
    }
}

/// A tile tone: the reference's app gradient pairs, or the neutral command
/// tile. See the module docs — tones are presentation, not identity. Only
/// the tones this build's known rows use are kept; the reference's other
/// app tones return with the rows that need them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum IconTone {
    Term,
    Code,
    Web,
    Folder,
    Command,
}

/// A reference-stated color: `0xRRGGBBAA`.
fn color(hex: u32) -> Hsla {
    rgb_to_hsla(rgba(hex))
}

/// (gradient top, gradient bottom, glyph color) for an app tone, from the
/// reference's `appTone` map. Same in both appearances — the reference's
/// gradients are saturated enough to hold a white glyph on either panel.
fn app_tone(tone: IconTone) -> Option<(Hsla, Hsla, Hsla)> {
    let (top, bottom, glyph) = match tone {
        IconTone::Term => (0x4A4D55FF, 0x1C1E22FF, 0xC8F5B4FF),
        IconTone::Code => (0x45A3F5FF, 0x1D62C8FF, 0xFFFFFFFF),
        IconTone::Web => (0xFFA24DFF, 0xE2530FFF, 0xFFFFFFFF),
        IconTone::Folder => (0x74B6FFFF, 0x2F78DEFF, 0xFFFFFFFF),
        IconTone::Command => return None,
    };
    Some((color(top), color(bottom), color(glyph)))
}

/// A bare glyph at `size`, tinted `color` — for places that use an icon
/// without a tile, like the search header's magnifier.
pub(crate) fn glyph(glyph: Glyph, size: gpui::Pixels, color: Hsla) -> Svg {
    svg().data(glyph.svg_bytes()).size(size).text_color(color)
}

/// The reference's icon tile: 28px, radius 7, a vertical gradient for app
/// tones or the theme's neutral surface for [`IconTone::Command`], with the
/// tile chrome from the reference — a thin pale edge, a top inset
/// highlight, and (app tones only) a short bottom shadow.
pub(crate) fn tile(tone: IconTone, glyph: Glyph, theme: &Theme) -> Div {
    let geometry = &theme.geometry;
    let tile = div()
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .size(geometry.tile_size)
        .rounded(geometry.tile_radius);
    match app_tone(tone) {
        Some((top, bottom, glyph_color)) => tile
            .bg(linear_gradient(
                180.,
                linear_color_stop(top, 0.),
                linear_color_stop(bottom, 1.),
            ))
            // inset 0 0 0 .5px rgba(255,255,255,.28), inset 0 1px 0
            // rgba(255,255,255,.35), 0 1px 3px rgba(0,0,0,.45)
            .shadow(vec![
                gpui::BoxShadow::new(px(0.), px(0.), theme.tile_app_edge)
                    .spread_radius(px(0.5))
                    .inset(),
                gpui::BoxShadow::new(px(0.), px(1.), theme.tile_app_highlight).inset(),
                gpui::BoxShadow::new(px(0.), px(1.), theme.tile_drop).blur_radius(px(3.)),
            ])
            .child(
                svg()
                    .data(glyph.svg_bytes())
                    .size(geometry.tile_glyph_size)
                    .text_color(glyph_color),
            ),
        None => tile
            .bg(theme.tile_background)
            // inset 0 0 0 1px rgba(255,255,255,.08), inset 0 1px 0
            // rgba(255,255,255,.1)
            .shadow(vec![
                gpui::BoxShadow::new(px(0.), px(0.), theme.tile_border)
                    .spread_radius(px(1.))
                    .inset(),
                gpui::BoxShadow::new(px(0.), px(1.), theme.tile_highlight).inset(),
            ])
            .child(
                svg()
                    .data(glyph.svg_bytes())
                    .size(geometry.tile_glyph_size)
                    .text_color(theme.tile_foreground),
            ),
    }
}
