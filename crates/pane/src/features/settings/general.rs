//! The General page: the choices that govern Pane as a whole — the Open
//! Pane hotkey, whether Pane starts at login, whether Pane shows its tray
//! or menu-bar entry, and, in its Appearance section, the theme and the
//! material (see [`super::appearance`]).
//!
//! The Open Pane hotkey is the application-owned global binding that
//! summons the launcher from any application, recorded here and applied
//! through the platform's global-shortcut registration. The binding is
//! Pane's own, not any extension's: the record it is kept in is the host
//! settings' (`settings.json`, whose rules the entity in
//! `crate::settings` keeps), it is registered through the launcher this
//! window shares with the launcher window, and it stays registered while
//! every extension is disabled and while the extension runtime has
//! failed — nothing of its lifecycle belongs to a package. A fresh
//! install starts with the fresh default of its platform (#268, ADR
//! 0039): the Windows key alone on Windows where the adapter's own
//! keyboard hook recognizes the tap, and, while another hotkey is set,
//! the "Use the Windows key" choice beside the recorder sets it in one
//! step — Raycast's "Replace Start Menu" — saying where the Start menu
//! remains, since only the lone tap changes. Beside it, the taskbar
//! choice: "Show the taskbar when Pane opens" keeps a taskbar that hides
//! itself on screen while the launcher is shown, so the Start button
//! stays one click away; Windows only, explained elsewhere.
//!
//! The recorder captures keys without executing them. While it listens,
//! its row holds focus and its key context swallows the keys that would
//! otherwise act (the sidebar's navigation, Tab's traversal), so a
//! captured combination records instead of navigating; Escape cancels
//! recording, changing nothing. A captured combination is checked —
//! against the combinations the system keeps for itself (the Windows key,
//! Spotlight, the window menu — never taken over silently) and against
//! the command hotkeys — and registered *before* the binding it replaces
//! is released, so a refusal, a cancellation, a reset that cannot be
//! applied or a save that fails never discards the previous working
//! binding. Reset goes back to the provisional default through the same
//! checks as recording.
//!
//! The launch-at-login choice is the same discipline with one more
//! party: the platform's own registration, reached through the
//! [`pane_core::autostart`] adapter the entity holds. The choice, the
//! registration and the platform's ability are three different truths,
//! and the page shows all three rather than one pretense. The switch
//! carries the user's *saved preference*; the note under it carries what
//! the platform actually holds or why the last change failed (a
//! registration that still awaits macOS's approval, a system that
//! refused, the freedesktop convention's limit on Linux); and where the
//! integration cannot manage a registration here at all — an unsupported
//! platform, a development build — the switch is not offered, and the
//! reason is shown instead. A failed registration, removal or save is
//! the page's status, never a switch that pretends it succeeded. Every
//! value shown and every choice taken goes through the host settings
//! ([`crate::settings`]), so the record's own rules (atomic writes, an
//! unreadable record never replaced) are the ones this choice lives by.
//!
//! The tray or menu-bar visibility is the third of the same discipline:
//! the native entry is shown or hidden through the platform's adapter
//! before the choice is kept, so only a change that took is saved, a
//! system that refused explains itself, and a platform with no entry at
//! all (Linux today) explains that instead of offering a switch that
//! would pretend. The entry is the one place outside Pane's own windows
//! whose menu opens the launcher, Settings and Quit — so hiding it
//! leaves the launcher's footer menu, its Settings root result and the
//! local Settings shortcut as the entry points they always were, and
//! hiding it never hides a window: a launcher whose Open Pane binding
//! failed keeps its window, because the entry is the only thing the
//! preference ever hides.
//!
//! What the page explains: the binding's state — why a chosen one is not
//! registered, including the Wayland limitation and the desktop-shortcut
//! guidance the adapter itself carries — the reason a recording was
//! refused, and what a save reported. A binding that could not be
//! registered at startup keeps the entry itself visible and honest
//! rather than hiding the only window that can fix it.

use gpui::{
    AnyElement, App, Context, Div, FocusHandle, Hsla, KeyBinding, KeyDownEvent, MouseDownEvent,
    Role, Stateful, Toggled, Window, actions, div, prelude::*,
};
use pane_core::Launcher;
use pane_core::Platform;
use pane_core::autostart::Registration;
use pane_core::hotkeys::Shortcut;

use super::{Page, SettingsWindow, appearance, search};
use crate::ui::controls::{self, status_note as note};
use crate::ui::icon::Glyph;
use crate::ui::keycap::KeySequence;
use crate::ui::theme::Theme;

/// The tray row's title, in the platform's own terms for the entry: the
/// menu bar's status item on macOS, the notification area's tray icon
/// elsewhere.
pub(crate) fn tray_row_title() -> &'static str {
    if cfg!(target_os = "macos") {
        "Show in menu bar"
    } else {
        "Show in tray"
    }
}

