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
//! An application tile draws its glyph at the reference's heavier 2px
//! stroke (`.ic.b`); the same path at 2px is derived from the asset once
//! (see [`Glyph::bold_svg_bytes`]).
//!
//! The reference's set has no menu, window-control, appearance, download,
//! copy, keyboard, disclosure or display glyph, so those are Pane's own
//! authoring in the same stroke style: the gear (the Settings rows), the Windows titlebar's close,
//! minimize and maximize marks (see the Settings window's custom
//! titlebar), the half-and-half circle (the Appearance page), the down
//! arrow, the two squares, the keyboard, the chevron and the monitor.
//!
//! Tones are the reference's `appTone` map, exactly — the gradients this
//! build's known identities and the visual workbench's reference rows use,
//! with their glyph colors (the map's others return with the rows that
//! need them) — plus the neutral command tile.
//! Tones carry *presentation* only — mapping a real row's identity to a
//! tone is the caller's job, and unknown identities should use
//! [`IconTone::Command`] rather than inventing app metadata from text.
//! The generic command glyph is the reference's terminal prompt.
//!
//! Tiles come in the reference's sizes ([`TileSize`]): the result row's
//! 28, a pinned slot's 42, the Actions header's 18, and the Appearance
//! preview's miniature row (22) and pinned slot (24). (The reference's
//! 34px toast tile has no Pane counterpart: Pane shows no launch toast.)

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use gpui::prelude::*;
use gpui::{Div, Hsla, Svg, div, linear_color_stop, linear_gradient, px, rgb_to_hsla, rgba, svg};

use crate::ui::theme::{Theme, TileMetrics};

