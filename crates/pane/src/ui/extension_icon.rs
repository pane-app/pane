//! Drawing the icons extensions give (ADR 0036, #139): a row's, an
//! accessory's, a package's and a command's — bare, without a tile behind
//! them (ADR 0035's "bare application icons"), while Pane's own rows keep
//! their tiles ([`RowIcon::Pane`]).
//!
//! Presentation only: the caller resolves what to draw ([`DrawnIcon`]) —
//! the file for the theme in force, the colour after contrast correction —
//! from the launcher's icon (see `crate::features::icons`), and this draws
//! it:
//!
//! - a built-in glyph, as a mask in its colour;
//! - an image file in its own colours, clipped to its mask, or, tinted, as
//!   a mask in its colour (an SVG through the SVG renderer, a PNG recoloured
//!   once and kept);
//! - image data (a `data:` URL's), alike;
//! - a first-letter tile, Pane's command tile with the letter in it.
//!
//! An image that cannot be loaded draws its fallback in its place. An icon
//! with a label (its tooltip) is an image to assistive technology, named
//! by the label, and shows the label on hover; one without is decoration,
//! hidden from assistive technology.
//!
//! The tests read what was drawn from debug selectors: the icon's own
//! `icon-<scope>`, and around what it draws `icon-<scope>-<what>` (such as
//! `glyph-star` or `image-logo@dark.png`), `icon-<scope>-color-<rrggbbaa>`
//! where a colour applies, and `icon-<scope>-mask-circle` or
//! `icon-<scope>-mask-rounded`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use gpui::{
    AnyElement, App, Div, ElementId, Hsla, Image, ImageCacheError, ObjectFit, Pixels, RenderImage,
    Role, SharedString, Stateful, Window, div, hsla_to_rgba, img, prelude::*, svg,
};

use crate::ui::icon::{Glyph, IconTone, TileSize, tile_at};
use crate::ui::theme::Theme;
use crate::ui::tooltip::{TooltipLook, text_tooltip};

/// What a row, a slot, a card or a header shows as its icon.
#[derive(Clone, Debug)]
pub(crate) enum RowIcon {
    /// One of Pane's own, on its tile: Pane's built-in commands keep their
    /// tiles (ADR 0035).
    Pane(IconTone, Glyph),
    /// An extension's or an item's icon, drawn bare.
    Drawn(DrawnIcon),
}

impl From<(IconTone, Glyph)> for RowIcon {
    fn from((tone, glyph): (IconTone, Glyph)) -> RowIcon {
        RowIcon::Pane(tone, glyph)
    }
}

/// An icon resolved for drawing: what to draw, in which colour, clipped
/// how, and what stands in for it when it cannot be drawn.
#[derive(Clone, Debug)]
pub(crate) struct DrawnIcon {
    pub(crate) image: IconImage,
    /// The colour of a glyph, a tinted image or a letter.
    pub(crate) color: Hsla,
    pub(crate) mask: Option<IconMask>,
    /// Drawn in its place when the image cannot be loaded.
    pub(crate) fallback: Option<Box<DrawnIcon>>,
    /// The icon's tooltip: its name to assistive technology and its hover
    /// text. `None` makes it decoration.
    pub(crate) label: Option<SharedString>,
    /// What it draws, for the debug selectors: `glyph-star`,
    /// `image-logo@dark.png`, `tinted-logo.svg`, `data`, `letter-I`.
    pub(crate) what: SharedString,
}

/// What a [`DrawnIcon`] draws.
#[derive(Clone, Debug)]
pub(crate) enum IconImage {
    /// A glyph's SVG markup, drawn as a mask in the icon's colour.
    Glyph(Arc<[u8]>),
    /// An image file (a packaged PNG or SVG; a downloaded web image of any
    /// kind GPUI decodes, or an extracted system icon, #142), in its own
    /// colours, or, `tinted`, as a mask in the icon's colour.
    File { path: PathBuf, tinted: bool },
    /// Image data, in its own colours, or, `tinted` (SVG only), as a mask
    /// in the icon's colour.
    Data { image: Arc<Image>, tinted: bool },
    /// A first-letter tile on `background`, the letter in the icon's
    /// colour.
    Letter { letter: char, background: Hsla },
}

/// The shape an image is clipped to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum IconMask {
    Circle,
    Rounded,
}

