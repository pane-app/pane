//! Icons, as ADR 0036 defines them (#139): what an extension's row, an
//! accessory, an action, a package and a command show.
//!
//! An icon is one of:
//!
//! - a **built-in icon**, by name, from the whole reicon set (MIT), which
//!   Pane vendors in `crates/pane-core/assets/reicon/` (see its README);
//! - a **packaged image**, a PNG or SVG file inside the package, by its path:
//!   `name@dark.png` and `name@light.png` beside `name.png` are drawn in the
//!   dark and the light theme, or the author names a light and a dark file;
//! - an **image by URL**: a `data:` URL is drawn as it is (the SDKs' avatar
//!   and progress ring helpers make one); a **web image**, by `http(s)`
//!   URL, is downloaded by Pane and cached as the package's extension cache
//!   (#142, see `launcher::icon_loads`), its fallback shown until it
//!   arrives and if it fails;
//! - a **system icon**: the icon the system shows for a file or an
//!   application, by its path, which the host extracts (#142, see
//!   [`crate::system_icons`]);
//! - a generated **first-letter tile**, which a package without an icon of
//!   its own gets.
//!
//! Every icon may have a tint (a theme tone, a colour, or a light and dark
//! pair of them), a mask (a circle or a rounded rectangle), a fallback
//! icon, drawn when it cannot be, and a tooltip, which also makes it
//! something assistive technology reads: an icon without one is decorative.
//!
//! The JSON forms are those of `docs/list-tree.md` ("Icons"); `pane.json`
//! names a package's and a command's icon the same way. A tree's icon is
//! read leniently ([`read`]): what Pane cannot draw is left out. A
//! manifest's is checked ([`parse_manifest_icon`]), and a package whose
//! icon names a built-in icon Pane does not have, or an image it does not
//! ship, is refused at install.

use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;

use serde_json::Value;

/// The whole reicon set's index: a line per icon, in name order, with the
/// offset and length in [`ICONS`] of its Outline and its Filled markup.
const INDEX: &str = include_str!("../assets/reicon/index.tsv");

/// Each icon's markup in each weight, compressed on its own with zlib.
const ICONS: &[u8] = include_bytes!("../assets/reicon/icons.bin");

/// How deep fallbacks may nest: an icon, its fallback, and so on.
pub const MAX_FALLBACK_DEPTH: usize = 4;

/// The size a published extension's icon is, in pixels each way: an
/// image the package ships smaller than this is cautioned about on an npm
/// or Git install's preview.
pub const PUBLISHED_ICON_SIZE: u32 = 512;

/// An icon as an extension names it, or as Pane resolved it to draw.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Icon {
    pub source: IconSource,
    /// The colour the icon is drawn in; `None` for its own colours (an
    /// image) or the theme's text colour (a built-in icon).
    pub tint: Option<Tint>,
    pub mask: Option<Mask>,
    /// Drawn instead when this icon cannot be: an unknown built-in name, a
    /// missing or unreadable image, a web image not loaded (yet).
    pub fallback: Option<Box<Icon>>,
    /// Shown on hover and read by assistive technology; without one the
    /// icon is decorative and assistive technology skips it.
    pub tooltip: Option<String>,
}

/// What an icon draws.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IconSource {
    /// A built-in icon: its reicon name in kebab case (`arrow-up-right`),
    /// in its Outline weight or, `filled`, its Filled one.
    Builtin { name: String, filled: bool },
    /// A packaged image (PNG or SVG): the file drawn in the light theme and
    /// the one drawn in the dark theme, the same file when it has no
    /// variants. Relative to the package folder as a tree or manifest names
    /// it, absolute once resolved.
    Image { light: PathBuf, dark: PathBuf },
    /// An image by URL: a `data:` URL, drawn as it is, or a web image by
    /// `http(s)` URL, which Pane downloads and caches (#142).
    Url(String),
    /// The system's icon of the file or application at this path (#142):
    /// a document's type icon, a folder's, an application's own. Absolute
    /// once resolved (a leading `~` is the user's home folder); a path that
    /// does not exist draws the fallback.
    File(PathBuf),
    /// A generated tile showing this letter: the icon of a package that
    /// has none of its own.
    Letter(char),
}

/// A theme tone an icon, an accessory or a tag can be coloured with: the
/// theme's text levels and accent, or a named colour each theme draws in
/// its own shade.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Primary,
    Secondary,
    Accent,
    Red,
    Orange,
    Yellow,
    Green,
    Blue,
    Purple,
    Magenta,
}

impl Tone {
    /// Every tone with its name in the tree, for reading and writing them.
    pub const ALL: [(&'static str, Tone); 10] = [
        ("primary", Tone::Primary),
        ("secondary", Tone::Secondary),
        ("accent", Tone::Accent),
        ("red", Tone::Red),
        ("orange", Tone::Orange),
        ("yellow", Tone::Yellow),
        ("green", Tone::Green),
        ("blue", Tone::Blue),
        ("purple", Tone::Purple),
        ("magenta", Tone::Magenta),
    ];

    /// The tone named `name` in the tree.
    pub fn named(name: &str) -> Option<Tone> {
        Tone::ALL
            .iter()
            .find(|(known, _)| known.eq_ignore_ascii_case(name))
            .map(|(_, tone)| *tone)
    }

    /// The tone's name in the tree.
    pub fn name(self) -> &'static str {
        Tone::ALL
            .iter()
            .find(|(_, tone)| *tone == self)
            .map_or("primary", |(name, _)| *name)
    }
}

/// One colour: a theme tone, or a raw colour (`0xRRGGBBAA`), which Pane
/// corrects for contrast against what it is drawn on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Color {
    Tone(Tone),
    Rgba(u32),
}

/// The colour an icon, an accessory or a tag is drawn in: one for both
/// themes, or one for the light theme and one for the dark.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tint {
    Same(Color),
    Pair { light: Color, dark: Color },
}

impl Tint {
    /// The colour for the dark theme when `dark`, else the light theme.
    pub fn for_theme(self, dark: bool) -> Color {
        match self {
            Tint::Same(color) => color,
            Tint::Pair {
                light,
                dark: on_dark,
            } => {
                if dark {
                    on_dark
                } else {
                    light
                }
            }
        }
    }
}