/// A stroke glyph: the reference's own icon set, and Pane's additions in
/// its style (see the module docs).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Glyph {
    Search,
    Terminal,
    Prompt,
    Code,
    Folder,
    Blocks,
    /// A page with a folded corner: a file.
    File,
    /// A pen: drawing and design.
    Pen,
    /// A clipboard: clipboard history.
    Clipboard,
    /// A window split in two: window layouts.
    Layout,
    /// A crescent moon: dark mode.
    Moon,
    /// A padlock: locking the screen.
    Lock,
    /// A magnifier over a minus: the no-results notice (#96).
    SearchNone,
    /// An arrow pointing right: the answer card's, from what was typed to
    /// its answer.
    ArrowRight,
    /// A calculator: a calculation history row.
    Calculator,
    /// A clock: a time-zone history row.
    Clock,
    /// A box: an extension suggestion.
    Package,
    /// Concentric circles with cross hairs: an extension suggestion.
    Target,

    /// A cog: the Settings root row and the Settings window's sidebar.
    Gear,
    /// A globe: the web.
    Globe,
    /// A keyboard: the Settings window's Keyboard section.
    Keyboard,
    /// A chevron pointing right: a group of rows — rotated to point
    /// down by [`glyph_rotated`] while the group it belongs to is
    /// expanded, on the disclosure's timeline.
    ChevronRight,
    /// A circle split down the middle: the Appearance page's sidebar entry
    /// (the two palettes its theme choice stands between).
    Theme,
    /// A display with its stand: the Settings window's Launcher section
    /// (the window this page's choices place).
    Monitor,
    /// Two sliders: settings — the reference's Settings command, and the
    /// General page's sidebar entry (the choices that govern Pane as a
    /// whole).
    Sliders,
    /// An arrow out to the upper right: the Actions panel's primary
    /// action on an application (the reference's `A.open`).
    ActionOpen,
    /// A play triangle: the Actions panel's primary action on anything
    /// else (`A.run`).
    ActionRun,
    /// A keyboard: the Actions panel's hotkey entry (`A.kb`; the Settings
    /// window's Keyboard section keeps Pane's own [`Glyph::Keyboard`]).
    ActionHotkey,
    /// A tag: the Actions panel's alias entry (`A.tag`).
    ActionAlias,
    /// An arrow pointing left: the split view's back button.
    ArrowLeft,
    /// Two bars: the split view's Pause.
    Pause,
    /// A shield with a check: the split view's caption on what is kept.
    Shield,
    /// Lines of text: a clipboard history record, which is text.
    Lines,
    /// Two links of a chain: the fixture's link records.
    Link,
    /// A framed landscape: the fixture's image records.
    Image,
    /// An envelope: the fixture's mail record.
    Mail,
    /// A page with lines: notes (the reference's `notes`, a pinned
    /// sample's).
    Notes,
    /// Two beamed notes: music (the reference's `music`, a pinned
    /// sample's).
    Music,
    /// A pushpin: the Actions panel's quick slot entries (`A.pin`).
    ActionPin,
    /// A plus: the Appearance board's custom accent swatch (the
    /// reference's own path; fixture-only, #100).
    Plus,
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
    /// Every glyph, for the checks that walk the set.
    #[cfg(test)]
    const ALL: &'static [Glyph] = &[
        Glyph::Search,
        Glyph::Terminal,
        Glyph::Prompt,
        Glyph::Code,
        Glyph::Folder,
        Glyph::Blocks,
        Glyph::File,
        Glyph::Pen,
        Glyph::Clipboard,
        Glyph::Layout,
        Glyph::Moon,
        Glyph::Lock,
        Glyph::SearchNone,
        Glyph::ArrowRight,
        Glyph::Calculator,
        Glyph::Clock,
        Glyph::Package,
        Glyph::Target,
        Glyph::Gear,
        Glyph::Globe,
        Glyph::Keyboard,
        Glyph::ChevronRight,
        Glyph::Theme,
        Glyph::Monitor,
        Glyph::Sliders,
        Glyph::ActionOpen,
        Glyph::ActionRun,
        Glyph::ActionHotkey,
        Glyph::ActionAlias,
        Glyph::ArrowLeft,
        Glyph::Pause,
        Glyph::Shield,
        Glyph::Lines,
        Glyph::Link,
        Glyph::Image,
        Glyph::Mail,
        Glyph::Notes,
        Glyph::Music,
        Glyph::ActionPin,
        Glyph::Plus,
    ];

    /// The embedded SVG bytes for this glyph.
    pub(crate) fn svg_bytes(self) -> &'static [u8] {
        match self {
            Glyph::Search => include_bytes!("../../assets/icons/search.svg"),
            Glyph::Terminal => include_bytes!("../../assets/icons/terminal.svg"),
            Glyph::Prompt => include_bytes!("../../assets/icons/prompt.svg"),
            Glyph::Code => include_bytes!("../../assets/icons/code.svg"),
            Glyph::Folder => include_bytes!("../../assets/icons/folder.svg"),
            Glyph::Blocks => include_bytes!("../../assets/icons/blocks.svg"),
            Glyph::File => include_bytes!("../../assets/icons/file.svg"),
            Glyph::Pen => include_bytes!("../../assets/icons/pen.svg"),
            Glyph::Clipboard => include_bytes!("../../assets/icons/clipboard.svg"),
            Glyph::Layout => include_bytes!("../../assets/icons/layout.svg"),
            Glyph::Moon => include_bytes!("../../assets/icons/moon.svg"),
            Glyph::Lock => include_bytes!("../../assets/icons/lock.svg"),
            Glyph::SearchNone => include_bytes!("../../assets/icons/search-none.svg"),
            Glyph::ArrowRight => include_bytes!("../../assets/icons/arrow-right.svg"),
            Glyph::Calculator => include_bytes!("../../assets/icons/calculator.svg"),
            Glyph::Clock => include_bytes!("../../assets/icons/clock.svg"),
            Glyph::Package => include_bytes!("../../assets/icons/package.svg"),
            Glyph::Target => include_bytes!("../../assets/icons/target.svg"),
            Glyph::Gear => include_bytes!("../../assets/icons/gear.svg"),
            Glyph::Globe => include_bytes!("../../assets/icons/globe.svg"),
            Glyph::Keyboard => include_bytes!("../../assets/icons/keyboard.svg"),
            Glyph::ChevronRight => include_bytes!("../../assets/icons/chevron-right.svg"),
            Glyph::Theme => include_bytes!("../../assets/icons/theme.svg"),
            Glyph::Monitor => include_bytes!("../../assets/icons/monitor.svg"),
            Glyph::Sliders => include_bytes!("../../assets/icons/sliders.svg"),
            Glyph::ActionOpen => include_bytes!("../../assets/icons/action-open.svg"),
            Glyph::ActionRun => include_bytes!("../../assets/icons/action-run.svg"),
            Glyph::ActionHotkey => include_bytes!("../../assets/icons/action-hotkey.svg"),
            Glyph::ActionAlias => include_bytes!("../../assets/icons/action-alias.svg"),
            Glyph::ArrowLeft => include_bytes!("../../assets/icons/arrow-left.svg"),
            Glyph::Pause => include_bytes!("../../assets/icons/pause.svg"),
            Glyph::Shield => include_bytes!("../../assets/icons/shield.svg"),
            Glyph::Lines => include_bytes!("../../assets/icons/lines.svg"),
            Glyph::Link => include_bytes!("../../assets/icons/link.svg"),
            Glyph::Image => include_bytes!("../../assets/icons/image.svg"),
            Glyph::Mail => include_bytes!("../../assets/icons/mail.svg"),
            Glyph::Notes => include_bytes!("../../assets/icons/notes.svg"),
            Glyph::Music => include_bytes!("../../assets/icons/music.svg"),
            Glyph::ActionPin => include_bytes!("../../assets/icons/action-pin.svg"),
            Glyph::Plus => include_bytes!("../../assets/icons/plus.svg"),
            #[cfg(target_os = "windows")]
            Glyph::WindowClose => include_bytes!("../../assets/icons/window-close.svg"),
            #[cfg(target_os = "windows")]
            Glyph::WindowMinimize => include_bytes!("../../assets/icons/window-minimize.svg"),
            #[cfg(target_os = "windows")]
            Glyph::WindowMaximize => include_bytes!("../../assets/icons/window-maximize.svg"),
        }
    }

    /// The glyph at the reference's 2px application stroke (`.ic.b`):
    /// the asset with its 1.6 stroke width replaced, derived once per
    /// glyph and kept for the process's life (a bounded set).
    pub(crate) fn bold_svg_bytes(self) -> &'static [u8] {
        static BOLD: OnceLock<Mutex<HashMap<Glyph, &'static [u8]>>> = OnceLock::new();
        let mut bold = BOLD
            .get_or_init(Default::default)
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        bold.entry(self).or_insert_with(|| {
            let svg = String::from_utf8_lossy(self.svg_bytes())
                .replace(r#"stroke-width="1.6""#, r#"stroke-width="2""#);
            Box::leak(svg.into_bytes().into_boxed_slice())
        })
    }
}

