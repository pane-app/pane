//! Pane's semantic design tokens: the one place colors, type and geometry
//! come from. Every value is stated by role, not by screen, so restyling is
//! a one-file edit.
//!
//! Provenance: the dark palette, the geometry and the control chrome are the
//! authored reference values, read from the reference's `REFERENCE.md`
//! and the unchanged authored `launcher.html` beside it (archived with the
//! retired comparison, ADR 0029:
//! <https://github.com/pane-app/pane/tree/archive/impl-ui-91-2026-10-06/docs/evidence/ui-prototype/reference>).
//! The light palette is *derived*, not authored: the reference ships dark
//! only. Light keeps the reference's geometry and icon gradients and swaps
//! the neutrals for light-glass counterparts; it is a starting point for
//! review, not a claim of reference fidelity.
//!
//! No globals: the host settings resolve the user's preference (and the
//! system's appearance, where the preference follows it) to one palette
//! and pass the built [`Theme`] down.
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

use std::sync::Arc;

use gpui::{
    FontFeatures, FontWeight, Hsla, Pixels, SharedString, hsla, px, rgb_to_hsla, rgba,
    transparent_black,
};

/// Which palette a [`Theme`] carries. The host settings pick one — the
/// user's preference, or the system's appearance where the preference
/// follows it — and nothing here observes the system itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Appearance {
    Dark,
    Light,
}

/// A reference-stated color: `0xRRGGBBAA`.
fn color(hex: u32) -> Hsla {
    rgb_to_hsla(rgba(hex))
}

/// The stronger wash a control takes while pressed: its hover `wash` at
/// twice the alpha (at most opaque). Derived from the hover wash so every
/// theme — dark, light, over a background image — gets one without new
/// tokens. A selected row's press doubles its selected wash instead. An
/// opaque wash (the close button's danger) has no alpha left to double,
/// so it darkens instead.
pub(crate) fn pressed(wash: Hsla) -> Hsla {
    if wash.alpha >= 1. {
        let mut darker = wash;
        darker.lightness *= 0.85;
        return darker;
    }
    Hsla {
        alpha: (wash.alpha * 2.).min(1.),
        ..wash
    }
}

/// `color` at `strength` of its alpha: a hover wash's look as it fades
/// out behind the pointer (#245). Full strength is the wash itself; 0
/// is nothing drawn at all.
pub(crate) fn faded(color: Hsla, strength: f32) -> Hsla {
    Hsla {
        alpha: (color.alpha * strength.clamp(0., 1.)).min(1.),
        ..color
    }
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

    // -- Selection and hover (ADR 0035) -------------------------------
    /// The selected row's wash in every launcher list — root search's
    /// rows, a command's list, a command search, a confirmation's rows,
    /// the Actions panel's and the Pane menu's entries: the text colour
    /// at 10% (Raycast's value), with no inset edge and no ring (ADR
    /// 0035). A selected row over a background image keeps this wash over
    /// its frost, without the edge.
    pub(crate) selection_wash: Hsla,
    /// The hover wash where hovering does not move the selection (a
    /// command's list, rows under an open overlay, pinned slots, the
    /// footer's buttons, the Pane menu's entries): the text colour at
    /// 5%, fainter than the selection, fading out over
    /// [`crate::ui::motion::HOVER_FADE`] once the pointer leaves (ADR
    /// 0035). Where hovering *does* move the selection — root search's
    /// rows, the Actions panel's entries — only the selection wash ever
    /// shows.
    pub(crate) hover_wash: Hsla,

    // -- Root search's result layouts (#96) ----------------------------------
    /// The no-results notice, the computed answer's card and the empty
    /// board's Install pill (which a Settings button takes).
    pub(crate) results: ResultColors,

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
    /// An app tile's thin pale edge. The reference's is half a pixel of
    /// rgba(255,255,255,.28), which Chrome rasterizes at 100% as its tile's
    /// outermost pixel at half that alpha (measured 13–14%). GPUI draws
    /// next to nothing for an inset spread under a pixel, so the tile draws
    /// that pixel: a 1px ring of white 14%.
    pub(crate) tile_app_edge: Hsla,
    /// An app tile's top inset (reference: rgba(255,255,255,.35)).
    pub(crate) tile_app_highlight: Hsla,
    /// An app tile's short bottom shadow (reference: rgba(0,0,0,.45)).
    pub(crate) tile_drop: Hsla,
    /// A keycap's fill (reference `.kbd`: rgba(255,255,255,.07)).
    pub(crate) keycap_background: Hsla,
    /// A keycap's 1px inset ring (rgba(255,255,255,.08)).
    pub(crate) keycap_edge: Hsla,
    /// The line inset along a keycap's bottom (rgba(0,0,0,.35)).
    pub(crate) keycap_bottom: Hsla,
    /// A keycap's label (#C9CACE).
    pub(crate) keycap_text: Hsla,
    /// The accent as a *fill*: the primary action's keycap (the
    /// reference's lime, #C9EE6A, in both palettes — it carries its own
    /// near-black ink, so it reads on either panel).
    pub(crate) accent: Hsla,
    /// Ink on an accent fill (#111210).
    pub(crate) accent_ink: Hsla,
    /// A row's alias chip text (reference `.alias`: #B9BABE).
    pub(crate) alias_text: Hsla,
    /// The alias chip's 1px inset ring (rgba(255,255,255,.14)).
    pub(crate) alias_edge: Hsla,

    // -- The Settings window (the reference's Settings board) ------------------
    /// The Settings panel's tint: the board's `.glass` at .78, against the
    /// root's .70 (see `Material::settings_panel`). The light value is the
    /// light panel's own: the reference authors dark only.
    pub(crate) settings_tint: Hsla,
    /// The sidebar's fill (black 10%).
    pub(crate) sidebar_fill: Hsla,
    /// A sidebar item's label at rest (`.nav`: #B3B4B9).
    pub(crate) nav_text: Hsla,
    /// A sidebar item's wash under the pointer (white 5%), with
    /// [`Theme::nav_hover_text`] on it.
    pub(crate) nav_hover: Hsla,
    pub(crate) nav_hover_text: Hsla,
    /// The selected sidebar item's wash (`.nav.on`: white 9%), with
    /// [`Theme::nav_selected_text`] on it; it stays under the pointer.
    pub(crate) nav_selected: Hsla,
    pub(crate) nav_selected_text: Hsla,
    /// A sidebar item's glyph and count, and the search field's magnifier
    /// (#8E8F94), whatever the item's state.
    pub(crate) nav_icon: Hsla,
    /// A Settings field's well — the sidebar's search — (black 24%) and
    /// its 1px inset ring (white 6%).
    pub(crate) field_fill: Hsla,
    pub(crate) field_edge: Hsla,
    /// A Settings card: the raised block a section's rows sit in (white
    /// 4.7% over the panel in dark), its 1px inset ring and the rule
    /// between its rows. Pane's own: the reference's board has no cards.
    pub(crate) card_fill: Hsla,
    pub(crate) card_edge: Hsla,
    pub(crate) card_rule: Hsla,
    /// The Settings controls (#98): see [`ControlColors`].
    pub(crate) controls: ControlColors,

    // -- The footer's buttons and the Actions panel ---------------------------
    /// The footer mark's filled square (#EDEDEF at .92); its stroked one
    /// is [`Theme::text_muted`].
    pub(crate) footer_mark: Hsla,
    /// A footer button's label (reference `.fbtn`: #D9DADD).
    pub(crate) footer_button_text: Hsla,
    /// The Actions button while its panel is open (white 10%), with
    /// [`Theme::footer_button_open_text`] on it.
    pub(crate) footer_button_open: Hsla,
    /// The open Actions button's label (#FFFFFF).
    pub(crate) footer_button_open_text: Hsla,
    /// The 1×16 rule between the footer's buttons (white 10%).
    pub(crate) footer_divider: Hsla,
    /// An Actions row's label (#E4E4E7).
    pub(crate) action_text: Hsla,
    /// An Actions row's glyph (#A3A4A9).
    pub(crate) action_icon: Hsla,
    /// The Actions panel's rules: its separators and the line above its
    /// search (white 7%).
    pub(crate) action_rule: Hsla,
    /// The dimmer over the results while the Actions panel is open
    /// (rgba(6,7,8,.34)).
    pub(crate) actions_dimmer: Hsla,
    /// A popover's outer shadows (`.pop`): its 0.5px dark outline (black
    /// 80%) and its long soft drop (black 75%).
    pub(crate) popover_outline: Hsla,
    pub(crate) popover_drop: Hsla,
    /// The footer row's imperceptible fill (black at 1/255), which keeps
    /// its content above a popup's drop shadow; see
    /// `crate::ui::footer::footer_row`.
    pub(crate) footer_order_fill: Hsla,

    /// The split view's own tokens (the reference's clipboard board):
    /// see [`SplitTokens`].
    pub(crate) split: SplitTokens,

    // -- The pinned home's quick slots (#101) ---------------------------------
    /// A pinned slot's fill at rest (reference `.slot`: white 3.5%).
    pub(crate) slot_background: Hsla,
    /// A pinned slot's 1px inset edge (white 5%).
    pub(crate) slot_edge: Hsla,
    /// A pinned slot's title (`.slot-t`: #D9DADD).
    pub(crate) slot_title: Hsla,
    /// The pin hint's dashed outline, in the strip's cell after the last
    /// pin (Pane's own: the reference authors no such tile; white 10%).
    pub(crate) slot_empty_edge: Hsla,

    // -- Over a background image (ADR 0028) -----------------------------------
    /// The frosted surfaces over the launcher's background image — the
    /// search pill, the pins, the selected row and the footer — when one
    /// is drawn (see [`Theme::over_backdrop`]); `None`, the palettes' own,
    /// draws none of it.
    pub(crate) frost: Option<Frost>,

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
    /// The OpenType features every text run asks for: kerning. Browsers
    /// kern by default, and the reference's Geist is laid out kerned; GPUI
    /// on Windows passes DirectWrite an explicit feature list without
    /// `kern`, so unkerned Geist ran about 2% wider than the reference's
    /// ("opens instantly ·" at 12.5px: 94.95px against 93, #95).
    pub(crate) features: FontFeatures,
    /// The monospace family (embedded Geist Mono): keycaps and aliases.
    pub(crate) mono_family: SharedString,
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
    /// A keycap's label: Geist Mono 11.
    pub(crate) keycap_size: Pixels,
    /// A compact keycap's label: 10.
    pub(crate) keycap_compact_size: Pixels,
    /// A row's alias chip: Geist Mono 11.
    pub(crate) alias_size: Pixels,
    /// Geist Mono's natural line height, as a multiple of its size: its
    /// ascent and descent (1005 + 295 per 1000), what CSS's `normal`
    /// gives text the reference sets no line height for.
    pub(crate) mono_line_height: f32,
    /// A section label's 12px.
    pub(crate) section_size: Pixels,
    /// A section label's tracking, in em (.01).
    pub(crate) section_tracking: f32,
    /// Geist's natural line height, as a multiple of its size: its
    /// ascent and descent (1005 + 295 per 1000), CSS's `normal` for text
    /// the reference sets no line height for.
    pub(crate) line_height: f32,
    /// The Settings window's 13px: its titlebar label, its sidebar items
    /// and its search field.
    pub(crate) settings_text_size: Pixels,
    /// The Settings window's 12px captions: a sidebar item's count. (A
    /// page column's label, "Preview", is [`SettingsType::caption`].)
    pub(crate) settings_caption_size: Pixels,
    /// Root search's result layouts' type (#96).
    pub(crate) results: ResultType,
    /// The Settings pages' type lines: the heading block and the controls
    /// (#98).
    pub(crate) settings: SettingsType,
    /// An Actions row's 13px label, its search's 13px and its empty
    /// note's.
    pub(crate) action_size: Pixels,
    /// An Actions row's label weight (450).
    pub(crate) action_weight: FontWeight,
    /// The Actions panel header's 12px.
    pub(crate) actions_header_size: Pixels,
    /// An Actions group label's 11.5px (`.alabel`).
    pub(crate) action_group_size: Pixels,
    /// A pinned slot's 12.5px title (`.slot-t`).
    pub(crate) slot_title_size: Pixels,
    /// The 11.5px reason under an unavailable slot's title (Pane's own).
    pub(crate) slot_reason_size: Pixels,
    /// Title and label weight (500).
    pub(crate) medium: FontWeight,
    /// Body weight (400): a section label's note.
    pub(crate) regular: FontWeight,
}