/// The group the tray row sits in, as the search names it.
pub(crate) fn tray_group_title() -> &'static str {
    if cfg!(target_os = "macos") {
        "Menu bar"
    } else {
        "System tray"
    }
}

/// The recorder's key context while it listens: its keys are the binding
/// being recorded, not the window's navigation.
const RECORDER: &str = "OpenPaneRecorder";

/// The recorder's key context while it rests: a button.
const RECORDER_IDLE: &str = "OpenPaneRecorderIdle";

actions!(general, [ActivateRecorder, CancelRecording]);

/// Registers the recorder's key bindings, in the recorder's own context —
/// deeper in the focus stack than the sidebar's and the window's keys, so
/// while the recorder holds focus they never fall through. The navigation
/// and traversal keys are bound to [`gpui::NoAction`] there: a key the
/// recorder cannot use does nothing instead of acting, and everything
/// else reaches the recorder's own key handler as the combination being
/// recorded.
pub(crate) fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        // The recorder is a button: Enter and Space activate it, as a
        // click does.
        KeyBinding::new("enter", ActivateRecorder, Some(RECORDER_IDLE)),
        KeyBinding::new("space", ActivateRecorder, Some(RECORDER_IDLE)),
        KeyBinding::new("enter", ActivateRecorder, Some(RECORDER)),
        KeyBinding::new("space", ActivateRecorder, Some(RECORDER)),
        // Escape and Tab leave recording, changing nothing.
        KeyBinding::new("escape", CancelRecording, Some(RECORDER)),
        KeyBinding::new("tab", CancelRecording, Some(RECORDER)),
        KeyBinding::new("shift-tab", CancelRecording, Some(RECORDER)),
    ]);
    cx.bind_keys(super::captured_while_recording(RECORDER));
}

/// The page's title.
pub(crate) const TITLE: &str = "General";

/// What the page is, in one line: its sidebar entry's description in
/// the search.
pub(crate) const ABOUT: &str = "Hotkey, startup, tray and appearance";

/// What the recorder's row says under its name while it rests.
pub(crate) const RECORDER_HINT: &str = "Shows or hides the launcher from any app";

/// The "Use the Windows key" choice's title (Raycast's "Replace Start
/// Menu", #268): what sets the Windows key alone as the Open Pane hotkey
/// in one step, while another hotkey is set.
pub(crate) const USE_WINDOWS_KEY: &str = "Use the Windows key";

/// What the choice says under its title: the Start menu stays reachable
/// while the Windows key alone opens Pane, and only the lone tap changes
/// (#268, ADR 0039).
const WINDOWS_KEY_NOTE: &str = "Only the lone tap of the Windows key changes; Win+E, \
                                Win+D, Win+L and the rest keep \
                                Windows' meaning";
const START_MENU_NOTE: &str = "The Start menu stays reachable from the taskbar's Start \
                              button and Ctrl+Esc";

/// The taskbar choice's title (#268).
pub(crate) const SHOW_TASKBAR: &str = "Show the taskbar when Pane opens";

/// What the taskbar row says under its title while the choice works.
const TASKBAR_NOTE: &str = "While the launcher is open, a taskbar that hides itself \
                           stays on screen, so the Start button stays one \
                           click away";

/// What the taskbar row says where the platform has no taskbar of the
/// kind (macOS, Linux today): the choice is Windows only (#268).
const TASKBAR_NOT_OFFERED: &str = "The taskbar choice is available on Windows only";

/// The General page, registered first in the window's page list: the
/// page of Pane as a whole, the one the window opens on.
pub(crate) fn page() -> Page {
    Page {
        title: TITLE,
        about: ABOUT,
        icon: Glyph::Sliders,
        count: None,
        render,
        search: entries,
        focus,
    }
}

/// The General page's state, held by the window as a field: the recorder
/// and the tray toggle's last refusal.
pub(crate) struct State {
    /// Whether the recorder is listening for a new binding.
    pub(crate) recording: bool,
    /// Why the last attempt was refused, if it was: a validation, a
    /// collision or the system's refusal. Shown as the page's status; the
    /// recorder keeps listening for another try.
    pub(crate) rejection: Option<String>,
    /// Why the last tray or menu-bar toggle was refused, if it was: the
    /// system's refusal, or the platform's lack of an entry. Shown as the
    /// page's status; the entry's own state is explained beside the
    /// toggle.
    tray_refusal: Option<String>,
    /// The recorder row's focus, held while it listens (and a tab stop
    /// otherwise, so the keyboard reaches the row).
    focus: FocusHandle,
}

impl State {
    /// The recorder's state, over the window's `cx` (its focus handle).
    pub(crate) fn new(cx: &mut Context<SettingsWindow>) -> State {
        State {
            recording: false,
            rejection: None,
            tray_refusal: None,
            focus: cx.focus_handle().tab_stop(true),
        }
    }
}

