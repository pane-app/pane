//! Pane's Settings window: a normal, nonmodal window separate from the
//! launcher, opened or focused from the launcher footer's ellipsis menu,
//! the Settings root result and the local `Cmd+,`/`Ctrl+,` shortcut
//! ([`open`] converges all three on one window).
//!
//! The window shares the launcher's own handle — the same
//! [`pane_core::Launcher`] the launcher window holds — so Settings runs
//! no second extension runtime and duplicates no launcher state. The two
//! windows also share the host settings (`crate::settings`): what the
//! Appearance page chooses repaints both, without a restart, and what the
//! General page chooses — today, the Open Pane hotkey and whether Pane
//! starts at login — reaches the platform through the same entity. Its shell is
//! the reference's Settings composition: the frost panel with the custom
//! titlebar where the platform hides its own (macOS's traffic lights,
//! Windows's caption buttons; Linux keeps the window manager's frame), a
//! sidebar of sections with the search field above them ([`search`]),
//! and the selected page's content.
//!
//! ## Section transitions
//!
//! Switching sections runs the shared motion policy's section arrival
//! (see [`crate::ui::motion`]): the selected page's content fades in over
//! a tiny shift from the side the sidebar moved, while the sidebar, the
//! titlebar, the window bounds and the page's scroll viewport stay
//! exactly where they were. The switch itself — the sidebar's selected
//! row, the visible selection, the focus — is applied before the frame
//! draws, and the page's own state (a filter, collapsed groups, an open
//! edit) survives the round trip untouched; a rapid switch retargets
//! from the presentation on screen. Nothing resizes the window, and
//! reduced motion draws every switch settled.
//!
//! ## Page registration
//!
//! A page is one [`Page`]: its sidebar entry (title, description, icon),
//! a function that draws its content, the settings it offers the
//! sidebar's search (see [`search`]), and the focus it gives a control
//! the search jumps to — registered by pushing it in
//! [`SettingsWindow::new`]. Later pages add their module under
//! `settings/` and one line there — no empty feature folder, no new
//! framework — and the sidebar lists only registered pages, so no
//! section ships as a placeholder. The Appearance and General pages'
//! choices live in
//! the shared host settings rather than the window, since the launcher
//! window renders by them too (and the platform's login registration
//! outlives any window); a page whose state is the window's own
//! lives in its module, held by the window as a field.

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
use crate::ui::motion;
use crate::ui::result_row::{RowContent, result_row};
use crate::{FocusNext, FocusPrevious};

mod about;
mod appearance;
mod extensions;
mod general;
mod keyboard;
mod launcher;
mod search;
mod shortcuts;

actions!(settings, [NextSection, PreviousSection, CloseSettings]);

/// The id of the window's key context, which the sidebar's keys are bound
/// to.
const CONTEXT: &str = "Settings";

/// Registers the Settings window's key bindings: the sidebar's navigation
/// keys, the window's focus traversal and the platform's close-window
/// shortcut, which apply only while the Settings window is focused. Enter
/// is left unbound: the sidebar's selection already shows the page Enter
/// would choose, so the key does nothing, and later pages' controls bind
/// it for their own submitting.
pub(crate) fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("down", NextSection, Some(CONTEXT)),
        KeyBinding::new("up", PreviousSection, Some(CONTEXT)),
        // Tab and Shift-Tab move through the pages' controls, as the
        // launcher window's do through its: the pages' fields and buttons
        // are tab stops (the Shortcuts page's filter, group headers and
        // alias cells among them).
        KeyBinding::new("tab", FocusNext, Some(CONTEXT)),
        KeyBinding::new("shift-tab", FocusPrevious, Some(CONTEXT)),
        // The platform's close-window shortcut, in this window's context:
        // Cmd+W on macOS, Ctrl+W elsewhere. Window-local — it closes only
        // Settings, and the launcher's own dismiss binding stays in the
        // launcher's window, where the Keyboard page rebinds it.
        KeyBinding::new(
            if cfg!(target_os = "macos") {
                "cmd-w"
            } else {
                "ctrl-w"
            },
            CloseSettings,
            Some(CONTEXT),
        ),
    ]);
    general::bind_keys(cx);
    search::bind_keys(cx);
    shortcuts::bind_keys(cx);
    keyboard::bind_keys(cx);
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
    /// The section arrival in flight, if any: the selected page's
    /// content is fading in over a tiny directional shift. Presentation
    /// only — see [`crate::ui::motion`].
    section_arrival: Option<motion::Tween>,
    /// The section the last frame drew, an index into `pages`, to tell a
    /// real section change (which transitions) from a page's own content
    /// update (which never animates).
    drawn_section: Option<usize>,
    /// The arriving content's presentation as the last frame drew it
    /// (see [`SettingsWindow::section_arrival`]). Test and debug builds
    /// only.
    #[cfg(any(test, debug_assertions))]
    arriving: Option<(f32, f32)>,
    /// The sidebar's focus, which is the window's keyboard focus.
    focus: FocusHandle,
    /// The About page's state, owned by its module.
    about: about::State,
    /// The General page's state, owned by its module.
    general: general::State,
    /// The Launcher page's state, owned by its module.
    launcher_page: launcher::State,
    /// The Shortcuts page's state, owned by its module.
    shortcuts: shortcuts::State,
    /// The Keyboard page's state, owned by its module.
    keyboard: keyboard::State,
    /// The sidebar's search, owned by its module.
    search: search::State,
}