/// The reference's geometry: the launcher panel is 760px wide, the search
/// field 64px tall, rows 44px, the panel radius 18px where the platform
/// shows it (none on Windows, whose window the Desktop Window Manager
/// rounds; see [`panel_radius`]), and the keycaps are the reference's
/// `.kbd` family.
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
    /// The search header's magnifier glyph.
    pub(crate) search_glyph_size: Pixels,
    /// The query text's inset inside its field: the reference's `<input>`
    /// keeps the browser's own 2px inline padding, so its text begins 2px
    /// after the field does.
    pub(crate) search_text_inset: Pixels,
    /// A launcher screen's 12 above and below its heading, and above and
    /// below a form's fields (their sides are the search header's 20).
    pub(crate) screen_padding_y: Pixels,
    /// The result list's padding above its first row (the reference's
    /// root body: 4).
    pub(crate) list_padding_top: Pixels,
    /// The result list's padding below its last row (10).
    pub(crate) list_padding_bottom: Pixels,
    /// The result list's side padding: the rows' inset from the panel
    /// (10). A separate token from a row's own [`Self::row_padding_x`],
    /// which the reference also authors as 10.
    pub(crate) list_padding_x: Pixels,
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
    /// A row's kind label's least width, right-aligned in it (88).
    pub(crate) row_kind_min_width: Pixels,
    /// Root search's result layouts (#96).
    pub(crate) results: ResultGeometry,
    /// The alias chip's padding: 2 above and below, 6 either side.
    pub(crate) alias_padding_y: Pixels,
    pub(crate) alias_padding_x: Pixels,
    /// The alias chip's corner radius (5).
    pub(crate) alias_radius: Pixels,
    /// A section label's height (30), its padding above (8) and either
    /// side (10), and the gap between its title and note (8).
    pub(crate) section_height: Pixels,
    pub(crate) section_padding_top: Pixels,
    pub(crate) section_padding_x: Pixels,
    pub(crate) section_gap: Pixels,
    /// A result row's icon tile: 28, radius 7, a 16px glyph.
    pub(crate) tile: TileMetrics,
    /// A pinned slot's icon tile: 42, radius 11, a 22px glyph.
    pub(crate) slot_tile: TileMetrics,
    /// The Actions panel header's icon tile: 18, radius 5, an 11px glyph.
    pub(crate) mini_tile: TileMetrics,
    /// The Settings window's shell, sidebar and page composition.
    pub(crate) settings: SettingsGeometry,
    /// The Settings controls: fields, segments and toggles (#98).
    pub(crate) controls: ControlGeometry,
    /// The footer's height.
    pub(crate) footer_height: Pixels,
    /// The footer's left padding (the reference's 16).
    pub(crate) footer_padding_left: Pixels,
    /// The footer's right padding (8: the right-hand buttons carry their
    /// own 8px padding, so their labels end 16px from the edge).
    pub(crate) footer_padding_right: Pixels,
    /// A footer button's height (reference `.fbtn`).
    pub(crate) action_height: Pixels,
    /// A footer button's corner radius.
    pub(crate) action_radius: Pixels,
    /// A footer button's horizontal padding.
    pub(crate) action_padding_x: Pixels,
    /// The gap between a footer button's label and its keycaps.
    pub(crate) action_gap: Pixels,
    /// The gap between the footer's right-hand buttons and their rule.
    pub(crate) footer_buttons_gap: Pixels,
    /// The rule between the footer's buttons: 1 wide, this tall.
    pub(crate) footer_divider_height: Pixels,
    /// The gap between the footer's mark and its hint.
    pub(crate) footer_lead_gap: Pixels,
    /// The gap between the parts of the footer's hint.
    pub(crate) footer_hint_gap: Pixels,
    /// The footer's Pane mark.
    pub(crate) footer_mark_size: Pixels,
    /// How far the mark's button box bleeds past the mark on every side,
    /// for its hover wash.
    pub(crate) footer_mark_bleed: Pixels,
    /// A footer status message's padding above and below each line block:
    /// one line of it centers in the strip's first line, as the hint does.
    pub(crate) footer_status_padding_y: Pixels,
    /// A popover's outline width and its drop: offset down, blur and
    /// spread (`0 0 0 .5px`, `0 28px 70px -14px`).
    pub(crate) popover_outline_width: Pixels,
    pub(crate) popover_drop_offset: Pixels,
    pub(crate) popover_drop_blur: Pixels,
    pub(crate) popover_drop_spread: Pixels,
    /// The Actions panel.
    pub(crate) actions: ActionsGeometry,
    /// The pinned home's strip of quick slots.
    pub(crate) pinned: PinnedGeometry,
    /// A keycap's height, and its least width.
    pub(crate) keycap_height: Pixels,
    /// A keycap's corner radius.
    pub(crate) keycap_radius: Pixels,
    /// A keycap's horizontal padding.
    pub(crate) keycap_padding_x: Pixels,
    /// A compact keycap's height, and its least width.
    pub(crate) keycap_compact_height: Pixels,
    /// A compact keycap's horizontal padding.
    pub(crate) keycap_compact_padding_x: Pixels,
    /// The gap between the caps of one key sequence.
    pub(crate) key_gap: Pixels,
    /// A popover's corner radius (the reference's L2 `.pop`: 14px, shown
    /// on every platform, since a popover floats inside the window rather
    /// than at its edge).
    pub(crate) popover_radius: Pixels,
}

