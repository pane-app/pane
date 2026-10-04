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
    Role, ScrollAnchor, Stateful, Toggled, Window, actions, div, prelude::*, px,
};
use pane_core::Launcher;
use pane_core::autostart::Registration;
use pane_core::hotkeys::Shortcut;

use super::{Page, SettingsWindow, search};
use crate::ui::icon::{Glyph, IconTone};
use crate::ui::theme::Theme;

/// The tray row's title, in the platform's own terms for the entry: the
/// menu bar's status item on macOS, the notification area's tray icon
/// elsewhere.
fn tray_row_title() -> &'static str {
    if cfg!(target_os = "macos") {
        "Show in Menu Bar"
    } else {
        "Show in tray"
    }
}

/// The tray group's label, in the same terms.
fn tray_group_title() -> &'static str {
    if cfg!(target_os = "macos") {
        "Menu Bar"
    } else {
        "System tray"
    }
}

/// The tray row's subtitle, in the same terms: what the entry is and
/// what its menu holds.
fn tray_row_subtitle() -> &'static str {
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

/// The General page, registered first in the window's page list: the
/// page of Pane as a whole, the one the window opens on.
pub(crate) fn page() -> Page {
    Page {
        title: "General",
        about: "The Open Pane hotkey and the launch-at-login choice",
        icon: (IconTone::Command, Glyph::Sliders),
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

/// Draws the General page: the Open Pane hotkey — its recorder and reset —
/// the launch-at-login and tray switches, what each of them explains, and
/// what the last attempt or save reported.
fn render(
    this: &mut SettingsWindow,
    _window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let theme = crate::settings::visuals(cx).theme;
    let typography = &theme.typography;
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
    let problem = this.launcher.open_pane_problem();
    let rejection = this.general.rejection.clone();
    let tray_refusal = this.general.tray_refusal.clone();
    let resettable = choice != Shortcut::open_pane_default();
    // The rows' scroll anchors, which the search's reveal scrolls to
    // (see the window's render): the reset row and the login switch take
    // no focus of their own, so a jump to them reveals them.
    let recorder_anchor = this.search_anchor("open-pane-recorder");
    let reset_anchor = this.search_anchor("open-pane-reset");
    let login_anchor = this.search_anchor("launch-at-login");
    let tray_anchor = this.search_anchor("tray-visibility");

    let page = div()
        .id("general")
        .debug_selector(|| "general".into())
        .flex()
        .flex_col()
        .gap(px(4.))
        .child(
            div()
                .id("general-title")
                .debug_selector(|| "general-title".into())
                .pb(px(8.))
                .text_size(typography.search_size)
                .font_weight(typography.medium)
                .text_color(theme.text_title)
                .child("General"),
        )
        .child(group(
            "Open Pane",
            vec![
                recorder_row(this, recorder_anchor, cx),
                reset_row(resettable, reset_anchor, cx),
            ],
            &theme,
        ))
        // The binding's state: why the chosen one is not registered — a
        // registration the system refused, or a system where global
        // hotkeys cannot be used at all, with the adapter's own
        // explanation (Wayland's limitation and the desktop-shortcut
        // guidance it carries).
        .when_some(problem, |page, problem| {
            page.child(note(
                "open-pane-note",
                &format!("Not active: {problem}"),
                theme.warning,
                &theme,
            ))
        })
        .child(group(
            "Startup",
            vec![switch(
                "launch-at-login",
                "general-launch-at-login",
                "Launch Pane at login",
                "Pane is ready when you log in",
                preference,
                unavailable.is_none(),
                login_anchor,
                &theme,
                // The click reports the choice to the host settings: the
                // registration is changed, the record written, and the
                // switch redrawn with what was actually kept.
                cx.listener(move |_, _, _, cx| {
                    crate::settings::shared(cx).update(cx, |settings, cx| {
                        settings.set_launch_at_login(!preference, cx);
                    });
                }),
            )],
            &theme,
        ))
        .when_some(
            login_note(preference, unavailable, registration, &theme),
            |page, note| page.child(note),
        )
        .child(group(
            tray_group_title(),
            vec![switch(
                "tray-visibility",
                "tray-visibility",
                tray_row_title(),
                tray_row_subtitle(),
                tray_visible,
                tray_unavailable.is_none(),
                tray_anchor,
                &theme,
                // The click reports the choice to the host settings: the
                // native entry is shown or hidden, the record written,
                // and the switch redrawn with what was actually kept —
                // the preference read as it is now, not as the frame that
                // drew the row holds it.
                cx.listener(|this, _, _, cx| {
                    let visible = crate::settings::shared(cx).read(cx).tray_visible();
                    this.apply_tray_visible(!visible, cx);
                }),
            )],
            &theme,
        ))
        // The entry's state: why the native entry is not what the
        // preference names — a system with no tray or menu-bar entry at
        // all (Linux today, with the adapter's own guidance), or a show
        // or hide the system refused. An unavailable entry is explained
        // rather than represented as a successful toggle, and its switch
        // is not offered where the platform has none.
        .when_some(tray_status, |page, status| {
            page.child(note("tray-note", &status, theme.warning, &theme))
        })
        // What the last tray toggle was refused with, if anything.
        .when_some(tray_refusal, |page, refusal| {
            page.child(note("tray-refusal", &refusal, theme.danger, &theme))
        })
        // What the last attempt to record a binding was refused with, if
        // anything.
        .when_some(rejection, |page, rejection| {
            page.child(note("general-refusal", &rejection, theme.danger, &theme))
        })
        // What a save reported, if it failed — the same status the
        // Appearance page shows for its own choices.
        .when_some(status, |page, status| {
            page.child(note("general-status", &status, theme.danger, &theme))
        });
    page.into_any_element()
}

/// One labelled group of rows, as the Appearance page's groups.
fn group(label: &'static str, rows: Vec<Stateful<Div>>, theme: &Theme) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(2.))
        .child(
            div()
                .pb(px(4.))
                .text_size(theme.typography.row_kind_size)
                .font_weight(theme.typography.medium)
                .text_color(theme.text_muted)
                .child(label),
        )
        .children(rows)
}

/// The Open Pane hotkey's recorder row: a button that shows the binding
/// in effect and, clicked or pressed with Enter, listens for the keys of
/// the next one. While it listens it holds focus, shows what it is doing,
/// and takes the keys pressed as the binding being recorded (Escape
/// cancels); a combination that is refused keeps it listening for another
/// try.
fn recorder_row(
    this: &mut SettingsWindow,
    anchor: ScrollAnchor,
    cx: &mut Context<SettingsWindow>,
) -> Stateful<Div> {
    let theme = crate::settings::visuals(cx).theme;
    let typography = &theme.typography;
    let geometry = &theme.geometry;
    let recording = this.general.recording;
    let choice = crate::settings::shared(cx).read(cx).open_pane();
    let binding = format!("{choice}");
    let subtitle = if recording {
        "The keys are captured here: they do not act"
    } else {
        "Shows the launcher from any application, and hides it when it has focus"
    };
    let label = format!(
        "{}Open Pane with {binding}",
        if recording { "Recording; " } else { "" }
    );
    let focus = this.general.focus.clone();
    div()
        .id("open-pane-recorder")
        .debug_selector(|| "open-pane-recorder".into())
        .flex()
        .items_center()
        .gap(geometry.row_gap)
        .min_h(geometry.row_min_height)
        .px(geometry.row_padding_x)
        .rounded(geometry.row_radius)
        .cursor_pointer()
        .hover(|row| row.bg(theme.row_hover))
        // Pressed: the selected wash, one rung above the hover one.
        .active(|row| row.bg(theme.row_selected))
        .transitions(|fades| fades.bg(crate::ui::motion::pointer_fade()))
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .flex_col()
                .child(
                    div()
                        .truncate()
                        .text_size(typography.row_title_size)
                        .font_weight(typography.medium)
                        .text_color(theme.text_title)
                        .child(if recording {
                            "Press the keys…"
                        } else {
                            "Record shortcut"
                        }),
                )
                .child(
                    div()
                        .text_size(typography.row_subtitle_size)
                        .text_color(theme.text_muted)
                        .child(subtitle),
                ),
        )
        .child(binding_chip(&binding, recording, &theme))
        .anchor_scroll(Some(anchor))
        .key_context(RECORDER)
        .track_focus(&focus)
        .role(Role::Button)
        .aria_label(label)
        .aria_description(subtitle)
        .on_action(cx.listener(SettingsWindow::activate_recorder))
        .on_action(cx.listener(SettingsWindow::cancel_recording))
        .on_key_down(cx.listener(SettingsWindow::recorder_key_down))
        // A mouse-down anywhere outside the row while it listens cancels
        // the recording and is consumed, as the footer menu's popup does:
        // the click underneath does not act, and the recorder gives up
        // the keys.
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
        }))
}

