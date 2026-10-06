//! The launcher window: orchestration of the launcher's screens, navigation
//! and action dispatch.
//!
//! [`LauncherWindow`] is a thin renderer over [`pane_core::Launcher`]: key
//! and mouse input call launcher actions, and each frame draws the
//! launcher's snapshot. The query field, forms and custom views bind their
//! data in their own modules — [`crate::features`] and
//! [`crate::extension_views`] — whose `impl LauncherWindow` blocks supply
//! the per-screen sync and render methods this orchestration calls.

mod frame_motion;
mod presence;

use std::future::Future;
use std::mem::{Discriminant, discriminant};
use std::path::Path;

use gpui::{
    App, ClipboardItem, Context, Div, EntityInputHandler, FocusHandle, Focusable, Hsla,
    KeyDownEvent, MouseMoveEvent, ObjectFit, PathPromptOptions, Pixels, Point, Role, ScrollHandle,
    SharedString, Size, Stateful, Window, div, img, prelude::*, px, relative,
};
use pane_core::changes::Changes;
use pane_core::feedback::WindowRequest;
use pane_core::hotkeys::Shortcut;
use pane_core::tray::TrayAction;
use pane_core::{
    ComputedAnswer, Launcher, LauncherView, NextShowing, Presentation, Row, RowPresentation,
    Screen, SelectedAction, Status, WindowPresence,
};

use crate::extension_views::{custom_view, form};
use crate::features::actions_panel;
use crate::features::clipboard_history;
use crate::features::compact_pins;
use crate::features::footer_menu;
use crate::features::hud;
use crate::features::number_hints::row_number;
use crate::features::quick_slots;
use crate::features::root_search;
use crate::features::settings;
use crate::features::toast;
use crate::ui::footer;
use crate::ui::icon::{Glyph, IconTone};
use crate::ui::keycap::CapStyle;
use crate::ui::material::Material;
use crate::ui::motion;
use crate::ui::result_row::{RowContent, RowMeta, result_row_with};
use crate::ui::shell;
use crate::ui::theme::{Theme, pressed};
use crate::{
    Back, Confirm, DismissLauncher, FocusNext, FocusPrevious, OpenSettings, ReturnToRoot,
    SelectNext, SelectPrevious,
};

pub(crate) use frame_motion::FrameMotion;
use presence::{Fit, Presence, Press, WindowSize};

