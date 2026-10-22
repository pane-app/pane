//! The UI component set's public tokens (#237, ADR 0036): the mapping
//! layer a designed view's tree resolves its tokens and raw values
//! through, onto Pane's private theme — so the theme can change (as ADR
//! 0035 changes it) without renaming a token, and a token follows the
//! user's appearance and the background image as Pane's own screens do,
//! because the theme it resolves onto already has. The token *names* are
//! pane-core's (`pane_core::tokens`); this layer is the values.
//!
//! The values are the theme's own, chosen where the reference fixes them:
//! the space tokens the reference's spacing rhythm (its 4-pixel steps),
//! the radius and icon-size tokens the geometry its families use. Pane's
//! own screens read the same theme directly; that the designed tree
//! resolves onto the same values is what keeps the design one system.
//!
//! Raw values resolve here too: colours from the icon model's grammar
//! (a tone, a `0xRRGGBBAA`, a light and dark pair), text and icon colours
//! corrected for contrast against the surface they are drawn on at
//! [`contrast::DESIGNED`] unless the author turned correction off for
//! that colour, backgrounds drawn as they are (see `features::icons` for
//! the tone palette itself).

use gpui::{Hsla, Pixels, rgb_to_hsla, rgba};

use pane_core::{IconSize, Paint, Radius, Space, TextLevel, TextStyle};

use crate::ui::contrast;
use crate::ui::theme::Theme;

/// The appearance `theme` draws: whether it is a dark one.
pub(crate) fn is_dark(theme: &Theme) -> bool {
    theme.panel_solid.lightness <= 0.5
}

/// A space token, resolved onto the reference's spacing rhythm: the named
/// distances a tree's gaps, paddings and lengths use.
pub(crate) fn space(token: Space) -> Pixels {
    match token {
        Space::Xs => gpui::px(4.),
        Space::S => gpui::px(8.),
        Space::M => gpui::px(12.),
        Space::L => gpui::px(16.),
        Space::Xl => gpui::px(24.),
        Space::Xxl => gpui::px(32.),
    }
}

/// A radius token, resolved onto the geometry Pane's own rounded families
/// use. `Full` is a pill or a disc, as `rounded_full` draws.
pub(crate) fn radius(token: Radius) -> Pixels {
    match token {
        Radius::S => gpui::px(6.),
        Radius::M => gpui::px(10.),
        Radius::L => gpui::px(14.),
        Radius::Full => gpui::px(9999.),
    }
}

/// An icon-size token, resolved onto the sizes Pane's own icons are drawn
/// at.
pub(crate) fn icon_size(token: IconSize) -> Pixels {
    match token {
        IconSize::S => gpui::px(16.),
        IconSize::M => gpui::px(20.),
        IconSize::L => gpui::px(24.),
        IconSize::Xl => gpui::px(32.),
    }
}

/// A text style's type: its size, weight and family, resolved onto the
/// theme's typography.
pub(crate) fn text_style(
    style: Option<TextStyle>,
    theme: &Theme,
) -> (Pixels, gpui::FontWeight, gpui::SharedString) {
    let typography = &theme.typography;
    match style {
        None | Some(TextStyle::Body) => (
            typography.row_subtitle_size,
            typography.regular,
            typography.family.clone(),
        ),
        Some(TextStyle::Heading) => (gpui::px(16.), typography.medium, typography.family.clone()),
        Some(TextStyle::Title) => (
            typography.row_title_size,
            typography.medium,
            typography.family.clone(),
        ),
        Some(TextStyle::Caption) => (
            typography.row_kind_size,
            typography.regular,
            typography.family.clone(),
        ),
        Some(TextStyle::Mono) => (
            typography.row_subtitle_size,
            typography.regular,
            typography.mono_family.clone(),
        ),
        Some(TextStyle::SmallMono) => (
            typography.keycap_size,
            typography.regular,
            typography.mono_family.clone(),
        ),
    }
}

/// A text level, resolved onto the theme's text colours (ADR 0035's
/// colour-through-alpha levels): the primary is a row title's, the
/// secondary a subtitle's, the tertiary a section label's, the quaternary
/// a placeholder's.
pub(crate) fn text_level(level: Option<TextLevel>, theme: &Theme) -> Hsla {
    match level {
        None | Some(TextLevel::Primary) => theme.text_title,
        Some(TextLevel::Secondary) => theme.text_body,
        Some(TextLevel::Tertiary) => theme.text_muted,
        Some(TextLevel::Quaternary) => theme.text_placeholder,
    }
}

/// One colour the tree names, resolved for the theme in force and left
/// uncorrected: a tone as the theme draws it, a raw colour as it is. See
/// [`features::icons::tone_color`] for the tone palette.
pub(crate) fn paint_color(paint: &Paint, theme: &Theme) -> Hsla {
    match paint.tint.for_theme(is_dark(theme)) {
        pane_core::Color::Tone(tone) => crate::features::icons::tone_color(tone, theme),
        pane_core::Color::Rgba(hex) => rgb_to_hsla(rgba(hex)),
    }
}

/// A colour the tree names for text or an icon, corrected against the
/// `surface` it is drawn on at the designed tree's ratio (2.5) unless the
/// author turned correction off for that colour.
pub(crate) fn foreground(paint: &Paint, surface: Hsla, theme: &Theme) -> Hsla {
    let color = paint_color(paint, theme);
    if paint.exact {
        color
    } else {
        contrast::corrected(color, surface, contrast::DESIGNED)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_resolve_onto_the_reference_rhythm() {
        assert_eq!(space(Space::Xs), gpui::px(4.));
        assert_eq!(space(Space::Xxl), gpui::px(32.));
        assert_eq!(radius(Radius::S), gpui::px(6.));
        assert_eq!(radius(Radius::M), gpui::px(10.));
        assert_eq!(radius(Radius::L), gpui::px(14.));
        assert_eq!(icon_size(IconSize::S), gpui::px(16.));
        assert_eq!(icon_size(IconSize::Xl), gpui::px(32.));
    }
}