/// The shape an icon is clipped to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mask {
    Circle,
    RoundedRectangle,
}

impl Icon {
    /// An icon drawing `source`, with no tint, mask, fallback or tooltip.
    pub fn new(source: IconSource) -> Icon {
        Icon {
            source,
            tint: None,
            mask: None,
            fallback: None,
            tooltip: None,
        }
    }

    /// The generated first-letter tile of something titled `title`.
    pub fn letter_of(title: &str) -> Icon {
        let letter = title
            .chars()
            .find(|c| c.is_alphanumeric())
            .and_then(|c| c.to_uppercase().next())
            .unwrap_or('?');
        Icon::new(IconSource::Letter(letter))
    }

    /// Whether this icon is only decoration: it has no tooltip, so
    /// assistive technology skips it.
    pub fn is_decorative(&self) -> bool {
        self.tooltip
            .as_deref()
            .is_none_or(|tip| tip.trim().is_empty())
    }

    /// This icon as Pane draws it for a package whose files are in
    /// `folder`: a packaged image's path made absolute, with the `@light`
    /// and `@dark` variants it has picked; a system icon's path made
    /// absolute; what cannot be drawn (an unknown built-in name, an image
    /// missing from the package, a URL that is neither `data:` nor
    /// `http(s)`, a file that does not exist) replaced by its fallback,
    /// resolved alike. `None` when neither it nor any fallback can be
    /// drawn. A web image and a system icon stay as they are: what they
    /// show depends on their loading (`launcher::icon_loads`).
    pub fn resolved(self, folder: &Path) -> Option<Icon> {
        let Icon {
            source,
            tint,
            mask,
            fallback,
            tooltip,
        } = self;
        let fallback = fallback.and_then(|fallback| fallback.resolved(folder));
        let source = match source {
            IconSource::Builtin { name, filled } => {
                canonical_name(&name).map(|name| IconSource::Builtin {
                    name: name.to_owned(),
                    filled,
                })
            }
            IconSource::Image { light, dark } => image_files(folder, &light, &dark),
            // A data URL is drawn as it is; a web image once downloaded.
            IconSource::Url(url) => {
                (is_data_url(&url) || is_web_url(&url)).then_some(IconSource::Url(url))
            }
            IconSource::File(path) => system_path(&path).map(IconSource::File),
            IconSource::Letter(letter) => Some(IconSource::Letter(letter)),
        };
        match source {
            Some(source) => Some(Icon {
                source,
                tint,
                mask,
                fallback: fallback.map(Box::new),
                tooltip,
            }),
            None => fallback.map(|mut fallback| {
                // Drawn in its place, it says what the icon would have.
                if fallback.tooltip.is_none() {
                    fallback.tooltip = tooltip;
                }
                fallback
            }),
        }
    }
}

/// Reads `value`, a tree's icon, leniently: an icon whose source Pane
/// cannot read is `None`, and a tint, mask, fallback or tooltip it cannot
/// read is left out.
pub(crate) fn read(value: &Value) -> Option<Icon> {
    parse_at(value, 0, false).ok()
}

/// Reads `value` as an icon, strictly: anything Pane cannot read is an
/// error saying what. Names are not checked against the built-in set
/// ([`check_builtin_names`]).
pub(crate) fn parse(value: &Value) -> Result<Icon, String> {
    parse_at(value, 0, true)
}

/// `value`, the `icon` of `what` (such as "the package" or "command
/// `x`") in `pane.json`, checked: a built-in icon Pane has or an image the
/// package ships (checked against its files later, see
/// [`check_manifest_files`]), with any tint, mask and fallback.
pub(crate) fn parse_manifest_icon(value: &Value, what: &str) -> Result<Icon, String> {
    let icon = parse(value).map_err(|why| format!("the icon of {what} {why}"))?;
    check_manifest_sources(&icon).map_err(|why| format!("the icon of {what} {why}"))?;
    check_builtin_names(&icon).map_err(|why| format!("the icon of {what} {why}"))?;
    Ok(icon)
}

/// Checks that `icon` and its fallbacks name only sources a manifest may:
/// a built-in icon or an image the package ships.
fn check_manifest_sources(icon: &Icon) -> Result<(), String> {
    if let IconSource::Url(_) | IconSource::File(_) | IconSource::Letter(_) = icon.source {
        return Err(
            "is not a built-in icon's name or an image the package ships, which a package's \
             and a command's icon are"
                .into(),
        );
    }
    match &icon.fallback {
        Some(fallback) => check_manifest_sources(fallback),
        None => Ok(()),
    }
}

/// Checks that every built-in icon `icon` and its fallbacks name is one
/// Pane has.
pub(crate) fn check_builtin_names(icon: &Icon) -> Result<(), String> {
    if let IconSource::Builtin { name, .. } = &icon.source
        && canonical_name(name).is_none()
    {
        return Err(format!(
            "names the built-in icon `{name}`, which Pane does not have; built-in icons are \
             reicon's, named in kebab case such as `arrow-up-right` (see https://reicon.dev/icons)"
        ));
    }
    match &icon.fallback {
        Some(fallback) => check_builtin_names(fallback),
        None => Ok(()),
    }
}

/// Checks that every image `icon` and its fallbacks name is a file in
/// `folder`, the package's: for one path, that file (its `@light` and
/// `@dark` variants are optional), and both files of a light and dark
/// pair. `what` names the icon's owner in the message.
pub(crate) fn check_manifest_files(icon: &Icon, folder: &Path, what: &str) -> Result<(), String> {
    if let IconSource::Image { light, dark } = &icon.source {
        for file in [light, dark] {
            if !folder.join(file).is_file() {
                return Err(format!(
                    "the icon of {what} names {}, which is not in the package",
                    file.display()
                ));
            }
        }
    }
    match &icon.fallback {
        Some(fallback) => check_manifest_files(fallback, folder, what),
        None => Ok(()),
    }
}