pub(crate) const KEY_CONTEXT: &str = "Launcher";

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
    /// The open Actions panel, if any; see [`features::actions_panel`].
    pub(crate) actions: Option<actions_panel::ActionsPanel>,
    /// Pane's Clipboard History in the split view, while its command is
    /// open; see [`features::clipboard_history`].
    pub(crate) clipboard: Option<clipboard_history::ClipboardHistory>,
    /// The footer toast's focus and time; see [`features::toast`].
    pub(crate) toast: toast::ToastControls,
    /// The HUD's window, while one shows; see [`features::hud`].
    pub(crate) hud: hud::HudWindow,
    /// Root search's pinned home: its slots' focus; see
    /// [`features::quick_slots`].
    pub(crate) home: quick_slots::Home,
    /// What moves between frames — the view transition, the footer menu
    /// popup's entrance and exit, the number hints' slide — and the rule
    /// of which navigation arrives and which lands at once; see
    /// [`FrameMotion`].
    pub(crate) motion: FrameMotion,
    /// Whether the window is shown, when it was hidden, the Open Pane
    /// hotkey's repeat guard and the compact window mode's sizes; see
    /// [`Presence`].
    presence: Presence,
    /// The list's scroll position.
    scroll: ScrollHandle,
    /// Where the pointer last moved in the window, as the last pointer
    /// event reported it: a row selects on root search only when the
    /// pointer really moves over it, never on an event that repeats the
    /// position (see [`LauncherWindow::pointer_moved_over`]). `None` until
    /// the first event since the window was last shown, which only
    /// records where the pointer is: a window appearing under a resting
    /// pointer gets a move from the system, and that is not the user's.
    pointer: Option<Point<Pixels>>,
    /// Whether pointer movement and clicks leave root search's selection
    /// alone: while a layer over the list owns the selected target (the
    /// contextual Actions panel, #95), the target stays put under the
    /// moving pointer.
    pointer_selection_frozen: bool,
    /// What the list was last scrolled for.
    scrolled_for: Option<ScrolledFor>,
    /// Whether the next frame scrolls to the selected row again, once the
    /// list changed in this one has been laid out.
    scroll_again: bool,
    /// The launcher's view as the last frame drew it, for tests (see
    /// [`LauncherWindow::drawn_view`]). Test and debug builds only.
    #[cfg(any(test, debug_assertions))]
    drawn: Option<LauncherView>,
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
        // The placement the launcher window opens through, ensuring it
        // exists before the window below is placed by it.
        crate::placement::ensure(cx);
        // Quitting ends development: its watchers go and a running build
        // is stopped with the processes it started.
        cx.on_app_quit(|this: &mut Self, _| {
            this.launcher.stop_all_development();
            async {}
        })
        .detach();
        let mut this = LauncherWindow {
            launcher,
            focus_handle,
            query,
            form: None,
            scroll: ScrollHandle::new(),
            pointer: None,
            pointer_selection_frozen: false,
            scrolled_for: None,
            scroll_again: false,
            custom_view: None,
            menu_button,
            menu: None,
            actions: None,
            clipboard: None,
            toast: toast::ToastControls::new(cx),
            hud: hud::HudWindow::default(),
            home: quick_slots::Home::default(),
            motion: FrameMotion::new(),
            presence: Presence::default(),
            #[cfg(any(test, debug_assertions))]
            drawn: None,
        };
        // The number hints go when the window loses focus: the Ctrl
        // release would go to another window. So does the toast, an
        // animated one too (#141).
        cx.observe_window_activation(window, |this, window, cx| {
            if !window.is_window_active() {
                this.end_numbers(cx);
                this.launcher.window_deactivated();
                cx.notify();
            }
        })
        .detach();
        // What the launcher asks of the window for the host functions
        // commands call (#141): hiding it, showing a HUD.
        this.follow_window_requests(window, cx);
        // The launcher opens placed on the display the Launcher page's
        // choice resolves to, before the first frame is drawn.
        this.place(window, cx);
        // The home's slots resolve from the first visit.
        this.sync_home(cx);
        this
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
    /// arriving content's (offset from rest in px, below rest for a view
    /// that opens, and opacity); `None` when settled, which is also what
    /// backing out and reduced motion always report. Test and
    /// debug builds only.
    #[cfg(any(test, debug_assertions))]
    #[doc(hidden)]
    pub fn view_transition(&self) -> Option<(f32, f32)> {
        self.motion.view_transition()
    }

    /// Test support: whether the Open Pane hotkey has hidden the window —
    /// the platform's own visibility is not observable from outside GPUI,
    /// so the window reports the state it drove. Test and debug builds
    /// only.
    #[cfg(any(test, debug_assertions))]
    #[doc(hidden)]
    pub fn hidden(&self) -> bool {
        self.presence.hidden()
    }

    /// Test support: the footer menu popup's presentation as the last
    /// frame drew it — the offset from rest toward the strip in px and
    /// the opacity; `None` when the last frame drew the popup settled
    /// (at rest while open, absent while closed), which is also all
    /// reduced motion ever reports. Test and debug builds only.
    #[cfg(any(test, debug_assertions))]
    #[doc(hidden)]
    pub fn menu_popup_presentation(&self) -> Option<(f32, f32)> {
        self.motion.menu_popup_presentation()
    }

    /// Redraws whenever the launcher changes in the background, as
    /// `changes` (the other end of the launcher's
    /// [`with_development`](Launcher::with_development)) reports: a package
    /// being developed is building, failed to build or was reloaded, or a
    /// command another command launched opened. The window is shown when
    /// such a launch asks for it ([`Launcher::take_window_request`]).
    pub fn follow_changes(
        &mut self,
        mut changes: Changes,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.spawn_in(window, async move |this, cx| {
            while changes.next().await.is_some() {
                let shown = this.update_in(cx, |this, window, cx| {
                    if this.launcher.take_window_request() {
                        this.unhide(window, cx);
                        window.activate_window();
                        cx.activate(true);
                    }
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

    /// Has the launcher drive this window for the host functions commands
    /// call (#141): it hides the window and shows HUDs through it, as
    /// [`LauncherWindow::window_requested`] carries out.
    fn follow_window_requests(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (control, mut requests) = pane_core::feedback::channel();
        self.launcher.attach_window(control);
        cx.spawn_in(window, async move |this, cx| {
            while let Some(request) = requests.next().await {
                let done = this.update_in(cx, |this, window, cx| {
                    this.window_requested(request, window, cx);
                });
                if done.is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    /// Carries out what the launcher asked of the window for a command's
    /// host function: hides it (a command closed it, or is about to show a
    /// HUD), or shows a HUD in a window of its own.
    fn window_requested(
        &mut self,
        request: WindowRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match request {
            WindowRequest::Hide => {
                if !self.presence.hidden() {
                    self.hide(window, cx);
                }
            }
            WindowRequest::Hud(hud) => self.show_hud(hud, window, cx),
        }
        self.sync_screen(window, cx);
        cx.notify();
    }

    /// Development builds only: hides the launcher and shows `title` as a
    /// failure's HUD (3 seconds), as a command's `show-hud` would, for the
    /// opt-in native smoke of the HUD's placement
    /// (scripts/smoke-windows-hud.ps1), which names it in
    /// `PANE_TEST_SHOW_HUD`.
    #[cfg(debug_assertions)]
    #[doc(hidden)]
    pub fn show_smoke_hud(&mut self, title: String, window: &mut Window, cx: &mut Context<Self>) {
        self.window_requested(WindowRequest::Hide, window, cx);
        let hud = pane_core::Hud {
            title,
            style: pane_core::ToastStyle::Failure,
        };
        self.window_requested(WindowRequest::Hud(hud), window, cx);
    }

    /// Test support: shows `hud` as the launcher's window seam would.
    /// Test and debug builds only.
    #[cfg(any(test, debug_assertions))]
    #[doc(hidden)]
    pub fn request_hud(
        &mut self,
        hud: pane_core::Hud,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.window_requested(WindowRequest::Hud(hud), window, cx);
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
        // The open Actions panel takes Enter from the key press itself, once
        // per press ([`LauncherWindow::panel_keys`]).
        if self.actions.is_some() {
            cx.propagate();
            return;
        }
        // An item of a command's list runs its primary action once per
        // press: the key is handed on to [`LauncherWindow::item_action_keys`],
        // which sees whether it is a held key's repeat (an action cannot).
        if actions_panel::commands_list(&self.launcher.view().screen)
            && self.launcher.item_actions().is_some()
        {
            cx.propagate();
            return;
        }
        self.invoke_selected(window, cx);
    }

    /// What the invoke binding does with the selected row: submits a form,
    /// opens the Actions panel at the submenu an item's primary action
    /// opens (#140), or activates the row.
    fn invoke_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let primary_submenu = actions_panel::commands_list(&self.launcher.view().screen)
            && self
                .launcher
                .item_actions()
                .is_some_and(|actions| actions.actions.first().is_some_and(|first| first.submenu));
        if matches!(self.launcher.view().screen, Screen::Form(_)) {
            self.submit_form(window, cx);
        } else if primary_submenu {
            self.open_item_submenu(0, window, cx);
        } else {
            self.activate_selected(window, cx);
        }
    }

    /// A key pressed in the launcher while an open command's list (or its
    /// search field) has focus and nothing is open over it, before the
    /// focused control sees it: the invoke binding (which
    /// [`LauncherWindow::confirm`] hands on), Ctrl+Enter and
    /// Ctrl+Shift+Enter run the selected item's first, second and third
    /// action, and an action's own shortcut runs that action, without the
    /// Actions panel (#137); an action that opens a submenu opens the panel
    /// at that submenu instead (#140). A missing action runs nothing, and
    /// the key goes no further. Once per press: the system's repeats of a
    /// held key run nothing more.
    fn item_action_keys(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.actions.is_some()
            || self.menu.is_some()
            || !actions_panel::commands_list(&self.launcher.view().screen)
        {
            return;
        }
        let field = self.query_field().focus_handle(cx).is_focused(window);
        if !field && !self.focus_handle.is_focused(window) {
            return;
        }
        let Ok(pressed) = crate::keyboard::binding_of(&event.keystroke) else {
            return;
        };
        let Some(actions) = self.launcher.item_actions() else {
            return;
        };
        let invoke = crate::settings::keyboard_of(cx)
            .binding(pane_core::KeyboardAction::InvokeSelectedAction)
            .clone();
        let index = if pressed == invoke {
            Some(0)
        } else {
            (1..=2)
                .find(|index| pane_core::keyboard::action_key(*index).as_ref() == Some(&pressed))
                .or_else(|| actions.bound_to(&pressed))
        };
        let Some(index) = index else {
            return;
        };
        cx.stop_propagation();
        if event.is_held {
            return;
        }
        if actions
            .actions
            .get(index)
            .is_some_and(|action| action.submenu)
        {
            self.open_item_submenu(index, window, cx);
        } else if index == 0 {
            self.activate_selected(window, cx);
        } else {
            self.motion.land_at_once();
            let pending = self.launcher.run_selected_action(index);
            self.show_until_done(pending, window, cx);
        }
    }

    pub(crate) fn back(&mut self, _: &Back, window: &mut Window, cx: &mut Context<Self>) {
        // The back key's order, as the specification states it: an active
        // IME composition in the focused field is cancelled first, then an
        // open footer menu is dismissed, and only then does the key leave
        // the screen — one level at a time, clearing a command search's
        // text before leaving it and root search's before hiding the
        // launcher when it is already empty. (The menu's own Escape is
        // bound deeper still, in the menu's context, so it never reaches
        // here; this arm is for whatever key back is rebound to.)
        if self.cancel_composition(window, cx)
            || self.close_open_menu(window, cx)
            || self.close_actions(window, cx)
        {
            return;
        }
        // Clipboard History's own list, shown from its split view: back to
        // the split view first.
        if self.leave_clipboard_controls(window, cx) {
            return;
        }
        // The Keyboard page's escape behavior: hide from wherever the
        // launcher is, or go back one level and hide from an empty root
        // search.
        let hides =
            crate::settings::shared(cx).read(cx).escape() == pane_core::EscapeBehavior::Hide;
        if hides
            || matches!(&self.launcher.view().screen, Screen::Root { query } if query.is_empty())
        {
            self.hide(window, cx);
            return;
        }
        self.launcher.back();
        // Backing out is the one navigation that leaves a view, and it
        // lands at once: the next frame drops any arrival in flight and
        // starts none.
        self.motion.land_at_once();
        self.sync_screen(window, cx);
        cx.notify();
    }

    /// Returns to root search from wherever the launcher is — the state a
    /// summoned launcher starts from — leaving every open screen at once,
    /// as the back key leaves them one at a time.
    pub(crate) fn return_to_root(
        &mut self,
        _: &ReturnToRoot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.close_open_menu(window, cx) || self.close_actions(window, cx) {
            return;
        }
        self.launcher.show_root_search();
        // Leaving however many screens were open lands at once, as
        // backing out does.
        self.motion.land_at_once();
        self.sync_screen(window, cx);
        cx.notify();
    }

    /// Hides the launcher — hidden, not closed: Pane keeps running in the
    /// background, the Settings window stays open, the global hotkeys stay
    /// registered, and the Open Pane hotkey shows the same window and the
    /// same launcher again. In the Settings window the platform's close
    /// shortcut closes only that window.
    pub(crate) fn dismiss(
        &mut self,
        _: &DismissLauncher,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.hide(window, cx);
    }

    /// Cancels the IME composition active in one of this window's fields
    /// — the query field, or the open form's text fields — discarding its
    /// marked text, if one is active: the next key is free to act, as it
    /// is on a platform whose input method takes the key itself. Whether
    /// one was cancelled.
    fn cancel_composition(&self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let mut fields = vec![self.query_field()];
        if let Some(form) = &self.form {
            fields.extend(form.text_fields());
        }
        for input in fields {
            let marked = input.update(cx, |input, cx| input.marked_text_range(window, cx));
            if let Some(marked) = marked {
                input.update(cx, |input, cx| {
                    input.replace_text_in_range(Some(marked), "", window, cx)
                });
                return true;
            }
        }
        false
    }

    /// Closes the footer menu if it is open, restoring the focus it took.
    /// Whether it was open.
    pub(crate) fn close_open_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.menu.is_some() {
            self.close_menu(window, cx);
            return true;
        }
        false
    }

    /// Shows the package in `folder` with its identity and compatibility,
    /// redrawing when the check finishes.
    pub fn preview_package(&mut self, folder: &Path, window: &mut Window, cx: &mut Context<Self>) {
        self.motion.pointer_open();
        let pending = self.launcher.preview_package(folder);
        self.show_until_done(pending, window, cx);
    }

    /// Downloads and shows the npm package `spec` names, as
    /// [`LauncherWindow::preview_package`] shows a folder.
    pub fn preview_npm(&mut self, spec: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.motion.pointer_open();
        let pending = self.launcher.preview_npm(spec);
        self.show_until_done(pending, window, cx);
    }

    /// Fetches and shows the revision of the Git repository `spec` names,
    /// as [`LauncherWindow::preview_package`] shows a folder.
    pub fn preview_git(&mut self, spec: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.motion.pointer_open();
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
        self.choose_folder(
            "Choose",
            move |this, folder, window, cx| {
                let pending = this.launcher.grant_folder(&identity, folder);
                this.show_until_done(pending, window, cx);
            },
            window,
            cx,
        );
    }

    /// Asks for a package folder with the platform's folder picker, then
    /// previews it. Cancelling leaves root search as it was.
    fn choose_package_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.choose_folder(
            "Install",
            |this, folder, window, cx| this.preview_package(folder, window, cx),
            window,
            cx,
        );
    }

    /// Asks for a folder with the platform's folder picker, its button
    /// saying `prompt`, then hands the chosen one to `chosen`. Cancelling
    /// does nothing; a picker that cannot open is reported.
    fn choose_folder(
        &mut self,
        prompt: &'static str,
        chosen: impl FnOnce(&mut Self, &Path, &mut Window, &mut Context<Self>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(prompt.into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let folder = match picked.await {
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
                this.update_in(cx, |this, window, cx| chosen(this, &folder, window, cx))
                    .ok();
            }
        })
        .detach();
    }

    /// Launches the command whose global hotkey `shortcut` is, as the
    /// system reported it pressed while any application had focus: the
    /// window comes to the front and shows a view command. A no-view
    /// command runs without the window (ADR 0037): a hidden window stays
    /// hidden, and a shown one stays as it is. A press that launches
    /// nothing (a hotkey released meanwhile) leaves the window where it is.
    /// The Open Pane hotkey is not a command's: its press summons, focuses
    /// or hides the launcher itself ([`LauncherWindow::open_pane_pressed`]).
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
        let shows_window = self.launcher.hotkey_shows_window(shortcut);
        let Some(pending) = self.launcher.press_hotkey(shortcut) else {
            return;
        };
        if shows_window {
            self.unhide(window, cx);
            window.activate_window();
            cx.activate(true);
            // A hotkey is a keyboard open: the command's view lands at once.
            self.motion.land_at_once();
        }
        self.show_until_done(pending, window, cx);
    }

    /// Shows the window if the Open Pane hotkey hid it: every activation
    /// of the launcher — the hotkey's show path, a command's hotkey, an
    /// entry point that reaches the launcher from Settings — must find a
    /// visible window, opened on the display the placement resolves.
    fn unhide(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.launcher.set_window_presence(WindowPresence::Shown);
        if self.presence.show() {
            window.set_visible(true);
            // The pointer is wherever it is now: the next event records it.
            self.pointer = None;
            // A window that just appeared has nothing to arrive from: its
            // first frame draws whatever it shows settled, as the hotkey's
            // show is itself never animated. A view that changes after
            // that frame (a command's reply) still arrives as usual.
            self.motion.window_shown();
            // The launcher is opening: it is placed on the display the
            // Launcher page's choice resolves to, wherever the window was
            // left. Only this window is moved — the Settings window, which
            // shares nothing of the launcher's lifecycle, stays where the
            // user put it.
            self.place(window, cx);
        }
    }

    /// Hides the launcher window: the Open Pane hotkey's hide path,
    /// Escape's end at root search, and the dismiss binding and back key
    /// the Keyboard page can rebind all come here. Hidden, not closed —
    /// Pane keeps running, the Settings window stays open, and the next
    /// opening reuses the same live window and launcher.
    fn hide(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        window.set_visible(false);
        self.presence.hide(cx.background_executor().now());
        // A toast shown from now on is a HUD (#141).
        self.launcher.set_window_presence(WindowPresence::Hidden);
        self.pointer = None;
        // A hidden launcher keeps nothing armed for whatever shows next.
        self.motion.land_at_once();
        self.end_numbers(cx);
        cx.notify();
    }

    /// Places the launcher window on the display the Launcher page's
    /// opening-monitor choice resolves to, centered in that display's
    /// usable area. A choice whose display is gone — disconnected, or one
    /// this system does not tell Pane about — falls back to the primary
    /// display, or the first one there is, so the launcher opens with its
    /// controls reachable; a platform that cannot move a window at all
    /// leaves it where it is, and the page explains that rather than
    /// pretending the choice applied.
    fn place(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Collapsed to its search field, the launcher is placed as its
        // expanded size would be, so it grows downward from where it is.
        let size = self
            .presence
            .placement_size(window_size(window.bounds().size));
        self.place_sized(gpui::size(px(size.width), px(size.height)), window, cx);
    }

    /// Places the launcher window as [`LauncherWindow::place`] does, for a
    /// window of `size`: the size it is about to take (the Clipboard
    /// History view's), which the window reports only once the system has
    /// resized it.
    pub(crate) fn place_sized(
        &mut self,
        size: Size<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let choice = crate::settings::shared(cx).read(cx).opening_monitor();
        let placement = crate::placement::shared(cx);
        let layout = placement.layout();
        let Some(resolved) = pane_core::placement::resolve(&layout, choice) else {
            return;
        };
        // The window's size in the layout's own units, so the placement is
        // computed in the space its displays are measured in; a window
        // keeps that size as it moves.
        let units = crate::placement::units_per_pixel(window);
        let bounds = resolved.display.window_bounds(pane_core::placement::Size {
            width: size.width.as_f32() * units,
            height: size.height.as_f32() * units,
        });
        // A move that failed is said, not hidden: the launcher's own status
        // line carries it, so an opening that did not go where the choice
        // says is never mistaken for one that did.
        if let Err(why) = placement.place(window, bounds) {
            self.launcher.show_error(why);
            cx.notify();
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
        match self.presence.press(now, window.is_window_active()) {
            // The repeat of a key still held, not a new press.
            Press::Repeat => {}
            Press::Hide => self.hide(window, cx),
            Press::Summon => self.summon(window, cx),
        }
    }

    /// Shows and focuses the launcher: the hotkey's show path, and the
    /// one the tray's Open Pane takes — an entry point that reaches the
    /// launcher from outside Pane's own windows must find a visible,
    /// focused launcher. It never hides, whatever state the launcher
    /// was in: only the hotkey toggles, because its press is the user's
    /// other hand on the same control; the tray's item says Open Pane
    /// and does only that.
    fn summon(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // What the summoned launcher starts from is the Launcher page's
        // reopening choice. Restoring the view keeps whatever the launcher
        // was left showing — a search, a command's list, a form — when it
        // is still a view there is something to return to, with the
        // search focused where the view holds one and its own focus kept
        // where it does not (the window never lost it); the root-search
        // choice, or a view whose command is gone, starts from root
        // search. Nothing of the window that had focus before reaches in
        // here (the Settings window's focus is not the launcher's), and
        // nothing is run. Whether it pops counts the time the launcher was
        // hidden, so it is asked before the launcher is shown.
        // A command that closed the launcher may have asked to keep its
        // screen whatever the choice says (`suspended`, #141).
        let reopening = crate::settings::shared(cx).read(cx).reopening();
        let now = cx.background_executor().now();
        let pops = match self.launcher.take_next_showing() {
            NextShowing::BySetting => self.presence.pops_to_root(reopening.pops_after(), now),
            NextShowing::Restore => false,
        };
        self.unhide(window, cx);
        window.activate_window();
        cx.activate(true);
        if pops || !self.launcher.restorable_view() {
            self.launcher.show_root_search();
            // Popping to root search leaves the open views, as
            // `return_to_root` does: it lands at once.
            self.motion.land_at_once();
            self.sync_screen(window, cx);
        } else if self.launcher.view().search_field().is_some() {
            self.query.focus(window, cx);
        }
        cx.notify();
    }

    /// One selection of the native tray or menu-bar entry, as its adapter
    /// reported the menu's choice ([`pane_core::tray`]): Open Pane summons
    /// the launcher — shown and focused, never hidden, so the entry stays
    /// usable while the launcher is hidden and never starts a second
    /// window; Settings opens or focuses the one Settings window, which
    /// shares this launcher, so no second window or extension runtime
    /// comes of it; Quit ends Pane explicitly — Pane's own native
    /// resources, the tray entry and the global hotkey registrations, go
    /// first, and the quit hooks then stop the runtime's helpers and the
    /// development watches, as closing the launcher's window does.
    pub fn tray_selected(
        &mut self,
        action: TrayAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            TrayAction::OpenPane => self.summon(window, cx),
            TrayAction::Settings => {
                settings::open(&self.launcher, cx);
            }
            TrayAction::Quit => {
                // Pane's own resources are removed deliberately — a quit
                // that ended the process might never run a destructor —
                // and then the same quit path closing the launcher's
                // window is taken.
                crate::settings::shared(cx).update(cx, |settings, _| settings.release_tray());
                self.launcher.release_hotkeys();
                cx.quit();
            }
        }
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
                self.motion.land_at_once();
                self.show_until_done(pending, window, cx);
            }
            Err(problem) => {
                self.launcher.show_error(problem);
                cx.notify();
            }
        }
    }

    pub(crate) fn focus_next(
        &mut self,
        _: &FocusNext,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus_next(cx);
    }

    pub(crate) fn focus_previous(
        &mut self,
        _: &FocusPrevious,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus_prev(cx);
    }

    /// The open-Settings binding (Cmd+, on macOS / Ctrl+, elsewhere by
    /// default, rebindable on the Keyboard page): opens or focuses the
    /// Settings window, the same one the footer menu and the root result
    /// open.
    pub(crate) fn open_settings(
        &mut self,
        _: &OpenSettings,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        settings::open(&self.launcher, cx);
    }

    /// Starts the selected row's action and redraws when the guest answers,
    /// without blocking the window meanwhile. The view it opens lands at
    /// once: Enter is the launcher's most repeated key, and no keyboard
    /// open animates. It disarms whatever an earlier click left armed (a
    /// click that opened nothing — Settings, an app — changes no screen,
    /// so nothing used its arrival up); a pointer click arms the arrival
    /// again after calling this (see [`crate::ui::motion`]).
    pub(crate) fn activate_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.motion.land_at_once();
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
        // A click in the Settings window: the view it opens arrives, as a
        // click on the row would — unless the launcher was hidden, when
        // the window's first frame draws it settled (see `unhide`).
        let was_shown = !self.presence.hidden();
        // Root search is reached as Escape reaches it, one screen back at a
        // time, wherever the launcher is (a form, a command, the extension
        // list Settings entered); every `back` moves toward root search,
        // and at root search this stops.
        while !matches!(self.launcher.view().screen, Screen::Root { .. }) {
            self.launcher.back();
        }
        // Root search may have been left filtered by a query that hides
        // the row the page drew (Settings lists every root result): it is
        // shown unfiltered, as a summoned launcher starts, so the row is
        // there to find.
        if self
            .launcher
            .view()
            .query()
            .is_some_and(|query| !query.is_empty())
        {
            self.launcher.show_root_search();
            self.sync_screen(window, cx);
        }
        self.unhide(window, cx);
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
        if was_shown {
            self.motion.pointer_open();
        }
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
    fn keep_selected_visible(
        &mut self,
        view: &LauncherView,
        presentation: &Presentation,
        window: &mut Window,
        cx: &App,
    ) {
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
            // The list's children are its rows with the section labels
            // between them, after the pinned home's while it shows (a blank
            // query's) and after root search's no-results notice while it
            // shows (over the fallbacks, whichever is selected). (Another
            // screen's empty line shows only while nothing is selected,
            // when nothing is scrolled to.)
            let home = self.home_children(view, cx);
            let notice = root_search::layouts::nothing_found(&view.screen, presentation);
            let labels = section_labels(presentation);
            let child =
                home + root_search::layouts::child_of_row(notice.is_some(), &labels, selected);
            self.scroll.scroll_to_item(child);
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
        // The Actions panel belongs to root search: a screen that replaced
        // it (a hotkey pressed, a change from Settings) takes the panel
        // with it, and its own focus and submenus with it.
        if !matches!(self.launcher.view().screen, Screen::Root { .. })
            && self.actions.take().is_some()
        {
            self.launcher.close_submenus();
        }
        self.sync_form(window, cx);
        self.sync_custom_view(window, cx);
        // Last: coming back to root search, even as a view closes, focuses
        // the query rather than the list.
        self.sync_root_search(window, cx);
        // After it: the Clipboard History view focuses its own search.
        self.sync_clipboard_history(window, cx);
        self.sync_home(cx);
        cx.refresh_windows();
    }

    /// Freezes root search's selection against the pointer, or lets it
    /// follow the pointer again: while frozen, moving over a row or
    /// clicking one changes no selection, so the target a layer over the
    /// list acts on (the contextual Actions panel, #95) stays the one it
    /// opened for. The keys still move the selection. Test support too.
    #[doc(hidden)]
    pub fn freeze_pointer_selection(&mut self, frozen: bool, cx: &mut Context<Self>) {
        self.pointer_selection_frozen = frozen;
        cx.notify();
    }

    /// Whether the pointer may not move root search's selection now: it
    /// is frozen, or the footer menu or the Actions panel is open over the
    /// row it acts on.
    fn pointer_selection_held(&self) -> bool {
        self.pointer_selection_frozen || self.menu.is_some() || self.actions.is_some()
    }

    /// The pointer moved over root search's row `index` to `position`:
    /// real movement selects the row, as the reference's root does, so
    /// the footer and Enter act on what the pointer is on. An event that
    /// repeats the last position is not movement — a pointer resting on a
    /// row never undoes the keys' selection — and nothing moves while the
    /// selection is held (see `pointer_selection_held`).
    fn pointer_moved_over(
        &mut self,
        index: usize,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let moved = self.pointer.is_some_and(|last| last != position);
        if !moved || self.pointer_selection_held() {
            return;
        }
        if self.launcher.selected() != Some(index) {
            self.select_under_pointer(index);
            cx.notify();
        }
    }

    /// Selects root search's row `index` for the pointer, without
    /// scrolling the list: the row is where the pointer is, and scrolling
    /// a half-shown row into view under a still pointer would put another
    /// row under it, which the next small movement would select and
    /// scroll in turn. Only the keys' selection scrolls, as the
    /// reference's does.
    fn select_under_pointer(&mut self, index: usize) {
        self.launcher.select(index);
        if let Some(scrolled_for) = self.scrolled_for.as_mut() {
            scrolled_for.selected = Some(index);
        }
    }

    /// A click on root search's row `index`: the selected row runs; an
    /// unselected one — clicked where the pointer has not moved since the
    /// keys moved the selection — is selected first, as the reference's
    /// does. A pointer that moved onto the row selected it already, so an
    /// ordinary click runs it, once.
    fn click_root_row(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.pointer_selection_held() {
            return;
        }
        if self.launcher.selected() == Some(index) {
            self.activate_selected(window, cx);
            self.motion.pointer_open();
        } else {
            self.select_under_pointer(index);
            cx.notify();
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn render_row(
        &self,
        index: usize,
        row: Row,
        selected: bool,
        shown: RowPresentation,
        root: bool,
        number: Option<(usize, f32)>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let visuals = crate::settings::launcher_visuals(cx);
        let theme = &visuals.theme;
        let reason = row.unavailable.as_ref().map(|u| u.reason().to_owned());
        // The row's accessible description: its subtitle and, when it
        // cannot run, the reason, together.
        let description = match (&row.subtitle, &reason) {
            (Some(subtitle), Some(reason)) => Some(format!("{subtitle}. {reason}")),
            (subtitle, reason) => subtitle.clone().or(reason.clone()),
        };
        // Root search's rows carry what the launcher knows beyond the
        // title: where the query matched, the alias and the hotkey the
        // user gave the command, and its kind.
        let keys = shown.hotkey.as_ref().map(crate::keyboard::hotkey_keys);
        // The command's hotkey is the row's shortcut for assistive
        // technology too.
        let shortcut = keys.as_ref().map(|keys| keys.name());
        let meta = RowMeta {
            matched: shown.matched,
            alias: shown.alias.map(SharedString::from),
            keys,
            // Root search's rows always keep the kind's column, empty
            // where the launcher names no kind, so the alias and keys of
            // every row line up against it, as the reference's do.
            kind: if root {
                Some(shown.kind.map_or("", |kind| kind.label()).into())
            } else {
                None
            },
            number,
        };
        // Presentation only: the shared row paints the chrome, and the
        // identity, accessibility and click behavior are attached here.
        result_row_with(
            RowContent {
                title: row.title.clone().into(),
                subtitle: row.subtitle.clone().map(SharedString::from),
                unavailable_reason: reason.map(SharedString::from),
                selected,
                unavailable_id: ("unavailable", index).into(),
                icon: Some(row_icon(&row.id)),
            },
            meta,
            theme,
        )
        .id(("row", index))
        // While held, the row takes the stronger wash of its hover, or of
        // its selected wash, at once.
        .active({
            let press = crate::ui::result_row::pressed_wash(selected, theme);
            move |row| row.bg(press)
        })
        // Every row's washes change at once, as the reference's do (#100:
        // a command's rows share root search's visuals). Root search's
        // rows also select under the moving pointer; a command's rows keep
        // their click-runs semantics.
        .when(root, |row| {
            row.on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
                this.pointer_moved_over(index, event.position, cx);
            }))
        })
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
        .when_some(shortcut, |row, shortcut| row.aria_keyshortcuts(shortcut))
        .on_click(
            cx.listener(move |this, event: &gpui::ClickEvent, window, cx| {
                if root {
                    this.click_root_row(index, window, cx);
                } else if event.click_count() <= 1 {
                    // A double click's second click runs nothing more.
                    this.launcher.select(index);
                    this.activate_selected(window, cx);
                    this.motion.pointer_open();
                }
            }),
        )
    }

    /// Root search's row `index`, a computed answer, drawn as the answer
    /// card (#96): the row's identity, selection, pointer and click are
    /// the result row's, and so are its accessibility — named for what
    /// was typed and its answer ("6*7 = 42"), described by the row's
    /// subtitle — and its dispatch, which copies the answer.
    fn render_answer(
        &self,
        index: usize,
        row: Row,
        answer: &ComputedAnswer,
        selected: bool,
        number: Option<(usize, f32)>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let visuals = crate::settings::launcher_visuals(cx);
        let card = root_search::layouts::answer_card(answer, selected, &visuals.theme);
        match number {
            Some((number, look)) => {
                crate::ui::result_row::with_number_hint(card, number, look, &visuals.theme)
            }
            None => card,
        }
        .id(("row", index))
        // While held, the card takes the stronger wash of its own fill: it
        // has no hover or selected wash to derive one from (its selection
        // is a ring).
        .active({
            let press = pressed(visuals.theme.results.card_fill);
            move |card| card.bg(press)
        })
        .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
            this.pointer_moved_over(index, event.position, cx);
        }))
        .debug_selector(|| format!("row-{}", row.title))
        .role(Role::ListBoxOption)
        .aria_label(root_search::layouts::answer_label(answer))
        .aria_selected(selected)
        .when(selected, |card| card.aria_active_descendant())
        .when_some(row.subtitle, |card, subtitle| {
            card.aria_description(subtitle)
        })
        .on_click(cx.listener(move |this, _, window, cx| {
            this.click_root_row(index, window, cx);
        }))
    }

    /// The footer's right-hand buttons: the selected action's button,
    /// when the screen has a primary action at all (a custom view, the
    /// network details screen and a hotkey screen with nothing to remove
    /// have none) and no status shows, and on root search and a command's
    /// list the Actions button. `action` is the launcher's one
    /// selected-action definition ([`Launcher::selected_action`]): on a
    /// command's list it names the selected item's primary action.
    fn footer_buttons(
        &self,
        action: &SelectedAction,
        with_actions: bool,
        status: bool,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Vec<gpui::AnyElement> {
        let keyboard = crate::settings::shared(cx).read(cx).keyboard().clone();
        let primary = (!action.label.is_empty() && !status).then(|| {
            let invoke = keyboard.binding(pane_core::KeyboardAction::InvokeSelectedAction);
            action_button(action, invoke, theme)
                .on_click(cx.listener(|this, event: &gpui::ClickEvent, window, cx| {
                    // A double click's second click runs nothing more.
                    if event.click_count() <= 1 {
                        this.press_primary_action(window, cx);
                    }
                }))
                .into_any_element()
        });
        let actions = with_actions.then(|| {
            let open = crate::keyboard::binding_keys(
                keyboard.binding(pane_core::KeyboardAction::OpenActions),
            );
            footer::actions_button(&open, self.actions.is_some(), theme)
                .on_click(cx.listener(|this, _, window, cx| {
                    this.toggle_actions(&crate::OpenActions, window, cx);
                }))
                .into_any_element()
        });
        footer::buttons(primary, actions, theme)
    }

    /// The footer's hint while no status shows: on root search or a
    /// command's list with Actions open, "Type to filter actions · Esc goes
    /// back"; nothing elsewhere.
    fn footer_hint(&self, with_actions: bool, theme: &Theme) -> Option<Div> {
        // At rest the footer's buttons already show the keys; the hint says
        // only how the open Actions panel is used.
        if !with_actions || self.actions.is_none() {
            return None;
        }
        Some(footer::hint_line(
            footer::actions_hint(crate::keyboard::escape_keys()),
            theme,
        ))
    }

    /// Arms the next view change to arrive, for a pointer open: the click
    /// that ran it calls this after the activation (which disarms, as every
    /// keyboard open does), and the screen change it causes uses it up.
    pub(crate) fn arm_arrival(&mut self) {
        self.motion.pointer_open();
    }

    /// Navigates forward to the screen the launcher now shows, as
    /// activating a row does — landing at once, as a keyboard open does
    /// (a pointer caller arms the arrival after this returns).
    pub(crate) fn navigate_forward(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.motion.land_at_once();
        self.sync_screen(window, cx);
        cx.notify();
    }

    /// Dispatches the footer button's click: the same
    /// [`LauncherWindow::confirm`] path the invoke binding's key takes,
    /// but only when the selected-action definition says the action can
    /// run now. The frame that drew the button can be stale — an action
    /// may have started since it was laid out — so the check is made
    /// again here, at click time, against the launcher's current state.
    /// The binding's key is unchanged: it keeps the behavior it has
    /// always had; this keeps the button from dispatching what cannot
    /// run (no selection, an unavailable result, an action already
    /// running).
    pub(crate) fn press_primary_action(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.launcher.selected_action().available {
            self.invoke_selected(window, cx);
        }
    }

    /// Whether the last frame drew the launcher collapsed to its search
    /// field (and the pins' row, if shown): its rows are hidden then.
    pub(crate) fn is_collapsed(&self) -> bool {
        self.presence.is_collapsed()
    }

    /// Whether the launcher shows only its search field: the compact
    /// window mode, at root search with a blank query, with nothing open
    /// over it and no status to say.
    fn collapses(&self, view: &LauncherView, cx: &App) -> bool {
        crate::settings::shared(cx).read(cx).window_mode() == pane_core::WindowMode::Compact
            && quick_slots::home_shown(view)
            && matches!(view.status, Status::Idle)
            && self.actions.is_none()
            && self.menu.is_none()
    }

    /// Fits the window to the window mode as `view` is drawn: collapsing to
    /// the search field's height — and the pins' row under it, while the
    /// Launcher page shows the pins in Compact mode and something is pinned
    /// (see [`compact_pins`]) — keeps the size it had, which expanding gives
    /// back. The window's top stays where it is, so the results grow
    /// downward from the search field. The window's own height is what is
    /// compared, so a size another view gave it (Clipboard History's) is
    /// fitted too, and so is a collapsed height the switch or the pins no
    /// longer call for. Whether it is collapsed.
    fn fit_window_mode(&mut self, view: &LauncherView, window: &mut Window, cx: &App) -> bool {
        let collapsed = self.collapses(view, cx);
        let bar = crate::settings::visuals(cx).theme.geometry.search_height;
        let (width, height) = shell::LAUNCHER_CLIENT;
        let resize = self.presence.fit(Fit {
            collapses: collapsed,
            // The pins are resolved only while the window collapses.
            shows_pins: collapsed && self.shows_compact_pins(cx),
            bar: f32::from(bar),
            pin_row: compact_pins::ROW_HEIGHT,
            current: window_size(window.viewport_size()),
            default_expanded: WindowSize { width, height },
        });
        if let Some(size) = resize {
            window.resize(gpui::size(px(size.width), px(size.height)));
        }
        collapsed
    }
}

