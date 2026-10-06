//! Icons, accessories and tooltips of a list's items (#139), as an author
//! writes them; the SDK writes them into the tree (`docs/list-tree.md`,
//! "Icons" and "Accessories").
//!
//! ```ignore
//! use pane_guest::{Accessory, Icon, Item, Mask, Tone};
//!
//! Item::new("ada", "Ada Lovelace")
//!     .icon(Icon::builtin("user").tint(Tone::Blue))
//!     .title_tooltip("Ada Lovelace, the first programmer")
//!     .accessory(Accessory::text("3").tooltip("Unread"))
//!     .accessory(Accessory::date(1_767_225_600_000))
//!     .accessory(Accessory::tag("Open").color(Tone::Green));
//! ```
//!
//! An icon is a built-in icon by name (the whole reicon set, such as
//! `"star"` or `"arrow-up-right"`), a PNG or SVG image the package ships
//! (with `@dark` and `@light` variants beside it, or a light and dark
//! pair), or an image by URL (a `data:` URL; web images come with a later
//! version). Every icon may have a tint, a mask and a fallback, and a
//! tooltip, which also makes assistive technology read it. [`avatar`] and
//! [`progress_ring`] build two common ones from these.

use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write;

use crate::list::Item;

/// A theme tone: Pane's text levels and accent, or a colour each theme
/// draws in its own shade.
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
    /// The tone's name in the tree.
    pub fn name(self) -> &'static str {
        match self {
            Tone::Primary => "primary",
            Tone::Secondary => "secondary",
            Tone::Accent => "accent",
            Tone::Red => "red",
            Tone::Orange => "orange",
            Tone::Yellow => "yellow",
            Tone::Green => "green",
            Tone::Blue => "blue",
            Tone::Purple => "purple",
            Tone::Magenta => "magenta",
        }
    }
}

/// One colour: a theme tone, or a raw colour (`#rgb`, `#rrggbb`,
/// `#rrggbbaa`), which Pane corrects for contrast against what it is
/// drawn on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Color {
    Tone(Tone),
    Raw(String),
}

impl Color {
    /// A raw colour such as `#ff6363`.
    pub fn hex(color: impl Into<String>) -> Color {
        Color::Raw(color.into())
    }
}

impl From<Tone> for Color {
    fn from(tone: Tone) -> Color {
        Color::Tone(tone)
    }
}

/// The colour an icon or an accessory is drawn in: one for both themes,
/// or one for the light theme and one for the dark.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Tint {
    Same(Color),
    Pair { light: Color, dark: Color },
}

impl Tint {
    /// `light` in the light theme and `dark` in the dark one.
    pub fn pair(light: impl Into<Color>, dark: impl Into<Color>) -> Tint {
        Tint::Pair {
            light: light.into(),
            dark: dark.into(),
        }
    }
}

impl From<Color> for Tint {
    fn from(color: Color) -> Tint {
        Tint::Same(color)
    }
}

impl From<Tone> for Tint {
    fn from(tone: Tone) -> Tint {
        Tint::Same(Color::Tone(tone))
    }
}

/// The shape an icon is clipped to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mask {
    Circle,
    RoundedRectangle,
}

/// What an icon draws.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Source {
    Builtin { name: String, filled: bool },
    Path(String),
    Pair { light: String, dark: String },
    Url(String),
}

/// An icon (see the module docs).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Icon {
    source: Source,
    tint: Option<Tint>,
    mask: Option<Mask>,
    fallback: Option<Box<Icon>>,
    tooltip: Option<String>,
}

impl Icon {
    fn of(source: Source) -> Icon {
        Icon {
            source,
            tint: None,
            mask: None,
            fallback: None,
            tooltip: None,
        }
    }

    /// The built-in icon `name` (reicon's, in kebab case such as
    /// `"arrow-up-right"`), in its Outline weight. A name Pane does not
    /// have draws the fallback.
    pub fn builtin(name: impl Into<String>) -> Icon {
        Icon::of(Source::Builtin {
            name: name.into(),
            filled: false,
        })
    }

    /// The built-in icon `name` in its Filled weight.
    pub fn filled(name: impl Into<String>) -> Icon {
        Icon::of(Source::Builtin {
            name: name.into(),
            filled: true,
        })
    }