/// The settings the page offers the sidebar's search: the Open Pane
/// hotkey — its recorder and its reset — the launch-at-login switch and
/// the tray or menu-bar visibility, named as the page names them, in the
/// groups they sit in. The hotkey carries the binding's own problem as
/// its reason when one stands (the system refused the binding, or it
/// cannot be used at all here); the login choice carries the
/// integration's reason where it cannot manage a registration, and the
/// tray choice the platform's reason where it has no entry at all. Read
/// live, so a binding that changes or an integration that answers
/// differently is in the next catalog.
fn entries(launcher: &Launcher, cx: &App) -> Vec<search::Entry> {
    let (login_unavailable, tray_unavailable, taskbar_offered) = {
        let state = crate::settings::shared(cx).read(cx);
        (
            state.login_unavailable(),
            state.tray_unavailable(),
            state.taskbar_offered(),
        )
    };
    let mut entries = vec![
        search::Entry {
            control: Some("open-pane-recorder".into()),
            title: "Open Pane hotkey".into(),
            group: Some("Open Pane".into()),
            unavailable: launcher.open_pane_problem(),
        },
        search::Entry {
            control: Some("open-pane-reset".into()),
            title: "Reset the Open Pane hotkey".into(),
            group: Some("Open Pane".into()),
            unavailable: None,
        },
        search::Entry {
            control: Some("use-windows-key".into()),
            title: USE_WINDOWS_KEY.into(),
            group: Some("Open Pane".into()),
            unavailable: launcher.windows_key_unavailable(),
        },
        search::Entry {
            control: Some("launch-at-login".into()),
            title: "Launch Pane at login".into(),
            group: Some("Startup".into()),
            unavailable: login_unavailable,
        },
        search::Entry {
            control: Some("tray-visibility".into()),
            title: tray_row_title().into(),
            group: Some(tray_group_title().into()),
            unavailable: tray_unavailable,
        },
        search::Entry {
            control: Some("show-taskbar".into()),
            title: SHOW_TASKBAR.into(),
            group: Some("Taskbar".into()),
            unavailable: (!taskbar_offered).then(|| TASKBAR_NOT_OFFERED.into()),
        },
    ];
    entries.extend(appearance::entries(launcher, cx));
    entries
}

/// The recorder row takes keyboard focus — it is a tab stop, and Enter
/// or a click on it starts recording — so a jump to it focuses the row,
/// ready to record. The reset row and the switches take none (they are
/// chosen with the pointer, as the Appearance choices are), so a jump
/// reveals them and the sidebar keeps the focus: `false`.
fn focus(
    this: &mut SettingsWindow,
    target: &str,
    window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> bool {
    if target != "open-pane-recorder" {
        return false;
    }
    window.focus(&this.general.focus, cx);
    true
}

/// Which of the General page's controls an element is, for the caller of
/// [`compose`] that attaches its behavior.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GeneralControl {
    /// The Open Pane hotkey's recorder.
    Recorder,
    /// The recorder's Reset button.
    Reset,
    /// The "Use the Windows key" choice (#268).
    UseWindowsKey,
    /// The launch-at-login switch.
    Login,
    /// The tray or menu-bar switch.
    Tray,
    /// The taskbar switch (#268).
    Taskbar,
}

/// What the General page shows, as plain values: what [`render`] reads
/// from the host settings and the launcher (#99).
pub(crate) struct GeneralView {
    /// The Open Pane binding's caps, from the launcher's binding adapter
    /// (`crate::keyboard::hotkey_keys`), and the binding as the user names
    /// it.
    pub(crate) keys: KeySequence,
    pub(crate) binding: String,
    /// The provisional default, as the user names it.
    pub(crate) default: String,
    /// Whether the recorder is listening.
    pub(crate) recording: bool,
    /// Whether the binding differs from the default (Reset is offered).
    pub(crate) resettable: bool,
    /// Why the chosen binding is not registered, if it is not.
    pub(crate) problem: Option<String>,
    /// How the binding is dispatched when it is active, if it is not the
    /// system's own registration: through Pane's own keyboard hook
    /// (Windows, #252), which the row says below the binding.
    pub(crate) route: Option<String>,
    /// What the last recording was refused with, if anything.
    pub(crate) rejection: Option<String>,
    /// The "Use the Windows key" choice (#268): whether another hotkey
    /// is set — the choice shows only then, and not once the Windows key
    /// alone is the binding — whether it can be taken here, and why not
    /// where it cannot, as the platform-availability mechanism carries.
    pub(crate) windows_key_shown: bool,
    pub(crate) windows_key_offered: bool,
    pub(crate) windows_key_unavailable: Option<String>,
    /// The taskbar choice (#268): the saved preference, and whether the
    /// platform has a taskbar of the kind to show at all.
    pub(crate) taskbar: bool,
    pub(crate) taskbar_offered: bool,
    /// The saved launch-at-login preference, whether choosing it does
    /// anything here, and the note under it with its tone.
    pub(crate) login: bool,
    pub(crate) login_offered: bool,
    pub(crate) login_note: Option<(String, Hsla)>,
    /// The tray or menu-bar visibility, whether choosing it does anything
    /// here, the entry's state and the last refusal.
    pub(crate) tray: bool,
    pub(crate) tray_offered: bool,
    pub(crate) tray_status: Option<String>,
    pub(crate) tray_refusal: Option<String>,
    /// What a save reported, if it failed.
    pub(crate) status: Option<String>,
}

