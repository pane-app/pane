//! Pane's semantic design tokens: the one place colors, type and geometry
//! come from. Every value is stated by role, not by screen, so restyling is
//! a one-file edit.
//!
//! Provenance: the dark palette, the geometry and the control chrome are the
//! authored reference values, read from
//! `docs/evidence/ui-prototype/reference/REFERENCE.md` and the unchanged
//! authored `launcher.html` retained beside it.
//! The light palette is *derived*, not authored: the reference ships dark
//! only. Light keeps the reference's geometry and icon gradients and swaps
//! the neutrals for light-glass counterparts; it is a starting point for
//! review, not a claim of reference fidelity.
//!
//! No globals: the integration reads `PANE_THEME` once, builds a [`Theme`],
//! and passes it down.
//!
//! Contrast honesty — no blanket accessibility claim. The dark glass is
//! the reference's dark panel composited over whatever is behind the
//! window: over a bright (white) desktop the .7 tint yields about #5c5d5f,
//! where the muted role is ~2.04:1 and the title role ~5.64:1. That is
//! dark glass over a bright desktop, not a light theme, and it varies
//! with the desktop. The reference's 55%-alpha floor is its own design
//! rule, not a readability guarantee, and the light palette's body-level
//! neutrals are a legibility choice, not a certified ratio. Opaque mode is
//! the explicit deterministic fallback.

use gpui::{FontWeight, Hsla, Pixels, SharedString, px, rgb_to_hsla, rgba};

/// Which palette a [`Theme`] carries. The integration picks once, from
/// `PANE_THEME`; nothing here observes the system appearance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Appearance {
    Dark,
    Light,
}

/// A reference-stated color: `0xRRGGBBAA`.
fn color(hex: u32) -> Hsla {
    rgb_to_hsla(rgba(hex))
}

/// The semantic tokens. Field groups follow the roles the launcher uses:
/// text, background, borders, selection, focus, semantic states, control
/// chrome — plus the typography and geometry the reference fixes.
#[derive(Clone, Debug)]
pub(crate) struct Theme {
    // -- Text roles ---------------------------------------------------------
    /// Row titles, footer's primary label (reference Ink).
    pub(crate) text_title: Hsla,
    /// Body copy (reference Ink 2).
    pub(crate) text_body: Hsla,
    /// Section labels, subtitles, the footer's hint (reference Ink 3).
    pub(crate) text_muted: Hsla,
    /// Typed query text in the search field.
    pub(crate) text_query: Hsla,
    /// The search field's placeholder.
    pub(crate) text_placeholder: Hsla,

    // -- Background roles ---------------------------------------------------
    /// The panel's translucent tint over the frost (reference L1:
    /// rgba(22,23,26,.7) dark). Legibility floor: the reference keeps the
    /// tint at or above 55% alpha.
    pub(crate) panel_tint: Hsla,
    /// The opaque panel used when the window is not frosted (Material's
    /// fallback). Same base color at full alpha.
    pub(crate) panel_solid: Hsla,
    /// The top sheen the L1 material fades out over the first 36% of the
    /// panel's height (reference gradient's white overlay).
    pub(crate) panel_sheen: Hsla,
    /// The 1px inset line along the panel's top edge.
    pub(crate) panel_top_highlight: Hsla,
    /// The footer strip's translucent wash (reference: rgba(0,0,0,.14)).
    pub(crate) footer_tint: Hsla,

    // -- Popover roles -------------------------------------------------
    /// The L2 popover's translucent tint over what is behind it inside
    /// the window (reference `.pop`: rgba(38,39,43,.82) dark). Used for
    /// floating layers over the panel, such as the footer's menu.
    pub(crate) popover_tint: Hsla,
    /// The opaque popover used when the window is not frosted (Material's
    /// fallback): the same base color at full alpha.
    pub(crate) popover_solid: Hsla,
    /// The popover's sheen, fading out over its top 40%.
    pub(crate) popover_sheen: Hsla,
    /// The popover's 1px inset edge.
    pub(crate) popover_edge: Hsla,
    /// The popover's top inset highlight.
    pub(crate) popover_top_highlight: Hsla,