/// The package files `icon` and its fallbacks name, relative to the
/// package folder, each image with the `@light` and `@dark` variants it
/// may have: what Pane copies into the managed copy with the package.
pub(crate) fn package_files(icon: &Icon) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if let IconSource::Image { light, dark } = &icon.source {
        for file in [light, dark] {
            for variant in [None, Some("light"), Some("dark")] {
                let path = match variant {
                    None => file.clone(),
                    Some(variant) => variant_path(file, variant),
                };
                if !files.contains(&path) {
                    files.push(path);
                }
            }
        }
    }
    if let Some(fallback) = &icon.fallback {
        for file in package_files(fallback) {
            if !files.contains(&file) {
                files.push(file);
            }
        }
    }
    files
}

/// What an npm or Git install's preview cautions about the package's
/// `icon` (in `folder`): having none, or an image smaller than a published
/// extension's 512×512. `None` when there is nothing to say.
pub(crate) fn caution(folder: &Path, icon: Option<&Icon>) -> Option<String> {
    let Some(icon) = icon else {
        return Some(format!(
            "it has no icon of its own, so Pane shows a first-letter tile for it; a published \
             extension's icon is a {PUBLISHED_ICON_SIZE}×{PUBLISHED_ICON_SIZE} image"
        ));
    };
    let IconSource::Image { light, dark } = &icon.source else {
        return None;
    };
    for file in [light, dark] {
        let path = folder.join(file);
        if !is_png(file) {
            continue;
        }
        let Some((width, height)) = png_size(&path) else {
            return Some(format!(
                "its icon {} is not a PNG image Pane can read",
                file.display()
            ));
        };
        if width < PUBLISHED_ICON_SIZE || height < PUBLISHED_ICON_SIZE {
            return Some(format!(
                "its icon {} is {width}×{height}, smaller than the \
                 {PUBLISHED_ICON_SIZE}×{PUBLISHED_ICON_SIZE} a published extension's icon is",
                file.display()
            ));
        }
    }
    None
}

/// The width and height a PNG file states in its header.
pub(crate) fn png_size(path: &Path) -> Option<(u32, u32)> {
    let mut header = [0u8; 24];
    std::fs::File::open(path)
        .ok()?
        .read_exact(&mut header)
        .ok()?;
    if header[..8] != *b"\x89PNG\r\n\x1a\n" || header[12..16] != *b"IHDR" {
        return None;
    }
    let width = u32::from_be_bytes(header[16..20].try_into().ok()?);
    let height = u32::from_be_bytes(header[20..24].try_into().ok()?);
    Some((width, height))
}

fn parse_at(value: &Value, depth: usize, strict: bool) -> Result<Icon, String> {
    let fields = match value {
        Value::String(text) => return Ok(Icon::new(source_of_text(text)?)),
        Value::Object(fields) => fields,
        _ => return Err("is not a name, an image's path or an object".into()),
    };
    let text = |key: &str| -> Result<Option<String>, String> {
        match fields.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(text)) => Ok(Some(text.clone())),
            Some(_) => Err(format!("gives a `{key}` that is not text")),
        }
    };
    let mut sources = Vec::new();
    if let Some(name) = text("builtin")? {
        if name.trim().is_empty() {
            return Err("gives an empty `builtin` name".into());
        }
        let filled = match fields.get("filled") {
            None | Some(Value::Null) => false,
            Some(Value::Bool(filled)) => *filled,
            Some(_) if strict => return Err("gives a `filled` that is not true or false".into()),
            Some(_) => false,
        };
        sources.push(IconSource::Builtin {
            name: name.trim().to_owned(),
            filled,
        });
    }
    if let Some(path) = text("path")? {
        let path = image_path(&path)?;
        sources.push(IconSource::Image {
            light: path.clone(),
            dark: path,
        });
    }
    match (text("light")?, text("dark")?) {
        (None, None) => {}
        (Some(light), Some(dark)) => sources.push(IconSource::Image {
            light: image_path(&light)?,
            dark: image_path(&dark)?,
        }),
        _ => return Err("gives only one of `light` and `dark`; a pair names both".into()),
    }
    if let Some(url) = text("url")? {
        if url.trim().is_empty() {
            return Err("gives an empty `url`".into());
        }
        sources.push(IconSource::Url(url.trim().to_owned()));
    }
    if let Some(file) = text("file")? {
        if file.trim().is_empty() {
            return Err("gives an empty `file` path".into());
        }
        sources.push(IconSource::File(PathBuf::from(file.trim())));
    }
    let source = match sources.len() {
        0 => {
            return Err(
                "names nothing to draw: give `builtin`, `path`, `light` and `dark`, `url` or \
                 `file`"
                    .into(),
            );
        }
        1 => sources.remove(0),
        _ => return Err("names more than one thing to draw".into()),
    };
    // The options: in a tree, one Pane cannot read is left out.
    fn optional<T>(result: Result<Option<T>, String>, strict: bool) -> Result<Option<T>, String> {
        match result {
            Ok(value) => Ok(value),
            Err(why) if strict => Err(why),
            Err(_) => Ok(None),
        }
    }
    let tint = match fields.get("tint") {
        None | Some(Value::Null) => Ok(None),
        Some(tint) => parse_tint(tint)
            .map(Some)
            .map_err(|why| format!("has a tint that {why}")),
    };
    let tint = optional(tint, strict)?;
    let mask = match fields.get("mask") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(mask)) => match mask.as_str() {
            "circle" => Ok(Some(Mask::Circle)),
            "rounded-rectangle" | "rounded" => Ok(Some(Mask::RoundedRectangle)),
            other => Err(format!(
                "has the mask `{other}`; a mask is \"circle\" or \"rounded-rectangle\""
            )),
        },
        Some(_) => Err("gives a `mask` that is not text".into()),
    };
    let mask = optional(mask, strict)?;
    let fallback = match fields.get("fallback") {
        None | Some(Value::Null) => Ok(None),
        Some(_) if depth + 1 >= MAX_FALLBACK_DEPTH => Err(format!(
            "nests fallbacks more than {MAX_FALLBACK_DEPTH} deep"
        )),
        Some(fallback) => parse_at(fallback, depth + 1, strict)
            .map(|fallback| Some(Box::new(fallback)))
            .map_err(|why| format!("has a fallback that {why}")),
    };
    let fallback = optional(fallback, strict)?;
    let tooltip = optional(text("tooltip"), strict)?;
    Ok(Icon {
        source,
        tint,
        mask,
        fallback,
        tooltip,
    })
}

