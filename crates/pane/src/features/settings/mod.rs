//! Pane's Settings window: a normal, nonmodal window separate from the
//! launcher, opened or focused from the launcher footer's ellipsis menu,
//! the Settings root result and the local `Cmd+,`/`Ctrl+,` shortcut
//! ([`open`] converges all three on one window).
//!
//! The window shares the launcher's own handle — the same
//! [`pane_core::Launcher`] the launcher window holds — so Settings runs
//! no second extension runtime and duplicates no launcher state. Its
//! shell is the reference's Settings composition: the frost panel with
//! the custom titlebar where the platform hides its own (macOS's traffic
//! lights, Windows's caption buttons; Linux keeps the window manager's
//! frame), a sidebar of sections, and the selected page's content.
//!
//! ## Page registration
//!
//! A page is one [`Page`]: its sidebar entry (id, title, icon) and a
//! function that draws its content, registered by pushing it in
//! [`SettingsWindow::new`]. Later pages add their module under
//! `settings/` and one line there — no empty feature folder, no new
//! framework — and the sidebar lists only registered pages, so no
//! section ships as a placeholder. A page's state lives in its module,
//! held by the window as a field.

use gpui::{
    AnyElement, App, Bounds, Context, Div, FocusHandle, KeyBinding, Role, Stateful,
    TitlebarOptions, Window, WindowBounds, WindowHandle, WindowOptions, actions, div, prelude::*,
    px, size,
};
// The window-control areas mark the custom titlebar's controls, which
// exist only on the platforms whose own titlebar is hidden; the import
// follows the same gate so it is not unused on Linux.
#[cfg(any(target_os = "macos", target_os = "windows"))]
use gpui::WindowControlArea;
use pane_core::Launcher;

use crate::ui;
use crate::ui::icon::{Glyph, IconTone};
// The caption buttons' glyph painter, Windows-only like the buttons it
// draws; the import follows the same gate so it is not unused elsewhere.
#[cfg(target_os = "windows")]
use crate::ui::icon::glyph;
use crate::ui::result_row::{RowContent, result_row};

mod about;

actions!(settings, [NextSection, PreviousSection]);

/// The id of the window's key context, which the sidebar's keys are bound
/// to.
const CONTEXT: &str = "Settings";

/// Registers the Settings window's key bindings: the sidebar's navigation
/// keys, which apply only while the Settings window is focused. Enter is
/// left unbound: the sidebar's selection already shows the page Enter
/// would choose, so the key does nothing, and later pages' controls bind
/// it for their own submitting.
pub(crate) fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("down", NextSection, Some(CONTEXT)),
        KeyBinding::new("up", PreviousSection, Some(CONTEXT)),
    ]);
}

/// The Settings window's root view. One instance exists at most — see
/// [`open`].
pub struct SettingsWindow {
    /// The launcher this window shares with the launcher window: the same
    /// handle, not a second launcher or extension runtime.
    launcher: Launcher,
    /// The pages Settings offers, in sidebar order; the registry later
    /// pages join (see the module docs).
    pages: Vec<Page>,
    /// The selected page, an index into `pages`.
    selected: usize,
    /// The sidebar's focus, which is the window's keyboard focus.
    focus: FocusHandle,
    /// The About page's state, owned by its module.
    about: about::State,
}

impl SettingsWindow {
    /// The Settings window over `launcher`, its pages registered in
    /// sidebar order. Called only by [`open`].
    fn new(launcher: &Launcher, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle().tab_stop(true);
        window.focus(&focus, cx);
        SettingsWindow {
            launcher: launcher.clone(),
            pages: vec![about::page()],
            selected: 0,
            focus,
            about: about::State::default(),
        }
    }

    fn next_section(&mut self, _: &NextSection, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected + 1 < self.pages.len() {
            self.selected += 1;
            cx.notify();
        }
    }

    fn previous_section(&mut self, _: &PreviousSection, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected > 0 {
            self.selected -= 1;
            cx.notify();
        }
    }

