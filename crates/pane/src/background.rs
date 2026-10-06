//! The launcher's background image (ADR 0028): Pane's own copy of the
//! picture the user chose, and the backdrop baked from it that the
//! launcher draws behind its content.
//!
//! [`import`] keeps the copy: the chosen file is decoded (so a file that is
//! not a picture is refused before anything is kept), scaled down to at
//! most 1920 on its longer side and written as a JPEG into the data
//! folder's `backgrounds` folder under a fresh name, which is what the
//! settings record names. The user's own file is never read again, so
//! moving or deleting it changes nothing. [`prune`] removes the copies no
//! record or choice names any more.
//!
//! [`bake`] makes the backdrop: GPUI CE draws images but has no image
//! masks, so the whole look the background mockup settled on
//! (`docs/research/background-mockup`, the "Hero" preset) is composited here,
//! on the CPU, into one frame at the launcher panel's size in physical
//! pixels — the picture cover-fitted into the panel, the chosen
//! [`BackgroundEffect`], a blurred copy beneath a sharp one that gives way
//! below the middle, the fade to the canvas and the dim over it all. The
//! canvas is the panel's own color moved toward the picture's dominant
//! color (Roboco's `wallpaper_colors::extract`) and kept dark (or light)
//! enough for the palette's text; the launcher paints its panel with it,
//! and the frame has faded all but entirely into it by its lower edge, so
//! the two meet without a seam. On the solid material the frame is opaque
//! and the panel the opaque canvas. On glass the frame's alpha follows the
//! fade instead, and the panel is the canvas at the glass tint's alpha, so
//! the window's frost shows through the picture's lower part and the
//! panel below it, as Roboco's new-thread background lets it.
//!
//! The effects are Roboco's new-thread background treatments
//! (`roboco/crates/ui/src/new_thread_background_effects.rs`), drawn at the
//! frame's own pixels: their cells (the dither's dots, the glyphs, the
//! halftone's cells) are logical pixels, so a scaled display draws them
//! the size a standard one does.
//!
//! Everything here is blocking work for the background executor.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use gpui::{Hsla, RenderImage};
use image::imageops::{self, FilterType};
use image::{DynamicImage, ImageReader, Rgb, RgbImage, RgbaImage};
use pane_core::BackgroundEffect;

use crate::ui::theme::Appearance;

/// The data folder's folder Pane keeps its copies of background images in.
pub(crate) const FOLDER: &str = "backgrounds";

/// The longer side of the copy Pane keeps, in pixels.
const KEPT_SIDE: u32 = 1920;

/// The panel the backdrop is baked for, in logical pixels: the launcher's
/// client size (see [`crate::ui::shell::LAUNCHER_CLIENT`]). A window of
/// another size draws it cover-fitted.
pub(crate) const PANEL: (f32, f32) = crate::ui::shell::LAUNCHER_CLIENT;

/// Where the cover fit centers the picture: half across, 45% down.
const FOCUS: (f32, f32) = (0.5, 0.45);

/// The share of each picture layer over the canvas.
const IMAGE_OPACITY: f32 = 0.56;
/// The canvas laid back over the picture, where it shows.
const DIM: f32 = 0.64;
/// The blurred copy's blur, a standard deviation in logical pixels (CSS's
/// `blur(60px)`).
const BLUR: f32 = 60.;
/// The sharp copy holds to this share of the panel's height, then gives
/// way to the blurred one by [`SHARP_END`].
const SHARP_HOLD: f32 = 0.49;
const SHARP_END: f32 = 0.73;
/// Where the fade to the canvas ends, as a share of the panel's height.
const FADE_END: f32 = 1.05;
/// The fade's stops: its alpha at even steps from the top to
/// [`FADE_END`], a smoothstep's shape.
const FADE: [f32; 6] = [1., 0.9, 0.65, 0.35, 0.1, 0.];
/// The scanlines: a line one logical pixel high every three, black (white
/// in the light theme) at this strength.
const SCANLINE: f32 = 0.48 * 0.5;

