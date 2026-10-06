//! Pane's shared visual layer: semantic tokens, frost materials, the
//! reference's icon treatment, and the shared control chrome — result
//! rows, keycaps, the Settings window's shell and sidebar items (see
//! [`settings_shell`]) and the Settings controls (see [`controls`]) —
//! plus the motion policy (see [`motion`]) that the launcher's subtle
//! transitions share.
//!
//! This layer owns presentation only. It imports no `pane-core` types, so
//! the visual system is usable and reviewable without launcher state, and
//! screens translate launcher data into these calls.
//!
//! ## Where the visuals come from
//!
//! The theme and material every frame renders with are chosen by the
//! application's host settings — [`crate::settings::visuals`] — which
//! resolve the user's recorded preferences (with the system's appearance,
//! where the preference follows it, and the platform's normalization of
//! glass to solid) into a [`Visuals`]. Nothing visual reads the
//! environment or decides a preference: `Theme` stays the palette
//! construction, `Material` the surface treatment, and this layer holds no
//! state of its own between frames.
//!
//! The binary embeds the fonts once at startup through [`load_fonts`];
//! a window built without that step renders the same theme with the
//! system's default font wherever the theme names `Geist`.
//!
//! Fonts: `crates/pane/assets/fonts/` (Geist-Regular/Medium/SemiBold,
//! GeistMono-Regular/Medium, `OFL.txt` — Vercel's official release v1.7.2,
//! <https://github.com/vercel/geist-font>, SIL Open Font License 1.1).
//! Icons: `crates/pane/assets/icons/<set>/*.svg` (reicon 1.2.5,
//! <https://reicon.dev>, MIT, or Pane's own set), embedded the same way
//! (see [`icon`]). Extensions' icons (#139) — the whole reicon set by name,
//! their packaged images — are drawn by [`extension_icon`] from what the
//! caller resolved.

use std::borrow::Cow;

use gpui::App;

pub(crate) mod contrast;
pub(crate) mod controls;
pub(crate) mod extension_icon;
pub(crate) mod footer;
pub(crate) mod icon;
pub(crate) mod input;
pub(crate) mod keycap;
pub(crate) mod material;
pub(crate) mod motion;
pub(crate) mod pinned;
pub(crate) mod result_layouts;
pub(crate) mod result_row;
pub(crate) mod select;
pub(crate) mod settings_shell;
pub(crate) mod shell;
pub(crate) mod split_view;
pub(crate) mod theme;
pub(crate) mod tooltip;

use material::Material;
use theme::Theme;

/// The visuals a window renders with: the theme and the material the
/// host settings resolve to. Owned by the caller (the settings entity
/// recomputes it on every change); this layer only consumes it.
#[derive(Clone)]
pub(crate) struct Visuals {
    /// The palette every frame paints with.
    pub(crate) theme: Theme,
    /// The surface treatment every frame paints on.
    pub(crate) material: Material,
    /// The launcher's background image, baked, when one is chosen and
    /// ready (ADR 0028): only the launcher's visuals carry it, and their
    /// theme is then the palette over it (`Theme::over_backdrop`).
    pub(crate) backdrop: Option<crate::background::Backdrop>,
}

/// Embeds the Geist and Geist Mono families into the text system. Call
/// once at startup, before opening a window. Returns the text system's
/// error; a caller that continues past it gets the system default font
/// wherever the theme names `Geist`.
pub(crate) fn load_fonts(cx: &App) -> gpui::Result<()> {
    let fonts: Vec<Cow<'static, [u8]>> = FONTS
        .iter()
        .map(|&(_, bytes)| Cow::Borrowed(bytes))
        .collect();
    cx.text_system().add_fonts(fonts)
}

/// The embedded font files, by name.
pub(crate) const FONTS: &[(&str, &[u8])] = &[
    (
        "Geist-Regular.ttf",
        include_bytes!("../../assets/fonts/Geist-Regular.ttf"),
    ),
    (
        "Geist-Medium.ttf",
        include_bytes!("../../assets/fonts/Geist-Medium.ttf"),
    ),
    (
        "Geist-SemiBold.ttf",
        include_bytes!("../../assets/fonts/Geist-SemiBold.ttf"),
    ),
    (
        "GeistMono-Regular.ttf",
        include_bytes!("../../assets/fonts/GeistMono-Regular.ttf"),
    ),
    (
        "GeistMono-Medium.ttf",
        include_bytes!("../../assets/fonts/GeistMono-Medium.ttf"),
    ),
];