    /// The sidebar: the sections list, one row per registered page. It is
    /// the window's keyboard focus, so its keys (see [`bind_keys`]) drive
    /// the window.
    fn render_sidebar(&self, theme: &ui::theme::Theme, cx: &mut Context<Self>) -> Stateful<Div> {
        div()
            .id("sections")
            .debug_selector(|| "sections".into())
            .flex_none()
            .w(px(200.))
            .h_full()
            .flex()
            .flex_col()
            .p(px(8.))
            .gap(px(2.))
            .border_r_1()
            .border_color(theme.hairline_soft)
            .track_focus(&self.focus)
            .role(Role::ListBox)
            .aria_label("Settings sections")
            .on_action(cx.listener(Self::next_section))
            .on_action(cx.listener(Self::previous_section))
            .children(self.pages.iter().enumerate().map(|(index, page)| {
                let selected = index == self.selected;
                // Presentation only: the shared row paints the chrome, and
                // the identity, accessibility and click behavior are
                // attached here.
                result_row(
                    RowContent {
                        title: page.title.into(),
                        subtitle: None,
                        unavailable_reason: None,
                        unavailable_id: ("section-unavailable", index).into(),
                        selected,
                        icon: Some(page.icon),
                    },
                    theme,
                )
                .id(("section", index))
                .debug_selector(move || format!("section-{}", page.title))
                .role(Role::ListBoxOption)
                .aria_label(page.title)
                .aria_selected(selected)
                .when(selected, |row| row.aria_active_descendant())
                .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                    if this.selected != index {
                        this.selected = index;
                        cx.notify();
                    }
                }))
            }))
    }

    /// The selected page's content, scrolling when the window is short.
    fn render_page(
        &mut self,
        theme: &ui::theme::Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let render = self.pages[self.selected].render;
        let content = render(self, window, cx);
        div()
            .id("settings-page")
            .debug_selector(|| "settings-page".into())
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .overflow_y_scroll()
            .px(px(28.))
            .py(px(20.))
            .text_size(theme.typography.row_subtitle_size)
            .text_color(theme.text_body)
            .child(content)
    }
}

impl Render for SettingsWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = ui::visuals().theme.clone();
        let material = ui::visuals().material;
        // The content: the shared Geist family and base text color on
        // everything, the custom titlebar where the platform's is hidden,
        // then the sidebar and the selected page.
        let content = div()
            .key_context(CONTEXT)
            .size_full()
            .flex()
            .flex_col()
            .font_family(theme.typography.family.clone())
            .text_color(theme.text_title);
        // The titlebar exists only on the platforms whose own is hidden
        // (see [`titlebar`]), so the child is added under the same
        // compile-time gate — `cfg!` would leave the call compiled on
        // Linux, where the function does not exist.
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        let content = content.child(titlebar(&theme));
        let content = content.child(
            div()
                .flex_1()
                .min_h(px(0.))
                .flex()
                .flex_row()
                .child(self.render_sidebar(&theme, cx))
                .child(self.render_page(&theme, window, cx)),
        );
        material.panel(&theme, content)
    }
}

/// The custom titlebar, drawn only where the platform's own titlebar is
/// hidden: macOS (its traffic lights remain) and Windows (the caption
/// buttons are Pane's, below). Linux keeps the window manager's frame, so
/// it needs none of this.
///
/// The drag region is a sibling of, never an ancestor of, the control
/// buttons: the platform's window-control hit test walks the frame's
/// control hitboxes in paint order and takes the first one under the
/// pointer, so a drag region wrapping the buttons would swallow them.
#[cfg(any(target_os = "macos", target_os = "windows"))]
fn titlebar(theme: &ui::theme::Theme) -> Div {
    let titlebar = div()
        .flex_none()
        .flex()
        .h(px(44.))
        .items_center()
        .gap(px(8.));
    // macOS: clear of the traffic lights, which stay where AppKit puts
    // them over the transparent titlebar.
    #[cfg(target_os = "macos")]
    let titlebar = titlebar.child(div().flex_none().w(px(78.)));
    let titlebar = titlebar.child(
        // The one place to grab the window by, outside the page and
        // the sidebar.
        div()
            .flex_1()
            .min_w(px(0.))
            .window_control_area(WindowControlArea::Drag)
            .px(px(16.))
            .truncate()
            .text_size(theme.typography.row_title_size)
            .font_weight(theme.typography.medium)
            .text_color(theme.text_title)
            .child("Settings"),
    );
    // Windows: the caption buttons, marked with the platform's window
    // control areas so the hit test routes them to the system's real
    // close, minimize and maximize behavior. The click handlers are
    // the same behavior for platforms that never consult the hit test
    // (GPUI's test platform among them); on Windows itself the system
    // takes the click through the hit test and the handlers stay
    // idle. Added under the same compile-time gate as
    // [`window_controls`] — `cfg!` would leave the call compiled on
    // the other platforms, where the function does not exist.
    #[cfg(target_os = "windows")]
    let titlebar = titlebar.child(window_controls(theme));
    titlebar
}