    // -- Border roles -------------------------------------------------------
    /// The panel's inner edge (reference: rgba(255,255,255,.075)).
    pub(crate) hairline: Hsla,
    /// Dividers between header/footer and the list (reference: .06).
    pub(crate) hairline_soft: Hsla,

    // -- Selection roles ----------------------------------------------------
    /// A row under the pointer (reference: rgba(255,255,255,.035)).
    pub(crate) row_hover: Hsla,
    /// The selected row's wash (reference: rgba(255,255,255,.085)).
    pub(crate) row_selected: Hsla,
    /// The selected row's inset edge (reference: rgba(255,255,255,.05)).
    pub(crate) row_selected_border: Hsla,

    // -- Focus --------------------------------------------------------------
    /// The keyboard focus ring (reference focus-visible: white 50%).
    pub(crate) focus_ring: Hsla,

    // -- Semantic states ----------------------------------------------------
    /// Accent as *text or a thin stroke* on the panel: the reference's lime
    /// (#C9EE6A) in dark, a darkened readable green in light — the search
    /// caret and the IME marked-text underline use it, so light keeps it
    /// legible.
    pub(crate) accent_text: Hsla,
    /// Destructive states (reference: #FF9A92 dark).
    pub(crate) danger: Hsla,
    /// Warnings, including a row's unavailable reason (Pane's own value).
    pub(crate) warning: Hsla,
    /// Successful status results.
    pub(crate) success: Hsla,

    // -- Control chrome -----------------------------------------------------
    /// A neutral command tile's background (reference: rgba(255,255,255,.08)).
    pub(crate) tile_background: Hsla,
    /// A neutral command tile's glyph color (reference: #e9e9ec).
    pub(crate) tile_foreground: Hsla,
    /// A neutral tile's inset edge (reference: rgba(255,255,255,.08)).
    pub(crate) tile_border: Hsla,
    /// A neutral tile's top inset (reference: rgba(255,255,255,.1)).
    pub(crate) tile_highlight: Hsla,
    /// An app tile's thin pale edge (reference: rgba(255,255,255,.28)).
    pub(crate) tile_app_edge: Hsla,
    /// An app tile's top inset (reference: rgba(255,255,255,.35)).
    pub(crate) tile_app_highlight: Hsla,
    /// An app tile's short bottom shadow (reference: rgba(0,0,0,.45)).
    pub(crate) tile_drop: Hsla,

    // -- Type and geometry --------------------------------------------------
    /// Families, sizes and weights.
    pub(crate) typography: Typography,
    /// The reference's fixed dimensions.
    pub(crate) geometry: Geometry,
}

/// Type roles. Sizes are the reference's exact pixel values, not the
/// Tailwind scale; weights are Geist's 400/500/600.
#[derive(Clone, Debug)]
pub(crate) struct Typography {
    /// The UI family (embedded Geist; see [`super::load_fonts`]).
    pub(crate) family: SharedString,
    /// The search field's 19px.
    pub(crate) search_size: Pixels,
    /// A row title's 14px.
    pub(crate) row_title_size: Pixels,
    /// A row subtitle's 13px.
    pub(crate) row_subtitle_size: Pixels,
    /// A right-aligned row annotation's 12.5px (kind text, reasons).
    pub(crate) row_kind_size: Pixels,
    /// The footer's 12.5px.
    pub(crate) footer_size: Pixels,
    /// Title and label weight (500).
    pub(crate) medium: FontWeight,
}

