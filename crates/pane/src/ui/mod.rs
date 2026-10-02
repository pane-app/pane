//! Pane's shared visual layer: semantic tokens, frost materials, the
//! reference's icon treatment, and the shared control chrome.
//!
//! This layer owns presentation only. It imports no `pane-core` types, so
//! the visual system is usable and reviewable without launcher state, and
//! screens translate launcher data into these calls.
//!
//! ## Initialization
//!
//! [`configure`] is called once by the binary at startup, before the first
//! window opens. It reads `PANE_THEME` (`dark`, the default, or `light`)
//! and `PANE_MATERIAL` (`glass`, the default, or `opaque`) exactly once,
//! loads the embedded fonts, and fixes the [`Visuals`] every later frame
//! reuses. Nothing re-reads the environment.
//!
//! The window is not required to call it: [`visuals`] falls back to the
//! defaults (dark, opaque) on first use, so tests construct
//! `LauncherWindow` without any startup step and render the dark theme.
//!
//! Fonts: `crates/pane/assets/fonts/` (Geist-Regular/Medium/SemiBold,
//! GeistMono-Regular/Medium, `OFL.txt` — Vercel's official release v1.7.2,
//! <https://github.com/vercel/geist-font>, SIL Open Font License 1.1).
//! Icons: `crates/pane/assets/icons/*.svg`, embedded the same way (see
//! [`icon`]).

use std::borrow::Cow;
use std::sync::OnceLock;

use gpui::App;

pub(crate) mod icon;
pub(crate) mod input;
pub(crate) mod material;
pub(crate) mod result_row;
pub(crate) mod theme;

use material::{Material, MaterialMode};
use theme::{Appearance, Theme};

/// The launcher's fixed visual configuration: the theme and the material
/// every frame renders with, chosen once at startup.
pub(crate) struct Visuals {
    pub(crate) theme: Theme,
    pub(crate) material: Material,
}

static VISUALS: OnceLock<Visuals> = OnceLock::new();

/// The visuals the launcher renders with: the configuration
/// [`configure`] fixed, or the defaults (dark theme, opaque material) for
/// callers that never configured — tests, which construct the window
/// without a startup step.
pub(crate) fn visuals() -> &'static Visuals {
    VISUALS.get_or_init(|| Visuals {
        theme: Theme::dark(),
        material: Material::new(MaterialMode::Opaque),
    })
}

/// Reads `PANE_THEME` and `PANE_MATERIAL` once, loads the embedded fonts,
/// and fixes the visuals every later frame reuses. Call once, before the
/// first window opens. Unknown or missing values keep the defaults
/// (`dark`, `glass`). The chosen theme and material are stored whether or
/// not the fonts load; a font error is returned afterwards, and a caller
/// that continues past it renders the chosen appearance with the system's
/// default font wherever the theme names `Geist`.
pub(crate) fn configure(cx: &App) -> gpui::Result<()> {
    let appearance = match std::env::var("PANE_THEME").as_deref() {
        Ok("light") => Appearance::Light,
        _ => Appearance::Dark,
    };
    let mode = match std::env::var("PANE_MATERIAL").as_deref() {
        Ok("opaque") => MaterialMode::Opaque,
        _ => MaterialMode::Glass,
    };
    // The visuals are fixed first, so a font failure cannot silently
    // revert the chosen appearance to the default.
    let _ = VISUALS.set(Visuals {
        theme: Theme::new(appearance),
        // Material::new normalizes glass on frost-less platforms to the
        // opaque treatment (see material::Material::new).
        material: Material::new(mode),
    });
    load_fonts(cx)
}

/// Embeds the Geist and Geist Mono families into the text system. Call
/// once at startup, before opening a window. Returns the text system's
/// error; a caller that continues past it gets the system default font
/// wherever the theme names `Geist`.
pub(crate) fn load_fonts(cx: &App) -> gpui::Result<()> {
    let fonts: Vec<Cow<'static, [u8]>> = vec![
        Cow::Borrowed(include_bytes!("../../assets/fonts/Geist-Regular.ttf")),
        Cow::Borrowed(include_bytes!("../../assets/fonts/Geist-Medium.ttf")),
        Cow::Borrowed(include_bytes!("../../assets/fonts/Geist-SemiBold.ttf")),
        Cow::Borrowed(include_bytes!("../../assets/fonts/GeistMono-Regular.ttf")),
        Cow::Borrowed(include_bytes!("../../assets/fonts/GeistMono-Medium.ttf")),
    ];
    cx.text_system().add_fonts(fonts)
}