/// The sizes an icon is drawn at: its box, a glyph inside it, and the
/// corner radius of a rounded mask or a letter tile.
#[derive(Clone, Copy, Debug)]
pub(crate) struct IconSize {
    pub(crate) size: Pixels,
    pub(crate) glyph: Pixels,
    pub(crate) radius: Pixels,
}

impl IconSize {
    /// The sizes of a tile of `size`, from the theme.
    pub(crate) fn of(size: TileSize, theme: &Theme) -> IconSize {
        let metrics = size.metrics(theme);
        IconSize {
            size: metrics.size,
            glyph: metrics.glyph,
            radius: metrics.radius,
        }
    }

    /// An accessory's icon: a glyph and an image alike at `size`.
    pub(crate) fn small(size: Pixels) -> IconSize {
        IconSize {
            size,
            glyph: size,
            radius: size * 0.25,
        }
    }
}

/// `icon` at the tile size `size`: Pane's tile, or the extension's icon
/// bare in the tile's box. Its id is `id`; `scope` names it in the debug
/// selectors.
pub(crate) fn row_icon_at(
    icon: &RowIcon,
    size: TileSize,
    id: impl Into<ElementId>,
    scope: &str,
    theme: &Theme,
) -> Div {
    match icon {
        RowIcon::Pane(tone, glyph) => tile_at(size, *tone, *glyph, theme),
        RowIcon::Drawn(drawn) => {
            div()
                .flex_none()
                .child(draw(drawn, IconSize::of(size, theme), id, scope, theme))
        }
    }
}

/// `icon` drawn bare in a box of `size` (see the module docs), with the
/// id `id`, its accessibility and its tooltip; `scope` names it in the
/// debug selectors.
pub(crate) fn draw(
    icon: &DrawnIcon,
    size: IconSize,
    id: impl Into<ElementId>,
    scope: &str,
    theme: &Theme,
) -> Stateful<Div> {
    let scope = scope.to_owned();
    let selector = format!("icon-{scope}");
    let element = div()
        .id(id)
        .debug_selector(move || selector)
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .size(size.size)
        .child(content(icon, size, &scope));
    match &icon.label {
        Some(label) => element
            .role(Role::Image)
            .aria_label(label.clone())
            .tooltip(text_tooltip(label.clone(), TooltipLook::of(theme))),
        None => element.aria_hidden(),
    }
}

/// What `icon` draws inside its box, wrapped in the debug selectors of its
/// facets.
fn content(icon: &DrawnIcon, size: IconSize, scope: &str) -> AnyElement {
    let drawn = match &icon.image {
        IconImage::Glyph(markup) => svg()
            .data(markup)
            .size(size.glyph)
            .text_color(icon.color)
            .into_any_element(),
        IconImage::File { path, tinted: true } if is_svg(path) => svg()
            .external_path(path.to_string_lossy().into_owned())
            .size(size.size)
            .text_color(icon.color)
            .into_any_element(),
        IconImage::File { path, tinted: true } => {
            let path = path.clone();
            let color = icon.color;
            masked(
                img(move |_: &mut Window, _: &mut App| Some(tinted_png(&path, color))),
                icon,
                size,
                scope,
            )
        }
        IconImage::File { path, .. } => masked(img(path.clone()), icon, size, scope),
        IconImage::Data {
            image,
            tinted: true,
        } => svg()
            .data(&image.bytes)
            .size(size.size)
            .text_color(icon.color)
            .into_any_element(),
        IconImage::Data { image, .. } => masked(img(image.clone()), icon, size, scope),
        IconImage::Letter { letter, background } => div()
            .size(size.size)
            .flex()
            .items_center()
            .justify_center()
            .rounded(size.radius)
            .bg(*background)
            .text_size(size.size * 0.5)
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .text_color(icon.color)
            .child(letter.to_string())
            .into_any_element(),
    };
    let what = format!("icon-{scope}-{}", icon.what);
    let mut element = div()
        .debug_selector(move || what)
        .flex()
        .items_center()
        .justify_center()
        .child(drawn);
    if colored(icon) {
        let color = format!("icon-{scope}-color-{}", hex(icon.color));
        element = div()
            .debug_selector(move || color)
            .flex()
            .items_center()
            .justify_center()
            .child(element);
    }
    if let Some(mask) = icon.mask {
        let name = match mask {
            IconMask::Circle => "circle",
            IconMask::Rounded => "rounded",
        };
        let mask = format!("icon-{scope}-mask-{name}");
        element = div()
            .debug_selector(move || mask)
            .flex()
            .items_center()
            .justify_center()
            .child(element);
    }
    element.into_any_element()
}

