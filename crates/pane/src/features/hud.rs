//! The HUD (#141, ADR 0037): a short message in a small window of its own,
//! which a command shows once it has acted ("Copied to Clipboard"), and
//! which a toast becomes while the launcher is hidden or collapsed.
//!
//! The launcher closes itself first (`pane_core::Launcher` hides it
//! through the window seam), then the launcher window opens the HUD's
//! window: a pop-up that never takes the focus, over other applications,
//! near the bottom centre of the display the launcher was on, and, where
//! the system allows, click-through (`crate::make_click_through`). It
//! stays for `Hud::duration` — 1.2 seconds, or 3 for a failure (ADR
//! 0035) — and a new HUD replaces one still shown. On macOS and Linux the
//! same window is used where the system allows a window that does not
//! activate; Wayland places it as it places any window, so there it reads
//! as a toast-like message (the specification's fallback). How it looks
//! is refined by "Launcher polish" (#123).

use gpui::{
    App, Bounds, Context, IntoElement, Pixels, Render, Size, TitlebarOptions, Window,
    WindowBackgroundAppearance, WindowBounds, WindowHandle, WindowKind, WindowOptions, div, point,
    prelude::*, px, size,
};
use pane_core::{Hud, ToastStyle};

use crate::app::LauncherWindow;

/// How far above the display's bottom edge the HUD sits, as a share of
/// the display's height.
const ABOVE_BOTTOM: f32 = 0.12;

/// The HUD's window's title: never drawn, it names the window to the
/// system (the native smoke, scripts/smoke-windows-hud.ps1, finds it by
/// it).
pub(crate) const TITLE: &str = "Pane HUD";

/// The HUD's height, in logical pixels.
const HEIGHT: f32 = 44.;

/// The narrowest and the widest the HUD is, in logical pixels.
const WIDTH: (f32, f32) = (160., 560.);

/// The HUD's window's view: its message, in its style.
pub(crate) struct HudView {
    hud: Hud,
}

impl Render for HudView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = crate::settings::visuals(cx).theme;
        let dot = match self.hud.style {
            ToastStyle::Animated => theme.warning,
            ToastStyle::Success => theme.success,
            ToastStyle::Failure => theme.danger,
        };
        div()
            .id("hud")
            .debug_selector(|| "hud".into())
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .gap_2()
            .px_4()
            .rounded(px(HEIGHT / 2.))
            .bg(theme.popover_solid)
            .border_1()
            .border_color(theme.popover_edge)
            .font_family(theme.typography.family.clone())
            .text_size(theme.typography.footer_size)
            .text_color(theme.text_title)
            .child(div().flex_none().size(px(8.)).rounded_full().bg(dot))
            .child(
                div()
                    .debug_selector(|| "hud-title".into())
                    .min_w(px(0.))
                    .truncate()
                    .child(self.hud.title.clone()),
            )
    }
}

/// The HUD's window, while one is open.
#[derive(Default)]
pub(crate) struct HudWindow {
    window: Option<WindowHandle<HudView>>,
    /// Counts the HUDs shown, so the timer of one a newer HUD replaced
    /// closes nothing.
    shown: u64,
    /// What the open HUD says, for tests.
    title: Option<String>,
}

/// Where the HUD of `size` goes on the display whose bounds are
/// `display`: centred, near the bottom.
pub(crate) fn placed(display: Bounds<Pixels>, size: Size<Pixels>) -> Bounds<Pixels> {
    let x = display.origin.x + (display.size.width - size.width) / 2.;
    let y =
        display.origin.y + display.size.height - size.height - display.size.height * ABOVE_BOTTOM;
    Bounds {
        origin: point(x, y),
        size,
    }
}

/// The HUD's size for `title`: wide enough for its text, within
/// [`WIDTH`].
fn size_for(title: &str) -> Size<Pixels> {
    let width = (title.chars().count() as f32 * 7.5 + 64.).clamp(WIDTH.0, WIDTH.1);
    size(px(width), px(HEIGHT))
}

impl LauncherWindow {
    /// Shows `hud` in its own window on the display the launcher was on,
    /// replacing the HUD still shown, and closes it after its duration.
    pub(crate) fn show_hud(&mut self, hud: Hud, window: &mut Window, cx: &mut Context<Self>) {
        self.close_hud(cx);
        let size = size_for(&hud.title);
        let display = window.display(cx);
        let bounds = match &display {
            Some(display) => placed(display.bounds(), size),
            None => Bounds::centered(None, size, cx),
        };
        // Named on Windows, where a pop-up draws no title bar; elsewhere a
        // title bar option could draw one.
        let titlebar = cfg!(target_os = "windows").then(|| TitlebarOptions {
            title: Some(TITLE.into()),
            appears_transparent: true,
            ..TitlebarOptions::default()
        });
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar,
            focus: false,
            show: true,
            kind: WindowKind::PopUp,
            is_movable: false,
            is_resizable: false,
            is_minimizable: false,
            display_id: display.as_ref().map(|display| display.id()),
            window_background: WindowBackgroundAppearance::Transparent,
            ..WindowOptions::default()
        };
        let duration = hud.duration();
        let title = hud.title.clone();
        let opened = cx.open_window(options, |window, cx| {
            crate::make_click_through(window);
            cx.new(|_| HudView { hud })
        });
        let handle = match opened {
            Ok(handle) => handle,
            Err(error) => {
                eprintln!("Pane could not show a HUD: {error:#}");
                return;
            }
        };
        self.hud.shown += 1;
        self.hud.window = Some(handle);
        self.hud.title = Some(title);
        let shown = self.hud.shown;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(duration).await;
            this.update(cx, |this, cx| {
                if this.hud.shown == shown {
                    this.close_hud(cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// Closes the HUD's window, if one is open.
    pub(crate) fn close_hud(&mut self, cx: &mut App) {
        self.hud.title = None;
        if let Some(handle) = self.hud.window.take() {
            handle
                .update(cx, |_, window, _| window.remove_window())
                .ok();
        }
    }

    /// Test support: what the HUD's window shows while it is open; `None`
    /// while no HUD shows. Test and debug builds only.
    #[cfg(any(test, debug_assertions))]
    #[doc(hidden)]
    pub fn hud(&self) -> Option<String> {
        self.hud.title.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hud_sits_centred_near_the_bottom_of_its_display() {
        let display = Bounds {
            origin: point(px(1920.), px(0.)),
            size: size(px(1000.), px(800.)),
        };
        let hud = placed(display, size(px(200.), px(44.)));
        assert_eq!(hud.origin.x, px(1920. + 400.));
        assert_eq!(hud.origin.y, px(800. - 44. - 96.));
        assert_eq!(hud.size, size(px(200.), px(44.)));
    }

    #[test]
    fn a_hud_is_as_wide_as_its_title_within_bounds() {
        assert_eq!(size_for("Hi").width, px(WIDTH.0));
        assert_eq!(size_for(&"x".repeat(500)).width, px(WIDTH.1));
        let copied = size_for("Copied to Clipboard").width;
        assert!(copied > px(WIDTH.0) && copied < px(WIDTH.1), "{copied:?}");
    }
}