    /// The PNG or SVG image at `path` inside the package (a list's images
    /// live under its `assets` folder): `name@dark.png` and
    /// `name@light.png` beside it are drawn in the dark and the light
    /// theme. An image the package does not have draws the fallback.
    pub fn image(path: impl Into<String>) -> Icon {
        Icon::of(Source::Path(path.into()))
    }

    /// The image `light` in the light theme and `dark` in the dark one,
    /// both inside the package.
    pub fn pair(light: impl Into<String>, dark: impl Into<String>) -> Icon {
        Icon::of(Source::Pair {
            light: light.into(),
            dark: dark.into(),
        })
    }

    /// The image at `url`: a `data:` URL (an SVG or a PNG). A web address
    /// draws the fallback in this version of Pane.
    pub fn url(url: impl Into<String>) -> Icon {
        Icon::of(Source::Url(url.into()))
    }

    /// This icon drawn in `tint`: a built-in icon's colour, or an image
    /// drawn as a mask in it.
    pub fn tint(mut self, tint: impl Into<Tint>) -> Icon {
        self.tint = Some(tint.into());
        self
    }

    /// This icon clipped to `mask`.
    pub fn mask(mut self, mask: Mask) -> Icon {
        self.mask = Some(mask);
        self
    }

    /// This icon drawing `fallback` when it cannot be drawn.
    pub fn fallback(mut self, fallback: Icon) -> Icon {
        self.fallback = Some(Box::new(fallback));
        self
    }

    /// This icon with a tooltip, shown on hover and read by assistive
    /// technology; an icon without one is decoration.
    pub fn tooltip(mut self, tooltip: impl Into<String>) -> Icon {
        self.tooltip = Some(tooltip.into());
        self
    }
}

/// What an accessory shows.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Content {
    Text(String),
    Date(i64),
    Tag(String),
    IconAlone,
}

/// One accessory on the right of a row: text, a date or a coloured tag,
/// with an optional icon, colour and tooltip. A row shows at most three.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Accessory {
    content: Content,
    icon: Option<Icon>,
    color: Option<Tint>,
    tooltip: Option<String>,
}

impl Accessory {
    fn of(content: Content) -> Accessory {
        Accessory {
            content,
            icon: None,
            color: None,
            tooltip: None,
        }
    }

    /// Text, such as a count.
    pub fn text(text: impl Into<String>) -> Accessory {
        Accessory::of(Content::Text(text.into()))
    }

    /// A time, `at` milliseconds since the Unix epoch, shown relative to
    /// now ("2h") and kept current while the list is open; its tooltip is
    /// the absolute time unless it has one of its own.
    pub fn date(at: i64) -> Accessory {
        Accessory::of(Content::Date(at))
    }

    /// A tag, such as "Open", drawn on a wash of its colour.
    pub fn tag(tag: impl Into<String>) -> Accessory {
        Accessory::of(Content::Tag(tag.into()))
    }

    /// An icon alone.
    pub fn of_icon(icon: Icon) -> Accessory {
        Accessory {
            icon: Some(icon),
            ..Accessory::of(Content::IconAlone)
        }
    }

    /// This accessory with `icon` before its text.
    pub fn icon(mut self, icon: Icon) -> Accessory {
        self.icon = Some(icon);
        self
    }

    /// This accessory's text, or tag, in `color`.
    pub fn color(mut self, color: impl Into<Tint>) -> Accessory {
        self.color = Some(color.into());
        self
    }

    /// This accessory with a tooltip, shown on hover.
    pub fn tooltip(mut self, tooltip: impl Into<String>) -> Accessory {
        self.tooltip = Some(tooltip.into());
        self
    }
}

/// How an item looks beyond its title and subtitle.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Look {
    icon: Option<Icon>,
    title_tooltip: Option<String>,
    subtitle_tooltip: Option<String>,
    accessories: Vec<Accessory>,
}

impl Item {
    /// This item with `icon` before its title.
    pub fn icon(mut self, icon: Icon) -> Item {
        self.look.icon = Some(icon);
        self
    }