/// The Actions panel's geometry: the reference's `.pop` over the footer,
/// its header, `.arow`, `.alabel`, `.sep` and search.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ActionsGeometry {
    /// The panel's width.
    pub(crate) width: Pixels,
    /// Its inset from the window's nearer side edge: the Actions panel's
    /// right, the Pane menu's left.
    pub(crate) inset: Pixels,
    /// The space between its bottom and the footer's top.
    pub(crate) above_footer: Pixels,
    /// The header: its height, its top and side padding, the gap after
    /// its tile.
    pub(crate) header_height: Pixels,
    pub(crate) header_padding_top: Pixels,
    pub(crate) header_padding_x: Pixels,
    pub(crate) header_gap: Pixels,
    /// The list's padding and the gap between its rows.
    pub(crate) list_padding: Pixels,
    pub(crate) list_gap: Pixels,
    /// A row: its height, radius, side padding, the gap between its parts
    /// and its glyph's size.
    pub(crate) row_height: Pixels,
    pub(crate) row_radius: Pixels,
    pub(crate) row_padding_x: Pixels,
    pub(crate) row_gap: Pixels,
    pub(crate) glyph_size: Pixels,
    /// A group label: its height, side padding and bottom padding.
    pub(crate) group_height: Pixels,
    pub(crate) group_padding_x: Pixels,
    pub(crate) group_padding_bottom: Pixels,
    /// A separator's margin, above and below, and either side.
    pub(crate) rule_margin_y: Pixels,
    pub(crate) rule_margin_x: Pixels,
    /// The search row: its height, side padding, gap and glyph size.
    pub(crate) search_height: Pixels,
    pub(crate) search_padding_x: Pixels,
    pub(crate) search_gap: Pixels,
    pub(crate) search_glyph_size: Pixels,
    /// The empty note's padding, above and below, and either side.
    pub(crate) empty_padding_y: Pixels,
    pub(crate) empty_padding_x: Pixels,
}

/// The Settings window's geometry: the reference Settings board's
/// titlebar, its `nav` sidebar with the search field and the `.nav`
/// items, and Pane's own page area, its sections and cards (see
/// `crate::ui::settings_shell`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SettingsGeometry {
    /// The titlebar's height, its 1px rule included.
    pub(crate) titlebar_height: Pixels,
    /// A Windows caption button's width (the platform's own 46): the
    /// adaptation of the reference's lone close glyph. Its glyph's size.
    pub(crate) caption_width: Pixels,
    pub(crate) caption_glyph: Pixels,
    /// The sidebar: its width (its 1px right rule included), its padding
    /// above and below, either side, and the gap between its children.
    pub(crate) sidebar_width: Pixels,
    pub(crate) sidebar_padding_y: Pixels,
    pub(crate) sidebar_padding_x: Pixels,
    pub(crate) sidebar_gap: Pixels,
    /// The search field: its height, side padding, the gap after its
    /// magnifier, the space below it, its radius and its magnifier.
    pub(crate) search_height: Pixels,
    pub(crate) search_padding_x: Pixels,
    pub(crate) search_gap: Pixels,
    pub(crate) search_margin_bottom: Pixels,
    pub(crate) search_radius: Pixels,
    pub(crate) search_glyph: Pixels,
    /// A sidebar item (`.nav`): its height (a floor), radius, side
    /// padding, the gap between its parts and its glyph.
    pub(crate) item_height: Pixels,
    pub(crate) item_radius: Pixels,
    pub(crate) item_padding_x: Pixels,
    pub(crate) item_gap: Pixels,
    pub(crate) item_glyph: Pixels,
    /// Pane's own page layout: the content's padding above, either side
    /// and below, and the widest it grows in a large window.
    pub(crate) content_padding_top: Pixels,
    pub(crate) content_padding_x: Pixels,
    pub(crate) content_padding_bottom: Pixels,
    pub(crate) content_max_width: Pixels,
    /// A section: the space between sections, between a section's label
    /// and its card, and the label's inset from the card's edge.
    pub(crate) section_gap: Pixels,
    pub(crate) section_label_gap: Pixels,
    pub(crate) section_label_inset: Pixels,
    /// A card: its radius and its rows' side padding.
    pub(crate) card_radius: Pixels,
    pub(crate) card_padding_x: Pixels,
    /// A card's row: at least 48 high, 10 above and below its text.
    pub(crate) card_row_height: Pixels,
    pub(crate) card_row_padding_y: Pixels,
    /// The width of a choice at a row's end: a select's trigger and its
    /// popup, a segmented choice's track.
    pub(crate) choice_width: Pixels,
    /// The width of a segmented choice with more segments than
    /// [`SettingsGeometry::choice_width`] holds: the background image's
    /// five effects.
    pub(crate) wide_choice_width: Pixels,
}