/// The baked backdrop: the frame the launcher draws behind its content,
/// and the canvas its panel is painted with.
#[derive(Clone)]
pub(crate) struct Backdrop {
    /// The frame at the panel's size in physical pixels (BGRA, straight
    /// alpha, as GPUI draws it): opaque on the solid material, its alpha
    /// following the fade on glass.
    pub(crate) image: Arc<RenderImage>,
    /// The panel's color over this picture.
    pub(crate) canvas: Hsla,
}

/// Keeps Pane's copy of the picture at `source` in `data`'s backgrounds
/// folder, and returns the copy's file name — what the settings record
/// names. A file that cannot be read or is not a picture is refused with
/// the reason; nothing is kept then. The copy is written beside its final
/// name and renamed into place, so a half-written copy is never named.
pub(crate) fn import(source: &Path, data: &Path) -> Result<String, String> {
    let picture =
        decode(source).map_err(|why| format!("Pane could not read {}: {why}", source.display()))?;
    let picture = if picture.width().max(picture.height()) > KEPT_SIDE {
        picture.resize(KEPT_SIDE, KEPT_SIDE, FilterType::Lanczos3)
    } else {
        picture
    };
    let picture = DynamicImage::ImageRgb8(picture.to_rgb8());
    let folder = data.join(FOLDER);
    std::fs::create_dir_all(&folder)
        .map_err(|why| format!("Pane could not make {}: {why}", folder.display()))?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or_default();
    let name = format!("background-{stamp}.jpg");
    let file = folder.join(&name);
    let partial = folder.join(format!("{name}.tmp"));
    let mut bytes = Vec::new();
    let written = picture
        .write_with_encoder(image::codecs::jpeg::JpegEncoder::new_with_quality(
            &mut bytes, 90,
        ))
        .map_err(|why| why.to_string())
        .and_then(|()| std::fs::write(&partial, &bytes).map_err(|why| why.to_string()))
        .and_then(|()| std::fs::rename(&partial, &file).map_err(|why| why.to_string()));
    if let Err(why) = written {
        let _ = std::fs::remove_file(&partial);
        return Err(format!("Pane could not keep a copy of the picture: {why}"));
    }
    Ok(name)
}

/// Removes every copy in `data`'s backgrounds folder that `keep` does not
/// name, and any partial copy left behind. Removal is best effort: a copy
/// that cannot be removed now is tried again by the next prune.
pub(crate) fn prune(data: &Path, keep: &[&str]) {
    let Ok(entries) = std::fs::read_dir(data.join(FOLDER)) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if !keep.contains(&name) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// The copy named `name` in `data`'s backgrounds folder.
pub(crate) fn path(data: &Path, name: &str) -> PathBuf {
    data.join(FOLDER).join(name)
}

/// What a bake is for: the copy, its effect, the palette, the window's
/// scale factor and whether the glass material is in effect. A backdrop
/// is baked again only when one of them changes.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Key {
    pub(crate) name: String,
    pub(crate) effect: BackgroundEffect,
    pub(crate) appearance: Appearance,
    pub(crate) scale: f32,
    pub(crate) glass: bool,
}

/// Bakes the backdrop for the picture at `file` as `key` says (see the
/// module docs).
pub(crate) fn bake(file: &Path, key: &Key) -> Result<Backdrop, String> {
    let picture = decode(file)?.to_rgb8();
    let light = key.appearance == Appearance::Light;
    let canvas = canvas(dominant(&picture), light);
    let scale = key.scale.max(1.);
    let width = (PANEL.0 * scale).round() as u32;
    let height = (PANEL.1 * scale).round() as u32;
    let view = cover(&picture, width, height);
    // One logical pixel, in whole frame pixels: the effects' unit.
    let unit = (scale.round() as u32).max(1);
    let sharp = match key.effect {
        BackgroundEffect::Dither => dither(&view, unit),
        BackgroundEffect::Ascii => ascii(&view, unit, light),
        BackgroundEffect::Halftone => halftone(&view, unit, light),
        BackgroundEffect::None | BackgroundEffect::Scanlines => view,
    };
    let blurred = soften(&sharp, BLUR * scale);
    let scanlines = key.effect == BackgroundEffect::Scanlines;
    let frame = compose(&sharp, &blurred, canvas, scale, scanlines, light, key.glass);
    let [r, g, b] = canvas.map(|channel| channel as u32);
    Ok(Backdrop {
        image: Arc::new(RenderImage::new([image::Frame::new(frame)])),
        canvas: gpui::rgb_to_hsla(gpui::rgba(r << 24 | g << 16 | b << 8 | 0xFF)),
    })
}