    /// This item with a tooltip on its title, shown on hover: the whole
    /// title, say, when the row cuts it off.
    pub fn title_tooltip(mut self, tooltip: impl Into<String>) -> Item {
        self.look.title_tooltip = Some(tooltip.into());
        self
    }

    /// This item with a tooltip on its subtitle.
    pub fn subtitle_tooltip(mut self, tooltip: impl Into<String>) -> Item {
        self.look.subtitle_tooltip = Some(tooltip.into());
        self
    }

    /// This item with `accessory` after its accessories. A row shows the
    /// first three.
    pub fn accessory(mut self, accessory: Accessory) -> Item {
        self.look.accessories.push(accessory);
        self
    }

    /// This item with `accessories` after its accessories.
    pub fn accessories(mut self, accessories: impl IntoIterator<Item = Accessory>) -> Item {
        self.look.accessories.extend(accessories);
        self
    }
}

/// The colours [`avatar`] picks from, by name.
const AVATAR_COLORS: [&str; 8] = [
    "#e5484d", "#f76b15", "#ffb224", "#30a46c", "#12a594", "#0090ff", "#6e56cf", "#d6409f",
];

/// An avatar of `name`'s initials: up to two letters, white on a circle
/// whose colour `name` picks, as an SVG image clipped to a circle.
pub fn avatar(name: &str) -> Icon {
    let initials: String = name
        .split_whitespace()
        .filter_map(|word| word.chars().find(|c| c.is_alphanumeric()))
        .take(2)
        .flat_map(char::to_uppercase)
        .collect();
    let initials = if initials.is_empty() {
        String::from("?")
    } else {
        initials
    };
    // FNV-1a: the same name, the same colour, in every language.
    let hash = name.bytes().fold(0x811c_9dc5u32, |hash, byte| {
        (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193)
    });
    let color = AVATAR_COLORS[hash as usize % AVATAR_COLORS.len()];
    let size = if initials.chars().count() > 1 { 24 } else { 30 };
    let svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 64 64\">\
         <circle cx=\"32\" cy=\"32\" r=\"32\" fill=\"{color}\"/>\
         <text x=\"32\" y=\"32\" dominant-baseline=\"central\" text-anchor=\"middle\" \
         font-family=\"sans-serif\" font-size=\"{size}\" font-weight=\"600\" \
         fill=\"#ffffff\">{}</text></svg>",
        escape_xml(&initials)
    );
    Icon::url(svg_url(&svg))
        .mask(Mask::Circle)
        .tooltip(String::from(name))
}

/// A progress ring `fraction` full (0 to 1), as an SVG image drawn in the
/// accent; [`Icon::tint`] gives it another colour.
pub fn progress_ring(fraction: f32) -> Icon {
    let fraction = if fraction.is_nan() {
        0.
    } else {
        fraction.clamp(0., 1.)
    };
    // The ring's circumference: 2π × 9.
    let circumference = 56.548_67_f32;
    let filled = fraction * circumference;
    let svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\">\
         <circle cx=\"12\" cy=\"12\" r=\"9\" fill=\"none\" stroke=\"#000\" \
         stroke-opacity=\"0.25\" stroke-width=\"3\"/>\
         <circle cx=\"12\" cy=\"12\" r=\"9\" fill=\"none\" stroke=\"#000\" stroke-width=\"3\" \
         stroke-dasharray=\"{filled:.2} {circumference:.2}\" transform=\"rotate(-90 12 12)\"/>\
         </svg>"
    );
    Icon::url(svg_url(&svg)).tint(Tone::Accent)
}

/// `svg` as a `data:` URL, percent-encoded.
fn svg_url(svg: &str) -> String {
    let mut url = String::from("data:image/svg+xml,");
    for byte in svg.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.~/=:;,()".contains(&byte) {
            url.push(byte as char);
        } else {
            let _ = write!(url, "%{byte:02X}");
        }
    }
    url
}

/// `text` with the characters XML gives a meaning escaped.
fn escape_xml(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            other => escaped.push(other),
        }
    }
    escaped
}

