//! Frost materials: the panel and footer surfaces, and the window
//! background appearance they assume.
//!
//! Two different blurs exist and this module keeps them apart:
//!
//! - **Desktop (compositor) frost** — [`MaterialMode::window_appearance`]
//!   asks the platform for a blurred window background (Windows acrylic via
//!   DWM composition, macOS vibrancy). It is set once at window creation
//!   and blurs whatever is *behind the window*. This is the launcher's
//!   glass. It is never simulated: no wallpaper is drawn inside the app.
//! - **In-scene frost** — GPUI's `Styled::backdrop_blur(radius)` blurs
//!   content *inside* the window, behind an element (a future popover over
//!   the list). This slice uses none of it; the seam is noted so later
//!   overlays know the difference.
//!
//! Failure honesty: whether the compositor actually applied the blur is not
//! observable from the application — GPUI exposes no query for it. On
//! Windows startup checks the supported build, transparency preference and
//! high-contrast setting; a suppressed or unknown state selects opaque.
//! These checks cannot establish that composition succeeded. Glass paints its tint
//! (the dark panel at the reference's .7 alpha, the light panel at its own
//! higher tint), which puts the panel's own content on a consistent plate
//! even where the blur silently failed — but `Glass` does not promise
//! blur, the tint is not a readability guarantee, and this module reports
//! no success.
//!
//! One consistent strategy: `Glass` on a platform without compositor frost
//! (Linux, and any other non-Windows/macOS target) normalizes to opaque at
//! construction — opaque window *and* solid panel, the same as
//! [`MaterialMode::Opaque`], never a glass tint over an unblurred desktop.
//! [`MaterialMode::Opaque`] everywhere is the explicit deterministic
//! fallback: an opaque window and the solid panel.

use gpui::prelude::*;
use gpui::{
    BoxShadow, Div, WindowBackgroundAppearance, div, linear_color_stop, linear_gradient, px,
    relative, solid_background, transparent_black,
};

use crate::ui::theme::Theme;

/// Which surface treatment the launcher uses. The integration reads
/// `PANE_MATERIAL` once and constructs this; settings are sampled at startup,
/// with no claim that the compositor delivered blur. Constructing
/// normalizes: `Glass` where the platform has no compositor frost becomes
/// `Opaque` (see [`Material::new`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MaterialMode {
    /// A translucent panel over a compositor-blurred window where the
    /// platform supports it; a translucent panel over a plain window where
    /// it does not (see the module docs).
    Glass,
    /// A solid, fully opaque panel: opaque window and solid panel, the
    /// deterministic fallback for smokes, screenshots and unsupported
    /// platforms.
    Opaque,
}

/// Whether the platform can plausibly back a `Glass` window: compositor
/// frost exists on Windows and macOS in GPUI; on Linux there is no
/// guaranteed blur, so glass normalizes to opaque.
fn compositor_frost_supported() -> bool {
    cfg!(any(target_os = "windows", target_os = "macos"))
}

/// Read preferences, not compositor success. Fail closed to a solid surface
/// when the OS cannot answer. No system settings are changed or monitored.
#[cfg(target_os = "windows")]
fn windows_glass_allowed() -> bool {
    use windows::UI::ViewManagement::{AccessibilitySettings, UISettings};
    use windows::Wdk::System::SystemServices::RtlGetVersion;
    use windows::Win32::System::SystemInformation::OSVERSIONINFOW;

    let mut version = OSVERSIONINFOW {
        dwOSVersionInfoSize: std::mem::size_of::<OSVERSIONINFOW>() as u32,
        ..Default::default()
    };
    // This is the same minimum build as CE's acrylic implementation.
    // SAFETY: version is a correctly sized, writable OSVERSIONINFOW.
    if !unsafe { RtlGetVersion(&mut version) }.is_ok() || version.dwBuildNumber < 17763 {
        return false;
    }
    let effects = UISettings::new().and_then(|settings| settings.AdvancedEffectsEnabled());
    let contrast = AccessibilitySettings::new().and_then(|settings| settings.HighContrast());
    effects.unwrap_or(false) && !contrast.unwrap_or(true)
}

