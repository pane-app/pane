//! Pane's Settings window: a normal, nonmodal window separate from the
//! launcher, opened or focused from the launcher footer's ellipsis menu,
//! the Settings root result and the local `Cmd+,`/`Ctrl+,` shortcut
//! ([`open`] converges all three on one window).
//!
//! The window shares the launcher's own handle — the same
//! [`pane_core::Launcher`] the launcher window holds — so Settings runs
//! no second extension runtime and duplicates no launcher state. The two
//! windows also share the host settings (`crate::settings`): what the
//! General page's Appearance section chooses repaints both, without a
//! restart, and what the rest of the General page chooses (the Open Pane
//! hotkey, whether Pane starts at login) reaches the platform through the
//! same entity. Its shell is the reference's Settings board's (#97,
//! [`crate::ui::settings_shell`]) at Pane's own 860×600: the panel at the
//! board's own glass tint, the 48px custom titlebar naming the page where
//! the platform hides its own (macOS's traffic lights; on Windows, Pane's
//! caption buttons in place of the board's lone close glyph; Linux keeps
//! the window manager's frame), the 232px sidebar of section items with
//! the search field above them ([`search`]), and the selected page, its
//! sections of rows in raised cards. The shell module documents the
//! window's sizes.
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
//! A page is one [`Page`]: its sidebar entry (title, description, glyph),
//! a function that draws its content, the settings it offers the
//! sidebar's search (see [`search`]), and the focus it gives a control
//! the search jumps to — registered by pushing it in
//! [`SettingsWindow::new`]. Later pages add their module under
//! `settings/` and one line there — no empty feature folder, no new
//! framework — and the sidebar lists only registered pages, so no
//! section ships as a placeholder. The General page's choices live in
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
use crate::ui::icon::Glyph;
// The caption buttons' glyph painter, Windows-only like the buttons it
// draws; the import follows the same gate so it is not unused elsewhere.
#[cfg(target_os = "windows")]
use crate::ui::icon::glyph;
use crate::ui::motion;
use crate::ui::settings_shell::{self, SidebarItem};
use crate::{FocusNext, FocusPrevious};

pub(crate) mod about;
pub(crate) mod appearance;
pub(crate) mod extensions;
pub(crate) mod general;
pub(crate) mod keyboard;
pub(crate) mod launcher;
mod search;
mod shortcuts;

actions!(
    settings,
    [NextSection, PreviousSection, CloseSettings, EscapeSettings]
);

/// The id of the window's key context, which the sidebar's keys are bound
/// to.
const CONTEXT: &str = "Settings";

