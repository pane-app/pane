//! Extensions' icons and accessories in Pane's windows (#139): the
//! launcher's icons ([`pane_core::Icon`]) and accessories resolved for the
//! shared drawing ([`crate::ui::extension_icon`], [`crate::ui::result_row`])
//! in the theme in force.
//!
//! - A packaged image's file is the light or the dark one, as the theme
//!   is.
//! - A tint, an accessory's colour and a tag's are a theme tone (the text
//!   levels, the accent, or a named colour in the theme's own shade) or the
//!   author's raw colour, corrected for contrast against the panel: an icon
//!   to 3:1, text to 4.5:1 (see [`crate::ui::contrast`]).
//! - A built-in icon's markup comes from the whole reicon set Pane vendors
//!   (`pane_core::icons`), decoded once per name.
//! - An installed command's or package's icon replaces the generic glyph
//!   in root search, quick slots, the Settings Extensions page and the
//!   Shortcuts page; Pane's own rows keep their tiles ([`row_icon_of`]).
//! - While an open command's list shows a date, the window draws again
//!   every [`DATE_REFRESH`], so "2h" becomes "3h" without anything else
//!   happening ([`LauncherWindow::keep_dates_current`]).

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use gpui::{Context, Hsla, Image, ImageFormat, SharedString, rgb_to_hsla, rgba};
use pane_core::{
    AccessoryKind, Color, Icon, IconSource, Launcher, Mask, Presentation, ShownAccessory, Tint,
    Tone,
};

use crate::app::{LauncherWindow, row_icon};
use crate::ui::contrast;
use crate::ui::extension_icon::{DrawnIcon, IconImage, IconMask, RowIcon};
use crate::ui::result_row::AccessoryLook;
use crate::ui::theme::Theme;

/// How often the window draws a list showing a date again, to keep it
/// current.
pub(crate) const DATE_REFRESH: Duration = Duration::from_secs(30);

/// Whether `theme` is a dark one: the panel it draws is.
pub(crate) fn is_dark(theme: &Theme) -> bool {
    theme.panel_solid.lightness <= 0.5
}

/// What the installed command or package `id` shows as its icon: its own
/// (or its package's, or the package's first-letter tile), drawn bare; or,
/// for anything else (Pane's own rows), Pane's tile for it.
pub(crate) fn row_icon_of(launcher: &Launcher, id: &str, theme: &Theme) -> RowIcon {
    match launcher.icon_of(id) {
        Some(icon) => RowIcon::Drawn(drawn(&icon, theme)),
        None => row_icon(id).into(),
    }
}

/// `icon` as the shared drawing draws it in `theme`.
pub(crate) fn drawn(icon: &Icon, theme: &Theme) -> DrawnIcon {
    let dark = is_dark(theme);
    let tint = icon.tint.map(|tint| color(tint, theme, contrast::GRAPHIC));
    let fallback = icon
        .fallback
        .as_deref()
        .map(|fallback| Box::new(drawn(fallback, theme)));
    let label = icon
        .tooltip
        .clone()
        .filter(|tooltip| !tooltip.trim().is_empty())
        .map(SharedString::from);
    let (image, what, color) = match &icon.source {
        IconSource::Builtin { name, filled } => (
            IconImage::Glyph(glyph(name, *filled)),
            if *filled {
                format!("glyph-{name}-filled")
            } else {
                format!("glyph-{name}")
            },
            tint.unwrap_or(theme.text_body),
        ),
        IconSource::Image {
            light,
            dark: on_dark,
        } => {
            let path = if dark { on_dark } else { light };
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            let tinted = tint.is_some();
            (
                IconImage::File {
                    path: path.clone(),
                    tinted,
                },
                format!("{}-{name}", if tinted { "tinted" } else { "image" }),
                tint.unwrap_or(theme.text_body),
            )
        }
        IconSource::Url(url) => match data_image(url) {
            Some(image) => {
                let tinted = tint.is_some() && image.format == ImageFormat::Svg;
                (
                    IconImage::Data { image, tinted },
                    "data".to_owned(),
                    tint.unwrap_or(theme.text_body),
                )
            }
            // Data Pane cannot read: its fallback, else nothing.
            None => match &fallback {
                Some(fallback) => {
                    return DrawnIcon {
                        label: label.or(fallback.label.clone()),
                        ..(**fallback).clone()
                    };
                }
                None => (
                    IconImage::Glyph(Arc::from(EMPTY_SVG.as_bytes())),
                    "data".to_owned(),
                    theme.text_body,
                ),
            },
        },
        IconSource::Letter(letter) => (
            IconImage::Letter {
                letter: *letter,
                background: theme.tile_background,
            },
            format!("letter-{letter}"),
            theme.tile_foreground,
        ),
    };
    DrawnIcon {
        image,
        color,
        mask: icon.mask.map(|mask| match mask {
            Mask::Circle => IconMask::Circle,
            Mask::RoundedRectangle => IconMask::Rounded,
        }),
        fallback,
        label,
        what: what.into(),
    }
}

