//! The General page: the choices that govern Pane as a whole — today,
//! the Open Pane hotkey, whether Pane starts at login, and whether Pane
//! shows its tray or menu-bar entry.
//!
//! The Open Pane hotkey is the application-owned global binding that
//! summons the launcher from any application, recorded here and applied
//! through the platform's global-shortcut registration. The binding is
//! Pane's own, not any extension's: the record it is kept in is the host
//! settings' (`settings.json`, whose rules the entity in
//! `crate::settings` keeps), it is registered through the launcher this
//! window shares with the launcher window, and it stays registered while
//! every extension is disabled and while the extension runtime has
//! failed — nothing of its lifecycle belongs to a package.
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
use pane_core::autostart::Registration;
use pane_core::hotkeys::Shortcut;

use super::{Page, SettingsWindow, search};
use crate::ui::controls::{self, status_note as note};
use crate::ui::icon::Glyph;
use crate::ui::keycap::{CapStyle, KeySequence, key_sequence};
use crate::ui::settings_shell;
use crate::ui::theme::Theme;

/// The tray row's title, in the platform's own terms for the entry: the
/// menu bar's status item on macOS, the notification area's tray icon
/// elsewhere.
pub(crate) fn tray_row_title() -> &'static str {
    if cfg!(target_os = "macos") {
        "Show in Menu Bar"
    } else {
        "Show in tray"
    }
}

/// The tray group's label, in the same terms.
pub(crate) fn tray_group_title() -> &'static str {
    if cfg!(target_os = "macos") {
        "Menu Bar"
    } else {
        "System tray"
    }
}

/// The tray row's subtitle, in the same terms: what the entry is and
/// what its menu holds.
pub(crate) fn tray_row_subtitle() -> &'static str {
    if cfg!(target_os = "macos") {
        "Pane's menu-bar item, with Open Pane, Settings and Quit Pane"
    } else {
        "Pane's item in the notification area, with Open Pane, Settings and Exit"
    }
}

/// The recorder row's key context: while the recorder holds focus, its
/// keys are the binding being recorded, not the window's navigation.
const RECORDER: &str = "OpenPaneRecorder";

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
        // The row is a button: Enter and Space activate it, as a click does.
        KeyBinding::new("enter", ActivateRecorder, Some(RECORDER)),
        KeyBinding::new("space", ActivateRecorder, Some(RECORDER)),
        // Escape cancels recording.
        KeyBinding::new("escape", CancelRecording, Some(RECORDER)),
        // Swallowed while the recorder holds focus (and harmless when it
        // does not): the sidebar's navigation and Tab's traversal stay
        // put, so captured keys never act.
        KeyBinding::new("down", gpui::NoAction, Some(RECORDER)),
        KeyBinding::new("up", gpui::NoAction, Some(RECORDER)),
        KeyBinding::new("tab", gpui::NoAction, Some(RECORDER)),
        KeyBinding::new("shift-tab", gpui::NoAction, Some(RECORDER)),
    ]);
}

/// The page's heading.
pub(crate) const TITLE: &str = "General";

/// What the page is, in one line: its sidebar entry's description in
/// the search, and its heading's subtitle.
pub(crate) const ABOUT: &str = "The Open Pane hotkey and the launch-at-login choice";

/// What the recorder's row says under its name: at rest, and while it
/// listens.
pub(crate) const RECORDER_HINT: &str =
    "Shows the launcher from any application, and hides it when it has focus";