impl SettingsWindow {
    /// The Settings window over `launcher`, its pages registered in
    /// sidebar order. Called only by [`open`].
    fn new(launcher: &Launcher, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle().tab_stop(true);
        window.focus(&focus, cx);
        // The host settings this window renders through — the Appearance
        // page is one of its windows' shared consumers: what it chooses
        // repaints this window and the launcher without a restart, and
        // the platform's appearance notification feeds the system's
        // appearance back into them (see `crate::settings`).
        crate::settings::follow(&crate::settings::ensure(cx), window, cx);
        // The placement the Launcher page explains its choices through,
        // ensuring it exists before the page's search reads it.
        crate::placement::ensure(cx);
        // The Shortcuts page lists the launcher's commands, and the
        // launcher's packages can change while this window sits idle:
        // installed, disabled, enabled, updated or removed in the
        // launcher window, or in the background. Nothing tells this
        // window, so while it is open a small watcher wakes at
        // `shortcuts::WATCH`, compares the catalog the page last drew with
        // a fresh one and asks for a redraw when they differ — the next
        // frame draws the launcher as it is now. It ends with the window.
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(shortcuts::WATCH).await;
                if this
                    .update(cx, |window, cx| {
                        window.shortcuts_watched(cx);
                        window.search_watched(cx);
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        SettingsWindow {
            launcher: launcher.clone(),
            // The sidebar's order: the sections the reference lists
            // (General, Launcher, Appearance, Shortcuts, Keyboard,
            // Extensions), About last. Of those, this milestone ships
            // General, Launcher, Appearance, Shortcuts, Keyboard and
            // Extensions; General — the Open Pane hotkey and the
            // launch-at-login choice — is the page the window first
            // shows, and the later tickets' pages take their places in
            // this order as they land.
            pages: vec![
                general::page(),
                launcher::page(),
                appearance::page(),
                shortcuts::page(),
                keyboard::page(),
                extensions::page(),
                about::page(),
            ],
            selected: 0,
            section_arrival: None,
            drawn_section: None,
            #[cfg(any(test, debug_assertions))]
            arriving: None,
            focus,
            about: about::State::default(),
            general: general::State::new(cx),
            launcher_page: launcher::State::new(window, cx),
            shortcuts: shortcuts::State::new(launcher, cx),
            keyboard: keyboard::State::new(cx),
            search: search::State::new(cx),
        }
    }

    fn next_section(&mut self, _: &NextSection, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected + 1 < self.pages.len() {
            self.selected += 1;
            // Page navigation leaves the search: a query showing clears,
            // so the sidebar returns to the sections as the page changes.
            self.clear_search(cx);
            cx.notify();
        }
    }

    fn previous_section(&mut self, _: &PreviousSection, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected > 0 {
            self.selected -= 1;
            // As the sections' Down key does.
            self.clear_search(cx);
            cx.notify();
        }
    }

    fn focus_next(&mut self, _: &FocusNext, window: &mut Window, cx: &mut Context<Self>) {
        window.focus_next(cx);
    }

    /// Test support: the section arrival the last frame drew, as the
    /// arriving page content's (offset from rest in px — below rest when
    /// the sidebar moved down to the section, above when it moved up —
    /// and its opacity); `None` when the frame drew the page settled,
    /// which is also all reduced motion ever reports. Test and debug
    /// builds only.
    #[cfg(any(test, debug_assertions))]
    #[doc(hidden)]
    pub fn section_arrival(&self) -> Option<(f32, f32)> {
        self.arriving
    }

    fn focus_previous(&mut self, _: &FocusPrevious, window: &mut Window, cx: &mut Context<Self>) {
        window.focus_prev(cx);
    }

    /// The platform's close-window shortcut: closes this window only —
    /// the launcher keeps running, and with it the global hotkeys and
    /// whatever the launcher was showing.
    fn close_settings(&mut self, _: &CloseSettings, window: &mut Window, _: &mut Context<Self>) {
        window.remove_window();
    }

    /// The sidebar: the search field, then the sections list — or, while
    /// a query shows, the search's results (see the search module's
    /// docs). The list is the window's keyboard focus, so its keys (see
    /// [`bind_keys`]) drive the window.
    fn render_sidebar(&self, theme: &ui::theme::Theme, cx: &mut Context<Self>) -> Div {
        // While a query shows, the list is the search's results; the
        // search module builds those rows (or its no-results line).
        let searching = self.search.searching(cx);
        let rows: Vec<AnyElement> = if searching {
            search::result_rows(self, theme, cx)
        } else {
            self.pages
                .iter()
                .enumerate()
                .map(|(index, page)| {
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
                    // Pressed: the selected wash, one rung above the
                    // hover one, fading on the shared pointer span — and
                    // only while the section is unselected, so the
                    // sidebar's selected wash lands at once.
                    .when(!selected, |row| {
                        row.active(|row| row.bg(theme.row_selected))
                            .transitions(|fades| fades.bg(crate::ui::motion::pointer_fade()))
                    })
                    .debug_selector(move || format!("section-{}", page.title))
                    .role(Role::ListBoxOption)
                    .aria_label(page.title)
                    .aria_selected(selected)
                    .when(selected, |row| row.aria_active_descendant())
                    .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                        if this.selected != index {
                            this.selected = index;
                            // Choosing a section is page navigation: any
                            // query showing clears, as the sections' keys
                            // also do.
                            this.clear_search(cx);
                            cx.notify();
                        }
                    }))
                    .into_any_element()
                })
                .collect()
        };
        div()
            .flex_none()
            .w(px(200.))
            .h_full()
            .flex()
            .flex_col()
            .p(px(8.))
            .gap(px(2.))
            .border_r_1()
            .border_color(theme.hairline_soft)
            .child(search::field(self, theme, cx))
            .child(
                div()
                    .id("sections")
                    .debug_selector(|| "sections".into())
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .track_focus(&self.focus)
                    .role(Role::ListBox)
                    .aria_label(if searching {
                        "Settings search results"
                    } else {
                        "Settings sections"
                    })
                    .on_action(cx.listener(Self::next_section))
                    .on_action(cx.listener(Self::previous_section))
                    .children(rows),
            )
    }

    /// The selected page's content, scrolling when the window is short.
    /// The scroll container is tracked by the search's handle, and the
    /// controls' scroll anchors are the ones the page about to draw
    /// registers — cleared here, filled by the page's render — so a
    /// search reveal only ever scrolls a control on the page now
    /// showing.
    ///
    /// The section arrival: the content that changes between sections —
    /// the page — fades in over a tiny directional shift, from the side
    /// the sidebar moved, while everything around it (the sidebar, the
    /// titlebar, the scroll viewport itself) stays exactly where it was.
    /// The section is already switched — the sidebar's selected row and
    /// the window's focus were updated before this frame draws — and the
    /// page's own state (a filter, collapsed groups, an open edit) is
    /// the page's, untouched by the paint. Query and content updates
    /// never animate. See `crate::ui::motion` for the whole policy.
    fn render_page(
        &mut self,
        theme: &ui::theme::Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        self.search.clear_anchors();
        let now = cx.background_executor().now();
        // A section change transitions from the side the sidebar moved:
        // down the list, the page arrives from below; up, from above. A
        // rapid switch retargets from the interrupted presentation, and
        // the first frame a window draws is settled.
        let moving_down = self.drawn_section.is_some_and(|last| last < self.selected);
        let from = if moving_down {
            motion::VIEW_SHIFT
        } else {
            -motion::VIEW_SHIFT
        };
        let changed = self.drawn_section.is_some_and(|last| last != self.selected);
        let arriving = motion::advance_arrival(
            &mut self.section_arrival,
            from,
            changed,
            cx.reduce_motion(),
            now,
        );
        self.drawn_section = Some(self.selected);
        #[cfg(any(test, debug_assertions))]
        {
            self.arriving = arriving;
        }
        let render = self.pages[self.selected].render;
        let content = render(self, window, cx);
        // While the arriving page is still in flight — or one of its
        // groups is disclosing (the page's render asked for its own
        // frames) — keep frames coming; the frame that completes them
        // requests none, so a settled window is idle.
        if arriving.is_some() {
            window.request_animation_frame();
        }
        div()
            .id("settings-page")
            .debug_selector(|| "settings-page".into())
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .overflow_y_scroll()
            .track_scroll(self.search.scroll())
            .px(px(28.))
            .py(px(20.))
            .text_size(theme.typography.row_subtitle_size)
            .text_color(theme.text_body)
            .child(motion::arriving_page(content, arriving))
    }
}