/// The picture at `file`, decoded by its content rather than its name.
fn decode(file: &Path) -> Result<DynamicImage, String> {
    ImageReader::open(file)
        .and_then(|reader| reader.with_guessed_format())
        .map_err(|why| why.to_string())?
        .decode()
        .map_err(|why| why.to_string())
}

/// The picture's dominant color, as Roboco's `wallpaper_colors::extract`
/// finds it: a 4-bit-per-channel histogram of a small copy, each pixel
/// weighted by 0.2 plus its saturation squared, and the mean of the
/// heaviest bin.
fn dominant(picture: &RgbImage) -> [f32; 3] {
    let small = imageops::thumbnail(picture, 64, 64);
    let mut bins = vec![(0f64, [0f64; 3]); 4096];
    for Rgb([r, g, b]) in small.pixels().copied() {
        let high = r.max(g).max(b) as f64;
        let low = r.min(g).min(b) as f64;
        let saturation = (high - low) / high.max(1.);
        let weight = 0.2 + saturation * saturation;
        let index = ((r as usize >> 4) << 8) | ((g as usize >> 4) << 4) | (b as usize >> 4);
        let (count, sums) = &mut bins[index];
        *count += weight;
        for (sum, channel) in sums.iter_mut().zip([r, g, b]) {
            *sum += channel as f64 * weight;
        }
    }
    let (count, sums) = bins
        .into_iter()
        .max_by(|a, b| a.0.total_cmp(&b.0))
        .unwrap_or_default();
    if count <= 0. {
        return [0.; 3];
    }
    sums.map(|sum| (sum / count) as f32)
}

/// The canvas over a picture whose dominant color is `dominant`: the
/// panel's own solid color (the dark palette's #16171A, the light one's
/// #F6F6F8) moved 55% toward it, then held to a dark panel's lightness
/// (6–16%; 86–96% in the light theme) and at most 45% saturation, so the
/// palette's text keeps its contrast on it.
fn canvas(dominant: [f32; 3], light: bool) -> [f32; 3] {
    let panel: [f32; 3] = if light {
        [246., 246., 248.]
    } else {
        [22., 23., 26.]
    };
    let moved = [0, 1, 2].map(|i| (panel[i] + (dominant[i] - panel[i]) * 0.55).round());
    let (hue, saturation, lightness) = hsl(moved.map(|c| c / 255.));
    let lightness = if light {
        lightness.clamp(0.86, 0.96)
    } else {
        lightness.clamp(0.06, 0.16)
    };
    rgb(hue, saturation.min(0.45), lightness).map(|c| (c * 255.).round())
}

/// An sRGB color's hue (0–1), saturation and lightness, as HLS has them.
fn hsl([r, g, b]: [f32; 3]) -> (f32, f32, f32) {
    let high = r.max(g).max(b);
    let low = r.min(g).min(b);
    let lightness = (high + low) / 2.;
    if high == low {
        return (0., 0., lightness);
    }
    let span = high - low;
    let saturation = if lightness <= 0.5 {
        span / (high + low)
    } else {
        span / (2. - high - low)
    };
    let hue = if high == r {
        (g - b) / span
    } else if high == g {
        2. + (b - r) / span
    } else {
        4. + (r - g) / span
    };
    ((hue / 6.).rem_euclid(1.), saturation, lightness)
}

/// The sRGB color of a hue (0–1), saturation and lightness.
fn rgb(hue: f32, saturation: f32, lightness: f32) -> [f32; 3] {
    if saturation == 0. {
        return [lightness; 3];
    }
    let high = if lightness <= 0.5 {
        lightness * (1. + saturation)
    } else {
        lightness + saturation - lightness * saturation
    };
    let low = 2. * lightness - high;
    let channel = |hue: f32| {
        let hue = hue.rem_euclid(1.);
        if hue < 1. / 6. {
            low + (high - low) * hue * 6.
        } else if hue < 0.5 {
            high
        } else if hue < 2. / 3. {
            low + (high - low) * (2. / 3. - hue) * 6.
        } else {
            low
        }
    };
    [channel(hue + 1. / 3.), channel(hue), channel(hue - 1. / 3.)]
}

