//! Pane's icons sample (#139): a list whose rows show every kind of icon
//! and accessory, with tooltips. Its package has an icon of its own
//! (`icon.png`, 512×512) and its "Icons" command another (`command.svg`);
//! its second command has none, so it shows the package's. The "Plain
//! icons sample" package runs this component too and has no icon, so it
//! shows a first-letter tile.
//!
//! The rows, the same in the JavaScript and TypeScript samples:
//!
//! - "Built-in icon": reicon's `star`, a text accessory "3" with the
//!   tooltip "Unread", and a title tooltip;
//! - "Packaged image": `assets/logo.png`, whose `@light` and `@dark`
//!   variants Pane draws by theme, a date accessory (2026-01-01T00:00:00Z,
//!   shown relative to now) and a subtitle tooltip;
//! - "Light and dark pair": `assets/sun.svg` in the light theme and
//!   `assets/moon.svg` in the dark, with a green "Open" tag;
//! - "Tinted icon": reicon's `heart` tinted `#ff6363`, and a text
//!   accessory coloured by a light and dark pair;
//! - "Masked image": `assets/photo.png` clipped to a circle, with an
//!   accessory that is a tinted icon alone, whose tooltip is "Owner";
//! - "Failing image": `assets/missing.png`, which the package does not
//!   have, so its fallback, reicon's `warning`, shows, with the tooltip
//!   "Image missing";
//! - "Avatar and progress": the SDK's avatar of "Ada Lovelace" and an
//!   accessory with its progress ring at 40%;
//! - "Crowded row": five text accessories, of which a row draws three.
//!
//! And the icons Pane loads for the list (#142), which never waits for
//! them:
//!
//! - "Favicon": the SDK's favicon of the image server's site, with a globe
//!   as its fallback;
//! - "Slow web image": `<server>/images/slow.png`, which the test server
//!   holds back, so its fallback, reicon's `clock`, shows until it arrives;
//! - "Same slow image": the same URL, which Pane downloads once;
//! - "Broken image": `<server>/images/missing.png`, which the server does
//!   not have, so its fallback, reicon's `link-broken`, stays, with the
//!   tooltip "Image unavailable";
//! - "File icon": the SDK's file icon of the file the `iconFile` setting
//!   names (by default `~`, the home folder);
//! - "Application icon": the system icon of the application the
//!   `iconApplication` setting names (by default Windows' Notepad; on other
//!   systems its fallback shows until the setting names one).
//!
//! The image server is the `imageServer` setting, by default
//! `http://127.0.0.1:8741`; the tests set it to their own server.
//!
//! Every row's action answers "Chose <title>".
#![no_std]

use pane_guest::alloc::{format, string::String, vec::Vec};
use pane_guest::icon::{avatar, favicon, file_icon, progress_ring};
use pane_guest::{
    Accessory, Color, Command, CustomView, FieldValue, FormError, Icon, Item, List, Mask,
    NoCustomView, Tint, Tone, settings,
};

/// 2026-01-01T00:00:00Z, in milliseconds since the Unix epoch: the
/// "Packaged image" row's date.
const NEW_YEAR: i64 = 1_767_225_600_000;

struct Icons;
pane_guest::export!(Icons);

/// The setting naming the server the web images come from, and its
/// default.
const IMAGE_SERVER: (&str, &str) = ("imageServer", "http://127.0.0.1:8741");

/// The setting naming the file whose icon "File icon" shows, and its
/// default: the home folder.
const ICON_FILE: (&str, &str) = ("iconFile", "~");

/// The setting naming the application whose icon "Application icon"
/// shows, and its default: Windows' Notepad.
const ICON_APPLICATION: (&str, &str) = ("iconApplication", r"C:\Windows\System32\notepad.exe");

/// The value of the setting `(key, default)`: the saved one, or the
/// default.
fn setting((key, default): (&str, &str)) -> String {
    settings::get(key)
        .ok()
        .flatten()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| default.into())
}