/// Draws the General page: the Open Pane hotkey — its recorder and reset —
/// the launch-at-login and tray switches, what each of them explains, and
/// what the last attempt or save reported.
fn render(
    this: &mut SettingsWindow,
    window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let theme = crate::settings::visuals(cx).theme;
    // Everything the page shows comes from the host settings (the
    // choices, the save's word, the registration and ability each
    // integration reports, the entry's state) and the launcher (what is
    // registered, and why not): what the record holds and what actually
    // works stay distinguishable, choice by choice.
    let settings = crate::settings::shared(cx);
    let (
        choice,
        status,
        preference,
        unavailable,
        registration,
        tray_visible,
        tray_unavailable,
        tray_status,
        taskbar,
        taskbar_offered,
    ) = {
        let state = settings.read(cx);
        (
            state.open_pane(),
            state.status(),
            state.launch_at_login(),
            state.login_unavailable(),
            state.login_registration().clone(),
            state.tray_visible(),
            state.tray_unavailable(),
            state.tray_status(),
            state.show_taskbar(),
            state.taskbar_offered(),
        )
    };
    // The default the page resets to is the one a fresh data folder
    // starts with, decided as this launcher's adapter can take it (#268):
    // the Windows key alone where the adapter's hook recognizes the tap,
    // today's default otherwise.
    let default = this.launcher.open_pane_fresh_default().shortcut;
    let windows_key = Shortcut::windows_key();
    let windows_key_unavailable = this.launcher.windows_key_unavailable();
    let view = GeneralView {
        keys: crate::keyboard::hotkey_keys(&choice),
        binding: format!("{choice}"),
        default: format!("{default}"),
        recording: this.general.recording,
        resettable: choice != default,
        problem: this.launcher.open_pane_problem(),
        route: this
            .launcher
            .open_pane_route()
            .note_on(&choice, Platform::current()),
        rejection: this.general.rejection.clone(),
        // The choice shows while another hotkey is set — not once the
        // Windows key alone is the binding — and is offered only where
        // the adapter can take it, explained where it cannot.
        windows_key_shown: choice != windows_key,
        windows_key_offered: windows_key_unavailable.is_none(),
        windows_key_unavailable,
        taskbar,
        taskbar_offered,
        login: preference,
        login_offered: unavailable.is_none(),
        login_note: login_note(preference, unavailable, registration, &theme),
        tray: tray_visible,
        tray_offered: tray_unavailable.is_none(),
        tray_status,
        tray_refusal: this.general.tray_refusal.clone(),
        status,
    };
    // The controls' scroll anchors, which the search's reveal scrolls to
    // (see the window's render): the reset and the switches take no focus
    // of their own, so a jump to them reveals them.
    let recorder_anchor = this.search_anchor("open-pane-recorder");
    let reset_anchor = this.search_anchor("open-pane-reset");
    let windows_key_anchor = this.search_anchor("use-windows-key");
    let login_anchor = this.search_anchor("launch-at-login");
    let tray_anchor = this.search_anchor("tray-visibility");
    let taskbar_anchor = this.search_anchor("show-taskbar");
    let focus = this.general.focus.clone();
    let focused = focus.is_focused(window);
    let recording = this.general.recording;
    let (resettable, windows_key_offered, login_offered, tray_offered, taskbar_offered) = (
        view.resettable,
        view.windows_key_offered,
        view.login_offered,
        view.tray_offered,
        view.taskbar_offered,
    );
    let appearance = appearance::section(this, cx);
    compose(
        &view,
        focused,
        Some(appearance),
        &theme,
        |control, element| match control {
            GeneralControl::Recorder => element
                .anchor_scroll(Some(recorder_anchor.clone()))
                .key_context(if recording { RECORDER } else { RECORDER_IDLE })
                .track_focus(&focus)
                .on_action(cx.listener(SettingsWindow::activate_recorder))
                .on_action(cx.listener(SettingsWindow::cancel_recording))
                .on_key_down(cx.listener(SettingsWindow::recorder_key_down))
                // A mouse-down anywhere outside the recorder while it listens
                // cancels the recording and is consumed, as the footer menu's
                // popup does: the click underneath does not act, and the
                // recorder gives up the keys.
                .on_mouse_down_out(cx.listener(|this, _: &MouseDownEvent, window, cx| {
                    if this.general.recording {
                        this.stop_recording(window, cx);
                        cx.stop_propagation();
                    }
                }))
                // A click starts recording, or stops it again.
                .on_click(cx.listener(|this, _: &gpui::ClickEvent, window, cx| {
                    if this.general.recording {
                        this.stop_recording(window, cx);
                    } else {
                        this.start_recorder(window, cx);
                    }
                })),
            GeneralControl::Reset => {
                // The default is taken per call: the listener outlives this
                // frame, and it may be invoked again.
                let default = default.clone();
                element
                    .anchor_scroll(Some(reset_anchor.clone()))
                    // The reset sits inside the recorder: its click is its own.
                    .on_click(cx.listener(move |this, _: &gpui::ClickEvent, window, cx| {
                        cx.stop_propagation();
                        if resettable {
                            this.apply_open_pane(default.clone(), window, cx);
                        }
                    }))
            }
            GeneralControl::UseWindowsKey => {
                element
                    .anchor_scroll(Some(windows_key_anchor.clone()))
                    .when(windows_key_offered, |row| {
                        // The choice's click is its own step (#268): through
                        // the same checks a recording takes, so a refusal
                        // keeps the binding that works and says why. Where
                        // the Windows key cannot be taken, the row explains
                        // and takes no step.
                        row.on_click(cx.listener(|this, _: &gpui::ClickEvent, window, cx| {
                            cx.stop_propagation();
                            this.apply_open_pane(Shortcut::windows_key(), window, cx);
                        }))
                    })
            }
            GeneralControl::Login => {
                element
                    .anchor_scroll(Some(login_anchor.clone()))
                    .when(login_offered, |switch| {
                        // The click reports the choice to the host settings: the
                        // registration is changed, the record written, and the
                        // switch redrawn with what was actually kept.
                        switch.on_click(cx.listener(move |_, _: &gpui::ClickEvent, _, cx| {
                            crate::settings::shared(cx).update(cx, |settings, cx| {
                                settings.set_launch_at_login(!preference, cx);
                            });
                        }))
                    })
            }
            GeneralControl::Tray => {
                element
                    .anchor_scroll(Some(tray_anchor.clone()))
                    .when(tray_offered, |switch| {
                        // The click reports the choice to the host settings: the
                        // native entry is shown or hidden, the record written, and
                        // the switch redrawn with what was actually kept — the
                        // preference read as it is now, not as the frame that drew
                        // the row holds it.
                        switch.on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                            let visible = crate::settings::shared(cx).read(cx).tray_visible();
                            this.apply_tray_visible(!visible, cx);
                        }))
                    })
            }
            GeneralControl::Taskbar => {
                element.anchor_scroll(Some(taskbar_anchor.clone())).when(
                    taskbar_offered,
                    |switch| {
                        // The click reports the choice to the host settings: the
                        // record is written, and the launcher window — which
                        // observes the settings — shows the taskbar or puts it
                        // back at once, for it is open now (#268).
                        switch.on_click(cx.listener(|_, _: &gpui::ClickEvent, _, cx| {
                            crate::settings::shared(cx).update(cx, |settings, cx| {
                                let shown = settings.show_taskbar();
                                settings.set_show_taskbar(!shown, cx);
                            });
                        }))
                    },
                )
            }
        },
    )
    .into_any_element()
}