/// The binding the row shows: the shortcut as the user names it, in the
/// keycap chrome — or, while recording, the listening mark.
fn binding_chip(binding: &str, recording: bool, theme: &Theme) -> Stateful<Div> {
    let typography = &theme.typography;
    let geometry = &theme.geometry;
    div()
        .id("open-pane-binding")
        .debug_selector(|| "open-pane-binding".into())
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .h(geometry.keycap_height)
        .px(geometry.keycap_padding_x)
        .rounded(geometry.keycap_radius)
        .bg(theme.tile_background)
        .text_size(typography.row_title_size)
        .font_weight(typography.medium)
        .text_color(if recording {
            theme.warning
        } else {
            theme.text_title
        })
        .child(if recording {
            "…".to_owned()
        } else {
            binding.to_owned()
        })
}

/// The reset row: back to the provisional default, through the same
/// checks as recording (a reset that cannot be applied is refused and
/// leaves the binding working). Inert while the default is the choice.
fn reset_row(
    resettable: bool,
    anchor: ScrollAnchor,
    cx: &mut Context<SettingsWindow>,
) -> Stateful<Div> {
    let theme = crate::settings::visuals(cx).theme;
    let typography = &theme.typography;
    let geometry = &theme.geometry;
    let default = Shortcut::open_pane_default();
    let row = div()
        .flex()
        .items_center()
        .gap(geometry.row_gap)
        .min_h(geometry.row_min_height)
        .px(geometry.row_padding_x)
        .rounded(geometry.row_radius)
        .when(resettable, |row| row.cursor_pointer())
        .when(!resettable, |row| row.opacity(0.5).cursor_default())
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .flex_col()
                .child(
                    div()
                        .truncate()
                        .text_size(typography.row_title_size)
                        .font_weight(typography.medium)
                        .text_color(theme.text_title)
                        .child("Reset"),
                )
                .child(
                    div()
                        .text_size(typography.row_subtitle_size)
                        .text_color(theme.text_muted)
                        .child(format!("Back to {default}, the provisional default")),
                ),
        );
    row.id("open-pane-reset")
        // The pointer feedback, on the named row: the hover wash fades
        // over the shared pointer span, and the press takes the selected
        // wash, one rung above the hover one.
        .when(resettable, |row| {
            row.hover(|row| row.bg(theme.row_hover))
                .active(|row| row.bg(theme.row_selected))
                .transitions(|fades| fades.bg(crate::ui::motion::pointer_fade()))
        })
        .debug_selector(|| "open-pane-reset".into())
        .anchor_scroll(Some(anchor))
        .role(Role::Button)
        .aria_label(format!("Reset the Open Pane hotkey to {default}"))
        .when(!resettable, |row| row.aria_disabled(true))
        .when(resettable, |row| {
            row.on_click(cx.listener(|this, _: &gpui::ClickEvent, window, cx| {
                this.apply_open_pane(Shortcut::open_pane_default(), window, cx);
            }))
        })
}

