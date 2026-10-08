//! The installed applications' own icons (#172, ADR 0038's "an icon
//! cache"): extracted by the host at 256 pixels, one adapter per system,
//! kept in Pane's cache folder, and refreshed in the background, so that
//! root search and quick slots draw each application's icon bare (ADR
//! 0035) without typing ever waiting for one.
//!
//! - **Extraction** ([`IconExtractor`], [`NativeExtractor`]): on Windows the
//!   shell's image of the application's primary source at 256 pixels,
//!   rejecting a small icon padded into a large canvas or framed in a
//!   thumbnail ([`covers_enough`]) for the next source (a shortcut's own
//!   icon location, its target program's shell image, then that program's
//!   own icon at its largest, the shell's file information icon); a
//!   packaged app's logo from its manifest, with its light and dark
//!   variants ([`appx`]). On macOS the workspace's icon of the bundle. On
//!   Linux the desktop entry's `Icon` in the user's icon theme, its parents
//!   and `hicolor`, then `pixmaps` ([`theme`]).
//! - **Filling its place** ([`fill_its_place`]): whatever extracted it, an
//!   image whose content spans less than three quarters of its canvas (a
//!   small picture padded into a large square, or framed: within a thin
//!   frame, the pixels unlike its transparent or light fill, [`frame`]) is
//!   cropped to the square around that content and scaled to
//!   [`ICON_SIZE`], so every application's icon is drawn as large as the
//!   others.
//! - **The cache** ([`IconCache`]): image files in Pane's cache folder
//!   ([`FOLDER`]), keyed by the application's id and a fingerprint of its
//!   source (its path, size and modification time), each written
//!   atomically, with an index of them. An index that cannot be read is
//!   deleted with every image and rebuilt; files the index does not name
//!   are removed. It holds at most [`MAX_BYTES`] and [`MAX_ICONS`], the
//!   least recently drawn going first. Deleting it loses nothing but the
//!   time to extract the icons again.
//! - **Refreshing**: one worker thread at low priority extracts a small
//!   batch at a time ([`BATCH`]). After each start it re-extracts every
//!   listed application's icon once; an icon a row on screen wants goes
//!   first, drawn from the cache at once when its source has not changed
//!   and extracted at once when it has. A failed extraction is remembered
//!   for the session, and the row keeps its placeholder (or the icon kept
//!   from before). Extraction never runs on the window's thread or the
//!   extension runtime's: asking what an icon shows only looks at what is
//!   kept.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::system_icons::{ICON_SIZE, SystemIcon};

pub mod appx;
#[cfg(any(windows, test))]
mod click_once;
pub mod theme;
#[cfg(windows)]
mod windows;

/// The folder in Pane's cache folder holding the applications' icons.
pub const FOLDER: &str = "application-icons";

/// The most bytes of images the cache keeps.
pub const MAX_BYTES: u64 = 64 * 1024 * 1024;

/// The most applications whose icons the cache keeps.
pub const MAX_ICONS: usize = 10_000;

/// Up to this size, in pixels each way, an icon is drawn whatever its
/// content covers ([`covers_enough`]): the system's small sizes are drawn
/// edge to edge.
pub const CHECKED_ABOVE: u32 = 48;

/// How many icons the worker extracts before it looks again at what rows
/// on screen want.
pub const BATCH: usize = 8;

/// The alpha above which a pixel is visible content ([`covers_enough`]).
const VISIBLE_ALPHA: u8 = 16;

/// How deep a frame around a thumbnail may reach into its canvas, as a
/// fraction of its smaller side (1/16: 16 pixels of 256); its rounded
/// corners are left out of the content within as deep ([`frame`]).
const FRAME_DEPTH: usize = 16;

/// The smallest canvas, in pixels its smaller way, looked at for a frame
/// ([`frame`]).
const FRAMED_FROM: usize = 64;

/// How far apart two visible pixels' channels may be for them to be one
/// colour ([`alike`]).
const ALIKE: u8 = 24;

/// The least each channel of an opaque fill inside a frame has: a light
/// fill, as the shell's thumbnail's white ([`frame`]).
const LIGHT: u8 = 208;

/// The index of the kept icons, in [`FOLDER`].
const INDEX: &str = "index.json";

/// The index's format, and how its images were made: 2 since they are
/// cropped to fill their place ([`fill_its_place`]), 3 since a framed
/// thumbnail's content is found within its frame ([`frame`]), so that the
/// icons a Pane before that kept are extracted again.
const INDEX_VERSION: u32 = 3;

/// An icon whose visible content spans at least this share of its canvas,
/// its larger way, fills it and is kept as it is ([`fill_its_place`]): the
/// margins the systems' own icon grids draw (macOS's about a tenth each
/// side) are left alone.
const FILLS_FROM: (usize, usize) = (3, 4);

/// The margin a cropped icon keeps around its content, each side, as a
/// fraction of the content's larger span (1/32, about 3%).
const CROP_MARGIN: usize = 32;

/// How long the worker rests between two batches refreshing in the
/// background, so that a refresh after a start never competes with what
/// the user does.
const BACKGROUND_REST: Duration = Duration::from_millis(10);

/// Whether an icon of `width` × `height` pixels of straight RGBA (row by
/// row) fills its box enough to be drawn as the application's icon: one at
/// most [`CHECKED_ABOVE`] pixels each way always does; a larger one must
/// have content ([`content_box`]: pixels whose alpha is above a faint 16,
/// or within a thumbnail's frame those unlike its fill) spanning at least
/// half its width or half its height. A small icon the system padded into
/// a large canvas, a 32-pixel image in the middle of a 256-pixel square,
/// does not, nor does the shell's framed thumbnail of a program with only
/// a small icon (that icon in the middle of a light square with a thin
/// frame): either would be drawn as a tiny picture in an empty place.
pub fn covers_enough(width: u32, height: u32, rgba: &[u8]) -> bool {
    if width <= CHECKED_ABOVE && height <= CHECKED_ABOVE {
        return true;
    }
    let (width, height) = (width as usize, height as usize);
    let Some(content) = content_box(width, height, rgba) else {
        // Nothing visible at all.
        return false;
    };
    content.width() * 2 >= width || content.height() * 2 >= height
}

/// The box of an image's visible content, in pixels, its right and bottom
/// edges included.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ContentBox {
    left: usize,
    top: usize,
    right: usize,
    bottom: usize,
}

impl ContentBox {
    fn width(&self) -> usize {
        self.right - self.left + 1
    }

    fn height(&self) -> usize {
        self.bottom - self.top + 1
    }
}