/// The General page's composition: a card of the page's own rows (`ui::controls`) — the Open
/// Pane hotkey's (its recorder's well, with Reset beside it, and the
/// "Use the Windows key" choice under it while another hotkey is set,
/// #268), the launch-at-login switch's, the tray switch's and the
/// taskbar switch's, each with what it explains under its name — then
/// `appearance`, the Appearance section, and a failed save's status
/// above them all. `focused` is whether the recorder has the keyboard
/// (its well's ring). `attach` adds each control's behavior (focus, keys,
/// clicks, scroll anchors); the composition gives each its identity, its
/// accessibility and its look.
pub(crate) fn compose(
    view: &GeneralView,
    focused: bool,
    appearance: Option<Div>,
    theme: &Theme,
    attach: impl Fn(GeneralControl, Stateful<Div>) -> Stateful<Div>,
) -> Stateful<Div> {
    let recorder = recorder_row(view, focused, theme, &attach);
    let login = switch_row(
        SwitchRow {
            id: "launch-at-login",
            selector: "general-launch-at-login",
            title: "Launch Pane at login",
            on: view.login,
            offered: view.login_offered,
            // What the platform holds, when that needs saying.
            lines: view
                .login_note
                .as_ref()
                .map(|(text, color)| {
                    note("general-login-note", text.clone(), *color, theme).into_any_element()
                })
                .into_iter()
                .collect(),
        },
        theme,
        |switch| attach(GeneralControl::Login, switch),
    );
    // The entry's state: why the native entry is not what the preference
    // names (a system with no tray or menu-bar entry at all, or a show or
    // hide the system refused), and what the last toggle was refused with.
    let tray_lines = view
        .tray_status
        .as_ref()
        .map(|status| note("tray-note", status.clone(), theme.warning, theme))
        .into_iter()
        .chain(
            view.tray_refusal
                .as_ref()
                .map(|refusal| note("tray-refusal", refusal.clone(), theme.danger, theme)),
        )
        .map(IntoElement::into_any_element)
        .collect();
    let tray = switch_row(
        SwitchRow {
            id: "tray-visibility",
            selector: "tray-visibility",
            title: tray_row_title(),
            on: view.tray,
            offered: view.tray_offered,
            lines: tray_lines,
        },
        theme,
        |switch| attach(GeneralControl::Tray, switch),
    );
    // The taskbar while the launcher is open (#268): what it does while
    // the choice is on, and — in its place, as the other not-offered
    // choices are — that the platform has no taskbar of the kind.
    let taskbar_lines = if view.taskbar_offered {
        vec![controls::row_line(TASKBAR_NOTE, theme.text_muted, theme)]
    } else {
        vec![controls::row_line(
            TASKBAR_NOT_OFFERED,
            theme.text_muted,
            theme,
        )]
    };
    let taskbar = switch_row(
        SwitchRow {
            id: "show-taskbar",
            selector: "general-show-taskbar",
            title: SHOW_TASKBAR,
            on: view.taskbar,
            offered: view.taskbar_offered,
            lines: taskbar_lines,
        },
        theme,
        |switch| attach(GeneralControl::Taskbar, switch),
    );
    let card = {
        let mut rows = vec![recorder.into_any_element()];
        if view.windows_key_shown {
            rows.push(windows_key_row(view, theme, &attach).into_any_element());
        }
        rows.extend([login.into_any_element(), tray.into_any_element()]);
        rows.push(taskbar.into_any_element());
        controls::card(rows, theme)
    };
    let page = controls::page(theme)
        // What a save reported, if it failed.
        .children(view.status.as_ref().map(|status| {
            note("general-status", status.clone(), theme.danger, theme)
                .px(theme.geometry.settings.section_label_inset)
        }))
        .child(controls::section(None, card, theme).debug_selector(|| "general-card".into()))
        .children(appearance);
    div()
        .id("general")
        .debug_selector(|| "general".into())
        .child(page)
}