/// What an icon written as text draws: a packaged image when it is a
/// path to one (it ends in `.png` or `.svg`, or names a folder), else a
/// built-in icon by name.
fn source_of_text(text: &str) -> Result<IconSource, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("is empty".into());
    }
    if is_png(Path::new(text)) || is_svg(Path::new(text)) || text.contains('/') {
        let path = image_path(text)?;
        return Ok(IconSource::Image {
            light: path.clone(),
            dark: path,
        });
    }
    Ok(IconSource::Builtin {
        name: text.to_owned(),
        filled: false,
    })
}

/// `text` as the path of a packaged image: relative, inside the package,
/// and a PNG or SVG file.
fn image_path(text: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(text.trim());
    let inside = !text.trim().is_empty()
        && path
            .components()
            .all(|part| matches!(part, Component::Normal(_)));
    if !inside {
        return Err(format!(
            "names the image `{text}`, which is not a relative path inside the package"
        ));
    }
    if !is_png(&path) && !is_svg(&path) {
        return Err(format!(
            "names the image `{text}`, which is not a PNG or SVG file"
        ));
    }
    Ok(path)
}

fn has_extension(path: &Path, extension: &str) -> bool {
    path.extension()
        .and_then(|found| found.to_str())
        .is_some_and(|found| found.eq_ignore_ascii_case(extension))
}

/// Whether `path` names a PNG file.
pub(crate) fn is_png(path: &Path) -> bool {
    has_extension(path, "png")
}

/// Whether `path` names an SVG file.
pub fn is_svg(path: &Path) -> bool {
    has_extension(path, "svg")
}

/// `path` with `@variant` before its extension: `logo@dark.png`.
fn variant_path(path: &Path, variant: &str) -> PathBuf {
    let stem = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    let name = match path.extension() {
        Some(extension) => format!("{stem}@{variant}.{}", extension.to_string_lossy()),
        None => format!("{stem}@{variant}"),
    };
    path.with_file_name(name)
}

/// The files of a packaged image in `folder`, by theme: for one path
/// (`light == dark`) its `@light` and `@dark` variants where the package
/// has them, else the file itself; for a pair, its two files, either
/// standing in for the other when one is missing. `None` when none is
/// there.
fn image_files(folder: &Path, light: &Path, dark: &Path) -> Option<IconSource> {
    let file = |path: PathBuf| path.is_file().then_some(path);
    let (light, dark) = if light == dark {
        let base = file(folder.join(light));
        (
            file(folder.join(variant_path(light, "light"))).or_else(|| base.clone()),
            file(folder.join(variant_path(dark, "dark"))).or(base),
        )
    } else {
        (file(folder.join(light)), file(folder.join(dark)))
    };
    match (light, dark) {
        (Some(light), Some(dark)) => Some(IconSource::Image { light, dark }),
        (Some(one), None) | (None, Some(one)) => Some(IconSource::Image {
            light: one.clone(),
            dark: one,
        }),
        (None, None) => None,
    }
}

/// Reads a tint: a colour for both themes, or `{"light": …, "dark": …}`.
fn parse_tint(value: &Value) -> Result<Tint, String> {
    match value {
        Value::String(color) => parse_color(color).map(Tint::Same),
        Value::Object(fields) => match (fields.get("light"), fields.get("dark")) {
            (Some(Value::String(light)), Some(Value::String(dark))) => Ok(Tint::Pair {
                light: parse_color(light)?,
                dark: parse_color(dark)?,
            }),
            _ => Err("is an object without a `light` and a `dark` colour".into()),
        },
        _ => Err("is not a colour or a light and dark pair".into()),
    }
}

/// Reads a colour or tint as the tree gives one, or why it cannot.
pub(crate) fn read_tint(value: &Value) -> Result<Tint, String> {
    parse_tint(value)
}

/// Reads one colour: a tone's name (`red`, `secondary`), or `#rgb`,
/// `#rgba`, `#rrggbb` or `#rrggbbaa`.
fn parse_color(text: &str) -> Result<Color, String> {
    let text = text.trim();
    if let Some(hex) = text.strip_prefix('#') {
        let digits: Option<Vec<u32>> = hex.chars().map(|c| c.to_digit(16)).collect();
        let Some(digits) = digits else {
            return Err(format!("names `{text}`, which is not a colour"));
        };
        let full: Vec<u32> = match digits.len() {
            3 | 4 => digits.iter().flat_map(|&d| [d, d]).collect(),
            6 | 8 => digits,
            _ => return Err(format!("names `{text}`, which is not a colour")),
        };
        let mut rgba = full.iter().fold(0u32, |value, &digit| value << 4 | digit);
        if full.len() == 6 {
            rgba = rgba << 8 | 0xFF;
        }
        return Ok(Color::Rgba(rgba));
    }
    Tone::named(text).map(Color::Tone).ok_or_else(|| {
        let names: Vec<&str> = Tone::ALL.iter().map(|(name, _)| *name).collect();
        format!(
            "names `{text}`, which is neither a colour such as `#ff6363` nor a tone ({})",
            names.join(", ")
        )
    })
}

/// Whether `url` is a `data:` URL, which Pane draws without downloading.
pub fn is_data_url(url: &str) -> bool {
    url.get(..5)
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("data:"))
}

/// Whether `url` is a web image's address: `http://` or `https://` with a
/// host, which Pane downloads (#142).
pub fn is_web_url(url: &str) -> bool {
    let lower = url
        .get(..8)
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    let scheme_ok = lower.starts_with("http://") || lower.starts_with("https://");
    if !scheme_ok {
        return false;
    }
    let (_, rest) = url.split_once("://").unwrap_or_default();
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..end];
    !authority.is_empty() && !authority.contains('@') && !authority.contains(char::is_whitespace)
}