/// The General page, registered first in the window's page list: the
/// page of Pane as a whole, the one the window opens on.
pub(crate) fn page() -> Page {
    Page {
        title: "General",
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
    recording: bool,
    /// Why the last attempt was refused, if it was: a validation, a
    /// collision or the system's refusal. Shown as the page's status; the
    /// recorder keeps listening for another try.
    rejection: Option<String>,
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
    let (login_unavailable, tray_unavailable) = {
        let state = crate::settings::shared(cx).read(cx);
        (state.login_unavailable(), state.tray_unavailable())
    };
    vec![
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
    ]
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
    /// The launch-at-login switch.
    Login,
    /// The tray or menu-bar switch.
    Tray,
}

/// What the General page shows, as plain values: what [`render`] reads
/// from the host settings and the launcher, and what the visual
/// workbench's fixture supplies to draw the same page (#99).
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
    /// What the last recording was refused with, if anything.
    pub(crate) rejection: Option<String>,
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
        )
    };
    let default = Shortcut::open_pane_default();
    let view = GeneralView {
        keys: crate::keyboard::hotkey_keys(&choice),
        binding: format!("{choice}"),
        default: format!("{default}"),
        recording: this.general.recording,
        resettable: choice != default,
        problem: this.launcher.open_pane_problem(),
        rejection: this.general.rejection.clone(),
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
    let login_anchor = this.search_anchor("launch-at-login");
    let tray_anchor = this.search_anchor("tray-visibility");
    let focus = this.general.focus.clone();
    let focused = focus.is_focused(window);
    let (resettable, login_offered, tray_offered) =
        (view.resettable, view.login_offered, view.tray_offered);
    compose(&view, focused, &theme, |control, element| match control {
        GeneralControl::Recorder => element
            .anchor_scroll(Some(recorder_anchor.clone()))
            .key_context(RECORDER)
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
            .on_click(cx.listener(|this, _: &gpui::ClickEvent, window, cx| {
                if !this.general.recording {
                    this.general.recording = true;
                    this.general.rejection = None;
                    window.focus(&this.general.focus, cx);
                    cx.notify();
                }
            })),
        GeneralControl::Reset => {
            element
                .anchor_scroll(Some(reset_anchor.clone()))
                .when(resettable, |reset| {
                    reset.on_click(cx.listener(|this, _: &gpui::ClickEvent, window, cx| {
                        this.apply_open_pane(Shortcut::open_pane_default(), window, cx);
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
    })
    .into_any_element()
}

/// The General page's composition, which the visual workbench's fixture
/// draws too: the heading block, then a field group per setting in the
/// page's column (`ui::controls`) — the Open Pane hotkey's settings row
/// (its recorder's well, with Reset beside it) and what it explains, the
/// launch-at-login switch's row and note, the tray switch's row and notes
/// — and a failed save's status. `focused` is whether the recorder has the
/// keyboard (its well's ring). `attach` adds each control's behavior
/// (focus, keys, clicks, scroll anchors); the composition gives each its
/// identity, its accessibility and its look.
pub(crate) fn compose(
    view: &GeneralView,
    focused: bool,
    theme: &Theme,
    attach: impl Fn(GeneralControl, Stateful<Div>) -> Stateful<Div>,
) -> Stateful<Div> {
    let open_pane = controls::field(theme)
        .debug_selector(|| "general-open-pane-field".into())
        .child(controls::field_label("Open Pane", theme))
        .child(controls::setting_list().child(recorder_row(view, focused, theme, &attach)))
        // The binding's state: why the chosen one is not registered — a
        // registration the system refused, or a system where global
        // hotkeys cannot be used at all, with the adapter's own
        // explanation (Wayland's limitation and the desktop-shortcut
        // guidance it carries).
        .children(view.problem.as_ref().map(|problem| {
            note(
                "open-pane-note",
                format!("Not active: {problem}"),
                theme.warning,
                theme,
            )
        }))
        // What the last attempt to record a binding was refused with.
        .children(
            view.rejection
                .as_ref()
                .map(|rejection| note("general-refusal", rejection.clone(), theme.danger, theme)),
        );
    let login = switch_row(
        SwitchRow {
            id: "launch-at-login",
            selector: "general-launch-at-login",
            title: "Launch Pane at login",
            subtitle: "Pane is ready when you log in",
            on: view.login,
            offered: view.login_offered,
        },
        theme,
        |switch| attach(GeneralControl::Login, switch),
    );
    let startup = controls::field(theme)
        .debug_selector(|| "general-startup-field".into())
        .child(controls::field_label("Startup", theme))
        .child(controls::setting_list().child(login))
        .children(
            view.login_note
                .as_ref()
                .map(|(text, color)| note("general-login-note", text.clone(), *color, theme)),
        );
    let tray = switch_row(
        SwitchRow {
            id: "tray-visibility",
            selector: "tray-visibility",
            title: tray_row_title(),
            subtitle: tray_row_subtitle(),
            on: view.tray,
            offered: view.tray_offered,
        },
        theme,
        |switch| attach(GeneralControl::Tray, switch),
    );
    let tray = controls::field(theme)
        .debug_selector(|| "general-tray-field".into())
        .child(controls::field_label(tray_group_title(), theme))
        .child(controls::setting_list().child(tray))
        // The entry's state: why the native entry is not what the
        // preference names — a system with no tray or menu-bar entry at
        // all (Linux today, with the adapter's own guidance), or a show or
        // hide the system refused — and what the last toggle was refused
        // with. An unavailable entry is explained rather than represented
        // as a successful toggle.
        .children(
            view.tray_status
                .as_ref()
                .map(|status| note("tray-note", status.clone(), theme.warning, theme)),
        )
        .children(
            view.tray_refusal
                .as_ref()
                .map(|refusal| note("tray-refusal", refusal.clone(), theme.danger, theme)),
        );
    let column = controls::column(theme)
        .child(
            settings_shell::page_header(TITLE, Some(ABOUT.into()), theme)
                .id("general-title")
                .debug_selector(|| "general-title".into()),
        )
        .child(open_pane)
        .child(startup)
        .child(tray)
        // What a save reported, if it failed — the same status the
        // Appearance page shows for its own choices.
        .children(
            view.status
                .as_ref()
                .map(|status| note("general-status", status.clone(), theme.danger, theme)),
        );
    div()
        .id("general")
        .debug_selector(|| "general".into())
        .child(column)
}

/// The Open Pane hotkey's settings row: its name and what it does (or,
/// while the recorder listens, that the keys are captured) at the left,
/// and at its right end Reset beside the recorder — a well showing the
/// binding in effect as its caps, or, while it listens, the listening
/// mark. The recorder is a button: clicked or pressed with Enter it listens
/// for the keys of the next binding, holding focus, and takes the keys
/// pressed as the binding being recorded (Escape cancels); a refused
/// combination keeps it listening for another try. Reset goes back to the
/// provisional default, through the same checks as recording; it is drawn
/// disabled while the default is the choice.
fn recorder_row(
    view: &GeneralView,
    focused: bool,
    theme: &Theme,
    attach: &impl Fn(GeneralControl, Stateful<Div>) -> Stateful<Div>,
) -> Div {
    let subtitle = if view.recording {
        controls::RECORDING_HINT
    } else {
        RECORDER_HINT
    };
    let label = format!(
        "{}Open Pane with {}",
        if view.recording { "Recording; " } else { "" },
        view.binding
    );
    let shown = if view.recording {
        controls::listening_mark(theme).into_any_element()
    } else {
        key_sequence(&view.keys, CapStyle::Regular, theme).into_any_element()
    };
    let recorder = controls::recorder_well(
        div()
            .id("open-pane-binding")
            .debug_selector(|| "open-pane-binding".into())
            .flex()
            .child(shown),
        focused,
        theme,
    )
    .id("open-pane-recorder")
    .debug_selector(|| "open-pane-recorder".into())
    .role(Role::Button)
    .aria_label(label)
    .aria_description(subtitle);
    let reset = controls::ghost_button("Reset", view.resettable, theme)
        .id("open-pane-reset")
        .debug_selector(|| "open-pane-reset".into())
        .role(Role::Button)
        .aria_label(format!("Reset the Open Pane hotkey to {}", view.default))
        .aria_description(format!("Back to {}, the provisional default", view.default))
        .when(!view.resettable, |reset| reset.aria_disabled(true));
    let description = controls::field_description(subtitle, theme.text_muted, theme);
    controls::setting_row(
        "Open Pane hotkey",
        vec![description.into_any_element()],
        theme,
    )
    .debug_selector(|| "general-open-pane-row".into())
    .child(
        div()
            .flex_none()
            .flex()
            .items_center()
            .gap(theme.geometry.controls.button_gap)
            .child(attach(GeneralControl::Reset, reset))
            .child(attach(GeneralControl::Recorder, recorder)),
    )
}

/// One boolean choice of the page, as [`switch_row`] draws it.
struct SwitchRow {
    id: &'static str,
    selector: &'static str,
    title: &'static str,
    subtitle: &'static str,
    /// The saved preference the switch carries.
    on: bool,
    /// Whether choosing it does anything: nothing is offered where the
    /// platform cannot manage what the choice asks.
    offered: bool,
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
fn switch_row(
    row: SwitchRow,
    theme: &Theme,
    attach: impl FnOnce(Stateful<Div>) -> Stateful<Div>,
) -> Stateful<Div> {
    let SwitchRow {
        id,
        selector,
        title,
        subtitle,
        on,
        offered,
    } = row;
    let toggle = controls::toggle(on, theme).debug_selector(move || selector.into());
    let description = controls::field_description(subtitle, theme.text_muted, theme);
    let row = controls::setting_row(title, vec![description.into_any_element()], theme)
        .child(toggle)
        .id(id)
        .debug_selector(move || format!("{selector}-row"))
        .role(Role::Switch)
        .aria_label(title)
        .aria_description(subtitle)
        .aria_toggled(if on { Toggled::True } else { Toggled::False })
        .when(offered, |row| row.cursor_pointer())
        .when(!offered, |row| {
            row.opacity(theme.geometry.controls.disabled_opacity)
                .aria_disabled(true)
        });
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
            "Pane is registered, but macOS asks for your approval: open System Settings, \
             under General → Login Items, and allow Pane."
                .into(),
            theme.text_muted,
        ))
    } else if cfg!(target_os = "linux") && preference {
        Some((
            "The registration is an autostart entry in the freedesktop convention: the major \
             desktop environments start these, but not every desktop does, and Pane cannot see \
             whether it was started."
                .into(),
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
    /// pressed next are the binding being recorded.
    fn start_recorder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.general.recording = true;
        self.general.rejection = None;
        let focus = self.general.focus.clone();
        window.focus(&focus, cx);
        cx.notify();
    }

    /// Stops listening, without changing anything: focus returns to the
    /// sidebar, the window's own keyboard focus.
    fn stop_recording(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.general.recording = false;
        self.general.rejection = None;
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
    fn apply_open_pane(&mut self, shortcut: Shortcut, window: &mut Window, cx: &mut Context<Self>) {
        let applied = crate::settings::shared(cx)
            .update(cx, |settings, cx| settings.set_open_pane(shortcut, cx));
        match applied {
            Ok(()) => {
                // The change landed: the recorder is done, and focus
                // returns to the sidebar.
                self.general.recording = false;
                self.general.rejection = None;
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