/// The pinned home's geometry: the reference's grid of `.slot`s under the
/// "Pinned" label — five equal columns, wrapping onto more rows as pins
/// are added (`crate::ui::pinned::PINNED_COLUMNS`) — and each slot's
/// anatomy, which the pin hint's cell shares.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PinnedGeometry {
    /// The gap between the strip's equal columns, and between its rows
    /// once it wraps.
    pub(crate) columns_gap: Pixels,
    /// The grid's padding above and below its slots.
    pub(crate) strip_padding_top: Pixels,
    pub(crate) strip_padding_bottom: Pixels,
    /// A slot: its height, radius and paddings (8 all round), and the gap
    /// between its tile and its title — Pane's compact strip, lower than
    /// the reference's 100px slots so the results keep more room.
    pub(crate) slot_height: Pixels,
    pub(crate) slot_radius: Pixels,
    pub(crate) slot_padding_top: Pixels,
    pub(crate) slot_padding_x: Pixels,
    pub(crate) slot_padding_bottom: Pixels,
    pub(crate) slot_gap: Pixels,
    /// An unavailable slot's gap between its tile and its title, the
    /// reason right under the title (Pane's own state): the tile, both
    /// lines and the 7px gaps would overflow the slot's 64px inside its
    /// paddings, so the lines would shrink and clip; 30, 2 and the two
    /// lines (16.25 and 14.95) fit.
    pub(crate) unavailable_gap: Pixels,
    /// The corner key hint's inset from the slot's top and right (8).
    pub(crate) keys_inset: Pixels,
    /// The focus ring's width (the reference's 2px outline, inset).
    pub(crate) focus_width: Pixels,
    /// The inset edge's width (1px).
    pub(crate) edge_width: Pixels,
}

/// One icon tile size: the square's side, its corner radius and the
/// glyph inside it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TileMetrics {
    /// The square's side.
    pub(crate) size: Pixels,
    /// Its corner radius.
    pub(crate) radius: Pixels,
    /// The glyph's side inside it.
    pub(crate) glyph: Pixels,
}

/// The colors of root search's result layouts (#96): the reference's
/// empty and calculator boards. Text roles the launcher already has (a
/// title's, a muted label's) are the theme's own.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ResultColors {
    /// The notice's disc (white 6%), its 1px inset ring (white 8%) and its
    /// glyph (#A3A4A9).
    pub(crate) notice_disc: Hsla,
    pub(crate) notice_disc_edge: Hsla,
    pub(crate) notice_glyph: Hsla,
    /// The answer card's fill (white 6%); while selected the card shows
    /// [`Theme::selection_wash`] instead, with no ring (ADR 0035).
    pub(crate) card_fill: Hsla,
    /// The value typed (#D9DADD) and the answer (#FFFFFF).
    pub(crate) card_source: Hsla,
    pub(crate) card_answer: Hsla,
    /// The disc behind the card's arrow (white 7%) and the arrow (#A3A4A9).
    pub(crate) card_arrow_disc: Hsla,
    pub(crate) card_arrow: Hsla,
    /// The rule above the card's "Also" line (white 7%).
    pub(crate) card_rule: Hsla,
    /// A chip on that line: its fill (white 6%), ring (white 7%), hover
    /// fill (white 10%) and label (#D9DADD).
    pub(crate) chip_fill: Hsla,
    pub(crate) chip_edge: Hsla,
    pub(crate) chip_hover: Hsla,
    pub(crate) chip_text: Hsla,
    /// The empty board's Install pill, a Settings button's
    /// (`crate::ui::controls`): its fill (white 8%), ring (white 8%) and
    /// hover fill (white 13%).
    pub(crate) pill_fill: Hsla,
    pub(crate) pill_edge: Hsla,
    pub(crate) pill_hover: Hsla,
}

impl ResultColors {
    /// The reference's dark values, exactly as authored.
    fn dark() -> ResultColors {
        ResultColors {
            notice_disc: color(0xFFFFFF0F),
            notice_disc_edge: color(0xFFFFFF14),
            notice_glyph: color(0xA3A4A9FF),
            card_fill: color(0xFFFFFF0F),
            card_source: color(0xD9DADDFF),
            card_answer: color(0xFFFFFFFF),
            card_arrow_disc: color(0xFFFFFF12),
            card_arrow: color(0xA3A4A9FF),
            card_rule: color(0xFFFFFF12),
            chip_fill: color(0xFFFFFF0F),
            chip_edge: color(0xFFFFFF12),
            chip_hover: color(0xFFFFFF1A),
            chip_text: color(0xD9DADDFF),
            pill_fill: color(0xFFFFFF14),
            pill_edge: color(0xFFFFFF14),
            pill_hover: color(0xFFFFFF21),
        }
    }

    /// Derived light counterparts, as the light palette derives the rest:
    /// black washes for white ones and dark inks, a proposal for review.
    fn light() -> ResultColors {
        ResultColors {
            notice_disc: color(0x0000000F),
            notice_disc_edge: color(0x00000014),
            notice_glyph: color(0x575A63FF),
            card_fill: color(0x0000000F),
            card_source: color(0x2A2B31FF),
            card_answer: color(0x111214FF),
            card_arrow_disc: color(0x00000012),
            card_arrow: color(0x575A63FF),
            card_rule: color(0x00000012),
            chip_fill: color(0x0000000F),
            chip_edge: color(0x00000012),
            chip_hover: color(0x0000001A),
            chip_text: color(0x2A2B31FF),
            pill_fill: color(0x00000014),
            pill_edge: color(0x00000014),
            pill_hover: color(0x00000021),
        }
    }
}

/// A text role's size and its line box: CSS's `normal` line height for
/// Geist and Geist Mono (their ascent and descent, 1005 + 295 per 1000)
/// as Chrome lays it out, rounding the ascent and the descent each to a
/// whole pixel — so a line box here is the reference's own height.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TypeLine {
    pub(crate) size: Pixels,
    pub(crate) line_height: Pixels,
}

/// The type of root search's result layouts (#96).
#[derive(Clone, Copy, Debug)]
pub(crate) struct ResultType {
    /// The notice's title (15/500) and description (13).
    pub(crate) notice_title: TypeLine,
    pub(crate) notice_description: TypeLine,
    /// The card's values: Geist Mono 34/500 as authored, then the smaller
    /// steps a value too long for its column takes (24, then 18).
    pub(crate) answer_value: TypeLine,
    pub(crate) answer_value_compact: TypeLine,
    pub(crate) answer_value_small: TypeLine,
    /// The values' tracking, in em (−.03).
    pub(crate) answer_tracking: f32,
    /// Geist Mono's advance, in em: every glyph's is the same (600 units).
    pub(crate) mono_advance: f32,
    /// A value's caption (12.5), the "Also" label (12) and a chip's label
    /// (Geist Mono 12.5).
    pub(crate) answer_caption: TypeLine,
    pub(crate) answer_also: TypeLine,
    pub(crate) chip: TypeLine,
    pub(crate) pill: TypeLine,
}

impl ResultType {
    fn shared() -> ResultType {
        let line = |size: f32, line_height: f32| TypeLine {
            size: px(size),
            line_height: px(line_height),
        };
        ResultType {
            notice_title: line(15., 19.),
            notice_description: line(13., 17.),
            answer_value: line(34., 44.),
            answer_value_compact: line(24., 31.),
            answer_value_small: line(18., 23.),
            answer_tracking: -0.03,
            mono_advance: 0.6,
            answer_caption: line(12.5, 17.),
            answer_also: line(12., 16.),
            chip: line(12.5, 17.),
            pill: line(12.5, 17.),
        }
    }
}