/// `path`, a system icon's, made absolute (a leading `~` is the user's
/// home folder), if something is there: a file, a folder, an application
/// bundle. Windows' `shell:` names (a packaged application in the Apps
/// folder, `shell:AppsFolder\<id>`) are kept as they are, for the system
/// to find. `None` for a relative path or one that does not exist.
fn system_path(path: &Path) -> Option<PathBuf> {
    let text = path.to_string_lossy();
    if text
        .get(..6)
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("shell:"))
    {
        return Some(path.to_path_buf());
    }
    let path = match text.strip_prefix('~') {
        Some(rest) if rest.is_empty() || rest.starts_with(['/', '\\']) => {
            let home = std::env::var_os("HOME")
                .or_else(|| std::env::var_os("USERPROFILE"))
                .filter(|home| !home.is_empty())?;
            PathBuf::from(home).join(rest.trim_start_matches(['/', '\\']))
        }
        _ => path.to_path_buf(),
    };
    (path.is_absolute() && path.exists()).then_some(path)
}

/// The name Pane caches the web image at `url` under, without its
/// extension: the first 32 hex digits of the SHA-256 of the URL. The
/// extension is the image's kind ([`image_kind`]).
pub fn web_image_stem(url: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(url.as_bytes());
    digest[..16]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// The kinds of image a web image may be, by their file extension, as
/// Pane caches them; the window draws each.
pub const WEB_IMAGE_KINDS: [&str; 7] = ["png", "jpg", "gif", "webp", "bmp", "ico", "svg"];

/// What kind of image `bytes` are, by their first bytes: the extension of
/// one of [`WEB_IMAGE_KINDS`], or `None` when they are not an image Pane
/// draws (a web page, an error message, a truncated file).
pub fn image_kind(bytes: &[u8]) -> Option<&'static str> {
    let starts = |magic: &[u8]| bytes.starts_with(magic);
    if starts(b"\x89PNG\r\n\x1a\n") {
        return Some("png");
    }
    if starts(&[0xFF, 0xD8, 0xFF]) {
        return Some("jpg");
    }
    if starts(b"GIF87a") || starts(b"GIF89a") {
        return Some("gif");
    }
    if starts(b"RIFF") && bytes.get(8..12) == Some(&b"WEBP"[..]) {
        return Some("webp");
    }
    if starts(b"BM") && bytes.len() > 26 {
        return Some("bmp");
    }
    if starts(&[0, 0, 1, 0]) && bytes.len() > 6 {
        return Some("ico");
    }
    // An SVG: markup whose first element, after any declaration, comment
    // or doctype, is `<svg`.
    let head = &bytes[..bytes.len().min(4096)];
    if head.contains(&0) {
        return None;
    }
    let text = String::from_utf8_lossy(head);
    let mut rest = text.trim_start_matches('\u{feff}').trim_start();
    loop {
        if rest.starts_with("<svg") {
            return Some("svg");
        }
        let skipped = if rest.starts_with("<?") {
            rest.find("?>").map(|end| end + 2)
        } else if rest.starts_with("<!--") {
            rest.find("-->").map(|end| end + 3)
        } else if rest.starts_with("<!") {
            rest.find('>').map(|end| end + 1)
        } else {
            None
        };
        rest = rest[skipped?..].trim_start();
    }
}

/// `rgba`, `width` × `height` pixels of straight-alpha RGBA, row by row,
/// as a PNG file: how Pane keeps a system icon it extracted.
pub fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Option<Vec<u8>> {
    use std::io::Write;
    let row = usize::try_from(width).ok()?.checked_mul(4)?;
    if width == 0 || height == 0 || rgba.len() != row.checked_mul(usize::try_from(height).ok()?)? {
        return None;
    }
    let mut raw = Vec::with_capacity(rgba.len() + height as usize);
    for line in rgba.chunks_exact(row) {
        // Filter type 0: the row as it is.
        raw.push(0);
        raw.extend_from_slice(line);
    }
    let mut compressed =
        flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    compressed.write_all(&raw).ok()?;
    let compressed = compressed.finish().ok()?;
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut chunk = |kind: &[u8; 4], data: &[u8]| {
        png.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut crc = crc32fast::Hasher::new();
        crc.update(kind);
        crc.update(data);
        png.extend_from_slice(kind);
        png.extend_from_slice(data);
        png.extend_from_slice(&crc.finalize().to_be_bytes());
    };
    let mut header = Vec::with_capacity(13);
    header.extend_from_slice(&width.to_be_bytes());
    header.extend_from_slice(&height.to_be_bytes());
    // 8 bits a channel, RGBA, deflate, adaptive filtering, no interlace.
    header.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(b"IHDR", &header);
    chunk(b"IDAT", &compressed);
    chunk(b"IEND", &[]);
    Some(png)
}

/// What the `data:` URL `url` holds: its media type (`image/svg+xml`) and
/// its bytes, decoded from base64 or percent-encoding.
pub fn data_url(url: &str) -> Option<(String, Vec<u8>)> {
    if !is_data_url(url) {
        return None;
    }
    let (head, data) = url[5..].split_once(',')?;
    let (media, base64) = match head.strip_suffix(";base64") {
        Some(media) => (media, true),
        None => (head, false),
    };
    let media = media
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    let bytes = if base64 {
        decode_base64(data)?
    } else {
        decode_percent(data)
    };
    Some((media, bytes))
}

/// `text`, percent-decoded: a `%` and two hex digits is that byte, and
/// everything else is itself.
fn decode_percent(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let hex = |at: usize| bytes.get(at).and_then(|b| (*b as char).to_digit(16));
        match (bytes[index], hex(index + 1), hex(index + 2)) {
            (b'%', Some(high), Some(low)) => {
                decoded.push((high * 16 + low) as u8);
                index += 3;
            }
            (byte, ..) => {
                decoded.push(byte);
                index += 1;
            }
        }
    }
    decoded
}

/// `text`, decoded from (standard or URL-safe) base64, ignoring
/// whitespace and padding.
fn decode_base64(text: &str) -> Option<Vec<u8>> {
    let mut decoded = Vec::with_capacity(text.len() * 3 / 4);
    let mut buffer = 0u32;
    let mut bits = 0;
    for byte in text.bytes() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' | b' ' | b'\n' | b'\r' | b'\t' => continue,
            _ => return None,
        };
        buffer = buffer << 6 | u32::from(value);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            decoded.push((buffer >> bits) as u8);
            buffer &= (1 << bits) - 1;
        }
    }
    Some(decoded)
}