/// The picture cover-fitted into `width` by `height`: scaled to cover it,
/// centered on [`FOCUS`].
fn cover(picture: &RgbImage, width: u32, height: u32) -> RgbImage {
    let (source_width, source_height) = (picture.width() as f32, picture.height() as f32);
    let scale = (width as f32 / source_width).max(height as f32 / source_height);
    let shown_width = (width as f32 / scale).min(source_width);
    let shown_height = (height as f32 / scale).min(source_height);
    let left = ((source_width - shown_width) * FOCUS.0).round() as u32;
    let top = ((source_height - shown_height) * FOCUS.1).round() as u32;
    let shown = imageops::crop_imm(
        picture,
        left,
        top,
        (shown_width.round() as u32).max(1),
        (shown_height.round() as u32).max(1),
    )
    .to_image();
    imageops::resize(&shown, width, height, FilterType::Triangle)
}

/// `picture` blurred by `sigma` frame pixels, cheaply: the blur runs on a
/// copy an eighth the size and is scaled back up, which a blur this wide
/// cannot tell from the full-size one.
fn soften(picture: &RgbImage, sigma: f32) -> RgbImage {
    const DOWN: u32 = 8;
    let (width, height) = picture.dimensions();
    let small = imageops::resize(
        picture,
        (width / DOWN).max(1),
        (height / DOWN).max(1),
        FilterType::Triangle,
    );
    let blurred = imageops::blur(&small, sigma / DOWN as f32);
    imageops::resize(&blurred, width, height, FilterType::Triangle)
}

/// The fade's alpha at `share` of the panel's height (see [`FADE`]).
fn fade(share: f32) -> f32 {
    let at = (share / FADE_END).clamp(0., 1.) * (FADE.len() - 1) as f32;
    let index = (at.floor() as usize).min(FADE.len() - 2);
    let within = at - index as f32;
    FADE[index] + (FADE[index + 1] - FADE[index]) * within
}

/// How much of the sharp copy shows at `share` of the panel's height.
fn sharpness(share: f32) -> f32 {
    (1. - (share - SHARP_HOLD) / (SHARP_END - SHARP_HOLD)).clamp(0., 1.)
}

/// The frame: from the canvas up, the blurred copy and the sharp copy
/// (each at [`IMAGE_OPACITY`], the sharp one giving way below the middle),
/// the scanlines if `scanlines`, then the canvas again at [`DIM`] — each
/// layer under the fade, so the frame's lower part is the canvas alone.
/// In BGRA: opaque, or with `glass` its alpha the fade's, so the frame
/// gives way to the translucent panel beneath it rather than covering it.
fn compose(
    sharp: &RgbImage,
    blurred: &RgbImage,
    canvas: [f32; 3],
    scale: f32,
    scanlines: bool,
    light: bool,
    glass: bool,
) -> RgbaImage {
    let (width, height) = sharp.dimensions();
    let period = (3. * scale).round().max(2.) as u32;
    let thickness = (scale.round() as u32).clamp(1, period - 1);
    let paper = if light { 255. } else { 0. };
    let row = width as usize;
    let mut bytes = vec![255u8; row * height as usize * 4];
    let rows = bytes
        .chunks_exact_mut(row * 4)
        .zip(sharp.as_raw().chunks_exact(row * 3))
        .zip(blurred.as_raw().chunks_exact(row * 3));
    for (y, ((out, crisp), soft)) in rows.enumerate() {
        let share = y as f32 / height as f32;
        let shown = fade(share);
        let blurred_share = IMAGE_OPACITY * shown;
        let sharp_share = blurred_share * sharpness(share);
        let line = if scanlines && y as u32 % period < thickness {
            SCANLINE * shown
        } else {
            0.
        };
        let dim = DIM * shown;
        let alpha = if glass {
            (shown * 255.).round() as u8
        } else {
            255
        };
        // Below the fade the frame is the canvas alone.
        if shown <= 0. {
            for pixel in out.as_chunks_mut::<4>().0 {
                pixel[..3].copy_from_slice(&[2, 1, 0].map(|i| canvas[i].round() as u8));
                pixel[3] = alpha;
            }
            continue;
        }
        for ((pixel, crisp), soft) in out
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(crisp.as_chunks::<3>().0)
            .zip(soft.as_chunks::<3>().0)
        {
            // RGB in, BGRA out.
            for (to, from) in [(0, 2), (1, 1), (2, 0)] {
                let mut value = canvas[from];
                value += (soft[from] as f32 - value) * blurred_share;
                value += (crisp[from] as f32 - value) * sharp_share;
                value += (paper - value) * line;
                value += (canvas[from] - value) * dim;
                pixel[to] = value.round().clamp(0., 255.) as u8;
            }
            pixel[3] = alpha;
        }
    }
    RgbaImage::from_raw(width, height, bytes).expect("the frame's buffer fits its size")
}