/// The geometry of root search's result layouts (#96): the reference's
/// empty board's notice and Install pill, and its calculator board's
/// card.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ResultGeometry {
    /// The notice: 84 high, padded 8 above, 4 below and 12 either side,
    /// 16 between its disc and its text, and 4 between the text's lines.
    pub(crate) notice_height: Pixels,
    pub(crate) notice_padding_top: Pixels,
    pub(crate) notice_padding_bottom: Pixels,
    pub(crate) notice_padding_x: Pixels,
    pub(crate) notice_gap: Pixels,
    pub(crate) notice_text_gap: Pixels,
    /// The notice's 44px disc and its 20px glyph.
    pub(crate) notice_disc: Pixels,
    pub(crate) notice_glyph: Pixels,
    /// The card: 2 above it and 4 below, padded 20 above and either side
    /// and 16 below, 14 between its values and its "Also" line, radius 14.
    pub(crate) card_margin_top: Pixels,
    pub(crate) card_margin_bottom: Pixels,
    pub(crate) card_padding_top: Pixels,
    pub(crate) card_padding_x: Pixels,
    pub(crate) card_padding_bottom: Pixels,
    pub(crate) card_gap: Pixels,
    pub(crate) card_radius: Pixels,
    /// The 16 between the card's columns, and the 4 between a value and its
    /// caption.
    pub(crate) card_column_gap: Pixels,
    pub(crate) card_value_gap: Pixels,
    /// The arrow's 40px disc and its 18px glyph.
    pub(crate) card_arrow_disc: Pixels,
    pub(crate) card_arrow: Pixels,
    /// The "Also" line: 14 below its rule, 8 between its parts.
    pub(crate) also_padding_top: Pixels,
    pub(crate) also_gap: Pixels,
    /// A chip: 30 high, 10 either side, radius 8.
    pub(crate) chip_height: Pixels,
    pub(crate) chip_padding_x: Pixels,
    pub(crate) chip_radius: Pixels,
    /// The Install pill: 30 high, 12 either side, radius 8.
    pub(crate) pill_height: Pixels,
    pub(crate) pill_padding_x: Pixels,
    pub(crate) pill_radius: Pixels,
}

impl ResultGeometry {
    fn shared() -> ResultGeometry {
        ResultGeometry {
            notice_height: px(84.),
            notice_padding_top: px(8.),
            notice_padding_bottom: px(4.),
            notice_padding_x: px(12.),
            notice_gap: px(16.),
            notice_text_gap: px(4.),
            notice_disc: px(44.),
            notice_glyph: px(20.),
            card_margin_top: px(2.),
            card_margin_bottom: px(4.),
            card_padding_top: px(20.),
            card_padding_x: px(20.),
            card_padding_bottom: px(16.),
            card_gap: px(14.),
            card_radius: px(14.),
            card_column_gap: px(16.),
            card_value_gap: px(4.),
            card_arrow_disc: px(40.),
            card_arrow: px(18.),
            also_padding_top: px(14.),
            also_gap: px(8.),
            chip_height: px(30.),
            chip_padding_x: px(10.),
            chip_radius: px(8.),
            pill_height: px(30.),
            pill_padding_x: px(12.),
            pill_radius: px(8.),
        }
    }
}

/// The colors of the Settings controls (#98): the reference Settings
/// board's `.segwrap`/`.seg` and toggles. Text roles the theme already
/// has (a field's label is the title ink, its description the muted one)
/// are the theme's own.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ControlColors {
    /// A segmented control's track (black 24%) under its 1px inset ring
    /// (white 6%).
    pub(crate) segment_track: Hsla,
    pub(crate) segment_edge: Hsla,
    /// A segment's label at rest (#9A9BA0) and under the pointer (#EDEDEF).
    pub(crate) segment_text: Hsla,
    pub(crate) segment_hover_text: Hsla,
    /// The chosen segment: its wash (white 12%), its label (#FFFFFF) and
    /// its 1px top inset (white 8%).
    pub(crate) segment_on: Hsla,
    pub(crate) segment_on_text: Hsla,
    pub(crate) segment_on_highlight: Hsla,
    /// A toggle's track while off (white 16%; on, it is the accent), its
    /// knob (#FFFFFF) and the knob's short shadow (black 40%).
    pub(crate) toggle_off: Hsla,
    pub(crate) toggle_knob: Hsla,
    pub(crate) toggle_knob_shadow: Hsla,
}

impl ControlColors {
    /// The reference's dark values, exactly as authored.
    fn dark() -> ControlColors {
        ControlColors {
            segment_track: color(0x0000003D),
            segment_edge: color(0xFFFFFF0F),
            segment_text: color(0x9A9BA0FF),
            segment_hover_text: color(0xEDEDEFFF),
            segment_on: color(0xFFFFFF1F),
            segment_on_text: color(0xFFFFFFFF),
            segment_on_highlight: color(0xFFFFFF14),
            toggle_off: color(0xFFFFFF29),
            toggle_knob: color(0xFFFFFFFF),
            toggle_knob_shadow: color(0x00000066),
        }
    }

    /// Derived light counterparts, as the light palette derives the rest:
    /// black washes for white ones, a white chosen segment on the darker
    /// track, dark inks. A proposal for review, not reference truth.
    fn light() -> ControlColors {
        ControlColors {
            segment_track: color(0x0000000A),
            segment_edge: color(0x00000014),
            segment_text: color(0x575A63FF),
            segment_hover_text: color(0x202126FF),
            segment_on: color(0xFFFFFFD9),
            segment_on_text: color(0x111214FF),
            segment_on_highlight: color(0xFFFFFF66),
            toggle_off: color(0x00000029),
            toggle_knob: color(0xFFFFFFFF),
            toggle_knob_shadow: color(0x00000040),
        }
    }
}

/// The Settings pages' type: each role's size and its line box, as Chrome
/// lays the reference's `normal` line height out (see [`TypeLine`]), so a
/// column of fields stacks to the reference's own heights.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SettingsType {
    /// An aside column's caption ("Preview": 12/500, 16).
    pub(crate) caption: TypeLine,
    /// A field's label (`.flabel`: 13.5/500, 18) and its description
    /// (`.fdesc`: 12.5 at line height 1.45, 18.125).
    pub(crate) field_label: TypeLine,
    pub(crate) field_description: TypeLine,
    /// A segment's label (12.5/500, 17).
    pub(crate) segment: TypeLine,
}

impl SettingsType {
    fn shared() -> SettingsType {
        let line = |size: f32, line_height: f32| TypeLine {
            size: px(size),
            line_height: px(line_height),
        };
        SettingsType {
            caption: line(12., 16.),
            field_label: line(13.5, 18.),
            field_description: line(12.5, 18.125),
            segment: line(12.5, 17.),
        }
    }
}