impl Render for SettingsWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The search's results follow the query and the registered
        // settings as they stand: recomputed here, every frame, so what
        // the sidebar lists and what its keys act on are the same — and
        // a change anywhere (typing, a package the launcher window
        // installed, the host settings) is what the next frame shows.
        self.search.refresh(&self.launcher, &self.pages, cx);
        let visuals = crate::settings::visuals(cx);
        let theme = visuals.theme;
        let material = visuals.material;
        // The content: the shared Geist family and base text color on
        // everything, the custom titlebar where the platform's is hidden,
        // then the sidebar and the selected page.
        let content = div()
            .key_context(CONTEXT)
            .on_action(cx.listener(Self::focus_next))
            .on_action(cx.listener(Self::focus_previous))
            .on_action(cx.listener(Self::search_focus))
            .on_action(cx.listener(Self::close_settings))
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
            // paints it; the others take the row hover wash. The wash
            // fades in and out over the shared pointer span, and the
            // press takes the selected wash below it — a window control
            // closes or maximizes the frame the click lands, so the
            // press is a flicker at most, but it is never a delay.
            if area == WindowControlArea::Close {
                button.bg(theme.danger)
            } else {
                button.bg(theme.row_hover)
            }
        })
        .when(area != WindowControlArea::Close, |button| {
            button.active(|button| button.bg(theme.row_selected))
        })
        .transitions(|fades| fades.bg(crate::ui::motion::pointer_fade()))
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
        window_background: crate::settings::window_background(cx),
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