/// Writes the fields of `look` into an item's JSON object, each with its
/// leading comma (`docs/list-tree.md`).
pub(crate) fn write_look(json: &mut String, look: &Look) {
    if let Some(icon) = &look.icon {
        json.push_str(",\"icon\":");
        write_icon(json, icon);
    }
    if let Some(tooltip) = &look.title_tooltip {
        json.push_str(",\"titleTooltip\":");
        string(json, tooltip);
    }
    if let Some(tooltip) = &look.subtitle_tooltip {
        json.push_str(",\"subtitleTooltip\":");
        string(json, tooltip);
    }
    if !look.accessories.is_empty() {
        json.push_str(",\"accessories\":[");
        for (index, accessory) in look.accessories.iter().enumerate() {
            if index > 0 {
                json.push(',');
            }
            write_accessory(json, accessory);
        }
        json.push(']');
    }
}

/// Writes `icon` as the tree's JSON object.
pub(crate) fn write_icon(json: &mut String, icon: &Icon) {
    json.push('{');
    match &icon.source {
        Source::Builtin { name, filled } => {
            json.push_str("\"builtin\":");
            string(json, name);
            if *filled {
                json.push_str(",\"filled\":true");
            }
        }
        Source::Path(path) => {
            json.push_str("\"path\":");
            string(json, path);
        }
        Source::Pair { light, dark } => {
            json.push_str("\"light\":");
            string(json, light);
            json.push_str(",\"dark\":");
            string(json, dark);
        }
        Source::Url(url) => {
            json.push_str("\"url\":");
            string(json, url);
        }
    }
    if let Some(tint) = &icon.tint {
        json.push_str(",\"tint\":");
        write_tint(json, tint);
    }
    if let Some(mask) = icon.mask {
        json.push_str(match mask {
            Mask::Circle => ",\"mask\":\"circle\"",
            Mask::RoundedRectangle => ",\"mask\":\"rounded-rectangle\"",
        });
    }
    if let Some(fallback) = &icon.fallback {
        json.push_str(",\"fallback\":");
        write_icon(json, fallback);
    }
    if let Some(tooltip) = &icon.tooltip {
        json.push_str(",\"tooltip\":");
        string(json, tooltip);
    }
    json.push('}');
}

fn write_accessory(json: &mut String, accessory: &Accessory) {
    json.push('{');
    let mut first = true;
    let mut field = |json: &mut String, name: &str| {
        if !first {
            json.push(',');
        }
        first = false;
        let _ = write!(json, "\"{name}\":");
    };
    match &accessory.content {
        Content::Text(text) => {
            field(json, "text");
            string(json, text);
        }
        Content::Date(at) => {
            field(json, "date");
            let _ = write!(json, "{at}");
        }
        Content::Tag(tag) => {
            field(json, "tag");
            string(json, tag);
        }
        Content::IconAlone => {}
    }
    if let Some(icon) = &accessory.icon {
        field(json, "icon");
        write_icon(json, icon);
    }
    if let Some(color) = &accessory.color {
        field(json, "color");
        write_tint(json, color);
    }
    if let Some(tooltip) = &accessory.tooltip {
        field(json, "tooltip");
        string(json, tooltip);
    }
    json.push('}');
}

fn write_tint(json: &mut String, tint: &Tint) {
    match tint {
        Tint::Same(color) => write_color(json, color),
        Tint::Pair { light, dark } => {
            json.push_str("{\"light\":");
            write_color(json, light);
            json.push_str(",\"dark\":");
            write_color(json, dark);
            json.push('}');
        }
    }
}

fn write_color(json: &mut String, color: &Color) {
    match color {
        Color::Tone(tone) => string(json, tone.name()),
        Color::Raw(raw) => string(json, raw),
    }
}

/// Writes `text` as a JSON string.
fn string(json: &mut String, text: &str) {
    json.push('"');
    for character in text.chars() {
        match character {
            '"' => json.push_str("\\\""),
            '\\' => json.push_str("\\\\"),
            '\n' => json.push_str("\\n"),
            '\r' => json.push_str("\\r"),
            '\t' => json.push_str("\\t"),
            control if u32::from(control) < 0x20 => {
                let _ = write!(json, "\\u{:04x}", u32::from(control));
            }
            other => json.push(other),
        }
    }
    json.push('"');
}