/// The Settings controls' geometry (#98): the reference board's field
/// groups, `.segwrap`/`.seg` and toggles.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ControlGeometry {
    /// The gap between a page's field groups (18) and inside one, between
    /// its label, its control and its description (8).
    pub(crate) group_gap: Pixels,
    pub(crate) field_gap: Pixels,
    /// A segmented control: the track's padding (3), the gap between its
    /// segments (2) and its radius (10); a segment's height (30) and
    /// radius (7). The track is 36 high.
    pub(crate) track_padding: Pixels,
    pub(crate) segment_gap: Pixels,
    pub(crate) track_radius: Pixels,
    pub(crate) segment_height: Pixels,
    pub(crate) segment_radius: Pixels,
    /// A toggle's row (44 with its 1px rule above), the toggle (40×24,
    /// radius 12), its knob (18) and the knob's inset (3; on, it sits at
    /// 19).
    pub(crate) toggle_row_height: Pixels,
    pub(crate) toggle_width: Pixels,
    pub(crate) toggle_height: Pixels,
    pub(crate) toggle_knob: Pixels,
    pub(crate) toggle_inset: Pixels,
    /// A control's keyboard focus ring (Pane's adaptation: the reference
    /// draws none for these controls).
    pub(crate) focus_width: Pixels,
    /// A disabled field's opacity: the reference's 40% for the sliders
    /// Solid disables, Pane's for any control it cannot offer.
    pub(crate) disabled_opacity: f32,
    /// A settings row (#99), the toggle row generalized: at least the
    /// toggle row's 44, 10 above and below a label that has a description
    /// under it, and 16 between the text and the row's control. Derived:
    /// the board's toggle rows carry no description.
    pub(crate) row_padding_y: Pixels,
    pub(crate) row_gap: Pixels,
    /// The gap between a button's parts (`.pill`'s 6; its box is the
    /// result layouts' pill's, [`ResultGeometry`]), and between the
    /// controls at a settings row's end.
    pub(crate) button_gap: Pixels,
    /// The 2px between a Settings list's entries (an extension's, a group
    /// of commands'), as the sidebar's items stand.
    pub(crate) list_gap: Pixels,
    /// A field's well, the sidebar search's family: 34 high on its own
    /// line, 30 inside a settings row (a button's height), 10 either side,
    /// radius 8, 8 between its 14px glyph and its text.
    pub(crate) well_height: Pixels,
    pub(crate) inline_well_height: Pixels,
    pub(crate) well_padding_x: Pixels,
    pub(crate) well_radius: Pixels,
    pub(crate) well_gap: Pixels,
    pub(crate) well_glyph: Pixels,
    /// A key binding recorder (Discord's keybind field): 220×36, its icon
    /// buttons 4 in from its right edge.
    pub(crate) recorder_width: Pixels,
    pub(crate) recorder_height: Pixels,
    pub(crate) recorder_inset: Pixels,
    /// An icon button: a 28px square, radius 6, its glyph 16.
    pub(crate) icon_button_size: Pixels,
    pub(crate) icon_button_radius: Pixels,
    pub(crate) icon_button_glyph: Pixels,
    /// A pressable list row's radius (the sidebar item's 8) and the 14px
    /// chevron at its right end.
    pub(crate) list_radius: Pixels,
    pub(crate) chevron: Pixels,
    /// A select's list row: 4 above and below its label and description
    /// (Pane's own: the Actions panel's entries hold one line), and the
    /// committed choice's mark, a 6px disc.
    pub(crate) menu_padding_y: Pixels,
    pub(crate) mark_size: Pixels,
    /// The host's frame around an extension's own view: 4 of padding (the
    /// part of the frame a press only focuses) and radius 10.
    pub(crate) frame_padding: Pixels,
    pub(crate) frame_radius: Pixels,
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

            selection_wash: color(0xEDEDEF1A),
            hover_wash: color(0xEDEDEF0D),

            results: ResultColors::dark(),

            focus_ring: color(0xFFFFFF80),

            accent_text: color(0xC9EE6AFF),
            danger: color(0xFF9A92FF),
            warning: color(0xD6A36AFF),
            success: color(0x9FD8A8FF),

            tile_background: color(0xFFFFFF14),
            tile_foreground: color(0xE9E9ECFF),
            tile_border: color(0xFFFFFF14),
            tile_highlight: color(0xFFFFFF1A),
            tile_app_edge: color(0xFFFFFF24),
            tile_app_highlight: color(0xFFFFFF59),
            tile_drop: color(0x00000073),
            keycap_background: color(0xFFFFFF12),
            keycap_edge: color(0xFFFFFF14),
            keycap_bottom: color(0x00000059),
            keycap_text: color(0xC9CACEFF),
            accent: color(0xC9EE6AFF),
            accent_ink: color(0x111210FF),
            alias_text: color(0xB9BABEFF),
            alias_edge: color(0xFFFFFF24),

            settings_tint: color(0x16171AC7),
            sidebar_fill: color(0x0000001A),
            nav_text: color(0xB3B4B9FF),
            nav_hover: color(0xFFFFFF0D),
            nav_hover_text: color(0xEDEDEFFF),
            nav_selected: color(0xFFFFFF17),
            nav_selected_text: color(0xFFFFFFFF),
            nav_icon: color(0x8E8F94FF),
            field_fill: color(0x0000003D),
            field_edge: color(0xFFFFFF0F),
            card_fill: color(0xFFFFFF0C),
            card_edge: color(0xFFFFFF0F),
            card_rule: color(0xFFFFFF0D),
            controls: ControlColors::dark(),

            footer_mark: color(0xEDEDEFEB),
            footer_button_text: color(0xD9DADDFF),
            footer_button_open: color(0xFFFFFF1A),
            footer_button_open_text: color(0xFFFFFFFF),
            footer_divider: color(0xFFFFFF1A),
            action_text: color(0xE4E4E7FF),
            action_icon: color(0xA3A4A9FF),
            action_rule: color(0xFFFFFF12),
            actions_dimmer: color(0x06070857),
            popover_outline: color(0x000000CC),
            popover_drop: color(0x000000BF),
            footer_order_fill: color(0x00000001),

            split: SplitTokens::dark(),

            slot_background: color(0xFFFFFF09),
            slot_edge: color(0xFFFFFF0D),
            slot_title: color(0xD9DADDFF),
            slot_empty_edge: color(0xFFFFFF1A),

            frost: None,

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

            selection_wash: color(0x2021261A),
            hover_wash: color(0x2021260D),

            results: ResultColors::light(),

            focus_ring: color(0x00000073),

            accent_text: color(0x5C7A17FF),
            danger: color(0xC9372FFF),
            warning: color(0x8A5A1FFF),
            success: color(0x2E7D43FF),

            tile_background: color(0x00000012),
            tile_foreground: color(0x202126FF),
            tile_border: color(0x00000014),
            tile_highlight: color(0xFFFFFF59),
            tile_app_edge: color(0xFFFFFF24),
            tile_app_highlight: color(0xFFFFFF59),
            tile_drop: color(0x00000073),
            keycap_background: color(0x0000000D),
            keycap_edge: color(0x00000014),
            keycap_bottom: color(0x00000026),
            keycap_text: color(0x3B3D44FF),
            accent: color(0xC9EE6AFF),
            accent_ink: color(0x111210FF),
            alias_text: color(0x3B3D44FF),
            alias_edge: color(0x00000024),

            // Derived, like the rest of the light palette: the light
            // panel's own tint, and dark washes in place of the dark
            // board's white ones.
            settings_tint: color(0xF6F6F8CC),
            sidebar_fill: color(0x00000008),
            nav_text: color(0x3B3D44FF),
            nav_hover: color(0x0000000B),
            nav_hover_text: color(0x202126FF),
            nav_selected: color(0x00000016),
            nav_selected_text: color(0x111214FF),
            nav_icon: color(0x575A63FF),
            field_fill: color(0x0000000A),
            field_edge: color(0x00000014),
            card_fill: color(0xFFFFFFB3),
            card_edge: color(0x00000012),
            card_rule: color(0x0000000F),
            controls: ControlColors::light(),

            footer_mark: color(0x202126EB),
            footer_button_text: color(0x2A2B31FF),
            footer_button_open: color(0x0000001A),
            footer_button_open_text: color(0x111214FF),
            footer_divider: color(0x0000001A),
            action_text: color(0x202126FF),
            action_icon: color(0x575A63FF),
            action_rule: color(0x00000012),
            actions_dimmer: color(0x06070826),
            popover_outline: color(0x00000033),
            popover_drop: color(0x00000040),
            footer_order_fill: color(0x00000001),

            split: SplitTokens::light(),

            slot_background: color(0x00000009),
            slot_edge: color(0x0000000D),
            slot_title: color(0x2A2B31FF),
            slot_empty_edge: color(0x0000001A),

            frost: None,

            typography: Typography::shared(),
            geometry: Geometry::shared(),
        }
    }

    /// This palette over the launcher's background image (ADR 0028), its
    /// panel painted `canvas` — the color the backdrop was baked over (see
    /// `crate::background`). The values are the background mockup's
    /// "Hero" preset, as the user chose them: the panel is the canvas
    /// with no sheen — opaque on the solid material, at the palette's
    /// glass tint alpha on glass, so the window's frost still shows
    /// through it; the frosted surfaces blur what is behind
    /// them 30px under the canvas at 40% (the footer at 60%) and a cool
    /// silver edge; selection and hover are the text colour's washes
    /// (ADR 0035), which carry over from the palette beneath; and the
    /// secondary text steps up a shade to read over the picture. The
    /// light values are the dark ones turned over, not yet tuned.
    pub(crate) fn over_backdrop(&self, canvas: Hsla) -> Theme {
        let light = self.panel_solid.lightness > 0.5;
        let at = |base: Hsla, alpha: f32| Hsla { alpha, ..base };
        let wash = |alpha: f32| at(color(if light { 0x000000FF } else { 0xFFFFFFFF }), alpha);
        let edge = if light {
            color(0x00000017)
        } else {
            hsla(210. / 360., 0.18, 0.78, 0.09)
        };
        let frost = Frost {
            blur: px(30.),
            tint: at(canvas, 0.4),
            edge,
            top: wash(0.06),
            label: if light {
                color(0x3B3D44FF)
            } else {
                color(0xC9CACEFF)
            },
            pill_margin: (px(8.), px(4.), px(10.)),
            pill_padding_x: px(16.),
            pill_radius: px(16.),
        };
        Theme {
            text_muted: if light {
                color(0x4A4D55FF)
            } else {
                color(0xA9AAAFFF)
            },
            text_placeholder: if light {
                color(0x4A4D55FF)
            } else {
                color(0xA9AAAFFF)
            },
            panel_tint: at(canvas, self.panel_tint.alpha),
            panel_solid: canvas,
            panel_sheen: transparent_black(),
            footer_tint: at(canvas, 0.6),
            hairline_soft: edge,
            slot_background: frost.tint,
            slot_edge: edge,
            frost: Some(frost),
            ..self.clone()
        }
    }
}

