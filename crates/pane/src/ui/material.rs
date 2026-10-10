//! Frost materials: the panel and footer surfaces, and the window
//! background appearance they assume.
//!
//! Two different blurs exist and this module keeps them apart:
//!
//! - **Desktop (compositor) frost** — [`MaterialMode::window_appearance`]
//!   asks the platform for a blurred window background (Windows acrylic via
//!   DWM composition, macOS vibrancy). It is set at window creation and
//!   re-applied as the material changes, and blurs whatever is *behind the
//!   window*. This is the launcher's glass. It is never simulated: no copy
//!   of the desktop's wallpaper is drawn inside the app. A background
//!   image the user chooses for the launcher (ADR 0028) is not glass
//!   either: it is the user's own picture ([`Material::panel_over`]),
//!   drawn translucent over the glass panel so the frost still shows
//!   through, and on the opaque panel otherwise.
//! - **In-scene frost** — GPUI's `Styled::backdrop_blur(radius)` blurs
//!   content *inside* the window, behind an element. The L2 popover
//!   ([`Material::popover`]) uses it in glass mode, blurring the list
//!   behind the launcher footer's menu the way the reference's `.pop`
//!   blurs the page behind it; the window's own glass is never simulated
//!   with it. Over a background image the frosted surfaces use it too
//!   (`Theme::frost`).
//!
//! Failure honesty: whether the compositor actually applied the blur is not
//! observable from the application — GPUI exposes no query for it. On
//! Windows the checks read the supported build, transparency preference
//! and high-contrast setting; a suppressed or unknown state selects opaque
//! (see [`glass_fallback_reason`], which the appearance page shows where it
//! applies). These checks cannot establish that composition succeeded.
//! Glass paints its tint (the dark panel at the reference's .7 alpha, the
//! light panel at its own higher tint), which puts the panel's own content
//! on a consistent plate even where the blur silently failed — but `Glass`
//! does not promise blur, the tint is not a readability guarantee, and this
//! module reports no success.
//!
//! One consistent strategy: `Glass` on a platform without compositor frost
//! (Linux, and any other non-Windows/macOS target) normalizes to opaque at
//! construction — opaque window *and* solid panel, the same as
//! [`MaterialMode::Opaque`], never a glass tint over an unblurred desktop.
//! [`MaterialMode::Opaque`] everywhere is the explicit deterministic
//! fallback: an opaque window and the solid panel.

use gpui::prelude::*;
use gpui::{
    AnyElement, BoxShadow, Div, Hsla, Pixels, WindowBackgroundAppearance, div, linear_color_stop,
    linear_gradient, px, relative, solid_background, transparent_black,
};

use crate::ui::theme::Theme;

/// Which surface treatment the launcher uses. The host settings hold the
/// user's preference and construct this from it, as the Appearance page
/// changes it; a `Glass` preference does not claim the compositor
/// delivered blur (see the module docs). Constructing normalizes: `Glass`
/// where the platform has no compositor frost becomes `Opaque` (see
/// [`Material::new`]).
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

/// Why a `Glass` request normalizes to the solid surface here, if it does:
/// this platform does not expose compositor frost, or Windows' own
/// protections fail (its transparency preference, its high-contrast mode,
/// or a build older than the acrylic API). `None` when a glass request
/// stands — which is still not proof the compositor blurred anything, only
/// that nothing suppresses the request (see the module docs). The
/// appearance page shows this reason where glass is chosen but not in
/// effect, so the difference between the preference and the surface is
/// visible rather than silent.
pub(crate) fn glass_fallback_reason() -> Option<&'static str> {
    if !compositor_frost_supported() {
        return Some("this platform does not expose compositor frost behind a window");
    }
    #[cfg(target_os = "windows")]
    if !windows_glass_allowed() {
        return Some(
            "Windows has transparency turned off or high contrast on, or this build of Windows \
             predates the acrylic blur",
        );
    }
    None
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

/// The reference's inset edges, as one box shadow list: the 1px ring
/// (`inset 0 0 0 1px edge`) and the top inset line (`inset 0 1px 0 top`).
/// Box shadows take no layout space, so — unlike a border — the ring
/// leaves the content's box the surface's full size, as the reference's
/// `box-shadow` does: a 760px panel lays its header, list and footer out
/// across all 760px.
fn inset_edges(edge: Hsla, top: Hsla) -> Vec<BoxShadow> {
    vec![
        BoxShadow::new(px(0.), px(0.), edge)
            .spread_radius(px(1.))
            .inset(),
        BoxShadow::new(px(0.), px(1.), top).inset(),
    ]
}