/// A frame around a thumbnail: the rings of pixels the shell draws at its
/// canvas's edge (a thin line, a soft shadow) around a fill, transparent or
/// light, that the picture sits on. Windows draws one around a program that
/// ships only a small icon: that icon in the middle of a 256-pixel square.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Frame {
    /// How many pixels deep the frame's rings reach, each side.
    inset: usize,
    /// How far from each corner, both ways, pixels are left out of the
    /// content: a rounded frame's corners curve inside its inset.
    corner: usize,
    /// The fill inside the frame: what is not content.
    fill: [u8; 4],
}

impl Frame {
    /// Whether the pixel at `x`, `y` of a `width` × `height` canvas lies
    /// within the frame (not in its rings or its corners).
    fn encloses(&self, x: usize, y: usize, width: usize, height: usize) -> bool {
        let inside = |at: usize, side: usize| at >= self.inset && at + self.inset < side;
        let near_edge = |at: usize, side: usize| at < self.corner || at + self.corner >= side;
        inside(x, width) && inside(y, height) && !(near_edge(x, width) && near_edge(y, height))
    }

    /// What a pixel outside the frame becomes in a cropped icon: the fill,
    /// or nothing when the fill is transparent.
    fn background(&self) -> [u8; 4] {
        if self.fill[3] > VISIBLE_ALPHA {
            self.fill
        } else {
            [0; 4]
        }
    }
}

/// Whether two pixels of straight RGBA look alike: both transparent (alpha
/// at most [`VISIBLE_ALPHA`], whatever their colour), or both visible with
/// every channel within [`ALIKE`].
fn alike(one: &[u8], other: &[u8]) -> bool {
    let (one_seen, other_seen) = (one[3] > VISIBLE_ALPHA, other[3] > VISIBLE_ALPHA);
    if !one_seen || !other_seen {
        return one_seen == other_seen;
    }
    one.iter()
        .zip(other)
        .take(4)
        .all(|(one, other)| one.abs_diff(*other) <= ALIKE)
}

/// The frame around `width` × `height` straight RGBA (at least
/// [`FRAMED_FROM`] pixels each way), if it is a framed thumbnail: the ring
/// [`FRAME_DEPTH`]'s share of its smaller side in is of one fill,
/// transparent or light (each channel at least [`LIGHT`], opaque), and the
/// rings outside it are each of one colour, at least one of them unlike
/// the fill (the frame's line), and every ring from the canvas's edge to
/// the innermost such one visible. Only the middle half of each side of a
/// ring is looked at, so rounded corners do not count. `None` for an icon
/// whose picture reaches its edges, a coloured plate filling its canvas, a
/// light plate inside a transparent margin, or a small picture padded with
/// transparency alone.
fn frame(width: usize, height: usize, rgba: &[u8]) -> Option<Frame> {
    if width.min(height) < FRAMED_FROM || rgba.len() < width * height * 4 {
        return None;
    }
    let depth = width.min(height) / FRAME_DEPTH;
    let pixel = |x: usize, y: usize| &rgba[(y * width + x) * 4..(y * width + x) * 4 + 4];
    // The one colour of the ring `at` pixels in, if it has one.
    let ring = |at: usize| -> Option<[u8; 4]> {
        let first: [u8; 4] = pixel(width / 2, at).try_into().ok()?;
        let across = (width / 4..width - width / 4).flat_map(|x| [(x, at), (x, height - 1 - at)]);
        let down = (height / 4..height - height / 4).flat_map(|y| [(at, y), (width - 1 - at, y)]);
        across
            .chain(down)
            .all(|(x, y)| alike(pixel(x, y), &first))
            .then_some(first)
    };
    let fill = ring(depth)?;
    let light = fill[3] <= VISIBLE_ALPHA
        || (fill[3] >= u8::MAX - ALIKE && fill[..3].iter().all(|channel| *channel >= LIGHT));
    if !light {
        return None;
    }
    let rings = (0..depth).map(ring).collect::<Option<Vec<_>>>()?;
    let inset = rings
        .iter()
        .rposition(|colour| !alike(colour, &fill))
        .map_or(0, |at| at + 1);
    // The frame is drawn from the canvas's edge in, every ring of it
    // visible: a light plate inside a transparent margin is no frame.
    let drawn = rings[..inset]
        .iter()
        .all(|colour| colour[3] > VISIBLE_ALPHA);
    (inset > 0 && drawn).then_some(Frame {
        inset,
        corner: depth,
        fill,
    })
}

/// The box of the content of `width` × `height` straight RGBA: within a
/// thumbnail's frame ([`frame`]) the pixels unlike its fill, else the
/// visible pixels (alpha above [`VISIBLE_ALPHA`]); `None` when there is
/// none or `rgba` is too short for the size.
fn content_box(width: usize, height: usize, rgba: &[u8]) -> Option<ContentBox> {
    framed_content(width, height, rgba).map(|(content, _)| content)
}

/// The box of the content of `width` × `height` straight RGBA, and the
/// frame it was found within, if any ([`content_box`]).
fn framed_content(width: usize, height: usize, rgba: &[u8]) -> Option<(ContentBox, Option<Frame>)> {
    if width == 0 || height == 0 || rgba.len() < width.checked_mul(height)?.checked_mul(4)? {
        return None;
    }
    let frame = frame(width, height, rgba);
    let (mut left, mut right, mut top, mut bottom) = (width, 0, height, 0);
    for y in 0..height {
        let row = &rgba[y * width * 4..(y + 1) * width * 4];
        for (x, pixel) in row.as_chunks::<4>().0.iter().enumerate() {
            let content = match &frame {
                Some(frame) => frame.encloses(x, y, width, height) && !alike(pixel, &frame.fill),
                None => pixel[3] > VISIBLE_ALPHA,
            };
            if content {
                left = left.min(x);
                right = right.max(x);
                top = top.min(y);
                bottom = bottom.max(y);
            }
        }
    }
    (left <= right && top <= bottom).then_some((
        ContentBox {
            left,
            top,
            right,
            bottom,
        },
        frame,
    ))
}