/// The reference's geometry: the launcher panel is 760px wide, the search
/// field 64px tall, rows 44px, the panel radius 18px where the platform
/// shows it (none on Windows, whose window the Desktop Window Manager
/// rounds; see [`panel_radius`]). The reference authors no footer button
/// and no keycap — those two control's dimensions are Pane's own, at the
/// reference's control scale, so restyling stays a one-file edit here.
#[derive(Clone, Debug)]
pub(crate) struct Geometry {
    /// The panel's corner radius: [`panel_radius`] — 18px where the
    /// desktop shows through the window's corners, none on Windows, where
    /// the window itself is rounded instead.
    pub(crate) panel_radius: Pixels,
    /// The search header's height.
    pub(crate) search_height: Pixels,
    /// The search header's horizontal padding.
    pub(crate) search_padding_x: Pixels,
    /// The search header's icon-to-field gap.
    pub(crate) search_gap: Pixels,
    /// A row's height — a floor: a row holding a wrapped unavailable
    /// reason grows taller rather than clipping it.
    pub(crate) row_min_height: Pixels,
    /// A row's corner radius.
    pub(crate) row_radius: Pixels,
    /// A row's horizontal padding.
    pub(crate) row_padding_x: Pixels,
    /// The gap between a row's tile, title and subtitle.
    pub(crate) row_gap: Pixels,
    /// The gap between rows in the list.
    pub(crate) row_list_gap: Pixels,
    /// An icon tile's size.
    pub(crate) tile_size: Pixels,
    /// An icon tile's corner radius.
    pub(crate) tile_radius: Pixels,
    /// The glyph inside an icon tile.
    pub(crate) tile_glyph_size: Pixels,
    /// The footer's height.
    pub(crate) footer_height: Pixels,
    /// The footer action button's height.
    pub(crate) action_height: Pixels,
    /// The footer action button's corner radius.
    pub(crate) action_radius: Pixels,
    /// The footer action button's horizontal padding.
    pub(crate) action_padding_x: Pixels,
    /// The gap between the action button's label and its keycap.
    pub(crate) action_gap: Pixels,
    /// A keycap's height.
    pub(crate) keycap_height: Pixels,
    /// A keycap's corner radius.
    pub(crate) keycap_radius: Pixels,
    /// A keycap's horizontal padding.
    pub(crate) keycap_padding_x: Pixels,
    /// The glyph inside a keycap.
    pub(crate) keycap_glyph_size: Pixels,
    /// A popover's corner radius (the reference's L2 `.pop`: 14px, shown
    /// on every platform, since a popover floats inside the window rather
    /// than at its edge).
    pub(crate) popover_radius: Pixels,
}

impl Theme {
    /// The theme for `appearance`.
    pub(crate) fn new(appearance: Appearance) -> Theme {
        match appearance {
            Appearance::Dark => Theme::dark(),
            Appearance::Light => Theme::light(),
        }
    }

    /// The reference's dark palette, exactly as authored.
    pub(crate) fn dark() -> Theme {
        Theme {
            text_title: color(0xEDEDEFFF),
            text_body: color(0xA3A4A9FF),
            text_muted: color(0x8E8F94FF),
            text_query: color(0xF3F3F5FF),
            text_placeholder: color(0x86878CFF),

            panel_tint: color(0x16171AB3),
            panel_solid: color(0x16171AFF),
            panel_sheen: color(0xFFFFFF0D),
            panel_top_highlight: color(0xFFFFFF1A),
            footer_tint: color(0x00000024),

            popover_tint: color(0x26272BD1),
            popover_solid: color(0x26272BFF),
            popover_sheen: color(0xFFFFFF0F),
            popover_edge: color(0xFFFFFF17),
            popover_top_highlight: color(0xFFFFFF1A),

            hairline: color(0xFFFFFF13),
            hairline_soft: color(0xFFFFFF0F),

            row_hover: color(0xFFFFFF09),
            row_selected: color(0xFFFFFF16),
            row_selected_border: color(0xFFFFFF0D),

            focus_ring: color(0xFFFFFF80),

            accent_text: color(0xC9EE6AFF),
            danger: color(0xFF9A92FF),
            warning: color(0xD6A36AFF),
            success: color(0x9FD8A8FF),

            tile_background: color(0xFFFFFF14),
            tile_foreground: color(0xE9E9ECFF),
            tile_border: color(0xFFFFFF14),
            tile_highlight: color(0xFFFFFF1A),
            tile_app_edge: color(0xFFFFFF47),
            tile_app_highlight: color(0xFFFFFF59),
            tile_drop: color(0x00000073),

            typography: Typography::shared(),
            geometry: Geometry::shared(),
        }
    }