impl Render for LauncherWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Pane's own keys as they are bound now, which no action shortcut
        // of a command's list may take (#137).
        self.launcher.set_pane_keys(pane_core::PaneKeys::new(
            &crate::settings::keyboard_of(cx),
            crate::settings::navigation_of(cx),
        ));
        let (view, presentation) = self.launcher.presented_view();
        #[cfg(any(test, debug_assertions))]
        {
            self.drawn = Some(view.clone());
        }
        // The toast the footer shows, if any, and its time (#141).
        let toast = self.footer_toast(&view.status);
        self.time_toast(toast.as_ref(), window, cx);
        // Pane's Clipboard History draws its own split view (#102).
        if let Some(split) = self.render_clipboard_history(&view, cx) {
            return split;
        }
        // The compact window mode shows only the search field until
        // something is typed.
        let collapsed = self.fit_window_mode(&view, window, cx);
        // Collapsed to its search field, the launcher has no footer for a
        // toast: one shown is a HUD (#141).
        if !self.presence.hidden() {
            self.launcher.set_window_presence(if collapsed {
                WindowPresence::Compact
            } else {
                WindowPresence::Shown
            });
        }
        self.keep_selected_visible(&view, &presentation, window, cx);
        // What moves this frame — the arriving content, the footer menu
        // popup's entrance or exit, the number hints' slide — and whether
        // another frame is needed; see [`FrameMotion`] and
        // `crate::ui::motion` for the whole policy.
        let frame = self.motion.frame(
            discriminant(&view.screen),
            self.menu.is_some(),
            cx.reduce_motion(),
            cx.background_executor().now(),
        );
        let (arriving, menu_in_flight, numbers) = (frame.arriving, frame.menu_popup, frame.numbers);
        // The background image's backdrop, baked for this window's scale
        // (ADR 0028); the visuals below are over it once it is ready.
        let scale = window.scale_factor();
        crate::settings::shared(cx).update(cx, |settings, cx| settings.request_backdrop(scale, cx));
        let visuals = crate::settings::launcher_visuals(cx);
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
        // A blank query's pinned home, above the rows (read before the
        // status moves out of the view).
        let home = self.render_home(&view, numbers, &theme, cx);
        // Each row's number while Ctrl is held.
        let slots = self.numbered_slots();
        let row_numbers: Vec<Option<usize>> = (0..view.rows.len())
            .map(|index| row_number(&view, &slots, index))
            .collect();
        // The footer's status: while the launcher runs, works, answers or
        // fails, the strip is that message; `None` while it is idle, when
        // the strip becomes the selected action (below).
        // Whether an action runs or the status line has something to say:
        // the primary action steps aside then, toast or not.
        let status_busy = view.status != Status::Idle;
        let (status_selector, status, status_color): (&str, Option<SharedString>, Hsla) =
            match view.status {
                // A toast speaks where the status line would (#141).
                _ if toast.is_some() => ("status-toast", None, theme.text_body),
                Status::Idle => ("status-idle", None, theme.text_muted),
                Status::Running => ("status-running", Some("Running…".into()), theme.warning),
                Status::Progress(work) => ("status-progress", Some(work.into()), theme.warning),
                Status::Result(answer) => ("status-result", Some(answer.into()), theme.success),
                Status::Error(message) => ("status-error", Some(message.into()), theme.danger),
            };
        // The strip's name for assistive technology: the status, or the
        // toast's title and message.
        let announced: Option<SharedString> = match &toast {
            Some(shown) => Some(shown.toast.text().into()),
            None => status.clone(),
        };
        // The toast's actions take the footer's buttons' place.
        let toast_buttons = match &toast {
            Some(shown) => self.toast_buttons(shown, &theme, cx),
            None => Vec::new(),
        };
        let toast_middle = toast
            .as_ref()
            .map(|shown| self.render_toast(shown, &theme, cx).into_any_element());
        // The selected action: the one definition ([`SelectedAction`])
        // that drives the idle strip's button — its label, its
        // availability — and the dispatch both the button and Enter take.
        let action = self.launcher.selected_action();
        let root = matches!(view.screen, Screen::Root { .. });
        // Root search and a command's list have the Actions panel.
        let with_actions = root || actions_panel::commands_list(&view.screen);
        // Root search's notice when nothing but fallbacks is listed for
        // its query (#96).
        let notice = root_search::layouts::nothing_found(&view.screen, &presentation);
        // The rows — a computed answer as its card — with each section's
        // label ahead of its first row, after the notice.
        let rows: Vec<gpui::AnyElement> = view
            .rows
            .into_iter()
            .enumerate()
            .map(|(index, row)| {
                let selected = view.selected == Some(index);
                let shown = presentation.rows.get(index).cloned().unwrap_or_default();
                let number = row_numbers[index]
                    .filter(|_| numbers > 0.)
                    .map(|number| (number, numbers));
                if let Some(answer) = &shown.answer {
                    return self
                        .render_answer(index, row, answer, selected, number, cx)
                        .into_any_element();
                }
                self.render_row(index, row, selected, shown, root, number, cx)
                    .into_any_element()
            })
            .collect();
        let rows = root_search::layouts::list_children(
            notice
                .as_ref()
                .map(|copy| root_search::layouts::notice(copy, &theme).into_any_element()),
            rows,
            &section_labels(&presentation),
            &theme,
        );
        let empty = match &view.screen {
            Screen::CommandSearch { .. } if search_failed => div().id("empty"),
            Screen::CommandSearch { query } if !query.trim().is_empty() => div()
                .id("no-results")
                .debug_selector(|| "no-results".into())
                .child(format!("No results for “{}”", query.trim())),
            _ => div().id("empty").child(empty),
        };
        let list = shell::result_list(&theme)
            .aria_label(match view.screen {
                Screen::Root { .. } => "Results".into(),
                Screen::CommandSearch { .. } => format!("{} results", view.title),
                _ => view.title.clone(),
            })
            .track_scroll(&self.scroll)
            // The pinned home over a blank query, above the rows (#101).
            .when_some(home, |list, home| list.children(home))
            // Above the rows, with none selected: a command's search found
            // nothing, or a screen has no rows. (Root search's notice for a
            // query is among the rows' children, above its fallbacks.)
            .when(notice.is_none() && view.selected.is_none(), |rows| {
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
            _ => Some(shell::screen_heading(view.title.clone(), &theme)),
        };
        // Whether the result list is what scrolls: a form and a custom
        // view scroll their own content, which the background image does
        // not follow.
        let listed = !matches!(view.screen, Screen::Form(_) | Screen::CustomView(_));

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
            // While the Actions panel is open, its dimmer lies over the
            // results — between the search header and the footer — and
            // takes no input.
            // Collapsed, root search is its search field alone — with the
            // pins' row under it, where the Launcher page shows them.
            Screen::Root { query } if collapsed => {
                let pins = self.render_compact_pins(numbers, &theme, cx);
                self.render_search(
                    query,
                    root_search::ROOT_PLACEHOLDER,
                    div().children(pins),
                    cx,
                )
            }
            Screen::Root { query } => {
                let results = actions_panel::dimmed(
                    motion::arriving(list, arriving).into_any_element(),
                    self.actions.is_some(),
                    &theme,
                );
                self.render_search(query, root_search::ROOT_PLACEHOLDER, results, cx)
            }
            // The opened command's own search field, the same control.
            Screen::CommandSearch { query } => {
                let results = actions_panel::dimmed(
                    motion::arriving(list, arriving).into_any_element(),
                    self.actions.is_some(),
                    &theme,
                );
                self.render_search(query, root_search::COMMAND_PLACEHOLDER, results, cx)
            }
            // The list holds keyboard focus; the selected row is its active
            // descendant, and key actions bubble to the root. A command's
            // list is dimmed under its open Actions panel, as root search is.
            _ => actions_panel::dimmed(
                motion::arriving(list.track_focus(&self.focus_handle), arriving).into_any_element(),
                self.actions.is_some(),
                &theme,
            ),
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
            .on_action(cx.listener(Self::return_to_root))
            .on_action(cx.listener(Self::dismiss))
            .on_action(cx.listener(Self::open_settings))
            .on_action(cx.listener(Self::toggle_actions))
            .on_action(cx.listener(Self::focus_toast))
            .map(|content| Self::on_quick_slot_keys(content, cx))
            // A toast's actions' shortcuts first: the toast is what was
            // said last (#141).
            .capture_key_down(cx.listener(Self::toast_action_keys))
            .capture_key_down(cx.listener(Self::item_action_keys))
            .on_action(cx.listener(Self::focus_next))
            .on_action(cx.listener(Self::focus_previous))
            .on_modifiers_changed(cx.listener(Self::modifiers_changed))
            .on_key_down(cx.listener(Self::key_down))
            // Bubbling after the rows' own handlers, so a row compares the
            // event against the position before it.
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, _| {
                this.pointer = Some(event.position);
            }))
            .flex_1()
            .min_h(px(0.))
            .flex()
            .flex_col()
            .font_family(theme.typography.family.clone())
            .font_features(theme.typography.features.clone())
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
            .when(!collapsed, |content| {
                content.child(
                    // The footer: the launcher's status strip (see
                    // [`crate::ui::footer`]). On the left, the Pane mark (the
                    // app menu's button) and the hint — or, while a status
                    // shows (running, progress, a result or an error), the
                    // message instead, wrapping, growing and scrolling as it
                    // always has; on the right, the selected result's primary
                    // action and, on root search, Actions. The strip keeps its
                    // identity (id, role, status-* debug selectors) in every
                    // shape, so a test or a smoke can always find the
                    // launcher's footer where it was. The open menu's popup and
                    // the open Actions panel are the strip's first children:
                    // their capture-phase dismissal runs before the buttons'
                    // click tracking, while their own bounds stay above the
                    // strip (see `footer_menu` and `actions_panel`).
                    Material::footer(&theme)
                        .id("status")
                        // The popups are anchored to the strip (above its top
                        // edge, however tall the message has grown it), and
                        // the strip never scrolls — the message's viewport
                        // below does — so the buttons and the popups above
                        // them stay put while the message scrolls.
                        .relative()
                        .when_some(
                            self.render_menu_popup_layer(menu_in_flight, cx),
                            |strip, popup| strip.child(popup),
                        )
                        .when_some(self.render_actions_layer(cx), |strip, panel| {
                            strip.child(panel)
                        })
                        // The strip is the live region: it carries the
                        // message as its name, so assistive technology
                        // announces it. While idle the strip carries no
                        // message and stays silent.
                        .role(Role::Status)
                        .when_some(announced, |footer, text| footer.aria_label(text))
                        .debug_selector(|| status_selector.into())
                        .text_size(theme.typography.footer_size)
                        .text_color(status_color)
                        .child(footer::footer_row(
                            self.render_menu_button(&theme, cx).into_any_element(),
                            match (toast_middle, status.clone()) {
                                // The toast, in the hint's place (#141).
                                (Some(toast), _) => toast,
                                // Past the 35% cap the message scrolls in its
                                // own viewport, inside the strip, instead of
                                // being cut. The strip's bounds carry the
                                // status-* debug selectors.
                                (None, Some(text)) => {
                                    footer::status_message(text, &theme).into_any_element()
                                }
                                (None, None) => footer::hint_slot(
                                    self.footer_hint(with_actions, &theme),
                                    &theme,
                                )
                                .into_any_element(),
                            },
                            // A toast's actions, when it has any. While a
                            // status shows, the primary action steps aside —
                            // nothing is dispatched again from a frame the
                            // status has already overtaken (a double click on
                            // a quick open) — and Actions stays.
                            if toast_buttons.is_empty() {
                                self.footer_buttons(&action, with_actions, status_busy, &theme, cx)
                            } else {
                                toast_buttons
                            },
                            &theme,
                        )),
                )
            });
        // While anything is still in flight, keep frames coming; the frame
        // that settles it requests none, so a settled window is idle. The
        // scroll relayout above keeps its own separate request, for the
        // frame after the rows change.
        if frame.animating {
            window.request_animation_frame();
        }
        // The background image (ADR 0028), under the content: the
        // backdrop, a panel high (the expanded panel's height while the
        // window is collapsed, so the collapsed bar shows the top of the
        // same picture), moving up faster than the list as it scrolls and
        // dissolving by the time the list has scrolled half its height.
        let hero = visuals.backdrop.and_then(|backdrop| {
            let height = if collapsed {
                crate::background::PANEL.1
            } else {
                f32::from(window.viewport_size().height)
            };
            let scrolled = if listed && !collapsed {
                (-f32::from(self.scroll.offset().y)).max(0.)
            } else {
                0.
            };
            let shown = 1. - scrolled / (height * HERO_DISSOLVE);
            let opacity = if material.is_glass() {
                HERO_GLASS_OPACITY
            } else {
                1.
            };
            (shown > 0.).then(|| {
                img(backdrop.image)
                    .absolute()
                    .left_0()
                    .top(px(-scrolled * HERO_SCROLL))
                    .w_full()
                    .h(px(height))
                    .object_fit(ObjectFit::Cover)
                    .opacity(shown.min(1.) * opacity)
            })
        });
        // The panel surface: the frost material's L1 glass around the
        // content, with the sheen beneath it — and the background image
        // between the two, when there is one.
        match hero {
            Some(hero) => material.panel_over(&theme, hero, content),
            None => material.panel(&theme, content),
        }
    }
}