/// The frosted surfaces over the launcher's background image (see
/// [`Theme::over_backdrop`]): Roboco's composer pill, a blur of what is
/// behind the surface inside the window under a thin tint of the canvas.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Frost {
    /// The blur behind a frosted surface (GPUI's `backdrop_blur`).
    pub(crate) blur: Pixels,
    /// A frosted surface's tint: the canvas at 40%.
    pub(crate) tint: Hsla,
    /// A frosted surface's 1px inset edge: a cool silver at 9%.
    pub(crate) edge: Hsla,
    /// A frosted surface's top inset line: white 6%.
    pub(crate) top: Hsla,
    /// The section labels' ink over the picture (#C9CACE), a step above
    /// the muted ink the rest of the secondary text takes.
    pub(crate) label: Hsla,
    /// The search field as a frosted pill inside the search header's 64px
    /// row, which keeps its height (so the compact window's does not
    /// change): its margins above, below and either side (8, 4, 10), its
    /// own padding either side (16, against the plain header's 20) and its
    /// radius (16).
    pub(crate) pill_margin: (Pixels, Pixels, Pixels),
    pub(crate) pill_padding_x: Pixels,
    pub(crate) pill_radius: Pixels,
}

impl Frost {
    /// A frosted surface's inset edges: the 1px ring and the top line.
    pub(crate) fn edges(&self) -> Vec<gpui::BoxShadow> {
        vec![
            gpui::BoxShadow::new(px(0.), px(0.), self.edge)
                .spread_radius(px(1.))
                .inset(),
            gpui::BoxShadow::new(px(0.), px(1.), self.top).inset(),
        ]
    }
}

impl Typography {
    fn shared() -> Typography {
        Typography {
            family: "Geist".into(),
            features: FontFeatures(Arc::new(vec![("kern".into(), 1)])),
            mono_family: "Geist Mono".into(),
            search_size: px(19.),
            row_title_size: px(14.),
            row_subtitle_size: px(13.),
            row_kind_size: px(12.5),
            footer_size: px(12.5),
            keycap_size: px(11.),
            keycap_compact_size: px(10.),
            alias_size: px(11.),
            mono_line_height: 1.3,
            section_size: px(12.),
            section_tracking: 0.01,
            line_height: 1.3,
            settings_text_size: px(13.),
            settings_caption_size: px(12.),
            results: ResultType::shared(),
            settings: SettingsType::shared(),
            action_size: px(13.),
            action_weight: FontWeight(450.),
            actions_header_size: px(12.),
            action_group_size: px(11.5),
            slot_title_size: px(12.5),
            slot_reason_size: px(11.5),
            medium: FontWeight::MEDIUM,
            regular: FontWeight::NORMAL,
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
            search_glyph_size: px(20.),
            search_text_inset: px(2.),
            screen_padding_y: px(12.),
            list_padding_top: px(4.),
            list_padding_bottom: px(10.),
            list_padding_x: px(10.),
            row_min_height: px(44.),
            row_radius: px(10.),
            row_padding_x: px(10.),
            row_gap: px(12.),
            row_list_gap: px(2.),
            row_kind_min_width: px(88.),
            results: ResultGeometry::shared(),
            alias_padding_y: px(2.),
            alias_padding_x: px(6.),
            alias_radius: px(5.),
            section_height: px(30.),
            section_padding_top: px(8.),
            section_padding_x: px(10.),
            section_gap: px(8.),
            tile: TileMetrics {
                size: px(28.),
                radius: px(7.),
                glyph: px(16.),
            },
            slot_tile: TileMetrics {
                size: px(30.),
                radius: px(8.),
                glyph: px(17.),
            },
            mini_tile: TileMetrics {
                size: px(18.),
                radius: px(5.),
                glyph: px(11.),
            },
            settings: SettingsGeometry {
                titlebar_height: px(48.),
                caption_width: px(46.),
                caption_glyph: px(16.),
                sidebar_width: px(232.),
                sidebar_padding_y: px(12.),
                sidebar_padding_x: px(10.),
                sidebar_gap: px(2.),
                search_height: px(34.),
                search_padding_x: px(10.),
                search_gap: px(8.),
                search_margin_bottom: px(8.),
                search_radius: px(8.),
                search_glyph: px(14.),
                item_height: px(36.),
                item_radius: px(8.),
                item_padding_x: px(10.),
                item_gap: px(10.),
                item_glyph: px(16.),
                content_padding_top: px(20.),
                content_padding_x: px(24.),
                content_padding_bottom: px(24.),
                content_max_width: px(680.),
                section_gap: px(24.),
                section_label_gap: px(8.),
                section_label_inset: px(4.),
                card_radius: px(10.),
                card_padding_x: px(14.),
                card_row_height: px(48.),
                card_row_padding_y: px(10.),
                choice_width: px(200.),
                wide_choice_width: px(360.),
            },
            controls: ControlGeometry {
                group_gap: px(18.),
                field_gap: px(8.),
                track_padding: px(3.),
                segment_gap: px(2.),
                track_radius: px(10.),
                segment_height: px(30.),
                segment_radius: px(7.),
                toggle_row_height: px(44.),
                toggle_width: px(40.),
                toggle_height: px(24.),
                toggle_knob: px(18.),
                toggle_inset: px(3.),
                focus_width: px(2.),
                disabled_opacity: 0.4,
                row_padding_y: px(10.),
                row_gap: px(16.),
                button_gap: px(6.),
                list_gap: px(2.),
                well_height: px(34.),
                inline_well_height: px(30.),
                well_padding_x: px(10.),
                well_radius: px(8.),
                well_gap: px(8.),
                well_glyph: px(14.),
                recorder_width: px(220.),
                recorder_height: px(36.),
                recorder_inset: px(4.),
                icon_button_size: px(28.),
                icon_button_radius: px(6.),
                icon_button_glyph: px(16.),
                list_radius: px(8.),
                chevron: px(14.),
                menu_padding_y: px(4.),
                mark_size: px(6.),
                frame_padding: px(4.),
                frame_radius: px(10.),
            },
            footer_height: px(50.),
            footer_padding_left: px(16.),
            footer_padding_right: px(8.),
            action_height: px(34.),
            action_radius: px(8.),
            action_padding_x: px(8.),
            action_gap: px(6.),
            footer_buttons_gap: px(4.),
            footer_divider_height: px(16.),
            footer_lead_gap: px(12.),
            footer_hint_gap: px(6.),
            footer_mark_size: px(18.),
            footer_mark_bleed: px(5.),
            footer_status_padding_y: px(12.),
            popover_outline_width: px(0.5),
            popover_drop_offset: px(28.),
            popover_drop_blur: px(70.),
            popover_drop_spread: px(-14.),
            actions: ActionsGeometry {
                width: px(320.),
                inset: px(10.),
                above_footer: px(8.),
                header_height: px(30.),
                header_padding_top: px(8.),
                header_padding_x: px(14.),
                header_gap: px(8.),
                list_padding: px(6.),
                list_gap: px(1.),
                row_height: px(36.),
                row_radius: px(8.),
                row_padding_x: px(8.),
                row_gap: px(10.),
                glyph_size: px(16.),
                group_height: px(26.),
                group_padding_x: px(8.),
                group_padding_bottom: px(4.),
                rule_margin_y: px(4.),
                rule_margin_x: px(6.),
                search_height: px(44.),
                search_padding_x: px(14.),
                search_gap: px(10.),
                search_glyph_size: px(15.),
                empty_padding_y: px(14.),
                empty_padding_x: px(10.),
            },
            pinned: PinnedGeometry {
                columns_gap: px(8.),
                strip_padding_top: px(2.),
                strip_padding_bottom: px(6.),
                slot_height: px(80.),
                slot_radius: px(12.),
                slot_padding_top: px(8.),
                slot_padding_x: px(8.),
                slot_padding_bottom: px(8.),
                slot_gap: px(7.),
                unavailable_gap: px(2.),
                keys_inset: px(8.),
                focus_width: px(2.),
                edge_width: px(1.),
            },
            keycap_height: px(20.),
            keycap_radius: px(5.),
            keycap_padding_x: px(5.),
            keycap_compact_height: px(17.),
            keycap_compact_padding_x: px(4.),
            key_gap: px(3.),
            popover_radius: px(14.),
        }
    }
}