    /// The derived light palette. The reference authors dark only, so the
    /// neutrals are Pane's own light-glass counterparts: a warm light panel
    /// at high tint opacity, dark ink, and body-level muted and placeholder
    /// roles chosen for legibility (the reference's dim neutrals do not
    /// survive a light panel). Geometry and the app-tone icon gradients
    /// are the reference's, unchanged. This is a proposal for review, not
    /// reference truth.
    pub(crate) fn light() -> Theme {
        Theme {
            text_title: color(0x202126FF),
            text_body: color(0x575A63FF),
            text_muted: color(0x575A63FF),
            text_query: color(0x1D1E23FF),
            text_placeholder: color(0x575A63FF),

            panel_tint: color(0xF6F6F8CC),
            panel_solid: color(0xF6F6F8FF),
            panel_sheen: color(0xFFFFFF4D),
            panel_top_highlight: color(0xFFFFFF66),
            footer_tint: color(0x0000000D),

            // The light popover is derived, like the light panel: the
            // same base as the panel at a slightly higher tint, so a
            // popover over the panel reads as a raised layer.
            popover_tint: color(0xFBFBFDE6),
            popover_solid: color(0xFBFBFDFF),
            popover_sheen: color(0xFFFFFF66),
            popover_edge: color(0x0000001A),
            popover_top_highlight: color(0x00000012),

            hairline: color(0x00000017),
            hairline_soft: color(0x00000012),

            row_hover: color(0x0000000B),
            row_selected: color(0x00000016),
            row_selected_border: color(0x0000000D),

            focus_ring: color(0x00000073),

            accent_text: color(0x5C7A17FF),
            danger: color(0xC9372FFF),
            warning: color(0x8A5A1FFF),
            success: color(0x2E7D43FF),

            tile_background: color(0x00000012),
            tile_foreground: color(0x202126FF),
            tile_border: color(0x00000014),
            tile_highlight: color(0xFFFFFF59),
            tile_app_edge: color(0xFFFFFF47),
            tile_app_highlight: color(0xFFFFFF59),
            tile_drop: color(0x00000073),

            typography: Typography::shared(),
            geometry: Geometry::shared(),
        }
    }
}

impl Typography {
    fn shared() -> Typography {
        Typography {
            family: "Geist".into(),
            search_size: px(19.),
            row_title_size: px(14.),
            row_subtitle_size: px(13.),
            row_kind_size: px(12.5),
            footer_size: px(12.5),
            medium: FontWeight::MEDIUM,
        }
    }
}

/// The panel's corner radius the platforms actually show: the reference's
/// 18px where the window's corners are transparent (Linux, whose desktop
/// shows through the curve) or platform-rounded (macOS, whose authored
/// curve stands until #66's native material validation), and none on
/// Windows, where the panel fills the window to its edges and the Desktop
/// Window Manager rounds the window itself
/// ([`crate::prefer_rounded_window_corners`]). A painted radius on Windows
/// left the window's acrylic frost — or the opaque white clear — visible
/// as a rectangular plate behind the rounded corners.
fn panel_radius() -> Pixels {
    if cfg!(target_os = "windows") {
        px(0.)
    } else {
        px(18.)
    }
}

impl Geometry {
    fn shared() -> Geometry {
        Geometry {
            panel_radius: panel_radius(),
            search_height: px(64.),
            search_padding_x: px(20.),
            search_gap: px(14.),
            row_min_height: px(44.),
            row_radius: px(10.),
            row_padding_x: px(10.),
            row_gap: px(12.),
            row_list_gap: px(2.),
            tile_size: px(28.),
            tile_radius: px(7.),
            tile_glyph_size: px(16.),
            footer_height: px(50.),
            action_height: px(28.),
            action_radius: px(7.),
            action_padding_x: px(10.),
            action_gap: px(8.),
            keycap_height: px(20.),
            keycap_radius: px(5.),
            keycap_padding_x: px(5.),
            keycap_glyph_size: px(12.),
            popover_radius: px(14.),
        }
    }
}