impl MaterialMode {
    /// The window background appearance to open the window with: `Glass`
    /// asks for a blurred background on the frost-capable platforms, and
    /// both modes are opaque everywhere else.
    pub(crate) fn window_appearance(&self) -> WindowBackgroundAppearance {
        match self {
            MaterialMode::Glass if compositor_frost_supported() => {
                WindowBackgroundAppearance::Blurred
            }
            _ => WindowBackgroundAppearance::Opaque,
        }
    }
}

/// The launcher's surfaces. Construct once from the material mode and reuse
/// across frames; it holds no state beyond the mode.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Material {
    mode: MaterialMode,
}

impl Material {
    /// The material for `mode`, normalized: `Glass` on a platform without
    /// compositor frost (Linux and others) becomes `Opaque` — the opaque
    /// window and the solid panel, never a glass tint over an unblurred
    /// desktop.
    pub(crate) fn new(mode: MaterialMode) -> Material {
        let mode = match mode {
            MaterialMode::Glass if !compositor_frost_supported() => MaterialMode::Opaque,
            #[cfg(target_os = "windows")]
            MaterialMode::Glass if !windows_glass_allowed() => MaterialMode::Opaque,
            mode => mode,
        };
        Material { mode }
    }

    /// The window background appearance (see [`MaterialMode::window_appearance`]).
    pub(crate) fn window_appearance(&self) -> WindowBackgroundAppearance {
        self.mode.window_appearance()
    }

    /// The L1 panel: the reference's glass surface with `content` laid out
    /// over the top sheen. Draw order, inside out: the tint (or the solid
    /// fallback), the sheen fading out over the top 36%, then `content`.
    /// The sheen is a plain non-interactive layer, so it neither swallows
    /// pointer events nor takes focus.
    ///
    /// The panel clips its content (the reference rounds and clips the
    /// glass section), so `content` should size itself to fill —
    /// `.size_full().flex().flex_col()` is the expected shape. The
    /// panel fills the window. GPUI drop shadows also paint under its
    /// interior, so stacking the reference's outer shadows here would
    /// obscure the desktop through the translucent fill. Keep the inset
    /// highlight and border; the native window owns the outside shadow.
    pub(crate) fn panel(&self, theme: &Theme, content: impl IntoElement) -> Div {
        let background = match self.mode {
            MaterialMode::Glass => solid_background(theme.panel_tint),
            MaterialMode::Opaque => solid_background(theme.panel_solid),
        };
        let geometry = &theme.geometry;
        div()
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .rounded(geometry.panel_radius)
            .bg(background)
            // The reference's inner edge is an inset 1px box-shadow ring;
            // a 1px border at the same color is the same line.
            .border_1()
            .border_color(theme.hairline)
            .shadow(vec![
                BoxShadow::new(px(0.), px(1.), theme.panel_top_highlight).inset(),
            ])
            // The sheen paints beneath the content: earlier child, and a
            // plain div, so it never intercepts input.
            .child(div().absolute().size_full().bg(linear_gradient(
                180.,
                linear_color_stop(theme.panel_sheen, 0.),
                linear_color_stop(transparent_black(), 0.36),
            )))
            .child(content)
    }

    /// The footer strip: the reference's 50px status bar over the panel,
    /// with its translucent wash and top divider. Content (the status
    /// line) is added by the caller, as a column child: the text wraps at
    /// the strip's width (a long message is never one clipped line), the
    /// strip grows past its 50px floor with the wrapped text, and growth
    /// stops at 35% of the panel — past that the strip itself scrolls, so
    /// a long error can neither consume the list nor lose its tail, the
    /// way the launcher's auto-height status line behaved before it was
    /// styled. The strip is a column with no vertical centering, so the
    /// first line stays reachable when the content scrolls.
    pub(crate) fn footer(theme: &Theme) -> Div {
        div()
            .flex_none()
            .flex()
            .flex_col()
            .min_h(theme.geometry.footer_height)
            .max_h(relative(0.35))
            .px(px(16.))
            .bg(theme.footer_tint)
            .border_t_1()
            .border_color(theme.hairline_soft)
    }
}