/// One icon of the whole set: its name and where its markup is.
struct Builtin {
    name: &'static str,
    outline: (usize, usize),
    filled: (usize, usize),
}

/// The whole set's index, read once.
fn index() -> &'static [Builtin] {
    static ENTRIES: OnceLock<Vec<Builtin>> = OnceLock::new();
    ENTRIES.get_or_init(|| {
        INDEX
            .lines()
            .filter_map(|line| {
                let mut parts = line.trim_end_matches('\r').split('\t');
                let name = parts.next()?;
                let mut number = || parts.next()?.parse::<usize>().ok();
                let outline = (number()?, number()?);
                let filled = (number()?, number()?);
                Some(Builtin {
                    name,
                    outline,
                    filled,
                })
            })
            .collect()
    })
}

/// The built-in icon `name` names, as the set names it: names are kebab
/// case (`arrow-up-right`), and reicon's own spelling (`ArrowUpRight`)
/// names the same icon. `None` when the set has none.
pub fn canonical_name(name: &str) -> Option<&'static str> {
    let wanted = kebab(name.trim());
    let entries = index();
    entries
        .binary_search_by(|entry| entry.name.cmp(wanted.as_str()))
        .ok()
        .map(|at| entries[at].name)
}

/// Whether Pane has the built-in icon `name`.
pub fn is_builtin(name: &str) -> bool {
    canonical_name(name).is_some()
}

/// Every built-in icon's name, in order.
pub fn builtin_names() -> impl Iterator<Item = &'static str> {
    index().iter().map(|entry| entry.name)
}

/// The built-in icon `name` as a standalone 24×24 SVG document, in its
/// Outline weight or, `filled`, its Filled one; `None` when Pane has no
/// such icon.
pub fn builtin_svg(name: &str, filled: bool) -> Option<String> {
    let name = canonical_name(name)?;
    let entries = index();
    let entry = &entries[entries
        .binary_search_by(|entry| entry.name.cmp(name))
        .ok()?];
    let (offset, length) = if filled { entry.filled } else { entry.outline };
    let packed = ICONS.get(offset..offset.checked_add(length)?)?;
    let mut markup = String::new();
    flate2::read::ZlibDecoder::new(packed)
        .read_to_string(&mut markup)
        .ok()?;
    Some(format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none">{markup}</svg>"#
    ))
}

