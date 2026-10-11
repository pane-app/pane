//! The reference's icon treatment: stroke glyphs rendered as tinted SVG
//! masks, and the vertical-gradient tiles they sit on.
//!
//! The glyphs are SVG assets in `crates/pane/assets/icons/<set>/`, a folder
//! per icon set holding a file for every glyph, and `glyph_svg!` names the
//! set they are drawn from — switching sets is that one line:
//!
//! - `reicon/`, the set in use: the Outline weight of reicon
//!   (<https://reicon.dev>, MIT; 1.5 stroke-width, round caps and joins,
//!   24×24 viewBox, some glyphs drawn as filled 1.5-wide bands instead of
//!   strokes), vendored by `scripts/icons/vendor-reicon.py`, which pins the
//!   version and holds the glyph-to-icon mapping.
//! - `pane/`, the alternate: Pane's hand-authored glyphs — the reference's
//!   own path data and Pane's additions in its style (1.6 stroke-width).
//!
//! The Windows titlebar's close, minimize and maximize marks (see the
//! Settings window's custom titlebar) stay `pane/`'s thin marks whatever
//! the set, as does the Actions panel's Delete cross on every system, and
//! the Pane mark (`mark-*.svg`) is Pane's own.
//!
//! The glyphs are embedded at compile time with `include_bytes!` — no
//! runtime file lookup, no `AssetSource` registration; `svg().data(bytes)`
//! renders them directly. GPUI renders an SVG as an alpha mask and tints
//! it with the element's text color, so the glyph's color always comes
//! from the caller's token. An application tile draws its glyph at the
//! reference's heavier stroke (`.ic.b`, 2px over its 1.6); the heavier
//! glyph is derived from the asset once (see [`Glyph::bold_svg_bytes`]).
//!
//! Tones are the reference's `appTone` map, exactly — the gradients this
//! build's known identities use, with their glyph colors (the map's
//! others return with the rows that need them) — plus the neutral
//! command tile.
//! Tones carry *presentation* only — mapping a real row's identity to a
//! tone is the caller's job, and unknown identities should use
//! [`IconTone::Command`] rather than inventing app metadata from text.
//! The generic command glyph is the reference's terminal prompt.
//!
//! Tiles come in the reference's sizes ([`TileSize`]): the result row's
//! 28, a pinned slot's 30 and the Actions header's 18. The neutral
//! command tile is also what a built-in glyph an extension names is drawn
//! on at those sizes (ADR 0035's one exception to bare extension icons —
//! see [`neutral_chrome`]). (The reference's 34px toast tile has no Pane
//! counterpart: Pane shows no launch toast.)

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use gpui::prelude::*;
use gpui::{Div, Hsla, Svg, div, linear_color_stop, linear_gradient, px, rgb_to_hsla, rgba, svg};

use crate::ui::theme::{Theme, TileMetrics};

/// The embedded SVG of glyph file `$name` in the icon set in use — the one
/// place the set is chosen: `"reicon"` or `"pane"`, a folder under
/// `crates/pane/assets/icons/` (see the module docs).
macro_rules! glyph_svg {
    ($name:literal) => {
        include_bytes!(concat!("../../assets/icons/", "reicon", "/", $name, ".svg"))
    };
}