/// `width` × `height` pixels of straight RGBA made to fill their place, as
/// `size` × `size` pixels of straight RGBA: when the content
/// ([`content_box`]) spans less than three quarters of the canvas its
/// larger way ([`FILLS_FROM`]), a small picture padded into a large canvas
/// or framed in a thumbnail, the square around that content, centred on it
/// with a margin of 1/32 of its span each side ([`CROP_MARGIN`]; where the
/// square passes the canvas's edge or a frame's rings, transparent, or the
/// frame's opaque fill), scaled to `size` (smoothly: a small picture scaled
/// up is a little soft). `None` when the image already fills its canvas,
/// or shows nothing: it is kept as it is.
pub fn fill_its_place(width: u32, height: u32, rgba: &[u8], size: u32) -> Option<Vec<u8>> {
    let (width, height, size) = (width as usize, height as usize, size as usize);
    let (content, frame) = framed_content(width, height, rgba)?;
    let span = content.width().max(content.height());
    if size == 0 || span * FILLS_FROM.1 >= width.max(height) * FILLS_FROM.0 {
        return None;
    }
    let side = span + 2 * (span / CROP_MARGIN);
    // The square's top left corner, which may lie outside the canvas.
    let origin = |low: usize, high: usize| (low + high + 1) as isize / 2 - side as isize / 2;
    let (x0, y0) = (
        origin(content.left, content.right),
        origin(content.top, content.bottom),
    );
    let background = frame.map_or([0; 4], |frame| frame.background());
    let mut square = background.repeat(side * side);
    for y in 0..side {
        let from_y = y0 + y as isize;
        if from_y < 0 || from_y >= height as isize {
            continue;
        }
        for x in 0..side {
            let from_x = x0 + x as isize;
            if from_x < 0 || from_x >= width as isize {
                continue;
            }
            let (from_x, from_y) = (from_x as usize, from_y as usize);
            if frame.is_some_and(|frame| !frame.encloses(from_x, from_y, width, height)) {
                continue;
            }
            let from = (from_y * width + from_x) * 4;
            let to = (y * side + x) * 4;
            square[to..to + 4].copy_from_slice(&rgba[from..from + 4]);
        }
    }
    Some(scale_square(&square, side, size))
}

/// `rgba`, `from` × `from` pixels of straight RGBA, scaled to `to` × `to`:
/// bilinear when enlarging, the average of the pixels each one covers when
/// shrinking, both over premultiplied alpha so transparent pixels lend no
/// colour to the edges.
fn scale_square(rgba: &[u8], from: usize, to: usize) -> Vec<u8> {
    let taps = scale_taps(from, to);
    let premultiplied: Vec<[f32; 4]> = rgba
        .as_chunks::<4>()
        .0
        .iter()
        .map(|pixel| {
            let alpha = f32::from(pixel[3]) / 255.0;
            [
                f32::from(pixel[0]) * alpha,
                f32::from(pixel[1]) * alpha,
                f32::from(pixel[2]) * alpha,
                f32::from(pixel[3]),
            ]
        })
        .collect();
    // Across each row, then down each column.
    let mut across = vec![[0f32; 4]; to * from];
    for y in 0..from {
        for (x, row_taps) in taps.iter().enumerate() {
            let mut sum = [0f32; 4];
            for &(at, weight) in row_taps {
                for (total, value) in sum.iter_mut().zip(premultiplied[y * from + at]) {
                    *total += value * weight;
                }
            }
            across[y * to + x] = sum;
        }
    }
    let mut scaled = vec![0u8; to * to * 4];
    for (y, column_taps) in taps.iter().enumerate() {
        for x in 0..to {
            let mut sum = [0f32; 4];
            for &(at, weight) in column_taps {
                for (total, value) in sum.iter_mut().zip(across[at * to + x]) {
                    *total += value * weight;
                }
            }
            let alpha = sum[3].clamp(0.0, 255.0);
            let out = (y * to + x) * 4;
            if alpha > 0.0 {
                for (colour, total) in scaled[out..out + 3].iter_mut().zip(sum) {
                    *colour = (total * 255.0 / alpha).round().clamp(0.0, 255.0) as u8;
                }
            }
            scaled[out + 3] = alpha.round() as u8;
        }
    }
    scaled
}

/// For each of `to` pixels along a line scaled from `from`, the pixels it
/// draws from and their weights (summing to one).
fn scale_taps(from: usize, to: usize) -> Vec<Vec<(usize, f32)>> {
    let scale = from as f32 / to as f32;
    let last = from.saturating_sub(1);
    (0..to)
        .map(|out| {
            if scale <= 1.0 {
                // Enlarging: between the two nearest pixel centres.
                let at = ((out as f32 + 0.5) * scale - 0.5).clamp(0.0, last as f32);
                let low = at.floor() as usize;
                let high = (low + 1).min(last);
                let weight = at - low as f32;
                vec![(low, 1.0 - weight), (high, weight)]
            } else {
                // Shrinking: every pixel the output one covers, by how much.
                let start = out as f32 * scale;
                let end = start + scale;
                let mut taps = Vec::new();
                let mut index = start.floor() as usize;
                while index < from && (index as f32) < end {
                    let covered = end.min(index as f32 + 1.0) - start.max(index as f32);
                    if covered > 0.0 {
                        taps.push((index, covered / scale));
                    }
                    index += 1;
                }
                taps
            }
        })
        .collect()
}

/// The PNG `png` cropped and scaled to fill its place at [`ICON_SIZE`]
/// ([`fill_its_place`]); `None` when it is kept as it is (it fills its
/// canvas, or cannot be decoded).
fn filled_png(png: &[u8]) -> Option<Vec<u8>> {
    let (width, height, rgba) = crate::icons::decode_png(png)?;
    let filled = fill_its_place(width, height, &rgba, ICON_SIZE)?;
    crate::icons::encode_png(ICON_SIZE, ICON_SIZE, &filled)
}

/// An application's icon as its system gave it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Extracted {
    /// Drawn in the light theme, and in the dark one when there is no
    /// `dark`.
    pub light: SystemIcon,
    /// Drawn in the dark theme, when the application ships an icon of its
    /// own for it (a packaged app's dark-background logo).
    pub dark: Option<SystemIcon>,
}

impl Extracted {
    /// One icon for both themes.
    pub fn one(icon: SystemIcon) -> Extracted {
        Extracted {
            light: icon,
            dark: None,
        }
    }
}

/// Extracts the applications' icons: the system's ([`NativeExtractor`]), or
/// a test's.
pub trait IconExtractor: Send + Sync + 'static {
    /// What the source at `source` (an application's primary source path,
    /// [`super::Applications::icon_source`]) is now: when it changes, the
    /// icon kept for it is extracted again at once. `None` when it cannot
    /// be read, which extracts again on every start like an unchanged one.
    fn fingerprint(&self, source: &str) -> Option<String> {
        file_fingerprint(Path::new(source))
    }

    /// The icon of the application whose source is at `source`, or why the
    /// system has none. Blocks while the system draws it; never called on
    /// the window's or the extension runtime's thread.
    fn extract(&self, source: &str) -> Result<Extracted, String>;
}

/// The fingerprint of the file at `path`: its path, size and modification
/// time. `None` when it cannot be read.
pub fn file_fingerprint(path: &Path) -> Option<String> {
    let metadata = std::fs::metadata(path).ok()?;
    let modified = metadata
        .modified()
        .ok()
        .and_then(|at| at.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |at| at.as_millis());
    Some(format!(
        "{}\n{}\n{modified}",
        path.display(),
        metadata.len()
    ))
}

/// This system's extraction (see the module docs).
#[derive(Clone, Copy, Debug, Default)]
pub struct NativeExtractor;