/// How much faster than the list the background image moves as the list
/// scrolls (ADR 0028).
const HERO_SCROLL: f32 = 1.25;

/// The share of the panel's height the list scrolls by the time the
/// background image has dissolved.
const HERO_DISSOLVE: f32 = 0.5;

/// The background image's opacity over the glass panel, so the window's
/// frost shows through it too: Roboco's frosted new-thread background's.
const HERO_GLASS_OPACITY: f32 = 0.84;

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

/// The footer's primary action button, as the launcher's footer
/// composes it: the reference's `.fbtn` (see [`footer::footer_button`]),
/// the action's label truncating beside the effective `invoke` binding's
/// keys in the accent caps — the primary action's key — so a rebound
/// Ctrl+Enter shows (and announces) Ctrl and the return key, never a bare
/// Enter. Presentation only: the caller attaches the click (the
/// launcher's [`LauncherWindow::press_primary_action`] path).
///
/// A click never dispatches what the definition says cannot run now, so
/// an unavailable button is dimmed, marked for assistive technology, and
/// the pointer says nothing to click; what explains it stays where it
/// was — the row's reason, the empty state — not the button.
pub(crate) fn action_button(
    action: &SelectedAction,
    invoke: &pane_core::Binding,
    theme: &Theme,
) -> Stateful<Div> {
    let keys = crate::keyboard::binding_keys(invoke);
    footer::footer_button(
        "primary-action",
        action.label.clone(),
        &keys,
        CapStyle::Accent,
        footer::ButtonWash::Hover,
        theme,
    )
    .role(Role::Button)
    .aria_label(action.label.clone())
    // The key that presses this button from the keyboard: the keycaps
    // beside the label show the same binding.
    .aria_keyshortcuts(keys.name())
    .when(action.available, |button| button.cursor_pointer())
    .when(!action.available, |button| {
        button.opacity(0.5).cursor_default().aria_disabled(true)
    })
}