/// A stroke glyph, drawn from the icon set in use (see the module docs).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Glyph {
    Search,
    Terminal,
    Prompt,
    Folder,
    Blocks,
    /// A clipboard: clipboard history.
    Clipboard,
    /// A magnifier over a minus: the no-results notice (#96).
    SearchNone,
    /// An arrow pointing right: the answer card's, from what was typed to
    /// its answer.
    ArrowRight,
    /// A clock: a time-zone history row.
    Clock,

    /// A cog: the Settings root row and the Settings window's sidebar.
    Gear,
    /// A keyboard: the Settings window's Keyboard section.
    Keyboard,
    /// A chevron pointing right: a group of rows — rotated to point
    /// down by [`glyph_rotated`] while the group it belongs to is
    /// expanded, on the disclosure's timeline.
    ChevronRight,
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
    /// window's Keyboard section has its own [`Glyph::Keyboard`]).
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
    /// A pushpin: the Actions panel's quick slot entries (`A.pin`).
    ActionPin,
    /// A plus: the pinned home's "+ Pin" hint, and the Appearance board's
    /// custom accent swatch (the reference's own path).
    Plus,
    /// A counter-clockwise arrow: a key binding recorder's button that
    /// resets the binding to its default.
    Reset,
    /// A keyboard: a key binding recorder's record mark.
    Record,
    /// A cross: the Actions panel's Delete entry on a clipboard record
    /// (#166) — `pane/`'s close mark on every system, since the set in
    /// use has no cross.
    Delete,
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
        Glyph::Folder,
        Glyph::Blocks,
        Glyph::Clipboard,
        Glyph::SearchNone,
        Glyph::ArrowRight,
        Glyph::Clock,
        Glyph::Gear,
        Glyph::Keyboard,
        Glyph::ChevronRight,
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
        Glyph::ActionPin,
        Glyph::Plus,
        Glyph::Reset,
        Glyph::Record,
        Glyph::Delete,
    ];

    /// The embedded SVG bytes for this glyph.
    pub(crate) fn svg_bytes(self) -> &'static [u8] {
        match self {
            Glyph::Search => glyph_svg!("search"),
            Glyph::Terminal => glyph_svg!("terminal"),
            Glyph::Prompt => glyph_svg!("prompt"),
            Glyph::Folder => glyph_svg!("folder"),
            Glyph::Blocks => glyph_svg!("blocks"),
            Glyph::Clipboard => glyph_svg!("clipboard"),
            Glyph::SearchNone => glyph_svg!("search-none"),
            Glyph::ArrowRight => glyph_svg!("arrow-right"),
            Glyph::Clock => glyph_svg!("clock"),
            Glyph::Gear => glyph_svg!("gear"),
            Glyph::Keyboard => glyph_svg!("keyboard"),
            Glyph::ChevronRight => glyph_svg!("chevron-right"),
            Glyph::Monitor => glyph_svg!("monitor"),
            Glyph::Sliders => glyph_svg!("sliders"),
            Glyph::ActionOpen => glyph_svg!("action-open"),
            Glyph::ActionRun => glyph_svg!("action-run"),
            Glyph::ActionHotkey => glyph_svg!("action-hotkey"),
            Glyph::ActionAlias => glyph_svg!("action-alias"),
            Glyph::ArrowLeft => glyph_svg!("arrow-left"),
            Glyph::Pause => glyph_svg!("pause"),
            Glyph::Shield => glyph_svg!("shield"),
            Glyph::Lines => glyph_svg!("lines"),
            Glyph::ActionPin => glyph_svg!("action-pin"),
            Glyph::Plus => glyph_svg!("plus"),
            Glyph::Reset => glyph_svg!("reset"),
            Glyph::Record => glyph_svg!("record"),
            Glyph::Delete => include_bytes!("../../assets/icons/pane/window-close.svg"),
            #[cfg(target_os = "windows")]
            Glyph::WindowClose => include_bytes!("../../assets/icons/pane/window-close.svg"),
            #[cfg(target_os = "windows")]
            Glyph::WindowMinimize => include_bytes!("../../assets/icons/pane/window-minimize.svg"),
            #[cfg(target_os = "windows")]
            Glyph::WindowMaximize => include_bytes!("../../assets/icons/pane/window-maximize.svg"),
        }
    }

    /// The glyph at the reference's heavier application stroke (`.ic.b`)
    /// — see [`embolden`] — derived once per glyph and kept for the
    /// process's life (a bounded set).
    pub(crate) fn bold_svg_bytes(self) -> &'static [u8] {
        static BOLD: OnceLock<Mutex<HashMap<Glyph, &'static [u8]>>> = OnceLock::new();
        let mut bold = BOLD
            .get_or_init(Default::default)
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        bold.entry(self).or_insert_with(|| {
            let svg = embolden(&String::from_utf8_lossy(self.svg_bytes()));
            Box::leak(svg.into_bytes().into_boxed_slice())
        })
    }
}

/// The attribute the bold variant scales.
const STROKE_WIDTH: &str = r#"stroke-width=""#;

/// The application stroke over the set's own: the reference's 2px over
/// its 1.6, so `pane/`'s 1.6 draws at 2 and reicon's 1.5 at 1.875.
const BOLD_SCALE: f32 = 1.25;

/// A filled shape, as both sets state it.
const FILLED: &str = r#"fill="currentColor""#;

/// A filled shape in the bold variant: reicon draws some Outline glyphs as
/// filled 1.5-wide bands rather than strokes, and a 0.375 stroke of the
/// glyph's own color widens such a band by the same quarter as
/// [`BOLD_SCALE`].
const FILLED_BOLD: &str =
    r#"fill="currentColor" stroke="currentColor" stroke-width="0.375" stroke-linejoin="round""#;