impl IconExtractor for NativeExtractor {
    fn fingerprint(&self, source: &str) -> Option<String> {
        platform::fingerprint(source)
    }

    fn extract(&self, source: &str) -> Result<Extracted, String> {
        platform::extract(source)
    }
}

#[cfg(windows)]
use self::windows as platform;

#[cfg(target_os = "macos")]
mod platform {
    //! macOS: the workspace's icon of the bundle (`NSWorkspace`), its
    //! largest image up to twice 256 pixels, as Finder and the Dock show
    //! it. A bundle's fingerprint is its `Info.plist`'s, which an update
    //! rewrites.
    use std::path::Path;

    use super::{Extracted, file_fingerprint};
    use crate::system_icons::{NativeIcons, SystemIcons};

    pub(super) fn fingerprint(source: &str) -> Option<String> {
        let bundle = Path::new(source);
        file_fingerprint(&bundle.join("Contents/Info.plist")).or_else(|| file_fingerprint(bundle))
    }

    pub(super) fn extract(source: &str) -> Result<Extracted, String> {
        NativeIcons.icon(Path::new(source)).map(Extracted::one)
    }
}

#[cfg(target_os = "linux")]
mod platform {
    //! Linux: the icon a desktop entry names, looked up in the user's icon
    //! theme ([`super::theme`]).
    use std::path::Path;

    use super::theme::{IconThemes, entry_icon};
    use super::{Extracted, file_fingerprint};
    use crate::system_icons::SystemIcon;

    pub(super) fn fingerprint(source: &str) -> Option<String> {
        file_fingerprint(Path::new(source))
    }

    pub(super) fn extract(source: &str) -> Result<Extracted, String> {
        let text = std::fs::read_to_string(source)
            .map_err(|error| format!("cannot read {source}: {error}"))?;
        let name = entry_icon(&text).ok_or_else(|| format!("{source} names no icon"))?;
        IconThemes::from_env()
            .find(&name)
            .map(|file| Extracted::one(SystemIcon::File(file)))
            .ok_or_else(|| format!("the icon themes have no icon {name}"))
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
mod platform {
    use super::Extracted;

    pub(super) fn fingerprint(_source: &str) -> Option<String> {
        None
    }

    pub(super) fn extract(source: &str) -> Result<Extracted, String> {
        Err(format!("this system gives Pane no icon for {source}"))
    }
}

/// Makes the calling thread one of low priority, for the CPU and, where
/// the system has it, the disk: the worker refreshing icons never competes
/// with what the user does.
fn lower_priority() {
    #[cfg(windows)]
    {
        use ::windows::Win32::System::Threading::{
            GetCurrentThread, SetThreadPriority, THREAD_MODE_BACKGROUND_BEGIN,
        };
        // SAFETY: the pseudo handle of the calling thread, always valid.
        let _ = unsafe { SetThreadPriority(GetCurrentThread(), THREAD_MODE_BACKGROUND_BEGIN) };
    }
    #[cfg(target_os = "linux")]
    {
        // SAFETY: plain system calls on the calling thread's own id.
        let _ = unsafe { libc::setpriority(libc::PRIO_PROCESS, libc::gettid() as libc::id_t, 10) };
    }
    #[cfg(target_os = "macos")]
    {
        // SAFETY: a plain system call on the calling thread (`who` 0).
        let _ = unsafe { libc::setpriority(libc::PRIO_DARWIN_THREAD, 0, libc::PRIO_DARWIN_BG) };
    }
}

/// What the application whose id it is given opens: its icon's source
/// ([`super::Applications::icon_source`]). It may scan the system's
/// folders: only the worker calls it.
pub type SourceOf = Arc<dyn Fn(&str) -> Option<String> + Send + Sync>;

/// Told when icons changed: rows draw them again.
pub type Changed = Arc<dyn Fn() + Send + Sync>;

/// What an application's kept icon draws: an image file for each theme
/// (the same file when it has one for both).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shown {
    pub light: PathBuf,
    pub dark: PathBuf,
}

/// The applications' icons as Pane keeps them (see the module docs).
/// Cloning shares them.
#[derive(Clone)]
pub struct IconCache {
    shared: Arc<Shared>,
}

struct Shared {
    folder: PathBuf,
    extractor: Arc<dyn IconExtractor>,
    source_of: SourceOf,
    changed: Changed,
    limits: (u64, usize),
    state: Mutex<State>,
}

/// One kept icon, as the index records it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Kept {
    /// The fingerprint of the source it was extracted from.
    fingerprint: String,
    /// Its image for the light theme (and the dark without `dark`), a file
    /// name in the folder.
    light: String,
    dark: Option<String>,
    /// The bytes of its images.
    bytes: u64,
    /// When a row last drew it, as the cache counts ([`Index::tick`]).
    drawn: u64,
}

/// The index file.
#[derive(Debug, Default, Serialize, Deserialize)]
struct Index {
    version: u32,
    /// Moves on with every batch, so a larger `drawn` is more recent.
    tick: u64,
    icons: BTreeMap<String, Kept>,
}

/// What this start did about an application's icon.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Session {
    /// To be refreshed in the background.
    Queued,
    /// Wanted by a row on screen, ahead of the background.
    Wanted,
    /// Wanted, and drawn from the cache, its source unchanged; still to be
    /// refreshed in the background.
    Kept,
    /// Extracted this start.
    Refreshed,
    /// Its extraction failed this start: not tried again until the next.
    Failed,
}

#[derive(Default)]
struct State {
    /// Whether the index was read (or rebuilt).
    loaded: bool,
    index: Index,
    session: HashMap<String, Session>,
    /// Wanted by rows on screen, in the order they asked.
    urgent: VecDeque<String>,
    /// The background refresh, in the order the applications were listed.
    background: VecDeque<String>,
    /// The applications listed now: only theirs are refreshed in the
    /// background.
    listed: HashSet<String>,
    /// Whether the worker is running.
    working: bool,
    /// Whether the index changed since it was last written.
    dirty: bool,
}

/// One extraction to run, and why.
struct Job {
    id: String,
    /// Wanted by a row on screen: drawn from the cache when its source has
    /// not changed.
    urgent: bool,
}

impl IconCache {
    /// The icons kept in `folder` (Pane's cache folder's [`FOLDER`]),
    /// extracted with `extractor` from the source `source_of` gives each
    /// application, telling `changed` when what a row shows changed.
    /// Nothing is read or extracted until an icon is wanted or listed.
    pub fn new(
        folder: PathBuf,
        extractor: Arc<dyn IconExtractor>,
        source_of: SourceOf,
        changed: Changed,
    ) -> IconCache {
        IconCache {
            shared: Arc::new(Shared {
                folder,
                extractor,
                source_of,
                changed,
                limits: (MAX_BYTES, MAX_ICONS),
                state: Mutex::default(),
            }),
        }
    }

