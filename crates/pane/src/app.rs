//! The launcher window: orchestration of the launcher's screens, navigation
//! and action dispatch.
//!
//! [`LauncherWindow`] is a thin renderer over [`pane_core::Launcher`]: key
//! and mouse input call launcher actions, and each frame draws the
//! launcher's snapshot. The query field, forms and custom views bind their
//! data in their own modules — [`crate::features`] and
//! [`crate::extension_views`] — whose `impl LauncherWindow` blocks supply
//! the per-screen sync and render methods this orchestration calls.

use std::future::Future;
use std::mem::{Discriminant, discriminant};
use std::path::Path;
use std::time::{Duration, Instant};

use gpui::{
    App, BoxShadow, ClipboardItem, Context, Div, FocusHandle, Hsla, KeyDownEvent,
    PathPromptOptions, Pixels, Role, ScrollHandle, SharedString, Size, Stateful, Window,
    WindowControlArea, div, prelude::*, px, relative,
};
use pane_core::changes::Changes;
use pane_core::hotkeys::Shortcut;
use pane_core::{Launcher, LauncherView, Row, Screen, SelectedAction, Status};

use crate::extension_views::{custom_view, form};
use crate::features::footer_menu;
use crate::features::root_search;
use crate::features::settings;
use crate::ui::icon::{Glyph, IconTone};
use crate::ui::keycap::{self, Key as ActionKey};
use crate::ui::material::Material;
use crate::ui::motion::{self, Direction};
use crate::ui::result_row::{RowContent, result_row};
use crate::{Back, Confirm, FocusNext, FocusPrevious, OpenSettings, SelectNext, SelectPrevious};

pub(crate) const KEY_CONTEXT: &str = "Launcher";

/// How long after an accepted Open Pane press another press of the same
/// binding is treated as the repeat of a key still held, not a new press.
/// The Windows and X11 adapters stop the system's key repeat at its source
/// (`MOD_NOREPEAT`, detectable auto-repeat); macOS's Carbon hot keys
/// report a held key again, so the window keeps the guard itself. A
/// genuine second press after this long toggles again.
const OPEN_PANE_REPEAT: Duration = Duration::from_millis(600);

/// The launcher window's root view.
pub struct LauncherWindow {
    pub(crate) launcher: Launcher,
    /// The list's focus, on screens other than root search.
    pub(crate) focus_handle: FocusHandle,
    /// Root search's query field, which has focus on root search.
    pub(crate) query: root_search::QueryField,
    /// The open form's controls; `Some` exactly on the form screen.
    pub(crate) form: Option<form::FormControls>,
    /// The open custom view's focus and layout; `Some` exactly on the
    /// custom view screen.
    pub(crate) custom_view: Option<custom_view::CustomViewControls>,
    /// The footer menu's button: the leftmost control of the bottom strip
    /// (the open menu's own focus is held by the menu, while it is open).
    pub(crate) menu_button: FocusHandle,
    /// The open footer menu, if any; see [`features::footer_menu`].
    pub(crate) menu: Option<footer_menu::FooterMenu>,
    /// The list's scroll position.
    scroll: ScrollHandle,
    /// What the list was last scrolled for.
    scrolled_for: Option<ScrolledFor>,
    /// Whether the next frame scrolls to the selected row again, once the
    /// list changed in this one has been laid out.
    scroll_again: bool,
    /// The view transition in flight, if any: the arriving screen's
    /// content is fading in over a tiny directional shift. Presentation
    /// only — see [`crate::ui::motion`].
    transition: Option<motion::Transition>,
    /// Which way the last navigation went, for the next view transition's
    /// direction: `back()` leaves a view, everything else that changes the
    /// screen (opening a command, a form, a custom view, a preview, a
    /// hotkey) enters one.
    navigation: Direction,
    /// The screen *kind* the last frame drew, to tell a real view
    /// transition (the kind changed) from a query or result update (it
    /// did not — those never animate).
    drawn_screen: Option<Discriminant<Screen>>,
    /// The arriving content's presentation as the last frame drew it
    /// (see [`LauncherWindow::view_transition`]). Test and debug builds
    /// only.
    #[cfg(any(test, debug_assertions))]
    arriving: Option<(f32, f32)>,
    /// The launcher's view as the last frame drew it, for tests (see
    /// [`LauncherWindow::drawn_view`]). Test and debug builds only.
    #[cfg(any(test, debug_assertions))]
    drawn: Option<LauncherView>,
    /// When the Open Pane hotkey was last accepted, so the repeats of a
    /// held key do not toggle again and again (see [`OPEN_PANE_REPEAT`]).
    open_pane_press: Option<Instant>,
    /// Whether the launcher window is hidden by the Open Pane hotkey —
    /// hidden, not closed: Pane keeps running, and the next press shows
    /// the same window and the same launcher again. Drives the toggle's
    /// decision together with the window's focus, so what the hotkey does
    /// is the same whatever the platform reports about a hidden window.
    /// The test-observable copy is [`LauncherWindow::hidden`].
    hidden: bool,
}