/// One switch row, as the General page's boolean choices are drawn: the
/// reference's row chrome carrying a switch's marks and semantics, the
/// switch itself at the right, `title` and `subtitle` naming the choice.
/// The switch carries the *saved preference*; `offered` is whether
/// choosing it does anything (nothing is offered where the platform
/// cannot manage what the choice asks, and the row says so by its
/// state). One presentation for every boolean the page offers, so what
/// a switch says, whether it can be taken and what taking it does cannot
/// diverge between the choices.
#[allow(clippy::too_many_arguments)]
fn switch(
    id: &'static str,
    selector: &'static str,
    title: &'static str,
    subtitle: &'static str,
    preference: bool,
    offered: bool,
    anchor: ScrollAnchor,
    theme: &Theme,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let typography = &theme.typography;
    let geometry = &theme.geometry;
    div()
        .flex()
        .items_center()
        .gap(geometry.row_gap)
        .min_h(geometry.row_min_height)
        .px(geometry.row_padding_x)
        .rounded(geometry.row_radius)
        .when(offered, |row| row.cursor_pointer())
        .when(!offered, |row| row.opacity(0.5).cursor_default())
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .flex_col()
                .child(
                    div()
                        .truncate()
                        .text_size(typography.row_title_size)
                        .font_weight(typography.medium)
                        .text_color(theme.text_title)
                        .child(title),
                )
                .child(
                    div()
                        .text_size(typography.row_subtitle_size)
                        .text_color(theme.text_muted)
                        .child(subtitle),
                ),
        )
        .child(track(preference, theme))
        .id(id)
        // The pointer feedback, on the named row: the hover wash fades
        // over the shared pointer span, and the press takes the selected
        // wash, one rung above the hover one.
        .when(offered, |row| {
            row.hover(|row| row.bg(theme.row_hover))
                .active(|row| row.bg(theme.row_selected))
                .transitions(|fades| fades.bg(crate::ui::motion::pointer_fade()))
        })
        .debug_selector(move || selector.into())
        .anchor_scroll(Some(anchor))
        .role(Role::Switch)
        .aria_label(title)
        .aria_toggled(if preference {
            Toggled::True
        } else {
            Toggled::False
        })
        .when(!offered, |row| row.aria_disabled(true))
        .when(offered, |row| row.on_click(on_click))
}

