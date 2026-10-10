//! The HUD (#141, ADR 0037): a short message in a small window of its
//! own, which a command shows once it has acted ("Copied to Clipboard"),
//! and which a toast becomes while the launcher is hidden or collapsed.
//!
//! The launcher closes itself first (`pane_core::Launcher` hides it
//! through the window seam), then the launcher window opens the HUD's
//! window: a pop-up that never takes the focus, over other applications,
//! click-through where the system allows (`crate::make_click_through`),
//! on the display the launcher last showed on — centred horizontally,
//! its bottom edge 150 logical pixels above that display's bottom, its
//! height 46 logical pixels, 56 with a message, its width what its
//! content needs, at most 500 ("Launcher polish", #123). It draws an
//! optional icon, a one-line title and an optional one-line message in
//! Pane's popover material and type.
//!
//! A default or success HUD shows for 1.2 seconds, a failure for 3
//! (ADR 0035), then fades out over about a second — no fade under
//! reduced motion, which closes it at once. A pending HUD (work in
//! progress) stays until it is updated, which replaces it as any newer
//! HUD does, or until the launcher is active again. One at a time: a new
//! HUD replaces the one shown.
//!
//! The HUD's text is announced as the launcher's announcer is (#132): a
//! hidden live region in the window's own accessibility tree, which
//! Windows announces through UI Automation — a live region's change is
//! raised for an unfocused window too, so a screen reader hears a
//! confirmation from a hidden launcher.
//!
//! On macOS and Linux the same window is used where the system allows a
//! window that does not activate. Wayland places it as it places any
//! window, a client cannot put it where the specification has it, so
//! there it reads as a toast-like message (the specification's fallback;
//! see docs/platforms/linux.md).

use std::time::{Duration, Instant};

use gpui::accesskit::Live;
use gpui::{
    App, Bounds, Context, IntoElement, Pixels, Render, Role, Size, TitlebarOptions, Window,
    WindowBackgroundAppearance, WindowBounds, WindowHandle, WindowKind, WindowOptions, div, point,
    prelude::*, px, size,
};
use pane_core::Hud;

use crate::app::LauncherWindow;

/// How far above the display's bottom edge the HUD's bottom edge sits,
/// in logical pixels.
const ABOVE_BOTTOM: f32 = 150.;

/// The HUD's height, in logical pixels: 46, or 56 with a message.
const HEIGHT: (f32, f32) = (46., 56.);

/// The narrowest and the widest the HUD is, in logical pixels: its width
/// follows its content between them.
const WIDTH: (f32, f32) = (160., 500.);

/// How long the HUD fades out over, once its time is up: about a second.
const FADE: Duration = Duration::from_secs(1);

/// How wide the HUD's text is estimated per character, for its width.
const CHAR: f32 = 7.5;

/// The HUD's fixed width, in logical pixels: the padding of its content
/// and the dot that says its style.
const CHROME: f32 = 56.;

/// The gap between the HUD's dot, icon, title and message, in logical
/// pixels.
const GAP: f32 = 8.;

/// The HUD's icon's size, in logical pixels.
const ICON: f32 = 18.;

/// The HUD's window's title: never drawn, it names the window to the
/// system (the native smoke, scripts/smoke-windows-hud.ps1, finds it by
/// it).
pub(crate) const TITLE: &str = "Pane HUD";

/// The HUD's window's view: its icon, title and message, in its style.
pub(crate) struct HudView {
    hud: Hud,
    /// When the fade out began, if it has: the view draws fainter the
    /// further it runs, and is closed once it has.
    fading: Option<Instant>,
}