    /// These icons, keeping at most `bytes` of images and `icons`
    /// applications' icons instead of [`MAX_BYTES`] and [`MAX_ICONS`]: a
    /// test's bounds. Call it before using them.
    pub fn with_limits(self, bytes: u64, icons: usize) -> IconCache {
        let shared = &self.shared;
        IconCache {
            shared: Arc::new(Shared {
                folder: shared.folder.clone(),
                extractor: shared.extractor.clone(),
                source_of: shared.source_of.clone(),
                changed: shared.changed.clone(),
                limits: (bytes, icons),
                state: Mutex::default(),
            }),
        }
    }

    /// The folder the icons are kept in.
    pub fn folder(&self) -> &Path {
        &self.shared.folder
    }

    /// What the icon of the application `id` draws now, if one is kept;
    /// `None` while it is not (yet): the row shows its placeholder. A row
    /// on screen asks this, so the application's icon is wanted ahead of
    /// the background refresh. Never extracts or touches the disk.
    pub fn shown(&self, id: &str) -> Option<Shown> {
        let mut state = self.shared.lock();
        self.want_locked(&mut state, id);
        let tick = state.index.tick;
        let folder = &self.shared.folder;
        let kept = state.index.icons.get_mut(id)?;
        kept.drawn = tick;
        let light = folder.join(&kept.light);
        let dark = kept
            .dark
            .as_ref()
            .map_or_else(|| light.clone(), |dark| folder.join(dark));
        Some(Shown { light, dark })
    }

    /// Wants the icon of the application `id`, as a row on screen does,
    /// without asking what it draws.
    pub fn want(&self, id: &str) {
        let mut state = self.shared.lock();
        self.want_locked(&mut state, id);
    }

    fn want_locked(&self, state: &mut State, id: &str) {
        match state.session.get(id) {
            None | Some(Session::Queued) => {
                state.session.insert(id.to_owned(), Session::Wanted);
                state.urgent.push_back(id.to_owned());
                self.start_worker(state);
            }
            Some(_) => {}
        }
    }

    /// The applications listed now, by id: each one's icon not refreshed
    /// since this start is refreshed in the background, in this order;
    /// what was queued for an application no longer listed is dropped.
    pub fn listed(&self, ids: impl IntoIterator<Item = String>) {
        let mut state = self.shared.lock();
        let listed: Vec<String> = ids.into_iter().collect();
        state.listed = listed.iter().cloned().collect();
        let State {
            background,
            session,
            listed: now,
            ..
        } = &mut *state;
        background.retain(|id| now.contains(id));
        session.retain(|id, done| !matches!(done, Session::Queued) || now.contains(id));
        for id in listed {
            if !state.session.contains_key(&id) {
                state.session.insert(id.clone(), Session::Queued);
                state.background.push_back(id);
            }
        }
        if !state.background.is_empty() {
            self.start_worker(&mut state);
        }
    }

    /// Starts the worker, unless it runs.
    fn start_worker(&self, state: &mut State) {
        if state.working {
            return;
        }
        state.working = true;
        let shared = self.shared.clone();
        let started = std::thread::Builder::new()
            .name("pane-application-icons".into())
            .spawn(move || {
                lower_priority();
                shared.work();
            });
        if let Err(error) = started {
            state.working = false;
            eprintln!("pane: could not start the thread refreshing application icons: {error}");
        }
    }