/// One page of Pane's Settings: the sidebar entry that lists it, and
/// the content it draws. See the module docs for how a page registers.
pub(crate) struct Page {
    /// The sidebar entry's title, the page's identity in the sidebar and
    /// the tests' selectors.
    pub(crate) title: &'static str,
    /// What the page is, in one line: the description its entry in the
    /// sidebar's search carries, matched beside the page's title.
    pub(crate) about: &'static str,
    /// The icon the sidebar entry shows.
    pub(crate) icon: (IconTone, Glyph),
    /// Draws the page's content into the page area; the window hands
    /// itself over, since a page's state lives in its module, held by the
    /// window as a field.
    render: fn(&mut SettingsWindow, &mut Window, &mut Context<SettingsWindow>) -> AnyElement,
    /// The settings and controls the page offers the sidebar's search,
    /// read live: a control that appears or goes on the page is in or out
    /// of the search with it. See [`search::Entry`].
    search: fn(&Launcher, &App) -> Vec<search::Entry>,
    /// Focuses the control `target` when the search jumps to it: a
    /// control that takes keyboard focus focuses it and returns true;
    /// one that takes none — or no longer exists — returns false, and the
    /// reveal scrolls it into view where it drew while the sidebar keeps
    /// the window's keyboard focus.
    focus: fn(&mut SettingsWindow, &str, &mut Window, &mut Context<SettingsWindow>) -> bool,
}