/// The Open Pane hotkey's settings row: its name at the left, with why the
/// binding is not active and why the last recording was refused under it,
/// and its recorder at its right end ([`controls::recorder`]): the binding
/// written out, the record mark and the reset button, enabled while the
/// binding is not the default. Clicked or pressed with Enter the recorder
/// listens for the keys of the next binding, holding focus and ringed red,
/// and takes the keys pressed as the binding being recorded (Escape, Tab,
/// a click outside or another click on it cancels); a refused combination
/// keeps it listening for another try. Reset goes back to the default,
/// through the same checks as recording.
fn recorder_row(
    view: &GeneralView,
    _focused: bool,
    theme: &Theme,
    attach: &impl Fn(GeneralControl, Stateful<Div>) -> Stateful<Div>,
) -> Div {
    let label = format!(
        "{}Open Pane with {}",
        if view.recording { "Recording; " } else { "" },
        view.binding
    );
    let reset = controls::icon_button("open-pane-reset", Glyph::Reset, view.resettable, theme)
        .debug_selector(|| "open-pane-reset".into())
        .role(Role::Button)
        .aria_label(format!("Reset the Open Pane hotkey to {}", view.default))
        .when(!view.resettable, |reset| reset.aria_disabled(true));
    let recorder = controls::recorder(
        controls::binding_text(&view.keys),
        view.recording,
        Some(attach(GeneralControl::Reset, reset).into_any_element()),
        theme,
    )
    .id("open-pane-recorder")
    .debug_selector(|| "open-pane-recorder".into())
    .role(Role::Button)
    .aria_label(label)
    .aria_description(RECORDER_HINT);
    let mut lines = Vec::new();
    // The binding's state: why the chosen one is not registered (a
    // registration the system refused, or a system where global hotkeys
    // cannot be used at all, in the adapter's own words).
    lines.extend(view.problem.as_ref().map(|problem| {
        note(
            "open-pane-note",
            format!("Not active: {problem}"),
            theme.warning,
            theme,
        )
        .into_any_element()
    }));
    // The route of a binding the system refused and Pane's own keyboard
    // hook took (Windows, #252): the row says it below the binding, as
    // the Shortcuts page's cells do — the binding works, and behaves
    // differently (nothing while an elevated application is in front).
    lines.extend(view.route.as_ref().map(|route| {
        note(
            "open-pane-route",
            format!("Dispatched {route}"),
            theme.text_muted,
            theme,
        )
        .into_any_element()
    }));
    // What the last attempt to record a binding was refused with.
    lines.extend(view.rejection.as_ref().map(|rejection| {
        note("general-refusal", rejection.clone(), theme.danger, theme).into_any_element()
    }));
    controls::setting_row("Open Pane hotkey", lines, theme)
        .debug_selector(|| "general-open-pane-row".into())
        .child(attach(GeneralControl::Recorder, recorder))
}