/// What the list was last scrolled for. When any of it changes, the list
/// scrolls the least it can to keep the selected row visible: the screen,
/// title or selection; the rows, as reloaded after an install; or the size
/// of the window or of the list. The mouse wheel changes none of it, so the
/// list never scrolls back while the user scrolls it.
#[derive(PartialEq)]
struct ScrolledFor {
    /// Which screen, not its contents (a form's values, a view's drawing).
    screen: Discriminant<Screen>,
    title: String,
    selected: Option<usize>,
    rows: Vec<Row>,
    window: Size<Pixels>,
    list: Size<Pixels>,
}

impl LauncherWindow {
    pub fn new(launcher: Launcher, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        let query = root_search::QueryField::new(cx);
        // The launcher starts at root search.
        query.focus(window, cx);
        // The footer menu's button, first of the strip's controls.
        let menu_button = cx.focus_handle().tab_stop(true);
        // The host settings this window renders through: what the
        // Appearance page chooses repaints this window (and the Settings
        // window) without a restart, its background follows the material
        // in effect, and the platform's appearance notification feeds the
        // system's appearance back into them (see `crate::settings`).
        crate::settings::follow(&crate::settings::ensure(cx), window, cx);
        // The launcher this window runs owns the global-shortcut
        // registration: the recorded Open Pane hotkey is applied to the
        // system here, at startup, and the settings keep this launcher for
        // every later change (see `crate::settings::attach_launcher`).
        crate::settings::attach_launcher(&launcher, cx);
        // Quitting ends development: its watchers go and a running build
        // is stopped with the processes it started.
        cx.on_app_quit(|this: &mut Self, _| {
            this.launcher.stop_all_development();
            async {}
        })
        .detach();
        LauncherWindow {
            launcher,
            focus_handle,
            query,
            form: None,
            scroll: ScrollHandle::new(),
            scrolled_for: None,
            scroll_again: false,
            custom_view: None,
            transition: None,
            navigation: Direction::Forward,
            drawn_screen: None,
            #[cfg(any(test, debug_assertions))]
            arriving: None,
            menu_button,
            menu: None,
            open_pane_press: None,
            hidden: false,
            #[cfg(any(test, debug_assertions))]
            drawn: None,
        }
    }

    pub fn launcher(&self) -> &Launcher {
        &self.launcher
    }

    /// Test support: the launcher's view as the window last drew it; `None`
    /// before the first frame. The launcher changes on other threads (a
    /// crash pausing a package, a build) before the window is told to
    /// redraw, so a test compares this with [`Launcher::view`] to know that
    /// a frame shows what the launcher holds. Test and debug builds only.
    #[cfg(any(test, debug_assertions))]
    #[doc(hidden)]
    pub fn drawn_view(&self) -> Option<&LauncherView> {
        self.drawn.as_ref()
    }

    /// Test support: the view transition the last frame drew, as the
    /// arriving content's (offset from rest in px — below rest for a view
    /// that opens, above for backing out — and opacity); `None` when
    /// settled, which is also what reduced motion always reports. Test and
    /// debug builds only.
    #[cfg(any(test, debug_assertions))]
    #[doc(hidden)]
    pub fn view_transition(&self) -> Option<(f32, f32)> {
        self.arriving
    }

    /// Test support: whether the Open Pane hotkey has hidden the window —
    /// the platform's own visibility is not observable from outside GPUI,
    /// so the window reports the state it drove. Test and debug builds
    /// only.
    #[cfg(any(test, debug_assertions))]
    #[doc(hidden)]
    pub fn hidden(&self) -> bool {
        self.hidden
    }