/// A tile tone: the reference's app gradient pairs, or the neutral command
/// tile. See the module docs — tones are presentation, not identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum IconTone {
    Term,
    Code,
    Web,
    Pen,
    Folder,
    /// The reference's `note` tone (violet).
    Note,
    /// The reference's `music` tone (green).
    Music,
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
        IconTone::Pen => (0xFF739FFF, 0xCF2D63FF, 0xFFFFFFFF),
        IconTone::Folder => (0x74B6FFFF, 0x2F78DEFF, 0xFFFFFFFF),
        IconTone::Note => (0xA184FFFF, 0x5B3BD0FF, 0xFFFFFFFF),
        IconTone::Music => (0x3DDC78FF, 0x129245FF, 0xFFFFFFFF),
        IconTone::Command => return None,
    };
    Some((color(top), color(bottom), color(glyph)))
}

/// A bare glyph at `size`, tinted `color` — for places that use an icon
/// without a tile, like the search header's magnifier.
pub(crate) fn glyph(glyph: Glyph, size: gpui::Pixels, color: Hsla) -> Svg {
    svg().data(glyph.svg_bytes()).size(size).text_color(color)
}

/// The footer's Pane mark, as the reference draws it: a stroked square
/// in `back` behind a filled one in `front` (the reference's #EDEDEF at
/// .92), at `size`. Two masks, since an SVG mask takes one tint.
pub(crate) fn pane_mark(size: gpui::Pixels, back: Hsla, front: Hsla) -> Div {
    div()
        .relative()
        .flex_none()
        .size(size)
        .child(
            svg()
                .data(include_bytes!("../../assets/icons/mark-back.svg"))
                .absolute()
                .size(size)
                .text_color(back),
        )
        .child(
            svg()
                .data(include_bytes!("../../assets/icons/mark-front.svg"))
                .absolute()
                .size(size)
                .text_color(front),
        )
}

