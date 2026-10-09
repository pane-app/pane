//! Contrast correction (ADR 0036, #139): a colour an extension chooses — a
//! tint, an accessory's colour, a tag's — is drawn only once it reads
//! against what it is drawn on, so a raw value cannot make an icon or
//! text unreadable. A colour that falls short is moved toward white on a
//! dark surface, or toward black on a light one, keeping its hue, until
//! its contrast ratio (WCAG's) reaches what its role needs.

use gpui::{Hsla, Rgba, hsla_to_rgba, rgb_to_hsla};

/// The contrast an icon, a non-text graphic, needs (WCAG 1.4.11).
pub(crate) const GRAPHIC: f32 = 3.0;

/// The contrast text needs (WCAG 1.4.3).
pub(crate) const TEXT: f32 = 4.5;

/// The relative luminance of `color`, opaque (WCAG's).
pub(crate) fn luminance(color: Hsla) -> f32 {
    let rgba = hsla_to_rgba(color);
    let linear = |channel: f32| {
        if channel <= 0.039_28 {
            channel / 12.92
        } else {
            ((channel + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * linear(rgba.red) + 0.7152 * linear(rgba.green) + 0.0722 * linear(rgba.blue)
}

/// The contrast ratio of `a` against `b`, from 1 to 21.
pub(crate) fn ratio(a: Hsla, b: Hsla) -> f32 {
    let (a, b) = (luminance(a), luminance(b));
    let (light, dark) = if a > b { (a, b) } else { (b, a) };
    (light + 0.05) / (dark + 0.05)
}

/// `fg` drawn over `surface`, flattened to opaque: the colour a
/// translucent colour leaves on the surface beneath it, as GPUI composites
/// straight alpha in sRGB. The launcher's text levels (ADR 0035) are judged
/// through this — a level's alpha form against the surface it is drawn on.
pub(crate) fn over(fg: Hsla, surface: Hsla) -> Hsla {
    let fg = hsla_to_rgba(fg);
    let surface = hsla_to_rgba(surface);
    let alpha = fg.alpha;
    let mix = |fg: f32, surface: f32| alpha * fg + (1. - alpha) * surface;
    rgb_to_hsla(Rgba::new(
        mix(fg.red, surface.red),
        mix(fg.green, surface.green),
        mix(fg.blue, surface.blue),
        1.,
    ))
}

/// `color`, drawn on `surface`, moved toward white (on a dark surface) or
/// black (on a light one) until its contrast ratio is at least `needed`,
/// keeping its hue and alpha. A colour that already reads is unchanged.
pub(crate) fn corrected(color: Hsla, surface: Hsla, needed: f32) -> Hsla {
    let opaque = |mut color: Hsla| {
        color.alpha = 1.;
        color
    };
    if ratio(opaque(color), surface) >= needed {
        return color;
    }
    let lighten = luminance(surface) < 0.5;
    let mut moved = color;
    for _ in 0..50 {
        let lightness = if lighten {
            (moved.lightness + 0.02).min(1.)
        } else {
            (moved.lightness - 0.02).max(0.)
        };
        moved.lightness = lightness;
        if ratio(opaque(moved), surface) >= needed || lightness <= 0. || lightness >= 1. {
            break;
        }
    }
    moved
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{rgb_to_hsla, rgba};

    fn color(hex: u32) -> Hsla {
        rgb_to_hsla(rgba(hex))
    }

    #[test]
    fn a_colour_that_reads_is_kept_and_one_that_does_not_is_corrected() {
        let dark = color(0x1C1C1EFF);
        let light = color(0xF5F5F7FF);
        // White on the dark panel reads.
        assert_eq!(corrected(color(0xFFFFFFFF), dark, TEXT), color(0xFFFFFFFF));
        // Near-black on the dark panel does not: it is lightened until it
        // does, and keeps its alpha.
        let fixed = corrected(color(0x202022C0), dark, TEXT);
        assert!(ratio(Hsla { alpha: 1., ..fixed }, dark) >= TEXT);
        assert!((fixed.alpha - 0xC0 as f32 / 255.).abs() < 0.01);
        // A pale yellow on the light panel is darkened.
        let fixed = corrected(color(0xFFF3A0FF), light, GRAPHIC);
        assert!(ratio(fixed, light) >= GRAPHIC);
        assert!(fixed.lightness < color(0xFFF3A0FF).lightness);
        assert!((ratio(color(0x000000FF), color(0xFFFFFFFF)) - 21.).abs() < 0.01);
    }

    #[test]
    fn a_translucent_colour_composites_over_the_surface_beneath_it() {
        // White at half alpha over black paints the grey between them; an
        // opaque colour leaves the surface nothing to show through.
        let half = Hsla {
            alpha: 0.5,
            ..color(0xFFFFFFFF)
        };
        let grey = hsla_to_rgba(over(half, color(0x000000FF)));
        for channel in [grey.red, grey.green, grey.blue] {
            assert!((channel - 0.5).abs() < 1e-3);
        }
        assert_eq!(grey.alpha, 1.);
        assert_eq!(over(color(0xFFFFFFFF), color(0x000000FF)), color(0xFFFFFFFF));
    }
}