/// `name` in kebab case, as the set names its icons: a dash before each
/// capital that follows a small letter or a digit, and before a capital
/// that starts a word after capitals (`ACUnit` is `ac-unit`); a name
/// already in kebab case stays as it is.
fn kebab(name: &str) -> String {
    let chars: Vec<char> = name.chars().collect();
    let mut kebab = String::with_capacity(name.len() + 4);
    for (index, &c) in chars.iter().enumerate() {
        if c.is_ascii_uppercase() && index > 0 {
            let before = chars[index - 1];
            let after = chars.get(index + 1).copied();
            let starts_word = before.is_ascii_lowercase()
                || before.is_ascii_digit()
                || (before.is_ascii_uppercase() && after.is_some_and(|a| a.is_ascii_lowercase()));
            if starts_word {
                kebab.push('-');
            }
        }
        kebab.push(c.to_ascii_lowercase());
    }
    kebab
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_whole_set_is_vendored_and_drawn_in_both_weights() {
        let names: Vec<&str> = builtin_names().collect();
        assert_eq!(names.len(), 2676, "reicon 1.2.5 has 2676 icons");
        assert!(names.windows(2).all(|pair| pair[0] < pair[1]), "in order");
        for name in ["star", "arrow-up-right", "setting4", "bell", "tag"] {
            for filled in [false, true] {
                let svg = builtin_svg(name, filled).unwrap_or_else(|| panic!("{name}"));
                assert!(svg.starts_with("<svg") && svg.ends_with("</svg>"), "{svg}");
                assert!(svg.contains("<path"), "{name}: {svg}");
            }
            assert_ne!(builtin_svg(name, false), builtin_svg(name, true));
        }
        // Every icon decodes.
        for name in names {
            assert!(builtin_svg(name, false).is_some(), "{name}");
        }
        assert_eq!(builtin_svg("no-such-icon", false), None);
    }

    #[test]
    fn names_are_kebab_case_and_reicons_own_spelling_names_the_same_icon() {
        assert_eq!(kebab("ArrowUpRight"), "arrow-up-right");
        assert_eq!(kebab("Setting4"), "setting4");
        assert_eq!(kebab("ACUnit"), "ac-unit");
        assert_eq!(kebab("arrow-up-right"), "arrow-up-right");
        assert_eq!(canonical_name("ArrowUpRight"), Some("arrow-up-right"));
        assert_eq!(canonical_name(" star "), Some("star"));
        assert!(is_builtin("TextalignLeft"));
        assert!(!is_builtin("stra"));
    }

    #[test]
    fn an_icon_reads_from_a_name_a_path_or_an_object() {
        assert_eq!(
            parse(&json!("star")).unwrap().source,
            IconSource::Builtin {
                name: "star".into(),
                filled: false
            }
        );
        assert_eq!(
            parse(&json!("assets/logo.png")).unwrap().source,
            IconSource::Image {
                light: "assets/logo.png".into(),
                dark: "assets/logo.png".into()
            }
        );
        let icon = parse(&json!({
            "light": "a.svg", "dark": "b.svg",
            "tint": {"light": "#000", "dark": "secondary"},
            "mask": "circle",
            "fallback": {"builtin": "star", "filled": true},
            "tooltip": "Logo"
        }))
        .unwrap();
        assert_eq!(
            icon.source,
            IconSource::Image {
                light: "a.svg".into(),
                dark: "b.svg".into()
            }
        );
        assert_eq!(
            icon.tint,
            Some(Tint::Pair {
                light: Color::Rgba(0x000000FF),
                dark: Color::Tone(Tone::Secondary)
            })
        );
        assert_eq!(icon.mask, Some(Mask::Circle));
        assert_eq!(
            icon.fallback.unwrap().source,
            IconSource::Builtin {
                name: "star".into(),
                filled: true
            }
        );
        assert_eq!(icon.tooltip.as_deref(), Some("Logo"));
        assert_eq!(
            parse(&json!({"builtin": "bell", "tint": "#ff636380"}))
                .unwrap()
                .tint,
            Some(Tint::Same(Color::Rgba(0xFF636380)))
        );
    }

    #[test]
    fn an_icon_pane_cannot_read_says_why_and_a_trees_drops_what_it_cannot_read() {
        for (icon, why) in [
            (json!(5), "is not a name"),
            (json!(""), "is empty"),
            (json!({"tint": "red"}), "names nothing to draw"),
            (json!({"builtin": "a", "path": "b.png"}), "more than one"),
            (json!({"light": "a.png"}), "only one of `light` and `dark`"),
            (json!("../outside.png"), "not a relative path inside"),
            (json!({"path": "logo.gif"}), "not a PNG or SVG"),
            (
                json!({"builtin": "star", "tint": "plaid"}),
                "neither a colour",
            ),
            (json!({"builtin": "star", "mask": "hexagon"}), "\"circle\""),
        ] {
            let error = parse(&icon).unwrap_err();
            assert!(error.contains(why), "{icon}: {error}");
        }
        // A tree's icon keeps what Pane can draw.
        let lenient = read(&json!({"builtin": "star", "tint": "plaid", "mask": 3})).unwrap();
        assert_eq!((lenient.tint, lenient.mask), (None, None));
        assert_eq!(read(&json!({"tint": "red"})), None);
        let mut deep = json!("star");
        for _ in 0..MAX_FALLBACK_DEPTH {
            deep = json!({"builtin": "bell", "fallback": deep});
        }
        assert!(parse(&deep).unwrap_err().contains("deep"));
    }

    #[test]
    fn a_manifests_icon_is_a_built_in_pane_has_or_an_image_the_package_ships() {
        assert!(parse_manifest_icon(&json!("star"), "the package").is_ok());
        let error = parse_manifest_icon(&json!("stra"), "command `x`").unwrap_err();
        assert!(
            error.starts_with("the icon of command `x` names the built-in icon `stra`"),
            "{error}"
        );
        let error =
            parse_manifest_icon(&json!({"path": "a.png", "fallback": "nope"}), "the package")
                .unwrap_err();
        assert!(error.contains("`nope`"), "{error}");
        let error = parse_manifest_icon(&json!({"url": "data:,x"}), "the package").unwrap_err();
        assert!(error.contains("not a built-in icon's name"), "{error}");

        let folder = tempfile::tempdir().unwrap();
        std::fs::write(folder.path().join("a.png"), b"x").unwrap();
        let icon = parse(&json!({"light": "a.png", "dark": "b.png"})).unwrap();
        let error = check_manifest_files(&icon, folder.path(), "the package").unwrap_err();
        assert_eq!(
            error,
            "the icon of the package names b.png, which is not in the package"
        );
        assert!(check_manifest_files(&parse(&json!("a.png")).unwrap(), folder.path(), "x").is_ok());
    }

    #[test]
    fn a_packaged_image_resolves_to_its_variants_by_theme_and_else_to_its_fallback() {
        let folder = tempfile::tempdir().unwrap();
        let dir = folder.path();
        for file in ["logo.png", "logo@dark.png", "plain.svg", "a.png"] {
            std::fs::write(dir.join(file), b"x").unwrap();
        }
        let resolved = |icon: Value| parse(&icon).unwrap().resolved(dir);
        assert_eq!(
            resolved(json!("logo.png")).unwrap().source,
            IconSource::Image {
                light: dir.join("logo.png"),
                dark: dir.join("logo@dark.png")
            }
        );
        assert_eq!(
            resolved(json!("plain.svg")).unwrap().source,
            IconSource::Image {
                light: dir.join("plain.svg"),
                dark: dir.join("plain.svg")
            }
        );
        // One file of a pair stands in for the other.
        assert_eq!(
            resolved(json!({"light": "a.png", "dark": "gone.png"}))
                .unwrap()
                .source,
            IconSource::Image {
                light: dir.join("a.png"),
                dark: dir.join("a.png")
            }
        );
        // Missing, or an unknown name: the fallback, with the tooltip.
        let fallen = resolved(json!({"path": "gone.png", "tooltip": "Logo",
                                     "fallback": "star"}))
        .unwrap();
        assert_eq!(
            (fallen.source, fallen.tooltip.as_deref()),
            (
                IconSource::Builtin {
                    name: "star".into(),
                    filled: false
                },
                Some("Logo")
            )
        );
        assert_eq!(resolved(json!("stra")), None);
        assert_eq!(
            resolved(json!({"builtin": "ArrowUpRight"})).unwrap().source,
            IconSource::Builtin {
                name: "arrow-up-right".into(),
                filled: false
            }
        );
        // A web image stays one, its fallback with it, for its loading to
        // decide; a URL Pane cannot download is its fallback.
        let web =
            resolved(json!({"url": "https://example.com/a.png", "fallback": "bell"})).unwrap();
        assert_eq!(
            web.source,
            IconSource::Url("https://example.com/a.png".into())
        );
        assert!(web.fallback.is_some());
        assert_eq!(
            resolved(json!({"url": "ftp://example.com/a.png", "fallback": "bell"}))
                .unwrap()
                .source,
            IconSource::Builtin {
                name: "bell".into(),
                filled: false
            }
        );
        assert!(matches!(
            resolved(json!({"url": "data:image/svg+xml,<svg/>"}))
                .unwrap()
                .source,
            IconSource::Url(_)
        ));
    }

    #[test]
    fn a_system_icon_names_a_path_that_exists_else_its_fallback_shows() {
        let folder = tempfile::tempdir().unwrap();
        let file = folder.path().join("notes.txt");
        std::fs::write(&file, b"x").unwrap();
        let icon = parse(&json!({"file": file.to_string_lossy(), "fallback": "document"})).unwrap();
        assert_eq!(icon.source, IconSource::File(file.clone()));
        let resolved = icon.resolved(folder.path()).unwrap();
        assert_eq!(resolved.source, IconSource::File(file.clone()));
        // A folder has an icon too.
        let folder_icon = parse(&json!({"file": folder.path().to_string_lossy()})).unwrap();
        assert!(folder_icon.resolved(folder.path()).is_some());
        // Missing, or relative: the fallback, else nothing.
        let gone = folder.path().join("gone.txt");
        let missing = parse(
            &json!({"file": gone.to_string_lossy(), "fallback": "document",
                                    "tooltip": "Notes"}),
        )
        .unwrap()
        .resolved(folder.path())
        .unwrap();
        assert_eq!(
            (missing.source, missing.tooltip.as_deref()),
            (
                IconSource::Builtin {
                    name: "document".into(),
                    filled: false
                },
                Some("Notes")
            )
        );
        assert_eq!(
            parse(&json!({"file": "notes.txt"}))
                .unwrap()
                .resolved(folder.path()),
            None
        );
        // Windows' shell names are the system's to find.
        let packaged = parse(&json!({"file": "shell:AppsFolder\\Microsoft.WindowsCalculator"}))
            .unwrap()
            .resolved(folder.path())
            .unwrap();
        assert!(matches!(packaged.source, IconSource::File(_)));
        // A manifest's icon cannot be one.
        let error = parse_manifest_icon(&json!({"file": "/bin/sh"}), "the package").unwrap_err();
        assert!(error.contains("not a built-in icon's name"), "{error}");
        assert!(parse(&json!({"file": " "})).unwrap_err().contains("empty"));
    }

    #[test]
    fn web_urls_are_http_and_https_addresses_with_a_host() {
        for url in [
            "https://example.com/favicon.ico",
            "HTTP://127.0.0.1:8741/images/a.png",
            "https://example.com",
        ] {
            assert!(is_web_url(url), "{url}");
        }
        for url in [
            "data:image/png;base64,AA",
            "ftp://example.com/a.png",
            "https://",
            "https:///a.png",
            "https://user@example.com/a.png",
            "example.com/a.png",
        ] {
            assert!(!is_web_url(url), "{url}");
        }
        let stem = web_image_stem("https://example.com/favicon.ico");
        assert_eq!(stem.len(), 32);
        assert!(stem.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(stem, web_image_stem("https://example.com/favicon.ico"));
        assert_ne!(stem, web_image_stem("https://example.org/favicon.ico"));
    }

    #[test]
    fn an_image_is_told_by_its_first_bytes() {
        let png = encode_png(2, 1, &[255, 0, 0, 255, 0, 0, 255, 128]).unwrap();
        assert_eq!(image_kind(&png), Some("png"));
        assert_eq!(image_kind(&[0xFF, 0xD8, 0xFF, 0xE0, 0, 0]), Some("jpg"));
        assert_eq!(image_kind(b"GIF89a\x01\x00"), Some("gif"));
        assert_eq!(image_kind(b"RIFF\0\0\0\0WEBPVP8 "), Some("webp"));
        assert_eq!(image_kind(&[0, 0, 1, 0, 1, 0, 16, 16]), Some("ico"));
        assert_eq!(
            image_kind(
                b"\xef\xbb\xbf<?xml version=\"1.0\"?>\n<!-- drawn -->\n\
                  <!DOCTYPE svg>\n<svg xmlns=\"http://www.w3.org/2000/svg\"/>"
            ),
            Some("svg")
        );
        for not_an_image in [
            &b"<!doctype html><html><body>Not found</body></html>"[..],
            &b"{\"error\": \"not found\"}"[..],
            &b""[..],
            &b"\x89PN"[..],
        ] {
            assert_eq!(image_kind(not_an_image), None);
        }
    }

    #[test]
    fn an_extracted_icon_is_kept_as_a_png_stating_its_size() {
        let folder = tempfile::tempdir().unwrap();
        let pixels: Vec<u8> = (0..3 * 2).flat_map(|_| [10, 20, 30, 255]).collect();
        let png = encode_png(3, 2, &pixels).unwrap();
        let path = folder.path().join("icon.png");
        std::fs::write(&path, &png).unwrap();
        assert_eq!(png_size(&path), Some((3, 2)));
        assert!(png.ends_with(&[0xAE, 0x42, 0x60, 0x82]), "IEND's CRC");
        assert_eq!(encode_png(3, 2, &pixels[..8]), None);
        assert_eq!(encode_png(0, 0, &[]), None);
    }

    #[test]
    fn the_first_letter_tile_takes_the_titles_first_letter() {
        assert_eq!(
            Icon::letter_of("icons sample").source,
            IconSource::Letter('I')
        );
        assert_eq!(
            Icon::letter_of("  2fa codes").source,
            IconSource::Letter('2')
        );
        assert_eq!(Icon::letter_of("…").source, IconSource::Letter('?'));
    }

    #[test]
    fn data_urls_decode_from_base64_and_percent_encoding() {
        assert_eq!(
            data_url("data:image/svg+xml;base64,PHN2Zy8+"),
            Some(("image/svg+xml".into(), b"<svg/>".to_vec()))
        );
        assert_eq!(
            data_url("data:image/svg+xml;utf8,%3Csvg%2F%3E"),
            Some(("image/svg+xml".into(), b"<svg/>".to_vec()))
        );
        assert_eq!(data_url("https://example.com"), None);
    }

    #[test]
    fn a_png_states_its_size_and_a_small_or_missing_icon_is_cautioned_about() {
        let folder = tempfile::tempdir().unwrap();
        let png = |width: u32, height: u32| {
            let mut bytes = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
            bytes.extend(width.to_be_bytes());
            bytes.extend(height.to_be_bytes());
            bytes
        };
        std::fs::write(folder.path().join("small.png"), png(64, 64)).unwrap();
        std::fs::write(folder.path().join("big.png"), png(512, 512)).unwrap();
        assert_eq!(png_size(&folder.path().join("small.png")), Some((64, 64)));
        let icon = |name: &str| parse(&json!(name)).unwrap();
        let small = caution(folder.path(), Some(&icon("small.png"))).unwrap();
        assert!(small.contains("small.png is 64×64"), "{small}");
        assert_eq!(caution(folder.path(), Some(&icon("big.png"))), None);
        assert_eq!(caution(folder.path(), Some(&icon("star"))), None);
        assert!(caution(folder.path(), None).unwrap().contains("no icon"));
    }
}