/// Roboco's dither: an ordered (Bayer 4×4) dither in dots two logical
/// pixels square, each dot its color pushed to full brightness or near
/// black against the matrix's threshold.
fn dither(picture: &RgbImage, unit: u32) -> RgbImage {
    const BAYER: [[u8; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];
    let dot = 2 * unit;
    let (width, height) = picture.dimensions();
    RgbImage::from_fn(width, height, |x, y| {
        let (column, row) = (x / dot, y / dot);
        let sample = picture.get_pixel(
            (column * dot + dot / 2).min(width - 1),
            (row * dot + dot / 2).min(height - 1),
        );
        let threshold = BAYER[row as usize % 4][column as usize % 4];
        let Rgb([r, g, b]) = *sample;
        let peak = r.max(g).max(b) as f32;
        let bright = peak / 255. > (threshold as f32 + 0.5) / 16.;
        let gain = if bright { 255. / peak.max(1.) } else { 0.08 };
        Rgb([r, g, b].map(|c| (c as f32 * gain).round().min(255.) as u8))
    })
}

/// Roboco's ASCII: the picture redrawn in five-by-seven bitmap glyphs in
/// cells six by eight logical pixels, the glyph chosen by the cell's
/// brightness (its darkness on light paper), each pixel 60% the picture
/// and 40% the glyph's ink — the cell's color — or the paper.
fn ascii(picture: &RgbImage, unit: u32, light: bool) -> RgbImage {
    const GLYPHS: [[u8; 7]; 10] = [
        [0, 0, 0, 0, 0, 0, 0],
        [0, 0, 0, 0, 0, 4, 0],
        [0, 4, 0, 0, 4, 0, 0],
        [0, 0, 0, 14, 0, 0, 0],
        [0, 0, 14, 0, 14, 0, 0],
        [0, 4, 4, 31, 4, 4, 0],
        [0, 21, 14, 31, 14, 21, 0],
        [10, 10, 31, 10, 31, 10, 10],
        [17, 2, 4, 4, 8, 16, 17],
        [14, 17, 23, 21, 23, 16, 14],
    ];
    let paper = if light { 255. } else { 0. };
    let (width, height) = picture.dimensions();
    RgbImage::from_fn(width, height, |x, y| {
        // The position in logical pixels, where the glyphs are drawn.
        let (lx, ly) = (x / unit, y / unit);
        let sample = *picture.get_pixel(
            ((lx / 6 * 6 + 3) * unit).min(width - 1),
            ((ly / 8 * 8 + 4) * unit).min(height - 1),
        );
        let brightness = luma(sample);
        let ink_density = if light { 1. - brightness } else { brightness };
        let glyph = &GLYPHS[(ink_density.sqrt() * 9.) as usize];
        let ink = lx % 6 < 5 && ly % 8 < 7 && glyph[(ly % 8) as usize] & (1 << (4 - lx % 6)) != 0;
        let Rgb(base) = *picture.get_pixel(x, y);
        let Rgb(cell) = sample;
        Rgb([0, 1, 2].map(|i| {
            let over = if ink { cell[i] as f32 } else { paper };
            (base[i] as f32 * 0.6 + over * 0.4) as u8
        }))
    })
}

/// Roboco's halftone: round dots in cells four logical pixels square,
/// each dot's radius growing with the cell's brightness (its darkness on
/// light paper) and colored as the cell's center, laid 40% over the
/// picture on the paper.
fn halftone(picture: &RgbImage, unit: u32, light: bool) -> RgbImage {
    let paper = if light { 255. } else { 0. };
    let cell = 4 * unit;
    let (width, height) = picture.dimensions();
    RgbImage::from_fn(width, height, |x, y| {
        let (column, row) = (x / cell, y / cell);
        let center = *picture.get_pixel(
            (column * cell + cell / 2).min(width - 1),
            (row * cell + cell / 2).min(height - 1),
        );
        let brightness = luma(center);
        let brightness = if light { 1. - brightness } else { brightness };
        let radius = 2. * (0.3 + 0.7 * brightness.sqrt());
        // The distance from the cell's middle, in logical pixels.
        let dx = (x - column * cell) as f32 / unit as f32 - 1.5;
        let dy = (y - row * cell) as f32 / unit as f32 - 1.5;
        let coverage = (radius + 0.5 - (dx * dx + dy * dy).sqrt()).clamp(0., 1.);
        let Rgb(source) = *picture.get_pixel(x, y);
        let Rgb(dot) = center;
        Rgb([0, 1, 2].map(|i| {
            let over = dot[i] as f32 * coverage + paper * (1. - coverage);
            (source[i] as f32 * 0.6 + over * 0.4) as u8
        }))
    })
}

/// A color's brightness, 0–1 (Rec. 601 luma, as `to_luma8` has it).
fn luma(Rgb([r, g, b]): Rgb<u8>) -> f32 {
    (0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32) / 255.
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A picture of one color, written as a PNG into `dir`.
    fn picture(dir: &Path, name: &str, size: (u32, u32), color: [u8; 3]) -> PathBuf {
        let file = dir.join(name);
        RgbImage::from_pixel(size.0, size.1, Rgb(color))
            .save(&file)
            .unwrap();
        file
    }

    #[test]
    fn importing_keeps_a_scaled_copy_and_refuses_what_is_not_a_picture() {
        let dir = tempfile::tempdir().unwrap();
        let source = picture(dir.path(), "wide.png", (3000, 1000), [30, 130, 220]);
        let name = import(&source, dir.path()).unwrap();
        assert!(name.starts_with("background-") && name.ends_with(".jpg"));
        let kept = image::open(path(dir.path(), &name)).unwrap();
        assert_eq!((kept.width(), kept.height()), (1920, 640));

        let text = dir.path().join("notes.png");
        std::fs::write(&text, "not a picture").unwrap();
        assert!(import(&text, dir.path()).is_err());
        // Only the one copy, and no partial file, is left.
        let names: Vec<_> = std::fs::read_dir(dir.path().join(FOLDER))
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names, [std::ffi::OsString::from(&name)]);
    }

    #[test]
    fn pruning_keeps_only_the_named_copies() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join(FOLDER);
        std::fs::create_dir_all(&folder).unwrap();
        for name in [
            "background-1.jpg",
            "background-2.jpg",
            "background-3.jpg.tmp",
        ] {
            std::fs::write(folder.join(name), "").unwrap();
        }
        prune(dir.path(), &["background-2.jpg"]);
        let names: Vec<_> = std::fs::read_dir(&folder)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names, [std::ffi::OsString::from("background-2.jpg")]);
    }

    #[test]
    fn the_canvas_leans_toward_the_picture_and_stays_a_dark_panel() {
        // A saturated blue picture pulls the dark panel toward blue, held
        // to the dark panel's lightness and saturation.
        let blue = canvas([30., 130., 220.], false);
        assert!(blue[2] > blue[0], "{blue:?}");
        // The holds apply before the 8-bit rounding, which moves lightness
        // by up to half a step (0.002) and a color this dark's saturation
        // by about a hundredth per step.
        let (_, saturation, lightness) = hsl(blue.map(|c| c / 255.));
        assert!((0.057..=0.163).contains(&lightness), "{lightness}");
        assert!(saturation <= 0.465, "{saturation}");
        // A white picture cannot lift the dark panel past 16%, nor a black
        // one darken the light panel past 86%.
        let (_, _, lightness) = hsl(canvas([255.; 3], false).map(|c| c / 255.));
        assert!(lightness <= 0.163, "{lightness}");
        let (_, _, lightness) = hsl(canvas([0.; 3], true).map(|c| c / 255.));
        assert!(lightness >= 0.857, "{lightness}");
    }

    #[test]
    fn the_dominant_color_favours_the_saturated_region() {
        let mut picture = RgbImage::from_pixel(10, 10, Rgb([90, 90, 90]));
        for x in 0..8 {
            for y in 0..10 {
                picture.put_pixel(x, y, Rgb([30, 130, 220]));
            }
        }
        let [r, g, b] = dominant(&picture);
        assert!(b > g && g > r, "{r} {g} {b}");
    }

    #[test]
    fn a_backdrop_is_the_panel_at_the_scale_and_fades_to_the_canvas() {
        let dir = tempfile::tempdir().unwrap();
        let file = picture(dir.path(), "red.png", (400, 300), [220, 40, 30]);
        for effect in [
            BackgroundEffect::None,
            BackgroundEffect::Dither,
            BackgroundEffect::Ascii,
            BackgroundEffect::Halftone,
            BackgroundEffect::Scanlines,
        ] {
            let key = Key {
                name: "red.png".into(),
                effect,
                appearance: Appearance::Dark,
                scale: 1.5,
                glass: false,
            };
            let backdrop = bake(&file, &key).unwrap();
            let size = backdrop.image.size(0);
            assert_eq!((size.width.0, size.height.0), (1140, 777), "{effect:?}");
            let bytes = backdrop.image.as_bytes(0).unwrap();
            // Opaque throughout.
            assert!(bytes.chunks(4).all(|pixel| pixel[3] == 255), "{effect:?}");
            // The last row is all but the canvas (BGRA): the fade ends
            // just past the panel, so a trace of the picture is left.
            let canvas = gpui::hsla_to_rgba(backdrop.canvas);
            let last = &bytes[bytes.len() - 4..];
            for (byte, channel) in
                last[..3]
                    .iter()
                    .zip([canvas.color.blue, canvas.color.green, canvas.color.red])
            {
                assert!(
                    (*byte as f32 - channel * 255.).abs() <= 4.5,
                    "{effect:?}: {last:?} against {canvas:?}"
                );
            }
        }
    }

    #[test]
    fn over_glass_the_frame_gives_way_to_the_panel_as_it_fades() {
        let dir = tempfile::tempdir().unwrap();
        let file = picture(dir.path(), "red.png", (400, 300), [220, 40, 30]);
        let key = Key {
            name: "red.png".into(),
            effect: BackgroundEffect::Scanlines,
            appearance: Appearance::Dark,
            scale: 1.,
            glass: true,
        };
        let backdrop = bake(&file, &key).unwrap();
        let bytes = backdrop.image.as_bytes(0).unwrap();
        let row = backdrop.image.size(0).width.0 as usize * 4;
        // Each row's alpha is the fade's: opaque at the top, all but gone
        // at the bottom, and never rising on the way down.
        let alphas: Vec<u8> = bytes.chunks(row).map(|line| line[3]).collect();
        assert_eq!(alphas[0], 255);
        assert!(alphas[alphas.len() - 1] < 26, "{:?}", alphas.last());
        assert!(alphas.windows(2).all(|pair| pair[1] <= pair[0]));
        assert!(
            bytes
                .chunks(row)
                .all(|line| line.chunks(4).all(|pixel| pixel[3] == line[3]))
        );
    }

    #[test]
    fn the_fade_runs_from_the_top_to_past_the_panel() {
        assert_eq!(fade(0.), 1.);
        assert!((fade(0.21) - 0.9).abs() < 1e-4);
        assert!(fade(1.) > 0. && fade(1.) < 0.1);
        assert_eq!(fade(FADE_END), 0.);
        assert_eq!(sharpness(0.3), 1.);
        assert_eq!(sharpness(0.8), 0.);
    }
}