/// Whether `icon` is drawn in its colour: a glyph, a letter or a tinted
/// image; an image in its own colours is not.
fn colored(icon: &DrawnIcon) -> bool {
    match &icon.image {
        IconImage::Glyph(_) | IconImage::Letter { .. } => true,
        IconImage::File { tinted, .. } | IconImage::Data { tinted, .. } => *tinted,
    }
}

/// `image` at the box's size, fit inside it, clipped to `icon`'s mask,
/// with its fallback drawn in its place if it cannot be loaded.
fn masked(image: gpui::Img, icon: &DrawnIcon, size: IconSize, scope: &str) -> AnyElement {
    let image = image.size(size.size).object_fit(ObjectFit::Contain);
    let image = match icon.mask {
        Some(IconMask::Circle) => image.rounded_full(),
        Some(IconMask::Rounded) => image.rounded(size.radius),
        None => image,
    };
    match icon.fallback.as_deref() {
        Some(fallback) => {
            let fallback = fallback.clone();
            let scope = format!("{scope}-fallback");
            image
                .with_fallback(move || content(&fallback, size, &scope))
                .into_any_element()
        }
        None => image.into_any_element(),
    }
}

fn is_svg(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("svg"))
}

/// `color` as `rrggbbaa`, as the debug selectors name it.
pub(crate) fn hex(color: Hsla) -> String {
    let rgba = hsla_to_rgba(color);
    let byte = |channel: f32| (channel.clamp(0., 1.) * 255.).round() as u8;
    format!(
        "{:02x}{:02x}{:02x}{:02x}",
        byte(rgba.red),
        byte(rgba.green),
        byte(rgba.blue),
        byte(rgba.alpha)
    )
}

/// The most recoloured PNGs kept: a list rarely shows more.
const MAX_TINTED: usize = 128;

/// The PNG at `path` with every pixel `color`, keeping its alpha: a tinted
/// packaged image, drawn as a mask is. Decoded once per file and colour.
fn tinted_png(path: &Path, color: Hsla) -> Result<Arc<RenderImage>, ImageCacheError> {
    type Tinted = HashMap<(PathBuf, String), Result<Arc<RenderImage>, ImageCacheError>>;
    static TINTED: OnceLock<Mutex<Tinted>> = OnceLock::new();
    let key = (path.to_path_buf(), hex(color));
    let mut tinted = TINTED
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(found) = tinted.get(&key) {
        return found.clone();
    }
    let made = recolor(path, color).map_err(|why| ImageCacheError::Asset(why.into()));
    if tinted.len() >= MAX_TINTED {
        tinted.clear();
    }
    tinted.insert(key, made.clone());
    made
}

/// Decodes the PNG at `path` and paints it `color`, keeping each pixel's
/// alpha, as the BGRA frame GPUI draws.
fn recolor(path: &Path, color: Hsla) -> Result<Arc<RenderImage>, String> {
    let picture = image::ImageReader::open(path)
        .and_then(|reader| reader.with_guessed_format())
        .map_err(|error| error.to_string())?
        .decode()
        .map_err(|error| error.to_string())?;
    let mut frame = picture.into_rgba8();
    let rgba = hsla_to_rgba(color);
    let byte = |channel: f32| (channel.clamp(0., 1.) * 255.).round() as u8;
    let (red, green, blue) = (byte(rgba.red), byte(rgba.green), byte(rgba.blue));
    let alpha = rgba.alpha.clamp(0., 1.);
    for pixel in frame.pixels_mut() {
        let [_, _, _, a] = pixel.0;
        // BGRA, straight alpha.
        pixel.0 = [blue, green, red, (f32::from(a) * alpha).round() as u8];
    }
    Ok(Arc::new(RenderImage::new([image::Frame::new(frame)])))
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{rgb_to_hsla, rgba};

    #[test]
    fn colours_are_named_as_hex_in_the_selectors() {
        assert_eq!(hex(rgb_to_hsla(rgba(0xFF6363FF))), "ff6363ff");
        assert_eq!(hex(rgb_to_hsla(rgba(0x00000080))), "00000080");
    }
}