    /// Waits up to `limit` until the worker has nothing left to do; whether
    /// it has not. For tests.
    pub fn wait_idle(&self, limit: Duration) -> bool {
        let deadline = std::time::Instant::now() + limit;
        loop {
            {
                let state = self.shared.lock();
                if !state.working && state.urgent.is_empty() && state.background.is_empty() {
                    return true;
                }
            }
            if std::time::Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// The worker: reads the index, then extracts batch after batch, rows
    /// on screen first, until nothing is left.
    fn work(&self) {
        if !self.lock().loaded {
            let index = self.load();
            let mut state = self.lock();
            state.index = index;
            state.loaded = true;
            drop(state);
            // What was kept draws at once.
            (self.changed)();
        }
        loop {
            let (jobs, background) = {
                let mut state = self.lock();
                let jobs = next_batch(&mut state);
                if jobs.is_empty() {
                    state.working = false;
                    return;
                }
                let background = jobs.iter().any(|job| !job.urgent);
                (jobs, background)
            };
            for job in jobs {
                self.run(job);
            }
            {
                let mut state = self.lock();
                state.index.tick += 1;
                self.keep_within_limits(&mut state);
                if state.dirty {
                    let written = serde_json::to_vec(&state.index)
                        .map_err(|error| error.to_string())
                        .and_then(|json| {
                            crate::atomic::write_atomically(
                                &self.folder.join(INDEX),
                                &json,
                                crate::atomic::Readers::Default,
                            )
                            .map_err(|error| error.to_string())
                        });
                    match written {
                        Ok(()) => state.dirty = false,
                        Err(why) => eprintln!(
                            "pane: could not keep the application icons' index in {}: {why}",
                            self.folder.display()
                        ),
                    }
                }
            }
            (self.changed)();
            if background {
                std::thread::sleep(BACKGROUND_REST);
            }
        }
    }

    /// The index in the folder: empty when there is none; deleted with
    /// every image and rebuilt when it cannot be read, or is of another
    /// [`INDEX_VERSION`] (its images made differently). Images it does not
    /// name, and icons whose images are gone, are dropped.
    fn load(&self) -> Index {
        let path = self.folder.join(INDEX);
        let read = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice::<Index>(&bytes)
                .ok()
                .filter(|index| index.version == INDEX_VERSION),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Some(Index::default()),
            Err(_) => None,
        };
        let mut index = match read {
            Some(index) => index,
            None => {
                eprintln!(
                    "pane: the application icons' cache in {} could not be read; it is rebuilt",
                    self.folder.display()
                );
                let _ = std::fs::remove_dir_all(&self.folder);
                Index::default()
            }
        };
        index.version = INDEX_VERSION;
        let folder = &self.folder;
        index.icons.retain(|_, kept| {
            folder.join(&kept.light).is_file()
                && kept
                    .dark
                    .as_ref()
                    .is_none_or(|dark| folder.join(dark).is_file())
        });
        let named: HashSet<String> = index
            .icons
            .values()
            .flat_map(|kept| std::iter::once(kept.light.clone()).chain(kept.dark.clone()))
            .collect();
        if let Ok(entries) = std::fs::read_dir(folder) {
            for entry in entries.filter_map(Result::ok) {
                let name = entry.file_name().to_string_lossy().into_owned();
                if name != INDEX && !named.contains(&name) && entry.path().is_file() {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
        index
    }

    /// Runs one extraction: keeps what it extracted, or remembers that it
    /// failed for this start.
    fn run(&self, job: Job) {
        let Some(source) = (self.source_of)(&job.id) else {
            self.lock().session.insert(job.id, Session::Failed);
            return;
        };
        let fingerprint = self.extractor.fingerprint(&source).unwrap_or_default();
        if job.urgent {
            let mut state = self.lock();
            let unchanged = state
                .index
                .icons
                .get(&job.id)
                .is_some_and(|kept| kept.fingerprint == fingerprint);
            if unchanged {
                // Drawn from the cache; refreshed in the background later.
                state.session.insert(job.id.clone(), Session::Kept);
                if !state.background.contains(&job.id) {
                    state.background.push_back(job.id);
                }
                return;
            }
        }
        let outcome = self
            .extractor
            .extract(&source)
            .and_then(|extracted| self.keep(&job.id, &fingerprint, extracted));
        let mut state = self.lock();
        match outcome {
            Ok(kept) => {
                let drawn = state
                    .index
                    .icons
                    .get(&job.id)
                    .map_or(state.index.tick, |old| old.drawn);
                let old = state
                    .index
                    .icons
                    .insert(job.id.clone(), Kept { drawn, ..kept });
                if let Some(old) = old {
                    let now = &state.index.icons[&job.id];
                    for file in std::iter::once(&old.light).chain(old.dark.as_ref()) {
                        if *file != now.light && Some(file) != now.dark.as_ref() {
                            let _ = std::fs::remove_file(self.folder.join(file));
                        }
                    }
                }
                state.dirty = true;
                state.session.insert(job.id, Session::Refreshed);
            }
            Err(why) => {
                eprintln!("pane: no icon for the application {source}: {why}");
                state.session.insert(job.id, Session::Failed);
            }
        }
    }

    /// Writes `extracted`, the icon of the application `id` from a source
    /// with `fingerprint`, into the folder.
    fn keep(&self, id: &str, fingerprint: &str, extracted: Extracted) -> Result<Kept, String> {
        let stem = crate::icons::web_image_stem(&format!("{id}\n{fingerprint}"));
        let (light, light_bytes) = self.write(&stem, extracted.light)?;
        let (dark, dark_bytes) = match extracted.dark {
            Some(dark) => {
                let (file, bytes) = self.write(&format!("{stem}-dark"), dark)?;
                (Some(file), bytes)
            }
            None => (None, 0),
        };
        Ok(Kept {
            fingerprint: fingerprint.to_owned(),
            light,
            dark,
            bytes: light_bytes + dark_bytes,
            drawn: 0,
        })
    }

    /// Writes `icon` as the image `<stem>.<its kind>` in the folder,
    /// atomically, a PNG cropped to fill its place first
    /// ([`fill_its_place`]; an SVG is kept as it is); its file name and
    /// size.
    fn write(&self, stem: &str, icon: SystemIcon) -> Result<(String, u64), String> {
        let mut bytes = match icon {
            SystemIcon::Png(png) => png,
            SystemIcon::File(file) => std::fs::read(&file)
                .map_err(|error| format!("cannot read {}: {error}", file.display()))?,
        };
        let kind = match crate::icons::image_kind(&bytes) {
            Some(kind @ ("png" | "svg")) => kind,
            _ => return Err("it is not a PNG or SVG image".into()),
        };
        if kind == "png"
            && let Some(filled) = filled_png(&bytes)
        {
            bytes = filled;
        }
        let name = format!("{stem}.{kind}");
        crate::atomic::write_atomically(
            &self.folder.join(&name),
            &bytes,
            crate::atomic::Readers::Default,
        )
        .map_err(|error| format!("cannot keep it in {}: {error}", self.folder.display()))?;
        Ok((name, bytes.len() as u64))
    }

    /// Removes the least recently drawn icons until the cache is within
    /// its bounds.
    fn keep_within_limits(&self, state: &mut State) {
        let (max_bytes, max_icons) = self.limits;
        let mut bytes: u64 = state.index.icons.values().map(|kept| kept.bytes).sum();
        while state.index.icons.len() > max_icons || bytes > max_bytes {
            let Some(oldest) = state
                .index
                .icons
                .iter()
                .min_by_key(|(id, kept)| (kept.drawn, (*id).clone()))
                .map(|(id, _)| id.clone())
            else {
                break;
            };
            if let Some(kept) = state.index.icons.remove(&oldest) {
                bytes = bytes.saturating_sub(kept.bytes);
                for file in std::iter::once(kept.light).chain(kept.dark) {
                    let _ = std::fs::remove_file(self.folder.join(file));
                }
                state.dirty = true;
            }
        }
    }
}

/// The next jobs: what rows on screen want first, then the background
/// refresh of the applications listed, at most [`BATCH`]. An application
/// is in a batch once: a row's icon wanted while its background refresh
/// waits is extracted for the row alone (which queues the refresh again
/// when it draws the kept icon instead).
fn next_batch(state: &mut State) -> Vec<Job> {
    let mut jobs: Vec<Job> = Vec::new();
    let taken = |jobs: &[Job], id: &str| jobs.iter().any(|job| job.id == id);
    while jobs.len() < BATCH {
        if let Some(id) = state.urgent.pop_front() {
            if state.session.get(&id) == Some(&Session::Wanted) && !taken(&jobs, &id) {
                jobs.push(Job { id, urgent: true });
            }
            continue;
        }
        let Some(id) = state.background.pop_front() else {
            break;
        };
        // A wanted icon is extracted for its row, ahead of this.
        let due = matches!(
            state.session.get(&id),
            Some(Session::Queued | Session::Kept)
        );
        if due && state.listed.contains(&id) && !taken(&jobs, &id) {
            jobs.push(Job { id, urgent: false });
        }
    }
    jobs
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `size` × `size` RGBA image, transparent but for an opaque square
    /// of `content` pixels in its middle.
    fn padded(size: u32, content: u32) -> Vec<u8> {
        let start = (size - content) / 2;
        let mut rgba = vec![0u8; (size * size * 4) as usize];
        for y in start..start + content {
            for x in start..start + content {
                let at = ((y * size + x) * 4) as usize;
                rgba[at..at + 4].copy_from_slice(&[200, 100, 50, 255]);
            }
        }
        rgba
    }

    #[test]
    fn a_small_icon_padded_into_a_large_canvas_does_not_cover_enough() {
        assert!(covers_enough(256, 256, &padded(256, 256)));
        assert!(covers_enough(256, 256, &padded(256, 200)));
        assert!(covers_enough(256, 256, &padded(256, 128)));
        assert!(!covers_enough(256, 256, &padded(256, 48)));
        assert!(!covers_enough(256, 256, &padded(256, 32)));
        // Nothing visible at all.
        assert!(!covers_enough(256, 256, &padded(256, 0)));
        // Small sizes are drawn whatever they hold.
        assert!(covers_enough(48, 48, &padded(48, 8)));
        assert!(covers_enough(32, 32, &padded(32, 0)));
        // A wide logo spans half the width.
        let mut wide = vec![0u8; 256 * 256 * 4];
        for x in 40..216 {
            let at = (128 * 256 + x) * 4;
            wide[at + 3] = 255;
        }
        assert!(covers_enough(256, 256, &wide));
        // Faint pixels are not content.
        let mut faint = padded(256, 32);
        for pixel in faint.as_chunks_mut::<4>().0 {
            if pixel[3] == 0 {
                pixel[3] = 10;
            }
        }
        assert!(!covers_enough(256, 256, &faint));
        // Too few bytes for the size given.
        assert!(!covers_enough(256, 256, &[0; 16]));
    }

    /// The box of `rgba`'s visible content, as left, top, width, height.
    fn visible(side: usize, rgba: &[u8]) -> (usize, usize, usize, usize) {
        let content = content_box(side, side, rgba).expect("something visible");
        (content.left, content.top, content.width(), content.height())
    }

    #[test]
    fn a_small_picture_padded_into_a_large_canvas_is_cropped_to_fill_its_place() {
        // A 32-pixel picture in the middle of 256 pixels: scaled up to fill
        // the square but for the small margin (1/32 of 32, a pixel, each
        // side, about 8 pixels once scaled, softened by the scaling).
        let filled = fill_its_place(256, 256, &padded(256, 32), 256).expect("cropped");
        assert_eq!(filled.len(), 256 * 256 * 4);
        let (left, top, width, height) = visible(256, &filled);
        assert!((2..=10).contains(&left), "left {left}");
        assert!((2..=10).contains(&top), "top {top}");
        assert!((236..=252).contains(&width), "width {width}");
        assert!((236..=252).contains(&height), "height {height}");
        // Its colour is kept in the middle, edges never bleeding black in.
        let middle = (128 * 256 + 128) * 4;
        assert_eq!(&filled[middle..middle + 4], &[200, 100, 50, 255]);
        let edge = (128 * 256 + left) * 4;
        assert_eq!(&filled[edge..edge + 3], &[200, 100, 50]);

        // A picture in a corner, touching the canvas's edges, is centred
        // (the square passing the edges is transparent there).
        let mut corner = vec![0u8; 256 * 256 * 4];
        for y in 0..40 {
            for x in 0..40 {
                let at = (y * 256 + x) * 4;
                corner[at..at + 4].copy_from_slice(&[0, 0, 0, 255]);
            }
        }
        let filled = fill_its_place(256, 256, &corner, 256).expect("cropped");
        let (left, top, width, height) = visible(256, &filled);
        assert!(left.abs_diff(256 - left - width) <= 1, "{left} {width}");
        assert!(top.abs_diff(256 - top - height) <= 1, "{top} {height}");
        assert!(width >= 236, "width {width}");

        // A wide picture keeps its proportions, centred in a square.
        let mut wide = vec![0u8; 256 * 256 * 4];
        for y in 120..136 {
            for x in 64..192 {
                wide[(y * 256 + x) * 4 + 3] = 255;
            }
        }
        let filled = fill_its_place(256, 256, &wide, 256).expect("cropped");
        let (_, _, width, height) = visible(256, &filled);
        assert!(
            width > 220 && (28..=36).contains(&height),
            "{width}×{height}"
        );

        // A small canvas with a smaller picture is cropped too, and scaled
        // to the size asked.
        let filled = fill_its_place(32, 32, &padded(32, 16), 256).expect("cropped");
        let (_, _, width, _) = visible(256, &filled);
        assert!(width >= 240, "width {width}");

        // A large picture is scaled down to the size asked.
        let filled = fill_its_place(512, 512, &padded(512, 300), 256).expect("cropped");
        let (_, _, width, _) = visible(256, &filled);
        assert!((236..=256).contains(&width), "width {width}");
    }

    #[test]
    fn an_icon_filling_its_canvas_or_showing_nothing_is_kept_as_it_is() {
        assert_eq!(fill_its_place(256, 256, &padded(256, 256), 256), None);
        assert_eq!(fill_its_place(256, 256, &padded(256, 240), 256), None);
        // macOS's grid: content about four fifths of the canvas.
        assert_eq!(fill_its_place(512, 512, &padded(512, 412), 256), None);
        assert_eq!(fill_its_place(32, 32, &padded(32, 28), 256), None);
        // A wide logo spanning the canvas's width.
        let mut wide = vec![0u8; 256 * 256 * 4];
        for x in 0..256 {
            wide[(128 * 256 + x) * 4 + 3] = 255;
        }
        assert_eq!(fill_its_place(256, 256, &wide, 256), None);
        // Nothing visible, or too few bytes.
        assert_eq!(fill_its_place(256, 256, &padded(256, 0), 256), None);
        assert_eq!(fill_its_place(256, 256, &[0; 16], 256), None);
    }

    #[test]
    fn a_kept_png_is_cropped_and_one_filling_its_canvas_is_left_alone() {
        let small = crate::icons::encode_png(256, 256, &padded(256, 48)).unwrap();
        let filled = filled_png(&small).expect("cropped");
        let (width, height, rgba) = crate::icons::decode_png(&filled).unwrap();
        assert_eq!((width, height), (ICON_SIZE, ICON_SIZE));
        assert!(covers_enough(width, height, &rgba));
        let full = crate::icons::encode_png(256, 256, &padded(256, 256)).unwrap();
        assert_eq!(filled_png(&full), None);
        // The shell's framed thumbnail too.
        let thumbnail = framed(256, 32, &SHELL_FRAME, [0; 4]);
        let thumbnail = crate::icons::encode_png(256, 256, &thumbnail).unwrap();
        let filled = filled_png(&thumbnail).expect("cropped");
        let (width, height, rgba) = crate::icons::decode_png(&filled).unwrap();
        assert!(covers_enough(width, height, &rgba));
    }

    /// The rings of the frame Windows draws around a program's thumbnail
    /// when it ships only a small icon (read from ame.exe's): a soft shadow
    /// two pixels wide, then a fading light line three pixels wide.
    const SHELL_FRAME: [[u8; 4]; 5] = [
        [0, 0, 0, 38],
        [0, 0, 0, 38],
        [255, 255, 255, 77],
        [255, 255, 255, 51],
        [255, 255, 255, 26],
    ];

    /// The picture's colour in [`framed`].
    const PICTURE: [u8; 4] = [200, 100, 50, 255];

    /// A `size` × `size` thumbnail: `rings` at its edges, its first colour
    /// also rounding the corners further in (eight pixels each way), around
    /// `fill`, with an opaque square of `content` pixels of [`PICTURE`] in
    /// the middle.
    fn framed(size: usize, content: usize, rings: &[[u8; 4]], fill: [u8; 4]) -> Vec<u8> {
        let mut rgba = fill.repeat(size * size);
        let start = (size - content) / 2;
        for y in 0..size {
            for x in 0..size {
                let (across, down) = (x.min(size - 1 - x), y.min(size - 1 - y));
                let colour = if across < 8 && down < 8 {
                    Some(rings[0])
                } else if (start..start + content).contains(&x)
                    && (start..start + content).contains(&y)
                {
                    Some(PICTURE)
                } else {
                    rings.get(across.min(down)).copied()
                };
                if let Some(colour) = colour {
                    let at = (y * size + x) * 4;
                    rgba[at..at + 4].copy_from_slice(&colour);
                }
            }
        }
        rgba
    }

    #[test]
    fn a_framed_thumbnail_s_content_is_what_its_frame_holds() {
        // Windows' thumbnail: transparent within its frame, whose rings
        // are all visible, so the visible pixels span the whole canvas.
        let thumbnail = framed(256, 32, &SHELL_FRAME, [0; 4]);
        assert_eq!(
            frame(256, 256, &thumbnail),
            Some(Frame {
                inset: 5,
                corner: 16,
                fill: [0; 4]
            })
        );
        assert_eq!(visible(256, &thumbnail), (112, 112, 32, 32));
        assert!(!covers_enough(256, 256, &thumbnail));
        // As it looks over white: an opaque light fill and a thin grey line.
        let opaque = framed(256, 32, &[[200, 200, 200, 255]], [255; 4]);
        assert_eq!(visible(256, &opaque), (112, 112, 32, 32));
        assert!(!covers_enough(256, 256, &opaque));
        // A larger picture in a frame covers enough.
        assert!(covers_enough(
            256,
            256,
            &framed(256, 160, &SHELL_FRAME, [0; 4])
        ));
    }

    #[test]
    fn a_framed_thumbnail_is_cropped_to_its_picture_without_the_frame() {
        let filled =
            fill_its_place(256, 256, &framed(256, 32, &SHELL_FRAME, [0; 4]), 256).expect("cropped");
        let (left, top, width, height) = visible(256, &filled);
        assert!((2..=10).contains(&left), "left {left}");
        assert!((2..=10).contains(&top), "top {top}");
        assert!((236..=252).contains(&width), "width {width}");
        assert!((236..=252).contains(&height), "height {height}");
        let middle = (128 * 256 + 128) * 4;
        assert_eq!(&filled[middle..middle + 4], &PICTURE);
        // Its corners are transparent: no frame was carried in.
        assert_eq!(&filled[..4], &[0; 4]);

        // Over an opaque light fill, the margin is that fill and no line
        // of the frame is left.
        let opaque = framed(256, 32, &[[200, 200, 200, 255]], [255; 4]);
        let filled = fill_its_place(256, 256, &opaque, 256).expect("cropped");
        assert!(
            filled[..256 * 4]
                .as_chunks::<4>()
                .0
                .iter()
                .all(|pixel| *pixel == [255; 4]),
            "the top row is the fill"
        );
        assert_eq!(&filled[middle..middle + 4], &PICTURE);

        // A picture filling three quarters of its frame is kept.
        assert_eq!(
            fill_its_place(256, 256, &framed(256, 220, &SHELL_FRAME, [0; 4]), 256),
            None
        );
    }

    #[test]
    fn a_plate_filling_its_canvas_is_not_taken_for_a_frame() {
        // A coloured plate edge to edge, a small glyph on it: kept.
        let mut plate = [40u8, 90, 200, 255].repeat(256 * 256);
        for y in 112..144 {
            for x in 112..144 {
                let at = (y * 256 + x) * 4;
                plate[at..at + 4].copy_from_slice(&[255; 4]);
            }
        }
        assert_eq!(frame(256, 256, &plate), None);
        assert!(covers_enough(256, 256, &plate));
        assert_eq!(fill_its_place(256, 256, &plate, 256), None);
        // A white plate with no line at its edge: kept.
        let white = framed(256, 32, &[[255; 4]], [255; 4]);
        assert_eq!(frame(256, 256, &white), None);
        assert_eq!(fill_its_place(256, 256, &white, 256), None);
        // Nor a white plate inside a transparent margin.
        let inset = framed(256, 32, &[[0; 4]; 8], [255; 4]);
        assert_eq!(frame(256, 256, &inset), None);
        // A dark fill inside a line is not the shell's thumbnail: kept.
        let dark = framed(256, 32, &[[200, 200, 200, 255]], [30, 30, 30, 255]);
        assert_eq!(frame(256, 256, &dark), None);
        assert!(covers_enough(256, 256, &dark));
        // A small picture padded with transparency alone has no frame.
        assert_eq!(frame(256, 256, &padded(256, 32)), None);
        // Nor has a small canvas.
        assert_eq!(frame(48, 48, &framed(48, 8, &SHELL_FRAME, [0; 4])), None);
    }

    #[test]
    fn a_file_s_fingerprint_changes_with_it() {
        let folder = tempfile::tempdir().unwrap();
        let file = folder.path().join("app.lnk");
        assert_eq!(file_fingerprint(&file), None);
        std::fs::write(&file, b"one").unwrap();
        let first = file_fingerprint(&file).unwrap();
        assert!(first.contains("app.lnk"), "{first}");
        std::fs::write(&file, b"three").unwrap();
        assert_ne!(file_fingerprint(&file).unwrap(), first);
    }

    #[test]
    fn rows_on_screen_go_before_the_background_and_a_batch_is_small() {
        let mut state = State::default();
        for index in 0..20 {
            let id = format!("app{index}");
            state.session.insert(id.clone(), Session::Queued);
            state.listed.insert(id.clone());
            state.background.push_back(id);
        }
        state.session.insert("app15".into(), Session::Wanted);
        state.urgent.push_back("app15".into());
        // Wanted twice: once.
        state.urgent.push_back("app15".into());
        state.session.insert("app3".into(), Session::Refreshed);
        state.listed.remove("app4");

        let batch = next_batch(&mut state);
        let ids: Vec<(&str, bool)> = batch
            .iter()
            .map(|job| (job.id.as_str(), job.urgent))
            .collect();
        assert_eq!(ids.len(), BATCH);
        assert_eq!(ids[0], ("app15", true));
        assert!(ids[1..].iter().all(|(_, urgent)| !urgent));
        // Done this start, or no longer listed: skipped.
        assert!(!ids.iter().any(|(id, _)| *id == "app3" || *id == "app4"));
    }
}