impl Render for HudView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = crate::settings::visuals(cx).theme;
        let (style, dot) = super::toast::style_look(self.hud.style, &theme);
        let icon = self
            .hud
            .icon
            .as_ref()
            .map(|icon| super::icons::drawn(icon, &theme));
        let title = self.hud.title.clone();
        let message = self.hud.message.clone();
        // The fade out: one frame is drawn as each of its moments is
        // reached, until the window is closed at its end.
        let opacity = match self.fading {
            Some(start) => {
                let elapsed = cx
                    .background_executor()
                    .now()
                    .saturating_duration_since(start);
                if elapsed < FADE {
                    window.request_animation_frame();
                }
                (1. - elapsed.as_secs_f32() / FADE.as_secs_f32()).clamp(0., 1.)
            }
            None => 1.,
        };
        let height = if message.is_some() {
            HEIGHT.1
        } else {
            HEIGHT.0
        };
        let text = announced(&self.hud);
        div()
            .id("hud")
            .debug_selector(|| "hud".into())
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(GAP))
            .px_4()
            .rounded(px(height / 2.))
            .bg(theme.popover_solid)
            .border_1()
            .border_color(theme.popover_edge)
            .font_family(theme.typography.family.clone())
            .text_size(theme.typography.footer_size)
            .text_color(theme.text_title)
            .opacity(opacity)
            .child(
                // The title's line: the style's dot, the icon, the title.
                div()
                    .flex()
                    .items_center()
                    .gap(px(GAP))
                    .child(
                        div()
                            .debug_selector(move || style.into())
                            .flex_none()
                            .size(px(super::toast::DOT))
                            .rounded_full()
                            .bg(dot),
                    )
                    .when_some(icon, |line, icon| {
                        line.child(crate::ui::extension_icon::draw(
                            &icon,
                            crate::ui::extension_icon::IconSize::small(px(ICON)),
                            "hud-icon",
                            "hud",
                            &theme,
                        ))
                    })
                    .child(
                        div()
                            .debug_selector(|| "hud-title".into())
                            .min_w(px(0.))
                            .truncate()
                            .child(title),
                    ),
            )
            // The message is the second line, in the muted body ink.
            .when_some(message, |hud, message| {
                hud.child(
                    div()
                        .debug_selector(|| "hud-message".into())
                        .min_w(px(0.))
                        .truncate()
                        .text_color(theme.text_muted)
                        .child(message),
                )
            })
            // The live region the HUD's text is announced through: hidden,
            // as the launcher's announcer is (#132).
            .child(
                div()
                    .id("hud-announcer")
                    .absolute()
                    .w(px(0.))
                    .h(px(0.))
                    .role(Role::Label)
                    .aria_live(Live::Polite)
                    .aria_live_atomic(true)
                    .when(!text.is_empty(), |node| {
                        node.aria_label(text.clone()).aria_value(text)
                    }),
            )
    }
}

