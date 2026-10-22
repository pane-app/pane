//! What a window painted, read on the scene its last frame drew: the
//! fills the window tests check a component's washes by.

use gpui::VisualTestContext;

/// Whether the window `cx` drives painted a quad of the color `hex`
/// (`0xRRGGBBAA`) over exactly `bounds` (logical px, within half a
/// pixel): the fill a component's wash leaves on the scene the window
/// drew.
pub fn paints_fill_at(
    cx: &mut VisualTestContext,
    bounds: gpui::Bounds<gpui::Pixels>,
    hex: u32,
) -> bool {
    let fill = gpui::solid_background(gpui::rgb_to_hsla(gpui::rgba(hex)));
    paints_background_at(cx, bounds, fill)
}

/// Whether the window `cx` drives painted a quad of exactly the
/// background `fill` over exactly `bounds` (logical px, within half a
/// pixel): for the fills a component's wash derives from a theme colour,
/// whose alpha no hex spells exactly.
pub fn paints_background_at(
    cx: &mut VisualTestContext,
    bounds: gpui::Bounds<gpui::Pixels>,
    fill: gpui::Background,
) -> bool {
    cx.update(|window, _| {
        let scale = window.scale_factor();
        let near = |scaled: gpui::ScaledPixels, logical: gpui::Pixels| {
            (scaled.0 / scale - f32::from(logical)).abs() <= 0.5
        };
        window.painted_quads().iter().any(|quad| {
            quad.background == fill
                && near(quad.bounds.origin.x, bounds.origin.x)
                && near(quad.bounds.origin.y, bounds.origin.y)
                && near(quad.bounds.size.width, bounds.size.width)
                && near(quad.bounds.size.height, bounds.size.height)
        })
    })
}