/// An SVG that draws nothing.
const EMPTY_SVG: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"/>"#;

/// The built-in icon `name`'s markup in its Outline weight or, `filled`,
/// its Filled one, decoded once and kept.
fn glyph(name: &str, filled: bool) -> Arc<[u8]> {
    static GLYPHS: OnceLock<Mutex<HashMap<(String, bool), Arc<[u8]>>>> = OnceLock::new();
    let mut glyphs = GLYPHS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    glyphs
        .entry((name.to_owned(), filled))
        .or_insert_with(|| {
            let markup =
                pane_core::icons::builtin_svg(name, filled).unwrap_or_else(|| EMPTY_SVG.to_owned());
            Arc::from(markup.into_bytes())
        })
        .clone()
}

/// The most `data:` images kept decoded.
const MAX_DATA_IMAGES: usize = 256;

/// The image a `data:` URL holds, when it is a PNG or an SVG; decoded once
/// and kept.
fn data_image(url: &str) -> Option<Arc<Image>> {
    type Images = HashMap<String, Option<Arc<Image>>>;
    static IMAGES: OnceLock<Mutex<Images>> = OnceLock::new();
    let mut images = IMAGES
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(image) = images.get(url) {
        return image.clone();
    }
    let image = pane_core::icons::data_url(url).and_then(|(media, bytes)| {
        let format = match media.as_str() {
            "image/svg+xml" => ImageFormat::Svg,
            "image/png" => ImageFormat::Png,
            _ => return None,
        };
        Some(Arc::new(Image::from_bytes(format, bytes)))
    });
    if images.len() >= MAX_DATA_IMAGES {
        images.clear();
    }
    images.insert(url.to_owned(), image.clone());
    image
}

/// The colour `tint` gives in `theme`, corrected to `needed` contrast
/// against the panel.
pub(crate) fn color(tint: Tint, theme: &Theme, needed: f32) -> Hsla {
    let raw = match tint.for_theme(is_dark(theme)) {
        Color::Tone(tone) => tone_color(tone, theme),
        Color::Rgba(hex) => rgb_to_hsla(rgba(hex)),
    };
    contrast::corrected(raw, theme.panel_solid, needed)
}

/// What `tone` is in `theme`: the theme's own text levels and accent, or a
/// named colour in a shade for the theme's panel.
pub(crate) fn tone_color(tone: Tone, theme: &Theme) -> Hsla {
    let dark = is_dark(theme);
    let shade =
        |on_dark: u32, on_light: u32| rgb_to_hsla(rgba(if dark { on_dark } else { on_light }));
    match tone {
        Tone::Primary => theme.text_title,
        Tone::Secondary => theme.text_muted,
        Tone::Accent => theme.accent,
        Tone::Red => shade(0xFF6B6BFF, 0xD6336CFF),
        Tone::Orange => shade(0xFFA94DFF, 0xE8590CFF),
        Tone::Yellow => shade(0xFFD43BFF, 0xB08800FF),
        Tone::Green => shade(0x51CF66FF, 0x2B8A3EFF),
        Tone::Blue => shade(0x4DABF7FF, 0x1971C2FF),
        Tone::Purple => shade(0xB197FCFF, 0x6741D9FF),
        Tone::Magenta => shade(0xF783ACFF, 0xC2255CFF),
    }
}

/// `accessory` as the shared row draws it in `theme`: text in the muted
/// ink, a tag in the body ink, or in the colour it names, corrected for
/// contrast as text.
pub(crate) fn accessory_look(accessory: &ShownAccessory, theme: &Theme) -> AccessoryLook {
    let tag = accessory.kind == AccessoryKind::Tag;
    let color = accessory
        .color
        .map(|tint| color(tint, theme, contrast::TEXT))
        .unwrap_or(if tag {
            theme.text_body
        } else {
            theme.text_muted
        });
    AccessoryLook {
        text: accessory.text.clone().into(),
        tag,
        color,
        icon: accessory.icon.as_ref().map(|icon| drawn(icon, theme)),
        tooltip: accessory.tooltip.clone().map(SharedString::from),
    }
}

/// Whether `presentation` shows a date anywhere.
fn shows_a_date(presentation: &Presentation) -> bool {
    presentation.rows.iter().any(|row| {
        row.accessories
            .iter()
            .any(|accessory| accessory.kind == AccessoryKind::Date)
    })
}

impl LauncherWindow {
    /// Keeps the dates of the rows on screen current: while
    /// `presentation` shows one, the window draws again every
    /// [`DATE_REFRESH`]; otherwise it does not.
    pub(crate) fn keep_dates_current(
        &mut self,
        presentation: &Presentation,
        cx: &mut Context<Self>,
    ) {
        if !shows_a_date(presentation) {
            self.dates = None;
            return;
        }
        if self.dates.is_some() {
            return;
        }
        self.dates = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(DATE_REFRESH).await;
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    break;
                }
            }
        }));
    }
}