/// The Windows caption buttons: minimize, maximize, close, right to left
/// as the platform draws them, each marked with its
/// [`WindowControlArea`] for the system's hit testing.
#[cfg(target_os = "windows")]
fn window_controls(theme: &ui::theme::Theme) -> Div {
    div()
        .flex_none()
        .flex()
        .child(control_button(
            "window-minimize",
            Glyph::WindowMinimize,
            WindowControlArea::Min,
            "Minimize",
            |window| window.minimize_window(),
            theme,
        ))
        .child(control_button(
            "window-maximize",
            Glyph::WindowMaximize,
            WindowControlArea::Max,
            "Maximize",
            |window| window.zoom_window(),
            theme,
        ))
        .child(control_button(
            "window-close",
            Glyph::WindowClose,
            WindowControlArea::Close,
            "Close",
            |window| window.remove_window(),
            theme,
        ))
}

/// One caption button: `glyph` at the platform's control `area`, running
/// `activate` when the platform's hit test does not take the click
/// itself.
#[cfg(target_os = "windows")]
fn control_button(
    id: &'static str,
    mark: Glyph,
    area: WindowControlArea,
    label: &'static str,
    activate: fn(&mut Window),
    theme: &ui::theme::Theme,
) -> impl IntoElement {
    div()
        .id(id)
        .debug_selector(move || id.into())
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .size(px(44.))
        .window_control_area(area)
        .role(Role::Button)
        .aria_label(label)
        .on_click(move |_: &gpui::ClickEvent, window, _| activate(window))
        .hover(|button| {
            // The close button's hover is the danger tone, as Windows
            // paints it; the others take the row hover wash.
            if area == WindowControlArea::Close {
                button.bg(theme.danger)
            } else {
                button.bg(theme.row_hover)
            }
        })
        .child(glyph(mark, px(16.), theme.text_title))
}

/// Opens Pane's Settings window, or focuses the one already open: the
/// window list is the one-window registry, so whichever of the ellipsis
/// menu, the Settings root result or the local shortcut calls this, the
/// user gets one window, brought to the front.
pub(crate) fn open(launcher: &Launcher, cx: &mut App) -> WindowHandle<SettingsWindow> {
    for window in cx.windows() {
        let Some(open) = window.downcast::<SettingsWindow>() else {
            continue;
        };
        if open
            .update(cx, |_, window, cx| {
                window.activate_window();
                cx.notify();
            })
            .is_ok()
        {
            return open;
        }
    }
    // The window is the reference's settings composition: as wide as the
    // launcher, tall enough for the sidebar and the page, with the same
    // frost background, a floor that keeps the layout usable at small
    // sizes and display scaling, and the custom titlebar where the
    // platform hides its own.
    let bounds = Bounds::centered(None, size(px(740.), px(530.)), cx);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        window_min_size: Some(size(px(560.), px(400.))),
        window_background: crate::window_background(),
        titlebar: Some(TitlebarOptions {
            title: Some("Settings".into()),
            appears_transparent: true,
            ..Default::default()
        }),
        ..Default::default()
    };
    cx.open_window(options, |window, cx| {
        // The window's own corners are rounded by the Desktop Window
        // Manager, as the launcher's are, so nothing shows behind the
        // panel that fills it.
        #[cfg(target_os = "windows")]
        crate::prefer_rounded_window_corners(window);
        cx.new(|cx| SettingsWindow::new(launcher, window, cx))
    })
    .and_then(|window| {
        // The new window comes to the front and takes focus, as focusing
        // an open one does.
        window.update(cx, |_, window, cx| {
            window.activate_window();
            cx.notify();
        })?;
        Ok(window)
    })
    .expect("failed to open Pane's Settings window")
}

/// One page of Pane's Settings: the sidebar entry that lists it, and the
/// content it draws. See the module docs for how a page registers.
pub(crate) struct Page {
    /// The sidebar entry's title, the page's identity in the sidebar and
    /// the tests' selectors.
    pub(crate) title: &'static str,
    /// The icon the sidebar entry shows.
    pub(crate) icon: (IconTone, Glyph),
    /// Draws the page's content into the page area; the window hands
    /// itself over, since a page's state lives in its module, held by the
    /// window as a field.
    render: fn(&mut SettingsWindow, &mut Window, &mut Context<SettingsWindow>) -> AnyElement,
}