/// A bare glyph at `size`, tinted `color`, rotated clockwise by `angle`
/// about its center — paint only: the element's layout, hit target and
/// debug bounds stay the unrotated box's, the renderer's scene
/// transformation carrying the turn. The one user is a disclosure
/// group's chevron, which turns from pointing right (the group
/// collapsed, at 0) to pointing down (expanded, at a quarter turn) on
/// the same timeline the group's content arrives on.
pub(crate) fn glyph_rotated(
    glyph: Glyph,
    size: gpui::Pixels,
    color: Hsla,
    angle: gpui::Radians,
) -> Svg {
    svg()
        .data(glyph.svg_bytes())
        .size(size)
        .text_color(color)
        .with_transformation(gpui::Transformation::rotate(angle))
}

/// Which of the reference's tile sizes a tile is drawn at; the theme
/// holds each one's metrics.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TileSize {
    /// A result row's tile.
    Row,
    /// A pinned slot's tile.
    Slot,
    /// The Actions panel header's tile.
    Mini,
    /// A row of the Appearance preview's miniature launcher (22).
    PreviewRow,
    /// A pinned slot of the Appearance preview's miniature launcher (24).
    PreviewPin,
}

impl TileSize {
    /// This size's metrics, from the theme's tokens.
    pub(crate) fn metrics(self, theme: &Theme) -> TileMetrics {
        let geometry = &theme.geometry;
        match self {
            TileSize::Row => geometry.tile,
            TileSize::Slot => geometry.slot_tile,
            TileSize::Mini => geometry.mini_tile,
            TileSize::PreviewRow => geometry.preview.row_tile,
            TileSize::PreviewPin => geometry.preview.pin_tile,
        }
    }
}

/// The reference's icon tile at the result row's size (see [`tile_at`]).
pub(crate) fn tile(tone: IconTone, glyph: Glyph, theme: &Theme) -> Div {
    tile_at(TileSize::Row, tone, glyph, theme)
}

/// The reference's icon tile at `size`: a vertical gradient under the
/// 2px-stroke glyph for app tones (`.tile.app`), or the theme's neutral
/// surface under the 1.6px glyph for [`IconTone::Command`] (`.tile`),
/// with the tile chrome from the reference — a thin pale edge, a top
/// inset highlight, and (app tones only) a short bottom shadow.
pub(crate) fn tile_at(size: TileSize, tone: IconTone, glyph: Glyph, theme: &Theme) -> Div {
    let metrics = size.metrics(theme);
    let glyph_size = metrics.glyph;
    let tile = div()
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .size(metrics.size)
        .rounded(metrics.radius);
    match app_tone(tone) {
        Some((top, bottom, glyph_color)) => tile
            .bg(linear_gradient(
                180.,
                linear_color_stop(top, 0.),
                linear_color_stop(bottom, 1.),
            ))
            // inset 0 0 0 .5px rgba(255,255,255,.28), inset 0 1px 0
            // rgba(255,255,255,.35), 0 1px 3px rgba(0,0,0,.45). The
            // half-pixel edge is drawn as the one pixel Chrome rasterizes
            // it to (see `Theme::tile_app_edge`).
            .shadow(vec![
                gpui::BoxShadow::new(px(0.), px(0.), theme.tile_app_edge)
                    .spread_radius(px(1.))
                    .inset(),
                gpui::BoxShadow::new(px(0.), px(1.), theme.tile_app_highlight).inset(),
                gpui::BoxShadow::new(px(0.), px(1.), theme.tile_drop).blur_radius(px(3.)),
            ])
            .child(
                svg()
                    .data(glyph.bold_svg_bytes())
                    .size(glyph_size)
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
                    .size(glyph_size)
                    .text_color(theme.tile_foreground),
            ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The 2px application stroke is derived by replacing the asset's
    /// 1.6 stroke width, so every asset must state it exactly once, in the
    /// form the replacement looks for — or an application tile would
    /// silently draw the lighter stroke.
    #[test]
    fn every_glyph_states_the_stroke_the_bold_variant_replaces() {
        for &glyph in Glyph::ALL {
            let svg = String::from_utf8_lossy(glyph.svg_bytes());
            assert_eq!(svg.matches(r#"stroke-width="1.6""#).count(), 1, "{glyph:?}");
            let bold = String::from_utf8_lossy(glyph.bold_svg_bytes());
            assert!(bold.contains(r#"stroke-width="2""#), "{glyph:?}");
            assert!(!bold.contains("1.6"), "{glyph:?}");
        }
    }
}