/// The split view's tokens: the reference's clipboard board (#102) — its
/// 64px header with the back button, the search and the type dropdown;
/// the 360px list of 44px rows beside the
/// preview card; and the 52px footer. Rows, section labels, keycaps and
/// footer buttons are the launcher's own families and take their tokens.
/// The board's source tones and its code, color, link and image previews
/// have no tokens: Pane keeps text alone and guesses no kind of it (#100).
#[derive(Clone, Debug)]
pub(crate) struct SplitTokens {
    /// The header: its height and its left and right padding (14, 12),
    /// the gap between its parts (12).
    pub(crate) header_height: Pixels,
    pub(crate) header_padding_left: Pixels,
    pub(crate) header_padding_right: Pixels,
    pub(crate) header_gap: Pixels,
    /// The back button: 32 square, radius 8, a 16px glyph, white 6% under
    /// #C9CACE.
    pub(crate) back_size: Pixels,
    pub(crate) back_radius: Pixels,
    pub(crate) back_glyph: Pixels,
    pub(crate) back_fill: Hsla,
    pub(crate) back_text: Hsla,
    /// The detail pane's Information (#166): 12px, rows 30 high, the
    /// source's 14px icon 6px before its name, 10 above the section.
    pub(crate) info_size: Pixels,
    pub(crate) info_row_height: Pixels,
    pub(crate) info_icon: Pixels,
    pub(crate) info_gap: Pixels,
    pub(crate) info_margin_top: Pixels,
    /// The list: 360 wide with its 1px rule on the right, padded 2 above,
    /// 8 either side and 10 below.
    pub(crate) list_width: Pixels,
    /// The most of a narrower window the list takes (half), so the
    /// preview keeps room beside it.
    pub(crate) list_max_share: f32,
    pub(crate) list_padding_top: Pixels,
    pub(crate) list_padding_x: Pixels,
    pub(crate) list_padding_bottom: Pixels,
    /// A row's title (13.5px/500) and its time (Geist Mono 11.5).
    pub(crate) title_size: Pixels,
    pub(crate) time_size: Pixels,
    /// The note in place of rows: 40 above and below, 16 either side, 13px.
    pub(crate) empty_padding_y: Pixels,
    pub(crate) empty_padding_x: Pixels,
    pub(crate) empty_size: Pixels,
    /// The preview pane's padding (12), and its card: radius 12, black 24%
    /// with a white 7% inset ring.
    pub(crate) preview_padding: Pixels,
    pub(crate) preview_radius: Pixels,
    pub(crate) preview_fill: Hsla,
    pub(crate) preview_edge: Hsla,
    /// Plain text, previewed: padding 28 by 30, 20px at line height 1.5
    /// with -.005em of tracking.
    pub(crate) text_padding_y: Pixels,
    pub(crate) text_padding_x: Pixels,
    pub(crate) text_size: Pixels,
    pub(crate) text_line_height: f32,
    pub(crate) text_tracking: f32,
    /// The footer: 52 high, padded 16 left and 8 right, its two sides 16
    /// apart.
    pub(crate) footer_height: Pixels,
    pub(crate) footer_gap: Pixels,
}

impl SplitTokens {
    /// The derived light values: the white overlays become black ones and
    /// the reference's light inks dark, as the light palette derives its
    /// own; the geometry is the dark one's. A proposal, not reference
    /// truth.
    fn light() -> SplitTokens {
        SplitTokens {
            back_fill: color(0x0000000F),
            back_text: color(0x3B3D44FF),
            preview_fill: color(0x0000000A),
            preview_edge: color(0x00000012),
            ..SplitTokens::dark()
        }
    }

    /// The reference's dark values, as authored: its geometry and its
    /// colors.
    fn dark() -> SplitTokens {
        SplitTokens {
            header_height: px(64.),
            header_padding_left: px(14.),
            header_padding_right: px(12.),
            header_gap: px(12.),
            back_size: px(32.),
            back_radius: px(8.),
            back_glyph: px(16.),
            back_fill: color(0xFFFFFF0F),
            back_text: color(0xC9CACEFF),
            info_size: px(12.),
            info_row_height: px(30.),
            info_icon: px(14.),
            info_gap: px(6.),
            info_margin_top: px(10.),
            list_width: px(360.),
            list_max_share: 0.5,
            list_padding_top: px(2.),
            list_padding_x: px(8.),
            list_padding_bottom: px(10.),
            title_size: px(13.5),
            time_size: px(11.5),
            empty_padding_y: px(40.),
            empty_padding_x: px(16.),
            empty_size: px(13.),
            preview_padding: px(12.),
            preview_radius: px(12.),
            preview_fill: color(0x0000003D),
            preview_edge: color(0xFFFFFF12),
            text_padding_y: px(28.),
            text_padding_x: px(30.),
            text_size: px(20.),
            text_line_height: 1.5,
            text_tracking: -0.005,
            footer_height: px(52.),
            footer_gap: px(16.),
        }
    }
}