/// The launcher's surfaces. Construct once from the material mode and reuse
/// across frames; it holds no state beyond the mode.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Material {
    mode: MaterialMode,
}

impl Material {
    /// The material for `mode`, normalized: `Glass` on a platform where
    /// [`glass_fallback_reason`] names a reason becomes `Opaque` — the
    /// opaque window and the solid panel, never a glass tint over an
    /// unblurred desktop.
    pub(crate) fn new(mode: MaterialMode) -> Material {
        let mode = match mode {
            MaterialMode::Glass if glass_fallback_reason().is_some() => MaterialMode::Opaque,
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
    /// ring and top highlight; the native window owns the outside shadow.
    ///
    /// The ring and highlight are inset box shadows, as the reference's
    /// are, not a border: they paint over the panel's own fill and under
    /// its content, and take no layout space, so the content spans the
    /// whole panel (see [`inset_edges`]). Content that paints a fill to
    /// the panel's edge (the footer's wash) covers them there, as it does
    /// in the reference.
    ///
    /// On Windows the corner and the outside shadow are the window's, not
    /// the panel's: the Desktop Window Manager rounds the window (its
    /// corner preference, documented at 8px — the reference curves at 18)
    /// and draws its own shadow in place of the reference's two. The
    /// acrylic covers the whole window rectangle, so an 18px curve painted
    /// here would show it as a plate behind the curve (#92).
    pub(crate) fn panel(&self, theme: &Theme, content: impl IntoElement) -> Div {
        self.tinted_panel(theme, theme.panel_tint, content)
    }

    /// [`Material::panel`] with `under` laid between the surface and
    /// `content`: the launcher's background image (ADR 0028), which paints
    /// over the panel's fill and sheen and under everything the launcher
    /// shows. `under` places itself (absolutely) and takes no input.
    pub(crate) fn panel_over(
        &self,
        theme: &Theme,
        under: impl IntoElement,
        content: impl IntoElement,
    ) -> Div {
        let shape = (theme.geometry.panel_radius, theme.hairline);
        self.l1_surface(
            theme,
            theme.panel_tint,
            shape,
            Some(under.into_any_element()),
            content,
        )
        .size_full()
    }

    /// The Settings window's L1 panel: [`Material::panel`]'s surface at
    /// the Settings board's own glass tint, `.78` against the root's
    /// `.70` ([`Theme::settings_tint`]). The solid fallback, the sheen and
    /// the inset edges are the panel's.
    pub(crate) fn settings_panel(&self, theme: &Theme, content: impl IntoElement) -> Div {
        self.tinted_panel(theme, theme.settings_tint, content)
    }

    /// Whether this material paints the glass tint, rather than the solid
    /// surface: what the Appearance preview shows is in effect.
    pub(crate) fn is_glass(&self) -> bool {
        self.mode == MaterialMode::Glass
    }

    /// The L1 panel with `tint` as its glass (see [`Material::panel`]).
    fn tinted_panel(&self, theme: &Theme, tint: Hsla, content: impl IntoElement) -> Div {
        let shape = (theme.geometry.panel_radius, theme.hairline);
        self.l1_surface(theme, tint, shape, None, content)
            .size_full()
    }

    /// The L1 surface: `tint` as its glass (the solid panel otherwise),
    /// rounded and ringed as `shape` (its radius, and its inset edge's
    /// color) says, under the panel's top highlight, with the sheen
    /// beneath `content`.
    fn l1_surface(
        &self,
        theme: &Theme,
        tint: Hsla,
        (radius, edge): (Pixels, Hsla),
        under: Option<AnyElement>,
        content: impl IntoElement,
    ) -> Div {
        let background = match self.mode {
            MaterialMode::Glass => solid_background(tint),
            MaterialMode::Opaque => solid_background(theme.panel_solid),
        };
        div()
            .relative()
            .flex()
            .flex_col()
            .overflow_hidden()
            .rounded(radius)
            .bg(background)
            .shadow(inset_edges(edge, theme.panel_top_highlight))
            // The sheen paints beneath the content: earlier child, and a
            // plain div, so it never intercepts input.
            .child(div().absolute().size_full().bg(linear_gradient(
                180.,
                linear_color_stop(theme.panel_sheen, 0.),
                linear_color_stop(transparent_black(), 0.36),
            )))
            .children(under)
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
    ///
    /// Its padding is the reference's: 16 on the left, 8 on the right,
    /// where the strip's buttons carry their own padding. Over a
    /// background image the strip is frosted: it blurs what scrolls
    /// behind it (see [`Theme::over_backdrop`]).
    pub(crate) fn footer(theme: &Theme) -> Div {
        let geometry = &theme.geometry;
        div()
            .flex_none()
            .flex()
            .flex_col()
            .min_h(geometry.footer_height)
            .max_h(relative(0.35))
            .pl(geometry.footer_padding_left)
            .pr(geometry.footer_padding_right)
            .when_some(theme.frost, |footer, frost| {
                footer.backdrop_blur(frost.blur)
            })
            .bg(theme.footer_tint)
            .border_t_1()
            .border_color(theme.hairline_soft)
    }

    /// The L2 popover: the reference's `.pop` surface, for a small floating
    /// layer over the panel (the launcher footer's menu). Draw order,
    /// inside out: the tint (or the solid fallback), the sheen fading out
    /// over the top 40%, then `content`. Like the panel, the surface owns
    /// the tint-or-solid choice, clips its content, and keeps the sheen a
    /// plain non-interactive layer.
    ///
    /// Two differences from the panel, both stated by the reference and
    /// GPUI's painting order:
    ///
    /// - In glass mode the popover blurs what is behind it *inside the
    ///   window* (the reference's `backdrop-filter: blur(30px)`), which
    ///   works on every platform — unlike the window's own frost. GPUI has
    ///   no saturation filter, so the reference's `saturate(160%)` is
    ///   dropped rather than approximated.
    /// - The reference's outer shadows are not painted here: GPUI paints a
    ///   non-inset shadow under the element's own translucent fill, where
    ///   it would show through the tint. A caller that wants the reference's
    ///   elevation wraps this surface in a plain `relative` div carrying
    ///   the shadow, so it paints behind the surface (see the footer menu).
    pub(crate) fn popover(&self, theme: &Theme, content: impl IntoElement) -> Div {
        let geometry = &theme.geometry;
        let (background, blur) = match self.mode {
            MaterialMode::Glass => (solid_background(theme.popover_tint), px(30.)),
            MaterialMode::Opaque => (solid_background(theme.popover_solid), px(0.)),
        };
        div()
            .relative()
            .flex()
            .flex_col()
            .overflow_hidden()
            .rounded(geometry.popover_radius)
            .when(blur > px(0.), |surface| surface.backdrop_blur(blur))
            .bg(background)
            // The reference's inset edge (`inset 0 0 0 1px`) and top inset
            // (`inset 0 1px 0`), as the panel renders its own: layout-free.
            .shadow(inset_edges(theme.popover_edge, theme.popover_top_highlight))
            // The sheen paints beneath the content, as on the panel.
            .child(div().absolute().size_full().bg(linear_gradient(
                180.,
                linear_color_stop(theme.popover_sheen, 0.),
                linear_color_stop(transparent_black(), 0.4),
            )))
            .child(content)
    }
}

/// A popover's outer shadows (the reference's `.pop`), for the element
/// that carries the popover: its 0.5px dark outline and its long soft
/// drop. GPUI paints them outside the popover's own clip, so they belong
/// to a wrapper around it (the Pane menu's, the Actions panel's).
pub(crate) fn popover_shadows(theme: &Theme) -> Vec<BoxShadow> {
    let geometry = &theme.geometry;
    vec![
        BoxShadow::new(px(0.), px(0.), theme.popover_outline)
            .spread_radius(geometry.popover_outline_width),
        BoxShadow::new(px(0.), geometry.popover_drop_offset, theme.popover_drop)
            .blur_radius(geometry.popover_drop_blur)
            .spread_radius(geometry.popover_drop_spread),
    ]
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    /// The macOS leg of the material policy, beside the Windows one in
    /// `windows_glass_allowed`: no suppression check fails a glass request
    /// here, so glass asks for the blurred window appearance and stays
    /// glass. The vibrancy the window then vends is the fork's
    /// `UnderWindowBackground` selection, covered by the fork's own macOS
    /// tests (see `docs/gpui-fork.md`); this test pins the app-side policy.
    #[test]
    fn a_glass_request_stands_and_blurs_the_window_on_macos() {
        assert!(
            glass_fallback_reason().is_none(),
            "nothing on macOS suppresses a glass request"
        );
        assert_eq!(
            MaterialMode::Glass.window_appearance(),
            WindowBackgroundAppearance::Blurred
        );
        assert!(Material::new(MaterialMode::Glass).is_glass());
    }
}