/// The row `id` titled `title`, whose action answers "Chose <title>".
fn row(id: &'static str, title: &'static str) -> Item {
    Item::new(id, title).on_action(move || async move { Ok(format!("Chose {title}")) })
}

impl Command for Icons {
    type CustomView = NoCustomView;

    async fn render() -> Result<List, String> {
        let server = setting(IMAGE_SERVER);
        let server = server.trim_end_matches('/');
        let slow = format!("{server}/images/slow.png");
        Ok(List::new("Icons sample").items([
            row("builtin", "Built-in icon")
                .subtitle("reicon's star, by name")
                .icon(Icon::builtin("star"))
                .title_tooltip("A built-in icon from the whole reicon set")
                .accessory(Accessory::text("3").tooltip("Unread")),
            row("packaged", "Packaged image")
                .subtitle("Its @light and @dark variants follow the theme")
                .icon(Icon::image("assets/logo.png"))
                .subtitle_tooltip("logo@light.png in the light theme, logo@dark.png in the dark")
                .accessory(Accessory::date(NEW_YEAR)),
            row("pair", "Light and dark pair")
                .subtitle("A sun in the light theme, a moon in the dark")
                .icon(Icon::pair("assets/sun.svg", "assets/moon.svg"))
                .accessory(Accessory::tag("Open").color(Tone::Green)),
            row("tinted", "Tinted icon")
                .subtitle("reicon's heart in a raw colour")
                .icon(Icon::builtin("heart").tint(Color::hex("#ff6363")))
                .accessory(
                    Accessory::text("tinted")
                        .color(Tint::pair(Color::hex("#b42318"), Color::hex("#ff8a80"))),
                ),
            row("masked", "Masked image")
                .subtitle("A photo clipped to a circle")
                .icon(Icon::image("assets/photo.png").mask(Mask::Circle))
                .accessory(Accessory::of_icon(
                    Icon::builtin("user").tint(Tone::Blue).tooltip("Owner"),
                )),
            row("fallback", "Failing image")
                .subtitle("Its image is missing, so its fallback shows")
                .icon(
                    Icon::image("assets/missing.png")
                        .fallback(Icon::builtin("warning").tint(Tone::Orange))
                        .tooltip("Image missing"),
                ),
            row("avatar", "Avatar and progress")
                .subtitle("Built by the SDK's helpers")
                .icon(avatar("Ada Lovelace"))
                .accessory(Accessory::text("40%").icon(progress_ring(0.4))),
            row("crowded", "Crowded row")
                .subtitle("Five accessories, of which a row draws three")
                .accessories(["1", "2", "3", "4", "5"].map(Accessory::text)),
            row("favicon", "Favicon")
                .subtitle("The site's favicon, downloaded and cached by Pane")
                .icon(favicon(&format!("{server}/"))),
            row("slow", "Slow web image")
                .subtitle("Its fallback shows until the image arrives")
                .icon(
                    Icon::url(slow.clone()).fallback(Icon::builtin("clock").tint(Tone::Secondary)),
                ),
            row("same", "Same slow image")
                .subtitle("The same address, downloaded once")
                .icon(Icon::url(slow).fallback(Icon::builtin("clock").tint(Tone::Secondary))),
            row("broken", "Broken image")
                .subtitle("Its address has no image, so its fallback stays")
                .icon(
                    Icon::url(format!("{server}/images/missing.png"))
                        .fallback(Icon::builtin("link-broken").tint(Tone::Orange))
                        .tooltip("Image unavailable"),
                ),
            row("file", "File icon")
                .subtitle("The system's icon of a file")
                .icon(file_icon(&setting(ICON_FILE))),
            row("application", "Application icon")
                .subtitle("The system's icon of an application, drawn bare")
                .icon(file_icon(&setting(ICON_APPLICATION))),
        ]))
    }

    async fn submit_form(_item_id: String, _values: Vec<FieldValue>) -> Result<String, FormError> {
        Err(FormError {
            field: None,
            message: "The icons sample has no forms".into(),
        })
    }

    async fn open_view(_item_id: String) -> Result<CustomView, String> {
        Err("The icons sample has no custom views".into())
    }
}