/// `svg` at the application stroke: every stated stroke width scaled by
/// [`BOLD_SCALE`] and every filled shape widened (see [`FILLED_BOLD`]),
/// whichever set the glyph is from.
fn embolden(svg: &str) -> String {
    let mut parts = svg.split(STROKE_WIDTH);
    let mut bold = parts.next().unwrap_or_default().to_owned();
    for part in parts {
        let (width, rest) = part.split_once('"').unwrap_or((part, ""));
        bold.push_str(STROKE_WIDTH);
        match width.parse::<f32>() {
            Ok(width) => bold.push_str(&(width * BOLD_SCALE).to_string()),
            Err(_) => bold.push_str(width),
        }
        bold.push('"');
        bold.push_str(rest);
    }
    bold.replace(FILLED, FILLED_BOLD)
}

/// A tile tone: the reference's app gradient pairs, or the neutral command
/// tile. See the module docs — tones are presentation, not identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum IconTone {
    Term,
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
}

impl TileSize {
    /// This size's metrics, from the theme's tokens.
    pub(crate) fn metrics(self, theme: &Theme) -> TileMetrics {
        let geometry = &theme.geometry;
        match self {
            TileSize::Row => geometry.tile,
            TileSize::Slot => geometry.slot_tile,
            TileSize::Mini => geometry.mini_tile,
        }
    }
}

/// The reference's icon tile at the result row's size (see [`tile_at`]).
pub(crate) fn tile(tone: IconTone, glyph: Glyph, theme: &Theme) -> Div {
    tile_at(TileSize::Row, tone, glyph, theme)
}

/// The reference's icon tile at `size`: a vertical gradient under the
/// bold glyph for app tones (`.tile.app`), or the theme's neutral
/// surface under the set's own stroke for [`IconTone::Command`] (`.tile`),
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
        None => neutral_chrome(tile, theme).child(
            svg()
                .data(glyph.svg_bytes())
                .size(glyph_size)
                .text_color(theme.tile_foreground),
        ),
    }
}

/// The neutral command tile's own look on `tile`, already sized and
/// rounded: the theme's translucent wash with its inset edge and top
/// highlight (the reference's `.tile`). What [`tile_at`] draws
/// [`IconTone::Command`]'s glyph on — and, by ADR 0035's one exception
/// to bare extension icons, what a built-in glyph an extension names is
/// drawn on at the tile sizes (`crate::ui::extension_icon`), so a
/// command naming one of Pane's glyphs reads as a command.
pub(crate) fn neutral_chrome<T: Styled>(tile: T, theme: &Theme) -> T {
    tile.bg(theme.tile_background)
        // inset 0 0 0 1px rgba(255,255,255,.08), inset 0 1px 0
        // rgba(255,255,255,.1)
        .shadow(vec![
            gpui::BoxShadow::new(px(0.), px(0.), theme.tile_border)
                .spread_radius(px(1.))
                .inset(),
            gpui::BoxShadow::new(px(0.), px(1.), theme.tile_highlight).inset(),
        ])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The stroke widths `svg` states.
    fn stroke_widths(svg: &str) -> Vec<&str> {
        svg.split(STROKE_WIDTH)
            .skip(1)
            .filter_map(|part| part.split('"').next())
            .collect()
    }

    /// The application stroke is derived from the asset (see
    /// [`embolden`]), so every asset must draw in a form the derivation
    /// reaches — a numeric stroke width, or a filled shape — and the
    /// derived glyph must keep none of the asset's widths, widen every
    /// filled shape, and state no attribute twice on one element (which
    /// would fail to parse and draw nothing) — or an application tile
    /// would silently draw the lighter stroke.
    #[test]
    fn every_glyph_states_the_stroke_the_bold_variant_replaces() {
        for &glyph in Glyph::ALL {
            let svg = String::from_utf8_lossy(glyph.svg_bytes());
            let widths = stroke_widths(&svg);
            let filled = svg.matches(FILLED).count();
            assert!(!widths.is_empty() || filled > 0, "{glyph:?}");
            assert!(
                widths.iter().all(|width| width.parse::<f32>().is_ok()),
                "{glyph:?}: {widths:?}"
            );
            let bold = String::from_utf8_lossy(glyph.bold_svg_bytes());
            let bold_widths = stroke_widths(&bold);
            for width in &widths {
                assert!(!bold_widths.contains(width), "{glyph:?} keeps {width}");
            }
            assert_eq!(bold.matches(FILLED_BOLD).count(), filled, "{glyph:?}");
            for element in bold.split('<') {
                for attribute in [" stroke=", " stroke-width=", " stroke-linejoin="] {
                    assert!(
                        element.matches(attribute).count() <= 1,
                        "{glyph:?}: {element}"
                    );
                }
            }
        }
    }
}