/// The switch itself: the track with its knob, slid to the side the
/// preference names. Presentation only — the row owns the interaction.
fn track(preference: bool, theme: &Theme) -> Div {
    div()
        .flex_none()
        .flex()
        .w(px(36.))
        .h(px(20.))
        .px(px(2.))
        .items_center()
        .rounded(px(10.))
        // The track says the choice itself: the success tone when Pane
        // starts at login, the quiet hairline when it does not.
        .when(preference, |track| track.bg(theme.success))
        .when(!preference, |track| track.bg(theme.hairline))
        .when(preference, |track| track.justify_end())
        .child(
            div()
                .flex_none()
                .size(px(16.))
                .rounded(px(8.))
                .bg(theme.panel_solid)
                // The knob's lift off the track: a faint shadow.
                .shadow(vec![gpui::BoxShadow::new(
                    px(0.),
                    px(0.),
                    gpui::rgb_to_hsla(gpui::rgba(0x00000026)),
                )]),
        )
}

/// One explanatory line of the page: `text` in `color`, named for
/// assistive technology and drawn as a status.
fn note(selector: &'static str, text: &str, color: Hsla, theme: &Theme) -> Stateful<Div> {
    div()
        .id(selector)
        .debug_selector(move || selector.into())
        .pt(px(6.))
        .role(Role::Status)
        .aria_label(text.to_owned())
        .text_size(theme.typography.row_subtitle_size)
        .text_color(color)
        .child(text.to_owned())
}

/// The note under the startup group, if the launch-at-login choice needs
/// one: why the integration is unavailable here, what the platform
/// actually holds when that differs from a working registration, or the
/// limit of the convention the platform uses. `None` when the preference
/// and the registration agree and the platform needs no explanation.
fn login_note(
    preference: bool,
    unavailable: Option<String>,
    registration: Result<Registration, String>,
    theme: &Theme,
) -> Option<Stateful<Div>> {
    let (text, color) = if let Some(reason) = unavailable {
        // The platform (or this build) cannot manage the registration
        // here at all: the reason, not a toggle that pretends.
        (reason, theme.warning)
    } else if let Err(problem) = registration {
        // The last query or change failed, and the preference is what it
        // was: the problem is the truth to show.
        (problem, theme.warning)
    } else if registration == Ok(Registration::NeedsApproval) {
        (
            "Pane is registered, but macOS asks for your approval: open System Settings, \
             under General → Login Items, and allow Pane."
                .into(),
            theme.text_muted,
        )
    } else if cfg!(target_os = "linux") && preference {
        (
            "The registration is an autostart entry in the freedesktop convention: the major \
             desktop environments start these, but not every desktop does, and Pane cannot see \
             whether it was started."
                .into(),
            theme.text_muted,
        )
    } else {
        return None;
    };
    Some(
        div()
            .id("general-login-note")
            .debug_selector(|| "general-login-note".into())
            .pt(px(6.))
            .role(Role::Status)
            .aria_label(text.clone())
            .text_size(theme.typography.row_kind_size)
            .text_color(color)
            .child(text),
    )
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