/// The window's own keys, bound to [`gpui::NoAction`] in a listening
/// recorder's `context`: the sidebar's Up and Down, the close shortcut and
/// the find shortcut. Bindings win over a key handler, so without these
/// the window would act on them; with them, they reach the recorder as the
/// combination being recorded.
pub(crate) fn captured_while_recording(context: &'static str) -> [KeyBinding; 4] {
    let (close, find) = if cfg!(target_os = "macos") {
        ("cmd-w", "cmd-f")
    } else {
        ("ctrl-w", "ctrl-f")
    };
    [
        KeyBinding::new("down", gpui::NoAction, Some(context)),
        KeyBinding::new("up", gpui::NoAction, Some(context)),
        KeyBinding::new(close, gpui::NoAction, Some(context)),
        KeyBinding::new(find, gpui::NoAction, Some(context)),
    ]
}

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
        // Escape closes the window where the Keyboard page says so; a
        // control that takes Escape itself (a recorder, a select, the
        // search field) binds it deeper and keeps it.
        KeyBinding::new("escape", EscapeSettings, Some(CONTEXT)),
    ]);
    appearance::bind_keys(cx);
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
    /// The Appearance page's state (its segments' focus), owned by its
    /// module.
    appearance: appearance::State,
    /// The General page's state, owned by its module.
    general: general::State,
    /// The Launcher page's state, owned by its module.
    launcher_page: launcher::State,
    /// The Shortcuts page's state, owned by its module.
    shortcuts: shortcuts::State,
    /// The Keyboard page's state, owned by its module.
    keyboard: keyboard::State,
    /// The Extensions page's state (its preferences' text fields), owned
    /// by its module.
    extensions: extensions::State,
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
        // A recorder stops listening when the window loses focus: the keys
        // it would capture go to another window.
        cx.observe_window_activation(window, |this, window, cx| {
            if !window.is_window_active() {
                this.stop_recorders(cx);
            }
        })
        .detach();
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
            // The sidebar's order: General (with the Appearance section),
            // Launcher, Shortcuts, Keyboard, Extensions, About last.
            // General is the page the window first shows.
            pages: vec![
                general::page(),
                launcher::page(),
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
            appearance: appearance::State::new(cx),
            general: general::State::new(cx),
            launcher_page: launcher::State::new(window, cx),
            shortcuts: shortcuts::State::new(launcher, cx),
            keyboard: keyboard::State::new(window, cx),
            extensions: extensions::State::default(),
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

    /// Stops every recorder listening, changing nothing: the window lost
    /// focus, so the keys it would capture go elsewhere.
    fn stop_recorders(&mut self, cx: &mut Context<Self>) {
        if self.general.recording || self.keyboard.recording.is_some() {
            self.general.recording = false;
            self.general.rejection = None;
            self.keyboard.recording = None;
            self.keyboard.rejection = None;
            cx.notify();
        }
        self.shortcuts_cancel_recording(cx);
    }

    /// Escape, where no control took it: closes this window if the
    /// Keyboard page's choice says Escape closes Settings.
    fn escape_settings(&mut self, _: &EscapeSettings, window: &mut Window, cx: &mut Context<Self>) {
        if crate::settings::shared(cx)
            .read(cx)
            .escape_closes_settings()
        {
            window.remove_window();
        }
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
                    // Presentation only: the sidebar's own item paints the
                    // chrome — its washes change at once, as the
                    // reference's `.nav` does — and the identity,
                    // accessibility and click behavior are attached here.
                    settings_shell::sidebar_item(
                        ("section", index),
                        SidebarItem {
                            label: page.title.into(),
                            glyph: page.icon,
                            detail: None,
                            reason: None,
                            count: page
                                .count
                                .map(|count| count(&self.launcher))
                                .filter(|&count| count > 0)
                                .map(|count| count.to_string().into()),
                            selected,
                        },
                        theme,
                    )
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
        // The sidebar's sections scroll inside it when the window is short,
        // independent of the page (see the shell's smaller-window policy).
        let sections = settings_shell::section_list(theme)
            .id("sections")
            .debug_selector(|| "sections".into())
            .overflow_y_scroll()
            .track_focus(&self.focus)
            .role(Role::ListBox)
            .aria_label(if searching {
                "Settings search results"
            } else {
                "Settings sections"
            })
            .on_action(cx.listener(Self::next_section))
            .on_action(cx.listener(Self::previous_section))
            .children(rows);
        let field = search::field(self, theme, cx);
        let sidebar = settings_shell::sidebar(field, sections, theme);
        sidebar.debug_selector(|| "settings-sidebar".into())
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
        // The page's padding, inside a viewport that scrolls on its own,
        // independent of the sidebar.
        settings_shell::content_viewport(theme)
            .id("settings-page")
            .debug_selector(|| "settings-page".into())
            .overflow_y_scroll()
            .track_scroll(self.search.scroll())
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
        // The window's root carries its key context and actions; the
        // shell — titlebar, sidebar and selected page — is laid out on it
        // by [`compose`].
        let root = div()
            .key_context(CONTEXT)
            .on_action(cx.listener(Self::focus_next))
            .on_action(cx.listener(Self::focus_previous))
            .on_action(cx.listener(Self::search_focus))
            .on_action(cx.listener(Self::close_settings))
            .on_action(cx.listener(Self::escape_settings));
        // The titlebar names the page showing.
        let title = self.pages[self.selected].title;
        compose(
            root,
            title,
            self.render_sidebar(&theme, cx),
            self.render_page(&theme, window, cx),
            &theme,
            material,
        )
    }
}