    /// Redraws whenever the launcher changes in the background, as
    /// `changes` (the other end of the launcher's
    /// [`with_development`](Launcher::with_development)) reports: a package
    /// being developed is building, failed to build or was reloaded.
    pub fn follow_changes(
        &mut self,
        mut changes: Changes,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.spawn_in(window, async move |this, cx| {
            while changes.next().await.is_some() {
                let shown = this.update_in(cx, |this, window, cx| {
                    this.sync_screen(window, cx);
                    cx.notify();
                });
                if shown.is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    fn select_next(&mut self, _: &SelectNext, _: &mut Window, cx: &mut Context<Self>) {
        self.launcher.move_selection(1);
        cx.notify();
    }

    fn select_previous(&mut self, _: &SelectPrevious, _: &mut Window, cx: &mut Context<Self>) {
        self.launcher.move_selection(-1);
        cx.notify();
    }

    fn confirm(&mut self, _: &Confirm, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(self.launcher.view().screen, Screen::Form(_)) {
            self.submit_form(window, cx);
        } else {
            self.activate_selected(window, cx);
        }
    }

    fn back(&mut self, _: &Back, window: &mut Window, cx: &mut Context<Self>) {
        self.launcher.back();
        // Backing out is the one navigation that leaves a view; the next
        // frame's view transition (if the screen kind changed) settles the
        // arriving content down into place.
        self.navigation = Direction::Back;
        self.sync_screen(window, cx);
        cx.notify();
    }

    /// Shows the package in `folder` with its identity and compatibility,
    /// redrawing when the check finishes.
    pub fn preview_package(&mut self, folder: &Path, window: &mut Window, cx: &mut Context<Self>) {
        self.navigation = Direction::Forward;
        let pending = self.launcher.preview_package(folder);
        self.show_until_done(pending, window, cx);
    }

    /// Downloads and shows the npm package `spec` names, as
    /// [`LauncherWindow::preview_package`] shows a folder.
    pub fn preview_npm(&mut self, spec: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.navigation = Direction::Forward;
        let pending = self.launcher.preview_npm(spec);
        self.show_until_done(pending, window, cx);
    }

    /// Fetches and shows the revision of the Git repository `spec` names,
    /// as [`LauncherWindow::preview_package`] shows a folder.
    pub fn preview_git(&mut self, spec: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.navigation = Direction::Forward;
        let pending = self.launcher.preview_git(spec);
        self.show_until_done(pending, window, cx);
    }

    /// Asks for the folder to grant the package with `identity` with the
    /// platform's folder picker, then has Pane check and record it.
    /// Cancelling changes nothing. A debug build run by the native smokes
    /// takes the folder `PANE_TEST_CHOOSE_FOLDER` names instead of showing
    /// the picker (nothing else sets it; a release build has no such hook).
    fn choose_granted_folder(
        &mut self,
        identity: pane_core::PackageIdentity,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        #[cfg(debug_assertions)]
        if let Some(folder) = std::env::var_os("PANE_TEST_CHOOSE_FOLDER") {
            let pending = self.launcher.grant_folder(&identity, Path::new(&folder));
            self.show_until_done(pending, window, cx);
            return;
        }
        let chosen = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Choose".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let folder = match chosen.await {
                Ok(Ok(Some(paths))) => paths.into_iter().next(),
                Ok(Ok(None)) | Err(_) => None,
                Ok(Err(error)) => {
                    this.update(cx, |this, cx| {
                        this.launcher
                            .show_error(format!("Could not open a folder picker: {error:#}"));
                        cx.notify();
                    })
                    .ok();
                    None
                }
            };
            if let Some(folder) = folder {
                this.update_in(cx, |this, window, cx| {
                    let pending = this.launcher.grant_folder(&identity, &folder);
                    this.show_until_done(pending, window, cx);
                })
                .ok();
            }
        })
        .detach();
    }

    /// Asks for a package folder with the platform's folder picker, then
    /// previews it. Cancelling leaves root search as it was.
    fn choose_package_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let chosen = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Install".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let folder = match chosen.await {
                Ok(Ok(Some(paths))) => paths.into_iter().next(),
                Ok(Ok(None)) | Err(_) => None,
                Ok(Err(error)) => {
                    this.update(cx, |this, cx| {
                        this.launcher
                            .show_error(format!("Could not open a folder picker: {error:#}"));
                        cx.notify();
                    })
                    .ok();
                    None
                }
            };
            if let Some(folder) = folder {
                this.update_in(cx, |this, window, cx| {
                    this.preview_package(&folder, window, cx)
                })
                .ok();
            }
        })
        .detach();
    }

    /// Opens the command whose global hotkey `shortcut` is, as the system
    /// reported it pressed while any application had focus: the window
    /// comes to the front and shows the command. A press that opens nothing
    /// (a hotkey released meanwhile) leaves the window where it is. The
    /// Open Pane hotkey is not a command's: its press summons, focuses or
    /// hides the launcher itself ([`LauncherWindow::open_pane_pressed`]).
    pub fn hotkey_pressed(
        &mut self,
        shortcut: &Shortcut,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.launcher.opens_pane(shortcut) {
            self.open_pane_pressed(window, cx);
            return;
        }
        let Some(pending) = self.launcher.press_hotkey(shortcut) else {
            return;
        };
        self.unhide(window);
        window.activate_window();
        cx.activate(true);
        self.navigation = Direction::Forward;
        self.show_until_done(pending, window, cx);
    }

    /// Shows the window if the Open Pane hotkey hid it: every activation
    /// of the launcher — the hotkey's show path, a command's hotkey, an
    /// entry point that reaches the launcher from Settings — must find a
    /// visible window.
    fn unhide(&mut self, window: &mut Window) {
        if self.hidden {
            window.set_visible(true);
            self.hidden = false;
        }
    }

    /// The Open Pane hotkey's press: hidden, the launcher is shown and its
    /// search focused; visible but without focus (another application's,
    /// or the Settings window's — its focus does not count), it is brought
    /// forward; already focused, it is hidden — hidden, not closed, so
    /// Pane keeps running in the background, the Settings window stays
    /// open and the next press reuses the same live launcher.
    fn open_pane_pressed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let now = cx.background_executor().now();
        if self
            .open_pane_press
            .is_some_and(|last| now.duration_since(last) < OPEN_PANE_REPEAT)
        {
            // The repeat of a key still held, not a new press.
            return;
        }
        self.open_pane_press = Some(now);
        if !self.hidden && window.is_window_active() {
            window.set_visible(false);
            self.hidden = true;
            return;
        }
        self.unhide(window);
        window.activate_window();
        cx.activate(true);
        // The summoned launcher's search has focus: the query of the search
        // on screen, or root search's when the launcher was left on a
        // screen with no search of its own.
        if self.launcher.view().search_field().is_some() {
            self.query.focus(window, cx);
        } else {
            self.launcher.show_root_search();
            self.sync_screen(window, cx);
        }
        cx.notify();
    }

    /// On the hotkey screen, a key pressed with its modifiers is the new
    /// hotkey. Keys the launcher binds (Enter, Escape, arrows, Tab) do not
    /// reach here; pressing a modifier alone is not a key press.
    fn key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(self.launcher.view().screen, Screen::Hotkey { .. }) {
            return;
        }
        let keystroke = &event.keystroke;
        let modifiers = keystroke.modifiers;
        let shortcut = Shortcut::new(
            modifiers.control,
            modifiers.alt,
            modifiers.shift,
            modifiers.platform,
            &keystroke.key,
        );
        cx.stop_propagation();
        match shortcut {
            Ok(shortcut) => {
                let pending = self.launcher.record_hotkey(shortcut);
                self.navigation = Direction::Forward;
                self.show_until_done(pending, window, cx);
            }
            Err(problem) => {
                self.launcher.show_error(problem);
                cx.notify();
            }
        }
    }

    fn focus_next(&mut self, _: &FocusNext, window: &mut Window, cx: &mut Context<Self>) {
        window.focus_next(cx);
    }

    fn focus_previous(&mut self, _: &FocusPrevious, window: &mut Window, cx: &mut Context<Self>) {
        window.focus_prev(cx);
    }

    /// The local `Cmd+,`/`Ctrl+,` shortcut: opens or focuses the Settings
    /// window, the same one the footer menu and the root result open.
    fn open_settings(&mut self, _: &OpenSettings, _: &mut Window, cx: &mut Context<Self>) {
        settings::open(&self.launcher, cx);
    }

    /// Starts the selected row's action and redraws when the guest answers,
    /// without blocking the window meanwhile.
    fn activate_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.navigation = Direction::Forward;
        // The Settings root result opens the Settings window; the launcher
        // itself does nothing (see [`Launcher::selected_opens_settings`]).
        if self.launcher.selected_opens_settings() {
            settings::open(&self.launcher, cx);
            return;
        }
        if self.launcher.selected_asks_for_folder() {
            self.choose_package_folder(window, cx);
            return;
        }
        if let Some(identity) = self.launcher.folder_to_choose() {
            self.choose_granted_folder(identity, window, cx);
            return;
        }
        if let Some(text) = self.launcher.selected_copy() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
        let pending = self.launcher.activate_selected();
        self.show_until_done(pending, window, cx);
    }

    /// Activates the root result with `id` in this window, as clicking it
    /// in root search does: the launcher returns to root search first,
    /// wherever it is, this window is summoned and focused, and the result
    /// is selected and activated through the same Enter path
    /// ([`LauncherWindow::activate_selected`]). The Settings window's
    /// Extensions page reaches the launcher's own install rows and a
    /// package's commands through this, so those flows keep running where
    /// their forms, folder pickers and key capture already live — here,
    /// with the window they belong to in front.
    pub(crate) fn activate_root_result(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.navigation = Direction::Forward;
        // Root search is reached as Escape reaches it, one screen back at a
        // time, wherever the launcher is (a form, a command, the extension
        // list Settings entered); every `back` moves toward root search,
        // and at root search this stops.
        while !matches!(self.launcher.view().screen, Screen::Root { .. }) {
            self.launcher.back();
        }
        self.unhide(window);
        window.activate_window();
        cx.activate(true);
        let Some(index) = self
            .launcher
            .view()
            .rows
            .iter()
            .position(|row| row.id == id)
        else {
            // No such root result (the row was disabled or removed since
            // the page drew it): root search is shown, focused, which is as
            // far as this reaches.
            self.sync_screen(window, cx);
            cx.notify();
            return;
        };
        self.launcher.select(index);
        self.activate_selected(window, cx);
    }

    /// Shows the launcher's state now and again when `pending`, a launcher
    /// action's reply, has been applied, without blocking the window
    /// meanwhile. Each time the form's and custom view's controls follow the
    /// launcher's screen (opening a form needs no guest call, so its controls
    /// appear at once).
    pub(crate) fn show_until_done(
        &mut self,
        pending: impl Future<Output = ()> + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.sync_screen(window, cx);
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            pending.await;
            this.update_in(cx, |this, window, cx| {
                this.sync_screen(window, cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Scrolls the list to the selected row when what it shows or its size
    /// changed since it was last scrolled for (see [`ScrolledFor`]).
    fn keep_selected_visible(&mut self, view: &LauncherView, window: &mut Window) {
        let shown = ScrolledFor {
            screen: discriminant(&view.screen),
            title: view.title.clone(),
            selected: view.selected,
            rows: view.rows.clone(),
            window: window.viewport_size(),
            // As laid out in the last frame.
            list: self.scroll.bounds().size,
        };
        let last = self.scrolled_for.as_ref();
        if last == Some(&shown) && !self.scroll_again {
            return;
        }
        // Scrolling uses the list's size and rows as last laid out. When the
        // window, the screen or the rows changed, those are known only once
        // this frame is laid out, so the next frame scrolls again with them:
        // otherwise a short screen after a long list, scrolled far down,
        // would keep an offset that hides its selected row.
        let relaid = last.is_some_and(|last| {
            last.window != shown.window
                || last.screen != shown.screen
                || last.title != shown.title
                || last.rows != shown.rows
        });
        self.scroll_again = relaid && !self.scroll_again;
        if self.scroll_again {
            window.request_animation_frame();
        }
        if let Some(selected) = view.selected {
            self.scroll.scroll_to_item(selected);
        }
        self.scrolled_for = Some(shown);
    }

    /// Makes the form's and custom view's controls, root search's query
    /// field, and focus, follow the launcher's screen.
    ///
    /// This also asks every window to redraw, not only this one: the
    /// Settings window's Extensions page reads the launcher's state — the
    /// same records this window shows — so wherever the launcher changed
    /// here (an operation's reply, a background change the changes channel
    /// reported, a key this window handled), each window showing it
    /// re-reads what it holds. A window refresh rather than a notify on
    /// one window's view, so no window is left out; it is an effect, so it
    /// is safe wherever the launcher changed, including from another
    /// window's own flow.
    fn sync_screen(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sync_form(window, cx);
        self.sync_custom_view(window, cx);
        // Last: coming back to root search, even as a view closes, focuses
        // the query rather than the list.
        self.sync_root_search(window, cx);
        cx.refresh_windows();
    }

    fn render_row(
        &self,
        index: usize,
        row: Row,
        selected: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let visuals = crate::settings::visuals(cx);
        let theme = &visuals.theme;
        let reason = row.unavailable.as_ref().map(|u| u.reason().to_owned());
        // The row's accessible description: its subtitle and, when it
        // cannot run, the reason, together.
        let description = match (&row.subtitle, &reason) {
            (Some(subtitle), Some(reason)) => Some(format!("{subtitle}. {reason}")),
            (subtitle, reason) => subtitle.clone().or(reason.clone()),
        };
        // Presentation only: the shared row paints the chrome, and the
        // identity, accessibility and click behavior are attached here.
        result_row(
            RowContent {
                title: row.title.clone().into(),
                subtitle: row.subtitle.clone().map(SharedString::from),
                unavailable_reason: reason.map(SharedString::from),
                selected,
                unavailable_id: ("unavailable", index).into(),
                icon: row_icon(&row.id),
            },
            theme,
        )
        .id(("row", index))
        .debug_selector(|| format!("row-{}", row.title))
        .role(Role::ListBoxOption)
        .aria_label(row.title.clone())
        .aria_selected(selected)
        .when(selected, |row| row.aria_active_descendant())
        // An unavailable row stays listed and selectable; it says why it
        // cannot run here, on screen and to assistive technology.
        .when(row.unavailable.is_some(), |row| row.aria_disabled(true))
        .when_some(description, |row, description| {
            row.aria_description(description)
        })
        .on_click(cx.listener(move |this, _, window, cx| {
            this.launcher.select(index);
            this.activate_selected(window, cx);
        }))
    }

    /// The idle footer's action strip: the selected action's button,
    /// right-aligned in the strip, beside the menu button the footer
    /// renders at the strip's far left (see [`Self::render_menu_button`]).
    /// `action` is the launcher's one selected-action definition
    /// ([`Launcher::selected_action`]); the screens with no primary action
    /// (a custom view, the network details screen) show no button, only
    /// the reserved space.
    fn render_action_strip(&self, action: &SelectedAction, cx: &mut Context<Self>) -> Div {
        div()
            // The strip fills the footer's height (its 50px floor), so the
            // button sits centered in it rather than at its top edge —
            // the idle line is always one row tall, unlike a wrapped
            // message, which keeps the footer's no-centering rule for its
            // first line.
            .flex()
            .flex_1()
            .min_h(px(0.))
            .w_full()
            .min_w(px(0.))
            .items_center()
            // Far left: the menu button's ellipsis, which the footer
            // renders ahead of this strip, is the room's occupant now; the
            // spacer keeps the action at the right.
            .child(div().flex_1().min_w(px(0.)))
            .when(!action.label.is_empty(), |strip| {
                strip.child(self.render_action_button(action, cx))
            })
    }

    /// The idle footer's button: the selected action's label with the
    /// Enter keycap beside it. Its click takes the same Confirm path Enter
    /// takes (see [`LauncherWindow::press_primary_action`]); its label and
    /// availability come from the definition, so what the button says,
    /// whether it can run and what it does cannot diverge. The selected
    /// row's chrome marks it as the strip's primary control.
    fn render_action_button(
        &self,
        action: &SelectedAction,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let theme = crate::settings::visuals(cx).theme;
        let geometry = &theme.geometry;
        div()
            .id("primary-action")
            .debug_selector(|| "primary-action".into())
            .role(Role::Button)
            .aria_label(action.label.clone())
            // The key that presses this button from the keyboard: the
            // keycap beside the label shows the same key.
            .aria_keyshortcuts(ActionKey::Enter.name())
            // A click never dispatches what the definition says cannot
            // run now; assistive technology is told the same thing.
            .when(!action.available, |button| button.aria_disabled(true))
            // The button shrinks under pressure (the label ellipsizes; the
            // keycap does not) so a narrow window keeps it inside the
            // strip instead of clipping at the window's right edge.
            .flex_initial()
            .min_w(px(0.))
            .h(geometry.action_height)
            .flex()
            .items_center()
            .gap(geometry.action_gap)
            .px(geometry.action_padding_x)
            .rounded(geometry.action_radius)
            .bg(theme.row_selected)
            // The selected row's 1px inset edge.
            .shadow(vec![
                BoxShadow::new(px(0.), px(0.), theme.row_selected_border)
                    .spread_radius(px(1.))
                    .inset(),
            ])
            .text_size(theme.typography.footer_size)
            .font_weight(theme.typography.medium)
            .text_color(theme.text_title)
            .when(action.available, |button| button.cursor_pointer())
            // Unavailable: dimmed, and the pointer says nothing to click.
            // What explains it stays where it was — the row's reason, the
            // empty state — not the button.
            .when(!action.available, |button| {
                button.opacity(0.5).cursor_default()
            })
            .child(
                div()
                    .flex_initial()
                    .min_w(px(0.))
                    .truncate()
                    .child(action.label.clone()),
            )
            .child(keycap::keycap(ActionKey::Enter, &theme))
            .on_click(cx.listener(|this, _, window, cx| {
                this.press_primary_action(window, cx);
            }))
    }

    /// Dispatches the footer button's click: the same
    /// [`LauncherWindow::confirm`] path Enter takes, but only when the
    /// selected-action definition says the action can run now. The frame
    /// that drew the button can be stale — an action may have started
    /// since it was laid out — so the check is made again here, at click
    /// time, against the launcher's current state. Enter is unchanged: it
    /// keeps the behavior it has always had; this keeps the button from
    /// dispatching what cannot run (no selection, an unavailable result, an
    /// action already running).
    fn press_primary_action(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.launcher.selected_action().available {
            self.confirm(&Confirm, window, cx);
        }
    }
}

impl Render for LauncherWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view = self.launcher.view();
        #[cfg(any(test, debug_assertions))]
        {
            self.drawn = Some(view.clone());
        }
        self.keep_selected_visible(&view, window);
        // A view transition runs when the screen *kind* changed — root
        // search to a command, a command back to root, a form or custom
        // view opening or closing — and moves only the content that
        // changed, while the shell chrome (panel, footer, query field,
        // heading) stays put. Query and result updates never animate; the
        // launcher has already navigated, dispatched and focused when the
        // first frame draws, so nothing waits on the transition. See
        // `crate::ui::motion` for the whole policy.
        let now = cx.background_executor().now();
        let screen = discriminant(&view.screen);
        let arriving = motion::advance(
            &mut self.transition,
            self.navigation,
            self.drawn_screen.is_some_and(|last| last != screen),
            cx.reduce_motion(),
            now,
        );
        self.drawn_screen = Some(screen);
        #[cfg(any(test, debug_assertions))]
        {
            self.arriving = arriving;
        }
        let visuals = crate::settings::visuals(cx);
        let theme = visuals.theme;
        let material = visuals.material;
        let empty = match &view.screen {
            Screen::Root { .. } => "No commands are installed.",
            Screen::Command | Screen::CommandSearch { .. } => "This command has no items.",
            Screen::Package { .. } => "Nothing to install.",
            Screen::Form(_) => "",
            Screen::Extensions { .. } => "No extensions are installed.",
            Screen::CustomView(_) | Screen::NetworkDetails { .. } => "",
            Screen::Confirm { .. }
            | Screen::Hotkey { .. }
            | Screen::PauseDetails { .. }
            | Screen::RuntimeDetails { .. }
            | Screen::BuildDetails { .. } => "",
        };
        // A confirmation, and a package preview offering Install or Update
        // (an npm or Git package's has several more lines), keep their choices in
        // view.
        let preview = matches!(view.screen, Screen::Package { .. }) && !view.rows.is_empty();
        let confirm = matches!(view.screen, Screen::Confirm { .. }) || preview;
        // A preview has one or two rows (Install or Update) and more lines
        // to read, which may take more of the window than a confirmation's.
        let details_share = if preview { 0.62 } else { 0.4 };
        let details: Vec<_> = view
            .details()
            .iter()
            .enumerate()
            .map(|(index, line)| {
                div()
                    .id(("detail", index))
                    .debug_selector(|| format!("detail-{line}"))
                    .text_size(theme.typography.row_subtitle_size)
                    .text_color(theme.text_body)
                    .child(line.clone())
            })
            .collect();
        // A command's search that failed lists nothing; its error says why,
        // not "No results".
        let search_failed = matches!(
            (&view.screen, &view.status),
            (Screen::CommandSearch { .. }, Status::Error(_))
        );
        // The footer's status: while the launcher runs, works, answers or
        // fails, the strip is that message; `None` while it is idle, when
        // the strip becomes the selected action (below).
        let (status_selector, status, status_color): (&str, Option<SharedString>, Hsla) =
            match view.status {
                Status::Idle => ("status-idle", None, theme.text_muted),
                Status::Running => ("status-running", Some("Running…".into()), theme.warning),
                Status::Progress(work) => ("status-progress", Some(work.into()), theme.warning),
                Status::Result(answer) => ("status-result", Some(answer.into()), theme.success),
                Status::Error(message) => ("status-error", Some(message.into()), theme.danger),
            };
        // The selected action: the one definition ([`SelectedAction`])
        // that drives the idle strip's button — its label, its
        // availability — and the dispatch both the button and Enter take.
        let action = self.launcher.selected_action();
        let rows: Vec<_> = view
            .rows
            .into_iter()
            .enumerate()
            .map(|(index, row)| {
                let selected = view.selected == Some(index);
                self.render_row(index, row, selected, cx)
            })
            .collect();
        let empty = match &view.screen {
            Screen::CommandSearch { .. } if search_failed => div().id("empty"),
            Screen::Root { query } | Screen::CommandSearch { query }
                if !query.trim().is_empty() =>
            {
                div()
                    .id("no-results")
                    .debug_selector(|| "no-results".into())
                    .child(format!("No results for “{}”", query.trim()))
            }
            _ => div().id("empty").child(empty),
        };
        let list = div()
            .id("rows")
            .debug_selector(|| "rows".into())
            .role(Role::ListBox)
            .aria_label(match view.screen {
                Screen::Root { .. } => "Results".into(),
                Screen::CommandSearch { .. } => format!("{} results", view.title),
                _ => view.title.clone(),
            })
            .flex_1()
            .flex()
            .flex_col()
            .gap(theme.geometry.row_list_gap)
            .px(theme.geometry.row_padding_x)
            .pt(px(4.))
            .pb(px(10.))
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            // Above the rows: with none selected, the only rows are root
            // search's fallbacks, listed below "No results".
            .when(view.selected.is_none(), |rows| {
                rows.child(empty.text_color(theme.text_muted))
            })
            .children(rows);
        // The launcher decides what an item opens; its screen says which.
        // The search screens carry their own header (the query field);
        // every other screen keeps its heading. Root search has no extra
        // title — the reference's launcher has none. The heading is
        // computed before the body dispatch, which moves the screen.
        // It is also the non-search screens' drag region: with the native
        // title bar hidden, the heading is the one place outside the
        // editable field to grab the window by, and a long heading
        // truncates instead of eating the list.
        let heading = match &view.screen {
            Screen::Root { .. } => None,
            _ => Some(
                div()
                    .flex_none()
                    .px(theme.geometry.search_padding_x)
                    .py(px(12.))
                    .truncate()
                    .text_size(theme.typography.row_title_size)
                    .font_weight(theme.typography.medium)
                    .text_color(theme.text_title)
                    .window_control_area(WindowControlArea::Drag)
                    .child(view.title.clone()),
            ),
        };

        // The content that changes between screens — the results, a form,
        // a custom view — is what arrives with the transition. On the
        // search screens the query field is the shell's search header,
        // above the results and outside the moving area, so the field
        // never moves while the list below it arrives.
        let body = match view.screen {
            Screen::Form(form) => {
                motion::arriving(self.render_form(view.title.clone(), form, cx), arriving)
                    .into_any_element()
            }
            Screen::CustomView(custom_view) => {
                motion::arriving(self.render_custom_view(custom_view, cx), arriving)
                    .into_any_element()
            }
            Screen::Root { query } => self.render_search(
                query,
                root_search::ROOT_PLACEHOLDER,
                motion::arriving(list, arriving),
                cx,
            ),
            // The opened command's own search field, the same control.
            Screen::CommandSearch { query } => self.render_search(
                query,
                root_search::COMMAND_PLACEHOLDER,
                motion::arriving(list, arriving),
                cx,
            ),
            // The list holds keyboard focus; the selected row is its active
            // descendant, and key actions bubble to the root.
            _ => {
                motion::arriving(list.track_focus(&self.focus_handle), arriving).into_any_element()
            }
        };

        // The launcher's content: the shared Geist family and base text
        // color on everything, the heading (or the search header, in
        // `body`), the details, the body and the status footer.
        let content = div()
            .key_context(KEY_CONTEXT)
            .on_action(cx.listener(Self::select_next))
            .on_action(cx.listener(Self::select_previous))
            .on_action(cx.listener(Self::confirm))
            .on_action(cx.listener(Self::back))
            .on_action(cx.listener(Self::open_settings))
            .on_action(cx.listener(Self::focus_next))
            .on_action(cx.listener(Self::focus_previous))
            .on_key_down(cx.listener(Self::key_down))
            .flex_1()
            .min_h(px(0.))
            .flex()
            .flex_col()
            .font_family(theme.typography.family.clone())
            .text_color(theme.text_title)
            .when_some(heading, |content, heading| content.child(heading))
            // A confirmation's or preview's long details scroll within 40%
            // (a preview's 62%) of the window, leaving the rest to its
            // choices, which stay visible.
            .when(!details.is_empty(), |content| {
                content.child(
                    div()
                        .id("details")
                        .flex()
                        .flex_col()
                        .gap_2()
                        .px(theme.geometry.search_padding_x)
                        .when(confirm, |details| {
                            details
                                .flex_shrink(1.)
                                .max_h(relative(details_share))
                                .overflow_y_scroll()
                        })
                        .children(details),
                )
            })
            .child(body)
            .child(
                // The footer: the launcher's status strip. While a status
                // shows — running, progress, a result or an error — the
                // strip is the message, wrapping, growing and scrolling
                // exactly as before. While the launcher is idle, the strip
                // is the selected action instead (see
                // [`LauncherWindow::render_action_strip`]): the idle
                // instruction text is gone, and the same strip keeps its
                // identity (id, role, status-* debug selectors) in both
                // shapes, so a test or a smoke can always find the
                // launcher's footer where it was. The menu button is the
                // strip's leftmost control, the action its rightmost, and
                // the open menu's popup is the strip's first child: its
                // capture-phase dismissal runs before the button's click
                // tracking, while its own bounds stay above the strip
                // (see `footer_menu`).
                Material::footer(&theme)
                    .id("status")
                    // The popup is anchored to the strip (its bottom edge
                    // on the strip's top edge, however tall the message
                    // has grown it), and the strip never scrolls — the
                    // message's viewport below does — so the button and
                    // the popup above it stay put while the message
                    // scrolls.
                    .relative()
                    .when_some(
                        self.menu
                            .as_ref()
                            .map(|menu| self.render_menu_popup(menu, cx)),
                        |strip, popup| strip.child(popup),
                    )
                    // The strip is the live region: it carries the
                    // message as its name, so assistive technology
                    // announces it. While idle the strip carries no
                    // message — the button is the announcement's content
                    // — and stays silent.
                    .role(Role::Status)
                    .when_some(status.clone(), |footer, text| footer.aria_label(text))
                    .debug_selector(|| status_selector.into())
                    .text_size(theme.typography.footer_size)
                    .text_color(status_color)
                    .child(
                        div()
                            .flex()
                            .w_full()
                            .min_w(px(0.))
                            .flex_1()
                            .min_h(px(0.))
                            .gap(px(8.))
                            // Far left: the menu button, the strip's
                            // leftmost control.
                            .child(self.render_menu_button(&theme, cx))
                            .child(match status.clone() {
                                Some(text) => div()
                                    // The message's own scroll viewport:
                                    // past the 35% cap the message scrolls
                                    // here — inside the strip — instead of
                                    // being cut, and the strip never
                                    // scrolls, so the button and any popup
                                    // above it stay put. The strip's bounds
                                    // carry the status-* debug selectors;
                                    // this one, the message's, lets tests
                                    // see wrapping and scroll. The message
                                    // fills the viewport's width and wraps
                                    // there — a long error is several
                                    // readable lines, never one clipped at
                                    // the window's right edge — and the
                                    // strip grows with it.
                                    .id("status-scroll")
                                    .flex_1()
                                    .min_w(px(0.))
                                    .overflow_y_scroll()
                                    .child(
                                        div()
                                            .w_full()
                                            .min_w(px(0.))
                                            .flex_none()
                                            .py(px(12.))
                                            .debug_selector(|| "status-message".into())
                                            .child(text),
                                    )
                                    .into_any_element(),
                                None => self.render_action_strip(&action, cx).into_any_element(),
                            }),
                    ),
            );
        // While the arriving content is still in flight, keep frames
        // coming; the frame that completes the transition requests none,
        // so a settled window is idle. The scroll relayout above keeps its
        // own separate request, for the frame after the rows change.
        if arriving.is_some() {
            window.request_animation_frame();
        }
        // The panel surface: the frost material's L1 glass around the
        // content, with the sheen beneath it.
        material.panel(&theme, content)
    }
}

/// Tells the launcher window that the launcher changed outside its own
/// flow — the Settings window's Extensions page drove an operation through
/// the launcher — so it redraws with what the launcher holds: its screen
/// may have moved under it (an open form closes when its package is
/// disabled from Settings), and the screen sync the update runs asks
/// every window to redraw, Settings included. Focus is not taken: the
/// flow runs in Settings.
pub(crate) fn launcher_changed_outside(cx: &mut App) {
    for window in cx.windows() {
        let Some(launcher) = window.downcast::<LauncherWindow>() else {
            continue;
        };
        launcher
            .update(cx, |this, window, cx| {
                this.sync_screen(window, cx);
                cx.notify();
            })
            .ok();
    }
}

/// The icon presentation for a row, chosen by the row's stable id: the
/// built-in rows and this build's sample commands are known identities,
/// each with a reference tone and glyph; everything else is a plain
/// command. No presentation is inferred from a title's text.
pub(crate) fn row_icon(id: &str) -> Option<(IconTone, Glyph)> {
    match id {
        "rust-sample" => Some((IconTone::Term, Glyph::Prompt)),
        "javascript-sample" | "typescript-sample" => Some((IconTone::Code, Glyph::Code)),
        "pane.install-from-folder" => Some((IconTone::Folder, Glyph::Folder)),
        "pane.install-from-npm" => Some((IconTone::Web, Glyph::Blocks)),
        "pane.install-from-git" => Some((IconTone::Term, Glyph::Terminal)),
        "pane.manage-extensions" => Some((IconTone::Command, Glyph::Blocks)),
        "pane.settings" => Some((IconTone::Command, Glyph::Gear)),
        _ => Some((IconTone::Command, Glyph::Prompt)),
    }
}