/// The "Use the Windows key" choice (Raycast's "Replace Start Menu"),
/// under the Open Pane hotkey's row while another hotkey is set (#268):
/// one step to the Windows key alone, with the note that says where the
/// Start menu remains — the taskbar's Start button and Ctrl+Esc — and
/// that only the lone tap changes, Win+E, Win+D, Win+L and the rest
/// keeping Windows' meaning. Where the Windows key alone cannot be bound
/// here — an adapter with no keyboard hook — the choice is explained
/// instead of offered: the row stays, dimmed, with the reason under it,
/// as the platform-availability mechanism carries the choices a system
/// cannot take.
fn windows_key_row(
    view: &GeneralView,
    theme: &Theme,
    attach: &impl Fn(GeneralControl, Stateful<Div>) -> Stateful<Div>,
) -> Stateful<Div> {
    // What the row says under the choice: the two halves of the note
    // while it is offered, the reason it is not where it is not.
    let lines = match &view.windows_key_unavailable {
        Some(reason) => vec![
            note("use-windows-key-note", reason.clone(), theme.warning, theme).into_any_element(),
        ],
        None => vec![
            note(
                "use-windows-key-note",
                WINDOWS_KEY_NOTE,
                theme.text_muted,
                theme,
            )
            .into_any_element(),
            note(
                "use-windows-key-menu",
                START_MENU_NOTE,
                theme.text_muted,
                theme,
            )
            .into_any_element(),
        ],
    };
    let offered = view.windows_key_offered;
    // A choice not offered dims its name; what it says under the name
    // stays legible, since it says why.
    let opacity = if offered {
        1.
    } else {
        theme.geometry.controls.disabled_opacity
    };
    attach(
        GeneralControl::UseWindowsKey,
        controls::setting_row_with(
            controls::field_label(USE_WINDOWS_KEY, theme).opacity(opacity),
            lines,
            theme,
        )
        .id("use-windows-key")
        .debug_selector(|| "general-use-windows-key".into())
        .role(Role::Button)
        .aria_label(USE_WINDOWS_KEY)
        .when(offered, |row| row.cursor_pointer())
        .when(!offered, |row| row.aria_disabled(true)),
    )
}

/// One boolean choice of a page, as [`switch_row`] draws it.
pub(crate) struct SwitchRow {
    pub(crate) id: &'static str,
    pub(crate) selector: &'static str,
    pub(crate) title: &'static str,
    /// The saved preference the switch carries.
    pub(crate) on: bool,
    /// Whether choosing it does anything: nothing is offered where the
    /// platform cannot manage what the choice asks.
    pub(crate) offered: bool,
    /// What the row says under its name: what the platform holds, why it
    /// is not offered, a refusal.
    pub(crate) lines: Vec<AnyElement>,
}

/// One switch row, as the General page's boolean choices are drawn: the
/// settings row naming the choice — the whole row the switch, as a click
/// anywhere on it takes the choice — with the board's switch at its right
/// end, its picture (its debug selector the choice's). The switch carries
/// the *saved preference*; a choice not offered is drawn at the disabled
/// opacity and takes no click (`attach` adds the click where it is
/// offered). One
/// presentation for every boolean the page offers, so what a switch says,
/// whether it can be taken and what taking it does cannot diverge between
/// the choices.
pub(crate) fn switch_row(
    row: SwitchRow,
    theme: &Theme,
    attach: impl FnOnce(Stateful<Div>) -> Stateful<Div>,
) -> Stateful<Div> {
    let SwitchRow {
        id,
        selector,
        title,
        on,
        offered,
        lines,
    } = row;
    // A choice not offered dims its name and its switch; what the row
    // says under its name stays legible, since it says why.
    let opacity = if offered {
        1.
    } else {
        theme.geometry.controls.disabled_opacity
    };
    let toggle = controls::toggle(on, theme)
        .debug_selector(move || selector.into())
        .opacity(opacity);
    let label = controls::field_label(title, theme).opacity(opacity);
    let row = controls::setting_row_with(label, lines, theme)
        .child(toggle)
        .id(id)
        .debug_selector(move || format!("{selector}-row"))
        .role(Role::Switch)
        .aria_label(title)
        .aria_toggled(if on { Toggled::True } else { Toggled::False })
        .when(offered, |row| row.cursor_pointer())
        .when(!offered, |row| row.aria_disabled(true));
    attach(row)
}