/// The Settings window's composition: `root` — the window's key context and actions —
/// made the shell's column, with the shared Geist family and base text
/// color on everything, the custom titlebar (labelled `title`) where the
/// platform hides its own, then `sidebar` beside `page`, on the Settings
/// panel.
pub(crate) fn compose(
    root: Div,
    title: &'static str,
    sidebar: impl IntoElement,
    page: impl IntoElement,
    theme: &ui::theme::Theme,
    material: ui::material::Material,
) -> Div {
    let content = root
        .size_full()
        .flex()
        .flex_col()
        .font_family(theme.typography.family.clone())
        .font_features(theme.typography.features.clone())
        .text_color(theme.text_title);
    // The titlebar exists only on the platforms whose own is hidden (see
    // [`titlebar`]), so the child is added under the same compile-time
    // gate — `cfg!` would leave the call compiled on Linux, where the
    // function does not exist.
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    let content = content.child(titlebar(title, theme));
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let _ = title;
    let content = content.child(settings_shell::body(sidebar, page));
    material.settings_panel(theme, content)
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
fn titlebar(title: &'static str, theme: &ui::theme::Theme) -> Div {
    let titlebar = settings_shell::titlebar(theme);
    let titlebar = titlebar.debug_selector(|| "settings-titlebar".into());
    // macOS: clear of the traffic lights, which stay where AppKit puts
    // them over the transparent titlebar.
    #[cfg(target_os = "macos")]
    let titlebar = titlebar.child(div().flex_none().w(px(78.)));
    // Windows: the label centers over the whole window, as the board's
    // does — the drag region starts as far in from the left as the
    // caption buttons reach in from the right.
    #[cfg(target_os = "windows")]
    let inset = theme.geometry.settings.caption_width * 3.;
    #[cfg(not(target_os = "windows"))]
    let inset = px(0.);
    let label = settings_shell::titlebar_label(title, theme);
    let titlebar = titlebar.child(
        // The one place to grab the window by, outside the page and
        // the sidebar.
        div()
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .window_control_area(WindowControlArea::Drag)
            .pl(inset)
            .flex()
            .items_center()
            .justify_center()
            .child(label.debug_selector(|| "settings-title".into())),
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
        .h_full()
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
    let glyph_size = theme.geometry.settings.caption_glyph;
    let hover = if area == WindowControlArea::Close {
        theme.danger
    } else {
        theme.row_hover
    };
    div()
        .id(id)
        .debug_selector(move || id.into())
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        // The platform's caption button: 46 wide, the titlebar's height
        // above its rule.
        .w(theme.geometry.settings.caption_width)
        .h_full()
        .window_control_area(area)
        .role(Role::Button)
        .aria_label(label)
        .on_click(move |_: &gpui::ClickEvent, window, _| activate(window))
        // The close button's hover is the danger tone, as Windows paints
        // it; the others take the row hover wash. The press takes the
        // stronger wash of either. Both change at once, as every control's
        // do: a fade here would have to be the press's too, since GPUI
        // fades a property the same way in every state — and a window
        // control closes or maximizes the frame the click lands, so the
        // press is a flicker at most, but it is never a delay.
        .hover(move |button| button.bg(hover))
        .active(move |button| button.bg(ui::theme::pressed(hover)))
        .child(glyph(mark, glyph_size, theme.text_title))
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
    // The window opens at 860×600, or the primary display's work area
    // less a margin where that is smaller, with the frost background, a
    // floor that keeps every control reachable at small sizes and display
    // scaling, and the custom titlebar where the platform hides its own.
    // See the shell module for the window's sizes.
    let work_area = cx
        .primary_display()
        .map(|display| display.visible_bounds().size)
        .map(|area| (f32::from(area.width), f32::from(area.height)));
    let (width, height) = settings_shell::opening_size(work_area);
    let (min_width, min_height) = settings_shell::SETTINGS_MINIMUM;
    let bounds = Bounds::centered(None, size(px(width), px(height)), cx);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        window_min_size: Some(size(px(min_width), px(min_height))),
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

/// Opens Pane's Settings window, or focuses the one already open, at the
/// page titled `page`, scrolled to the control `target` names (a search
/// anchor's id): the Actions panel's "Configure Command…" and "Configure
/// Extension…" open an extension's card on the Extensions page this way
/// (#143).
pub(crate) fn open_at(launcher: &Launcher, page: &str, target: &str, cx: &mut App) {
    let window = open(launcher, cx);
    window
        .update(cx, |settings, window, cx| {
            settings.show_at(page, target, window, cx);
        })
        .ok();
}

/// A select of a few fixed choices at a settings row's end: the shared
/// searchable select ([`crate::ui::select`]) over `choices` (read live),
/// marking `committed` (read live), committing through `commit`. The row
/// around it names the setting.
pub(crate) fn choice_select(
    name: &'static str,
    debug: &'static str,
    choices: fn(&App) -> Vec<crate::ui::select::Choice>,
    committed: fn(&App) -> &'static str,
    commit: fn(&str, &mut App),
    window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> gpui::Entity<crate::ui::select::Select> {
    cx.new(|cx| {
        crate::ui::select::Select::new(
            name,
            "",
            debug,
            std::rc::Rc::new(move |cx: &App| {
                let visuals = crate::settings::visuals(cx);
                crate::ui::select::Model {
                    theme: visuals.theme,
                    material: visuals.material,
                    choices: choices(cx),
                    committed: Some(committed(cx).into()),
                }
            }),
            std::rc::Rc::new(move |id: &str, _: &mut Window, cx: &mut App| commit(id, cx)),
            window,
            cx,
        )
    })
}

/// A plain choice for [`choice_select`]: `id` labelled `label`, offered
/// unless `unavailable` says why not.
pub(crate) fn choice(
    id: &'static str,
    label: impl Into<gpui::SharedString>,
    unavailable: Option<String>,
) -> crate::ui::select::Choice {
    crate::ui::select::Choice {
        id: id.into(),
        label: label.into(),
        subtitle: None,
        keywords: Vec::new(),
        unavailable_reason: unavailable.map(Into::into),
    }
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
    /// The 16px glyph the sidebar entry shows.
    pub(crate) icon: Glyph,
    /// The count the sidebar entry shows at its right end, read live, if
    /// the page has one (the Extensions page's installed extensions, as
    /// the reference counts its plugins); none shows while it is zero.
    pub(crate) count: Option<fn(&Launcher) -> usize>,
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