/// What the HUD announces: its title, with its message after it.
fn announced(hud: &Hud) -> String {
    match &hud.message {
        Some(message) if !message.trim().is_empty() => format!("{}: {message}", hud.title),
        _ => hud.title.clone(),
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
    /// Whether the open HUD is pending: it stays until it is updated or
    /// the launcher is active again.
    pending: bool,
}

/// Where the HUD of `size` goes on the display whose bounds are
/// `display`: centred, its bottom edge 150 logical pixels above the
/// display's bottom.
pub(crate) fn placed(display: Bounds<Pixels>, size: Size<Pixels>) -> Bounds<Pixels> {
    let x = display.origin.x + (display.size.width - size.width) / 2.;
    let y = display.origin.y + display.size.height - size.height - px(ABOVE_BOTTOM);
    Bounds {
        origin: point(x, y),
        size,
    }
}

/// The HUD's size for `hud`: as wide as its widest line, within
/// [`WIDTH`], 46 logical pixels tall — 56 with a message.
fn size_for(hud: &Hud) -> Size<Pixels> {
    let line = |text: &str| text.chars().count() as f32 * CHAR;
    let mut width = CHROME + line(&hud.title);
    if let Some(message) = &hud.message {
        width = width.max(CHROME + line(message));
    }
    if hud.icon.is_some() {
        width += ICON + GAP;
    }
    let height = if hud.message.is_some() {
        HEIGHT.1
    } else {
        HEIGHT.0
    };
    size(px(width.clamp(WIDTH.0, WIDTH.1)), px(height))
}

impl LauncherWindow {
    /// Shows `hud` in its own window on the display the launcher was on,
    /// replacing the HUD still shown, and closes it after its duration —
    /// fading out over [`FADE`], or at once under reduced motion — unless
    /// it is pending, in which case it stays until it is updated or the
    /// launcher is active again.
    pub(crate) fn show_hud(&mut self, hud: Hud, window: &mut Window, cx: &mut Context<Self>) {
        self.close_hud(cx);
        let size = size_for(&hud);
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
        let pending = !hud.hides_by_itself();
        let duration = hud.duration();
        let title = hud.title.clone();
        let opened = cx.open_window(options, |window, cx| {
            crate::make_click_through(window);
            cx.new(|_| HudView { hud, fading: None })
        });
        let handle = match opened {
            Ok(handle) => handle,
            Err(error) => {
                pane_core::diagnostic!("Pane could not show a HUD: {error:#}");
                return;
            }
        };
        self.hud.shown += 1;
        self.hud.window = Some(handle);
        self.hud.title = Some(title);
        self.hud.pending = pending;
        if pending {
            return;
        }
        let shown = self.hud.shown;
        let reduced = cx.reduce_motion();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(duration).await;
            let fading = this
                .update(cx, |this, cx| {
                    if this.hud.shown != shown {
                        return false;
                    }
                    if reduced {
                        this.close_hud(cx);
                        return false;
                    }
                    this.fade_hud(cx);
                    true
                })
                .unwrap_or(false);
            if !fading {
                return;
            }
            cx.background_executor().timer(FADE).await;
            this.update(cx, |this, cx| {
                if this.hud.shown == shown {
                    this.close_hud(cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// Begins the HUD's fade out, if one shows.
    fn fade_hud(&mut self, cx: &mut App) {
        let Some(handle) = self.hud.window else {
            return;
        };
        let _ = handle.update(cx, |view, _, cx| {
            view.fading = Some(cx.background_executor().now());
            cx.notify();
        });
    }

    /// Closes the HUD's window, if one is open.
    pub(crate) fn close_hud(&mut self, cx: &mut App) {
        self.hud.title = None;
        self.hud.pending = false;
        if let Some(handle) = self.hud.window.take() {
            handle
                .update(cx, |_, window, _| window.remove_window())
                .ok();
        }
    }

    /// Closes the HUD's window if the HUD is a pending one: work in
    /// progress is over once the launcher is active again.
    pub(crate) fn close_pending_hud(&mut self, cx: &mut App) {
        if self.hud.pending {
            self.close_hud(cx);
        }
    }

    /// Test support: what the HUD's window shows while it is open; `None`
    /// while no HUD shows. Test and debug builds only.
    #[cfg(any(test, debug_assertions))]
    #[doc(hidden)]
    pub fn hud(&self) -> Option<String> {
        self.hud.title.clone()
    }

    /// Test support: the HUD's window, while one shows, to read what it
    /// draws and announces as the launcher window's own is read. Test and
    /// debug builds only.
    #[cfg(any(test, debug_assertions))]
    #[doc(hidden)]
    pub fn hud_window(&self) -> Option<gpui::AnyWindowHandle> {
        self.hud
            .window
            .as_ref()
            .map(|handle| gpui::AnyWindowHandle::from(*handle))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pane_core::ToastStyle;

    #[test]
    fn the_hud_sits_centred_150_pixels_above_the_bottom_of_its_display() {
        let display = Bounds {
            origin: point(px(1920.), px(0.)),
            size: size(px(1000.), px(800.)),
        };
        let hud = placed(display, size(px(200.), px(46.)));
        assert_eq!(hud.origin.x, px(1920. + 400.));
        assert_eq!(hud.origin.y, px(800. - 46. - 150.));
        assert_eq!(hud.size, size(px(200.), px(46.)));
    }

    #[test]
    fn a_hud_is_content_sized_within_its_bounds() {
        assert_eq!(
            size_for(&Hud::new(ToastStyle::Success, "Hi")).height,
            px(HEIGHT.0)
        );
        assert_eq!(
            size_for(&Hud::new(ToastStyle::Success, "Hi")).width,
            px(WIDTH.0)
        );
        let copied = size_for(&Hud::new(ToastStyle::Success, "Copied to Clipboard"));
        assert!(
            copied.width > px(WIDTH.0) && copied.width < px(WIDTH.1),
            "{copied:?}"
        );
        assert_eq!(
            size_for(&Hud::new(ToastStyle::Success, "x".repeat(500))).width,
            px(WIDTH.1)
        );

        // A message makes a second line: taller, and as wide as the
        // widest of the two lines, not the two together.
        let explained = Hud {
            message: Some("the clipboard was full".into()),
            ..Hud::new(ToastStyle::Success, "Copied to Clipboard")
        };
        assert_eq!(size_for(&explained).height, px(HEIGHT.1));
        assert_eq!(
            size_for(&explained).width,
            px(CHROME + "the clipboard was full".chars().count() as f32 * CHAR)
        );

        // An icon takes its own room beside the title.
        let icon = Hud {
            icon: Some(pane_core::Icon::new(pane_core::IconSource::Builtin {
                name: "clipboard".into(),
                filled: false,
            })),
            ..Hud::new(ToastStyle::Success, "Copied to Clipboard")
        };
        assert_eq!(
            size_for(&icon).width,
            size_for(&Hud::new(ToastStyle::Success, "Copied to Clipboard")).width + px(ICON + GAP)
        );
    }

    #[test]
    fn the_huds_text_reads_as_its_title_and_message() {
        assert_eq!(
            announced(&Hud::new(ToastStyle::Success, "Copied")),
            "Copied"
        );
        let explained = Hud {
            message: Some("the server said no".into()),
            ..Hud::new(ToastStyle::Failure, "Upload failed")
        };
        assert_eq!(announced(&explained), "Upload failed: the server said no");
        let blank = Hud {
            message: Some("   ".into()),
            ..Hud::new(ToastStyle::Failure, "Upload failed")
        };
        assert_eq!(announced(&blank), "Upload failed");
    }
}