/// The note under the startup group, if the launch-at-login choice needs
/// one, with its tone: why the integration is unavailable here, what the
/// platform actually holds when that differs from a working registration,
/// or the limit of the convention the platform uses. `None` when the
/// preference and the registration agree and the platform needs no
/// explanation.
fn login_note(
    preference: bool,
    unavailable: Option<String>,
    registration: Result<Registration, String>,
    theme: &Theme,
) -> Option<(String, Hsla)> {
    if let Some(reason) = unavailable {
        // The platform (or this build) cannot manage the registration
        // here at all: the reason, not a toggle that pretends.
        Some((reason, theme.warning))
    } else if let Err(problem) = registration {
        // The last query or change failed, and the preference is what it
        // was: the problem is the truth to show.
        Some((problem, theme.warning))
    } else if registration == Ok(Registration::NeedsApproval) {
        Some((
            "Allow Pane in System Settings, under General > Login Items.".into(),
            theme.text_muted,
        ))
    } else if cfg!(target_os = "linux") && preference {
        Some((
            "Added as an autostart entry. Most desktops run these.".into(),
            theme.text_muted,
        ))
    } else {
        None
    }
}

impl SettingsWindow {
    /// The recorder's activation: Enter, Space or a click on its row. With
    /// no recording in progress it starts listening; while it listens the
    /// keys are captured, so this does nothing — Enter and Space cannot be
    /// part of a binding, and the recorder keeps listening.
    fn activate_recorder(
        &mut self,
        _: &ActivateRecorder,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.stop_propagation();
        if !self.general.recording {
            self.start_recorder(window, cx);
        }
    }

    /// Escape on the recorder's row: cancels recording, changing nothing.
    fn cancel_recording(
        &mut self,
        _: &CancelRecording,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.stop_propagation();
        if self.general.recording {
            self.stop_recording(window, cx);
        }
    }

    /// Starts listening: the recorder row takes focus, so the keys
    /// pressed next are the binding being recorded. On a system whose
    /// adapter has one, a recording session starts with it (#260): the
    /// adapter holds the keys back from the system and reports what the
    /// user presses, so the Windows key alone, a double tap and the side
    /// of a modifier are recorded without the Start menu opening; Escape
    /// and Tab still cancel, and the session ends when this stops
    /// listening, the window loses focus, or Pane quits.
    fn start_recorder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.general.recording = true;
        self.general.rejection = None;
        let focus = self.general.focus.clone();
        window.focus(&focus, cx);
        self.start_recording_session(window, cx);
        cx.notify();
    }

    /// Stops listening, without changing anything: focus returns to the
    /// sidebar, the window's own keyboard focus, and the recording
    /// session ends with the listening (#260), so the adapter stops
    /// holding the keys back.
    fn stop_recording(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.general.recording = false;
        self.general.rejection = None;
        self.end_recording_session();
        let sidebar = self.focus.clone();
        window.focus(&sidebar, cx);
        cx.notify();
    }

    /// A key pressed while the recorder listens: the combination it names
    /// is the binding to record. Keys the window binds (Enter, Space,
    /// Escape, the navigation and traversal keys) never reach here — they
    /// are bound in the recorder's context and handled above.
    fn recorder_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.general.recording {
            return;
        }
        let keystroke = &event.keystroke;
        let modifiers = keystroke.modifiers;
        cx.stop_propagation();
        let shortcut = Shortcut::new(
            modifiers.control,
            modifiers.alt,
            modifiers.shift,
            modifiers.platform,
            &keystroke.key,
        );
        match shortcut {
            Ok(shortcut) => self.apply_open_pane(shortcut, window, cx),
            Err(problem) => {
                self.general.rejection = Some(format!("{problem}."));
                cx.notify();
            }
        }
    }

    /// Applies `shortcut` as the Open Pane hotkey: through the host
    /// settings, which register it with the system first and only then
    /// keep and save the choice. A refusal leaves the previous binding
    /// working and nothing saved; the reason is the page's status, and the
    /// recorder — if one is listening — keeps listening for another try.
    /// Called with what a recording session reported too (#260).
    pub(super) fn apply_open_pane(
        &mut self,
        shortcut: Shortcut,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let applied = crate::settings::shared(cx)
            .update(cx, |settings, cx| settings.set_open_pane(shortcut, cx));
        match applied {
            Ok(()) => {
                // The change landed: the recorder is done, and focus
                // returns to the sidebar.
                self.general.recording = false;
                self.general.rejection = None;
                self.end_recording_session();
                let sidebar = self.focus.clone();
                window.focus(&sidebar, cx);
                cx.notify();
            }
            Err(reason) => {
                self.general.rejection = Some(reason);
                cx.notify();
            }
        }
    }

    /// Applies `visible` as the tray or menu-bar visibility, as the
    /// toggle's click does: through the host settings, which apply it to
    /// the native entry first and only then keep and save the choice. A
    /// refusal leaves the entry and the record as they were; the reason
    /// is the page's status, and the entry's own state is explained
    /// beside the toggle whether or not a change was attempted.
    fn apply_tray_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        let applied = crate::settings::shared(cx)
            .update(cx, |settings, cx| settings.set_tray_visible(visible, cx));
        match applied {
            Ok(()) => self.general.tray_refusal = None,
            Err(reason) => self.general.tray_refusal = Some(reason),
        }
        cx.notify();
    }
}