/// A window size as [`Presence`] reads it, in logical pixels.
fn window_size(size: Size<Pixels>) -> WindowSize {
    WindowSize {
        width: f32::from(size.width),
        height: f32::from(size.height),
    }
}

/// The launcher presentation's section labels, as the shared list draws
/// them.
fn section_labels(presentation: &Presentation) -> Vec<shell::SectionLabel> {
    presentation.sections.iter().map(section_label).collect()
}

/// A launcher section as the shared list labels it: the adapter between
/// the core's section and the presentation value (the shared UI imports no
/// core types).
pub(crate) fn section_label(section: &pane_core::Section) -> shell::SectionLabel {
    shell::SectionLabel {
        first: section.first,
        label: section.label.clone().into(),
        note: section.note.clone().map(SharedString::from),
    }
}

/// The icon presentation for a row, chosen by the row's stable id: the
/// built-in rows and this build's sample commands are known identities,
/// each with a reference tone and glyph; everything else is a plain
/// command. No presentation is inferred from a title's text.
pub(crate) fn row_icon(id: &str) -> (IconTone, Glyph) {
    match id {
        "rust-sample" => (IconTone::Term, Glyph::Prompt),
        "javascript-sample" | "typescript-sample" => (IconTone::Code, Glyph::Code),
        "pane.install-from-folder" => (IconTone::Folder, Glyph::Folder),
        "pane.install-from-npm" => (IconTone::Web, Glyph::Blocks),
        "pane.install-from-git" => (IconTone::Term, Glyph::Terminal),
        "pane.manage-extensions" => (IconTone::Command, Glyph::Blocks),
        "pane.settings" => (IconTone::Command, Glyph::Gear),
        _ => (IconTone::Command, Glyph::Prompt),
    }
}
