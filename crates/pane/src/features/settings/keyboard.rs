//! The Settings window's Keyboard page: the bounded set of in-app
//! navigation actions, each with its binding, rebindable here.
//!
//! The actions are Pane's own — previous/next result, invoke selected
//! action, back, return to root, dismiss launcher and open Settings —
//! never an extension's and never the text-editing keys a focused field
//! owns (see [`pane_core::keyboard`], the renderer-independent set and
//! its rules). Their bindings live in the host settings, so a change
//! takes effect in every window at once and survives a restart.
//!
//! The recorder is the General page's, per action ([`controls::recorder`],
//! after Discord's keybind field): a click (or Enter) starts listening —
//! the field ringed red — and the keys pressed then are the binding being
//! recorded, captured without acting, so neither the sidebar's navigation
//! nor the window's traversal moves, and never the action being rebound.
//! Escape, Tab, a click outside, another click on the field or the window
//! losing focus cancels; Enter and Space cannot be part of a binding,
//! being the recorder's own activation, so they are the defaults'
//! privilege and a reset's to restore. A captured combination is checked
//! — protected for a focused field, and against the other actions of the
//! set, whose contexts overlap in the launcher's window — applied through
//! the host settings (which re-make the keymap before saving), and then
//! saved; a refusal is explained under the row's name and keeps the
//! recorder listening.
//!
//! Each recorder's reset button is enabled while its binding is not the
//! default: pointer-only, so recovery from a binding that does not suit
//! the keyboard in front of the user never needs the very keys being
//! rebound. A reset goes through the same checks as a recording, so it
//! cannot land on another action's keys.
//!
//! Above the actions, the Behavior section (Raycast's keyboard settings):
//! what the back key does in the launcher, whether Escape closes Settings,
//! and extra Emacs or Vim Motions keys for moving the selection, on Alt as
//! Raycast for Windows has them (see [`NavigationBindings`]).
//!
//! Below the actions, the state of Pane's own keyboard hook (Windows,
//! #252): while a hotkey of yours is dispatched through it, a row says
//! whether it is installed, how many times Windows removed it and Pane
//! installed it again, and whether its pages are pinned in memory — and
//! why Pane gave up keeping it installed, if it did (#259, read from the
//! launcher's adapter). Where no hook is in use nothing shows: none is
//! needed, and the rows of the system's own registrations say nothing
//! either. Beside it, the Game mode section (#125): the on/off choice and
//! the programs to treat as games, applied through the launcher's
//! game-mode record (`pane_core::game_mode`), so that Pane's hotkeys
//! pause while a game is in front and come back when it leaves. Offered
//! where a foreground source is attached — Windows, or a test's fake —
//! and explained as Windows-only elsewhere.

use std::collections::BTreeMap;

use gpui::{
    AnyElement, App, Context, Div, Entity, FocusHandle, Focusable, KeyBinding, KeyDownEvent,
    MouseDownEvent, Role, ScrollAnchor, SharedString, Stateful, Toggled, Window, actions, div,
    prelude::*, px,
};
use gpui_elements::editable_text::{EditableTextState, StringStorage, TextChanged, text_input};
use pane_core::hotkeys::HookHealth;
use pane_core::{
    Binding, EscapeBehavior, GameMode, Keyboard, KeyboardAction, Launcher, NavigationBindings,
};

use super::{Page, SettingsWindow, search};
use crate::ui::controls::{self, status_note as note};
use crate::ui::icon::Glyph;
use crate::ui::keycap::KeySequence;
use crate::ui::select::{Choice, Select};
use crate::ui::theme::Theme;

/// A recording recorder's key context: while it listens, its keys are the
/// binding being recorded, not the window's navigation.
const RECORDER: &str = "KeyboardRecorder";

/// A recorder's key context while it rests: a button.
const RECORDER_IDLE: &str = "KeyboardRecorderIdle";

actions!(keyboard, [ActivateRecorder, CancelRecording]);

/// Registers the recorders' key bindings, in their own contexts — deeper
/// in the focus stack than the sidebar's and the window's keys, so while a
/// recorder listens they never fall through. The navigation keys are bound
/// to [`gpui::NoAction`] there: the sidebar stays put while keys are
/// captured, and everything else reaches the recorder's own key handler
/// as the combination being recorded.
pub(crate) fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        // The recorders are buttons: Enter and Space activate them, as a
        // click does — and while one listens, they do nothing: the keys
        // stay captured, and Enter and Space cannot be part of a binding.
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

/// What the page is, in one line: its sidebar entry's description in
/// the search.
pub(crate) const ABOUT: &str = "Keys for moving around Pane";

/// The labels of the page's sections.
pub(crate) const BEHAVIOR: &str = "Behavior";
pub(crate) const SECTION: &str = "Shortcuts";
pub(crate) const GAME: &str = "Game mode";

/// The name of the row that says the state of Pane's own keyboard hook
/// (Windows, #252), in a card under the page's actions: shown while a
/// hotkey of yours is dispatched through the hook, with whether it is
/// installed, how many times Windows removed it and Pane installed it
/// again, and whether its pages are pinned in memory (#259). Where no
/// hook is in use nothing shows, as the rows of the system's own
/// registrations say nothing of a route.
pub(crate) const HOOK: &str = "Pane's keyboard hook";

/// The Behavior section's rows.
pub(crate) const ESCAPE_NAME: &str = "Escape key behavior";
pub(crate) const ESCAPE_CLOSES_NAME: &str = "Escape key closes Settings";
pub(crate) const NAVIGATION_NAME: &str = "Navigation bindings";

/// The Game mode section's rows: the on/off choice, and the field a
/// program is named in (#125).
pub(crate) const GAME_MODE_NAME: &str = "Pause Pane's hotkeys while a game is in front";
pub(crate) const GAME_PROGRAMS_NAME: &str = "Treat a program as a game";

/// What the Game mode section says where no foreground source is
/// attached, so the choice is not offered: game mode is Windows only.
const GAME_NOT_OFFERED: &str = "Game mode is available on Windows only";
/// What it says there on Windows, whose source the system refused.
const GAME_NO_WATCH: &str = "Game mode needs Pane's watch on the windows that come to the front, \
which Windows did not grant";

/// The escape behaviors the page offers, in segment order: the
/// preference, the segment's name and its test selector.
pub(crate) const ESCAPES: [(EscapeBehavior, &str, &str); 2] = [
    (
        EscapeBehavior::BackOrHide,
        "Go back",
        "keyboard-escape-back",
    ),
    (EscapeBehavior::Hide, "Hide Pane", "keyboard-escape-hide"),
];

/// The navigation bindings the page offers: the preference, the choice's
/// id and its name, Raycast's. The label adds the keys the choice binds
/// ([`navigation_label`]).
const NAVIGATIONS: [(NavigationBindings, &str, &str); 3] = [
    (NavigationBindings::None, "none", "None"),
    (NavigationBindings::Emacs, "emacs", "Emacs"),
    (NavigationBindings::Vim, "vim", "Vim Motions"),
];

/// A navigation choice's label: its name, then the keys it binds as this
/// platform writes them ("Emacs (Alt+P, Alt+N, Alt+B, Alt+F)"), only the
/// keys that do something — the selection's pair, and Left and Right,
/// the keys that move between the query and the argument fields (#258).
fn navigation_label(navigation: NavigationBindings, name: &str) -> String {
    let Some((previous, next)) = navigation.bindings() else {
        return name.to_owned();
    };
    let shown = |id: &str| Binding::parse(id).map_or_else(|_| id.to_owned(), |b| b.to_string());
    let mut keys = vec![shown(previous), shown(next)];
    if let Some((left, right)) = navigation.left_right() {
        keys.push(shown(left));
        keys.push(shown(right));
    }
    format!("{name} ({})", keys.join(", "))
}

/// The Keyboard page, registered after Shortcuts in the window's page
/// list, as the reference's sections order it.
pub(crate) fn page() -> Page {
    Page {
        title: "Keyboard",
        about: ABOUT,
        icon: Glyph::Keyboard,
        count: None,
        render,
        search: entries,
        focus,
    }
}

/// The settings the page offers the sidebar's search: the Behavior
/// section's rows, then each navigation action of the bounded set, named
/// as the page's row names it, in the group it sits in, then the
/// keyboard hook's state while a hook is in use. Read live, so a change
/// is in the next catalog as it lands.
fn entries(launcher: &Launcher, _cx: &App) -> Vec<search::Entry> {
    let behavior = [
        ("keyboard-escape", ESCAPE_NAME),
        ("keyboard-escape-closes", ESCAPE_CLOSES_NAME),
        ("keyboard-navigation", NAVIGATION_NAME),
    ]
    .into_iter()
    .map(|(control, title)| search::Entry {
        control: Some(control.into()),
        title: title.into(),
        group: Some(BEHAVIOR.into()),
        unavailable: None,
    });
    let game = [
        ("keyboard-game-mode", GAME_MODE_NAME),
        ("keyboard-game-program", GAME_PROGRAMS_NAME),
    ]
    .into_iter()
    .map(|(control, title)| search::Entry {
        control: Some(control.into()),
        title: title.into(),
        group: Some(GAME.into()),
        // Game mode is offered where a foreground source is attached,
        // which the section itself says; the settings it lists are never
        // unavailable here.
        unavailable: None,
    });
    let actions = KeyboardAction::ALL.into_iter().map(|action| search::Entry {
        control: Some(action.id().into()),
        title: action.title().into(),
        group: Some(SECTION.into()),
        // A binding set to something the keyboard cannot use is the
        // page's own refusal to explain; the setting itself is never
        // unavailable here.
        unavailable: None,
    });
    // The hook's state row, offered exactly while the page shows it: a
    // jump reveals the row, which takes no focus of its own.
    let hook = launcher.hook_health().map(|_| search::Entry {
        control: Some("keyboard-hook".into()),
        title: HOOK.into(),
        group: None,
        unavailable: None,
    });
    behavior.chain(game).chain(actions).chain(hook).collect()
}

/// Each action's recorder takes keyboard focus — the recorders are tab
/// stops, and Enter on one starts recording — so a jump to an action
/// focuses its recorder, ready to rebind; a jump to the navigation
/// bindings focuses their select. Anything else is not focusable here:
/// `false` falls back to the sidebar.
fn focus(
    this: &mut SettingsWindow,
    target: &str,
    window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> bool {
    if target == "keyboard-navigation" {
        let trigger = this.keyboard.navigation.read(cx).trigger_focus();
        window.focus(&trigger, cx);
        return true;
    }
    if target == "keyboard-game-program" {
        let input = program_input(this, cx);
        window.focus(&input.focus_handle(cx), cx);
        return true;
    }
    let Some(action) = KeyboardAction::of(target) else {
        return false;
    };
    let Some(focus) = this.keyboard.focuses.get(&action) else {
        return false;
    };
    window.focus(focus, cx);
    true
}

/// The Keyboard page's state, held by the window as a field: the
/// recorder and the navigation bindings' select.
pub(crate) struct State {
    /// The action whose binding is being recorded, if any.
    pub(crate) recording: Option<KeyboardAction>,
    /// The action whose last attempt was refused, and why: a protected
    /// key, a collision, or what a save reported. Shown under that
    /// action's row; a recorder keeps listening for another try.
    pub(crate) rejection: Option<(KeyboardAction, String)>,
    /// Each action's recorder focus: a tab stop, so the keyboard reaches
    /// every recorder; the recording action's holds focus while it
    /// listens.
    focuses: BTreeMap<KeyboardAction, FocusHandle>,
    /// The navigation bindings' select.
    navigation: Entity<Select>,
    /// The Game mode section's state (#125): the program field, and what
    /// a change of the settings is doing.
    pub(crate) game: GameState,
}

impl State {
    /// The page's state, over the window's `cx` (its focus handles).
    pub(crate) fn new(window: &mut Window, cx: &mut Context<SettingsWindow>) -> State {
        let focuses = KeyboardAction::ALL
            .into_iter()
            .map(|action| {
                let focus = cx.focus_handle().tab_stop(true);
                (action, focus)
            })
            .collect();
        let navigation = super::choice_select(
            NAVIGATION_NAME,
            "keyboard-navigation",
            navigation_choices,
            |cx| {
                let chosen = crate::settings::shared(cx).read(cx).navigation();
                NAVIGATIONS
                    .iter()
                    .find(|&&(navigation, ..)| navigation == chosen)
                    .map_or("none", |&(_, id, _)| id)
            },
            |id, cx| {
                if let Some(&(navigation, ..)) = NAVIGATIONS.iter().find(|&&(_, of, _)| of == id) {
                    // A conflicting choice is listed unavailable, so the
                    // select never commits one.
                    let _ = crate::settings::shared(cx)
                        .update(cx, |settings, cx| settings.set_navigation(navigation, cx));
                }
            },
            window,
            cx,
        );
        State {
            recording: None,
            rejection: None,
            focuses,
            navigation,
            game: GameState::default(),
        }
    }
}

/// The Game mode section's state: the field a program is named in, and
/// what a change of the settings is doing (#125).
#[derive(Default)]
pub(crate) struct GameState {
    /// The program field, made the first time the section draws it.
    pub(crate) program: Option<Entity<EditableTextState>>,
    /// Whether a change of the settings is being recorded.
    pub(crate) busy: bool,
    /// Why the last change could not be recorded, if it could not.
    pub(crate) problem: Option<String>,
}

/// The navigation bindings' choices, a set whose keys an action has been
/// rebound to listed with the reason.
fn navigation_choices(cx: &App) -> Vec<Choice> {
    let settings = crate::settings::shared(cx);
    let settings = settings.read(cx);
    NAVIGATIONS
        .iter()
        .map(|&(navigation, id, name)| {
            super::choice(
                id,
                navigation_label(navigation, name),
                settings.navigation_conflict(navigation),
            )
        })
        .collect()
}

/// Which of the Keyboard page's controls an element is, for the caller of
/// [`compose`] that attaches its behavior.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum KeyboardControl {
    /// The action's recorder.
    Recorder(KeyboardAction),
    /// The action's reset button, enabled while its binding is not the
    /// default.
    Reset(KeyboardAction),
    /// An escape behavior's segment.
    Escape(EscapeBehavior),
    /// The Escape-closes-Settings switch.
    EscapeCloses,
}

/// One action of the page, as plain values.
pub(crate) struct KeyboardRow {
    pub(crate) action: KeyboardAction,
    /// The binding's caps, from the launcher's binding adapter
    /// (`crate::keyboard::binding_keys`), and the binding as the user
    /// names it.
    pub(crate) keys: KeySequence,
    pub(crate) binding: String,
    /// The default binding, named, while the binding is not the default
    /// (Reset is offered); `None` at the default.
    pub(crate) default: Option<String>,
}

/// What the Keyboard page shows, as plain values: what [`render`] reads
/// from the host settings (#99).
pub(crate) struct KeyboardView {
    pub(crate) rows: Vec<KeyboardRow>,
    /// The action whose recorder is listening, if any.
    pub(crate) recording: Option<KeyboardAction>,
    /// The action whose last attempt was refused, and what it was refused
    /// with.
    pub(crate) rejection: Option<(KeyboardAction, String)>,
    pub(crate) status: Option<String>,
    /// The Behavior section's choices.
    pub(crate) escape: EscapeBehavior,
    pub(crate) escape_closes: bool,
    /// The Game mode section's choices and state (#125).
    pub(crate) game: GameView,
}

/// What the Game mode section shows, as plain values (#125): the choices
/// as the launcher holds them, and what a change of them is doing.
pub(crate) struct GameView {
    /// Whether game mode is on.
    pub(crate) on: bool,
    /// The programs to treat as games.
    pub(crate) programs: Vec<String>,
    /// Whether the choice is offered here: a foreground source is
    /// attached, which is Windows' or a test's fake.
    pub(crate) offered: bool,
    /// Whether a change is being recorded.
    pub(crate) busy: bool,
    /// Why the last change could not be recorded, if it could not.
    pub(crate) problem: Option<String>,
}

/// The rows of the bounded set of actions, in the set's order, as
/// `keyboard` binds them.
pub(crate) fn rows(keyboard: &Keyboard) -> Vec<KeyboardRow> {
    let defaults = Keyboard::default_for_this_system();
    KeyboardAction::ALL
        .into_iter()
        .map(|action| {
            let binding = keyboard.binding(action);
            KeyboardRow {
                action,
                keys: crate::keyboard::binding_keys(binding),
                binding: binding.to_string(),
                default: (!keyboard.is_default(action))
                    .then(|| defaults.binding(action).to_string()),
            }
        })
        .collect()
}

/// Draws the Keyboard page: the Behavior section, then the navigation
/// actions, each with its recorder, then the Game mode section.
fn render(
    this: &mut SettingsWindow,
    window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let theme = crate::settings::visuals(cx).theme;
    // Everything the page shows about the bindings comes from the host
    // settings: the choices, and what a save reported.
    let (keyboard, status, escape, escape_closes) = {
        let settings = crate::settings::shared(cx).read(cx);
        (
            settings.keyboard(),
            settings.status(),
            settings.escape(),
            settings.escape_closes_settings(),
        )
    };
    // Game mode's choices come from the launcher, which applies them
    // with the pause they decide (#125).
    let mode = this.launcher.game_mode();
    let view = KeyboardView {
        rows: rows(&keyboard),
        recording: this.keyboard.recording,
        rejection: this.keyboard.rejection.clone(),
        status,
        escape,
        escape_closes,
        game: GameView {
            on: mode.on,
            programs: mode.programs,
            offered: this.launcher.game_mode_offered(),
            busy: this.keyboard.game.busy,
            problem: this.keyboard.game.problem.clone(),
        },
    };
    // Each row's scroll anchor, which the search's reveal scrolls to (see
    // the window's render), and each recorder's focus.
    let anchors: BTreeMap<KeyboardAction, ScrollAnchor> = KeyboardAction::ALL
        .into_iter()
        .map(|action| (action, this.search_anchor(action.id())))
        .collect();
    let escape_anchor = this.search_anchor("keyboard-escape");
    let closes_anchor = this.search_anchor("keyboard-escape-closes");
    let navigation = div()
        .id("keyboard-navigation")
        .flex_none()
        .anchor_scroll(Some(this.search_anchor("keyboard-navigation")))
        .child(this.keyboard.navigation.clone());
    // The keyboard hook's state, read from the launcher's adapter as of
    // this frame: the row shows only while a hook is in use, as its
    // entry in the search does.
    let hook_health = this.launcher.hook_health();
    let hook_anchor = this.search_anchor("keyboard-hook");
    let hook = hook_health
        .as_ref()
        .map(|health| hook_section(health, hook_anchor, &theme));
    let focuses = this.keyboard.focuses.clone();
    let recording = this.keyboard.recording;
    let defaults = Keyboard::default_for_this_system();
    let game = game_section(this, &view.game, &theme, window, cx);
    compose(
        &view,
        Some(navigation),
        hook,
        Some(game),
        &theme,
        |control, element| match control {
            KeyboardControl::Recorder(action) => {
                let focus = focuses
                    .get(&action)
                    .expect("every action has a recorder focus")
                    .clone();
                element
                    .anchor_scroll(anchors.get(&action).cloned())
                    .key_context(if recording == Some(action) {
                        RECORDER
                    } else {
                        RECORDER_IDLE
                    })
                    .track_focus(&focus)
                    .on_action(cx.listener(move |this, _: &ActivateRecorder, window, cx| {
                        this.keyboard_activate_recorder(action, window, cx);
                    }))
                    .on_action(cx.listener(move |this, _: &CancelRecording, window, cx| {
                        this.keyboard_cancel_recording(window, cx);
                    }))
                    .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                        this.keyboard_key_down(event, window, cx);
                    }))
                    // A mouse-down anywhere outside the recorder while it
                    // listens cancels the recording and is consumed, as the
                    // footer menu's popup does: the click underneath does not
                    // act, and the recorder gives up the keys.
                    .on_mouse_down_out(cx.listener(move |this, _: &MouseDownEvent, window, cx| {
                        if this.keyboard.recording == Some(action) {
                            this.keyboard_cancel_recording(window, cx);
                            cx.stop_propagation();
                        }
                    }))
                    // A click starts recording, or stops it again.
                    .on_click(cx.listener(move |this, _: &gpui::ClickEvent, window, cx| {
                        if this.keyboard.recording == Some(action) {
                            this.keyboard_cancel_recording(window, cx);
                        } else {
                            this.keyboard_activate_recorder(action, window, cx);
                        }
                    }))
            }
            KeyboardControl::Reset(action) => {
                let default = defaults.binding(action).clone();
                element.on_click(cx.listener(move |this, _: &gpui::ClickEvent, window, cx| {
                    // The reset sits inside the recorder: its click is its own.
                    cx.stop_propagation();
                    if !keyboard_is_default(action, cx) {
                        this.keyboard_apply(action, default.clone(), window, cx);
                    }
                }))
            }
            KeyboardControl::Escape(escape) => element
                .anchor_scroll(Some(escape_anchor.clone()))
                .on_click(cx.listener(move |_, _: &gpui::ClickEvent, _, cx| {
                    crate::settings::shared(cx).update(cx, |settings, cx| {
                        settings.set_escape(escape, cx);
                    });
                })),
            KeyboardControl::EscapeCloses => element
                .anchor_scroll(Some(closes_anchor.clone()))
                .on_click(cx.listener(|_, _: &gpui::ClickEvent, _, cx| {
                    let settings = crate::settings::shared(cx);
                    let closes = settings.read(cx).escape_closes_settings();
                    settings.update(cx, |settings, cx| {
                        settings.set_escape_closes_settings(!closes, cx);
                    });
                })),
        },
    )
    .into_any_element()
}

/// Whether `action`'s binding is the default now.
fn keyboard_is_default(action: KeyboardAction, cx: &App) -> bool {
    crate::settings::shared(cx)
        .read(cx)
        .keyboard()
        .is_default(action)
}

/// The Keyboard page's composition: the Behavior section — the escape behavior's segments, the
/// Escape-closes-Settings switch and `navigation` (the navigation
/// bindings' select) — then the Shortcuts section, a card of a settings
/// row per action with its recorder at its end, with what a save reported
/// under the card, and under them, where a keyboard hook is in use, the
/// card of its state (`hook`, see [`hook_section`]). `attach` adds each
/// control's behavior; the composition gives each its identity, its
/// accessibility and its look.
pub(crate) fn compose(
    view: &KeyboardView,
    navigation: Option<Stateful<Div>>,
    hook: Option<Div>,
    game: Option<AnyElement>,
    theme: &Theme,
    attach: impl Fn(KeyboardControl, Stateful<Div>) -> Stateful<Div>,
) -> Stateful<Div> {
    let segments = ESCAPES.iter().map(|&(escape, name, selector)| {
        let chosen = escape == view.escape;
        let segment = controls::segment(name, chosen, true, theme)
            .id(selector)
            .debug_selector(move || selector.into())
            .role(Role::RadioButton)
            .aria_label(name)
            .aria_toggled(if chosen {
                Toggled::True
            } else {
                Toggled::False
            });
        attach(KeyboardControl::Escape(escape), segment)
    });
    let escape = controls::setting_row(ESCAPE_NAME, Vec::new(), theme)
        .debug_selector(|| "keyboard-escape-field".into())
        .child(
            controls::row_segment_track(theme)
                .id("keyboard-escape")
                .role(Role::RadioGroup)
                .aria_label(ESCAPE_NAME)
                .children(segments),
        );
    let closes = super::general::switch_row(
        super::general::SwitchRow {
            id: "keyboard-escape-closes",
            selector: "keyboard-escape-closes",
            title: ESCAPE_CLOSES_NAME,
            on: view.escape_closes,
            offered: true,
            lines: Vec::new(),
        },
        theme,
        |switch| attach(KeyboardControl::EscapeCloses, switch),
    );
    let navigation = controls::setting_row(NAVIGATION_NAME, Vec::new(), theme)
        .debug_selector(|| "keyboard-navigation-field".into())
        .children(navigation);
    let behavior = controls::card(
        [
            escape.into_any_element(),
            closes.into_any_element(),
            navigation.into_any_element(),
        ],
        theme,
    );
    let rows = view.rows.iter().map(|row| {
        let listening = view.recording == Some(row.action);
        let rejection = view
            .rejection
            .as_ref()
            .filter(|(action, _)| *action == row.action)
            .map(|(_, why)| why);
        recorder_row(row, listening, rejection, theme, &attach).into_any_element()
    });
    let inset = theme.geometry.settings.section_label_inset;
    let shortcuts =
        div()
            .flex()
            .flex_col()
            .gap(theme.geometry.settings.section_label_gap)
            .child(controls::card(rows, theme))
            // What a save reported, if it failed.
            .children(view.status.as_ref().map(|status| {
                note("keyboard-status", status.clone(), theme.danger, theme).px(inset)
            }));
    let page = controls::page(theme)
        .child(controls::section(Some(BEHAVIOR.into()), behavior, theme))
        .child(
            controls::section(Some(SECTION.into()), shortcuts, theme)
                .debug_selector(|| "keyboard-field".into()),
        )
        .children(hook)
        // The Game mode section, its rows built where their behavior
        // lives (`game_section`), as the navigation select's are.
        .children(game);
    div()
        .id("keyboard")
        .debug_selector(|| "keyboard".into())
        .child(page)
}

/// The card that says the state of Pane's own keyboard hook (#259): one
/// row under the page's actions, named [`HOOK`], saying the adapter's
/// [`HookHealth::note`] under its name. A hook Pane gave up keeping
/// installed says its reason in the warning ink; a working one's state
/// is the muted ink of a row's description. It takes no focus and no
/// click — a jump from the search reveals it, and the sidebar keeps the
/// window's keyboard focus.
fn hook_section(health: &HookHealth, anchor: ScrollAnchor, theme: &Theme) -> Div {
    let tone = if health.given_up.is_some() {
        theme.warning
    } else {
        theme.text_muted
    };
    let state = note("keyboard-hook", health.note(), tone, theme);
    let row = controls::setting_row(HOOK, vec![state.into_any_element()], theme)
        .id("keyboard-hook-row")
        .debug_selector(|| "keyboard-hook-row".into())
        .anchor_scroll(Some(anchor));
    controls::section(None, controls::card([row.into_any_element()], theme), theme)
}

/// An action's settings row: its title at the left, with why its last
/// recording or reset was refused under it, and its recorder at
/// its right end — the binding written out, the record mark and the reset
/// button (enabled while the binding is not the default). The recorder is
/// a button: clicked or pressed with Enter it listens for the keys of the
/// next binding, holding focus and ringed red, and takes the keys pressed
/// as the binding being recorded (Escape, Tab, a click outside or another
/// click on it cancels); a combination that is refused keeps it listening
/// for another try. Reset goes back through the same checks a recording
/// takes, so a reset that would land on another action's binding is
/// refused with the same explanation.
fn recorder_row(
    row: &KeyboardRow,
    listening: bool,
    rejection: Option<&String>,
    theme: &Theme,
    attach: &impl Fn(KeyboardControl, Stateful<Div>) -> Stateful<Div>,
) -> Div {
    let action = row.action;
    let label = format!(
        "{}{} with {}",
        if listening { "Recording; " } else { "" },
        action.title(),
        row.binding
    );
    let reset_id = SharedString::from(format!("keyboard-reset-{}", action.id()));
    let reset = controls::icon_button(reset_id, Glyph::Reset, row.default.is_some(), theme)
        .debug_selector(move || format!("keyboard-reset-{}", action.id()))
        .role(Role::Button)
        .when_some(row.default.as_ref(), |reset, default| {
            reset.aria_label(format!("Reset {} to {default}", action.title()))
        })
        .when(row.default.is_none(), |reset| {
            reset
                .aria_label(format!("Reset {}", action.title()))
                .aria_disabled(true)
        });
    let reset = attach(KeyboardControl::Reset(action), reset);
    let recorder = controls::recorder(
        controls::binding_text(&row.keys),
        listening,
        Some(reset.into_any_element()),
        theme,
    )
    .id(action.id())
    .debug_selector(move || format!("keyboard-{}", action.id()))
    .role(Role::Button)
    .aria_label(label);
    let lines = rejection
        .map(|rejection| {
            note("keyboard-refusal", rejection.clone(), theme.danger, theme).into_any_element()
        })
        .into_iter()
        .collect();
    controls::setting_row(action.title(), lines, theme)
        .debug_selector(move || format!("keyboard-row-{}", action.id()))
        .child(attach(KeyboardControl::Recorder(action), recorder))
}

/// Draws the Game mode section (#125): the on/off choice, then — where
/// a foreground source is attached, which is Windows' or a test's fake —
/// each program to treat as a game with Remove, and the field a program
/// is named in with its Add button. Where none is, the choice is not
/// offered and the section says so: game mode is Windows only. What a
/// change could not record is the section's note, as the actions'
/// status is the card's.
fn game_section(
    this: &mut SettingsWindow,
    view: &GameView,
    theme: &Theme,
    window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let mode_anchor = this.search_anchor("keyboard-game-mode");
    let lines = if view.offered {
        vec![
            controls::row_line(
                "Pane's hotkeys are released while a game is in front, and come back when it \
                 leaves",
                theme.text_muted,
                theme,
            ),
            controls::row_line(
                "A full-screen game is recognized by itself; windowed ones are named below",
                theme.text_muted,
                theme,
            ),
        ]
    } else {
        vec![controls::row_line(
            if cfg!(target_os = "windows") {
                GAME_NO_WATCH
            } else {
                GAME_NOT_OFFERED
            },
            theme.text_muted,
            theme,
        )]
    };
    let switch = super::general::switch_row(
        super::general::SwitchRow {
            id: "keyboard-game-mode",
            selector: "keyboard-game-mode",
            title: GAME_MODE_NAME,
            on: view.on,
            offered: view.offered && !view.busy,
            lines,
        },
        theme,
        |switch| {
            switch
                .anchor_scroll(Some(mode_anchor))
                .when(view.offered && !view.busy, |switch| {
                    switch.on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                        let on = !this.launcher.game_mode().on;
                        game_change(this, move |mode| mode.on = on, cx);
                    }))
                })
        },
    )
    .into_any_element();
    let mut rows = vec![switch];
    if view.offered {
        for program in &view.programs {
            let removed = program.clone();
            let shown = program.clone();
            rows.push(
                controls::setting_row(
                    program.clone(),
                    vec![controls::row_line(
                        "Treated as a game while it is in front",
                        theme.text_muted,
                        theme,
                    )],
                    theme,
                )
                .debug_selector(move || format!("keyboard-game-program-{shown}"))
                .child(
                    button(
                        format!("keyboard-game-remove-{program}"),
                        "Remove",
                        !view.busy,
                        theme,
                    )
                    .when(!view.busy, |button| {
                        button.on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                            this.remove_game_program(&removed, cx);
                        }))
                    }),
                )
                .into_any_element(),
            );
        }
        let input = program_input(this, cx);
        let focused = input.focus_handle(cx).is_focused(window);
        let text = input.read(cx).as_str().to_owned();
        let placeholder = "such as game.exe";
        let field_anchor = this.search_anchor("keyboard-game-program");
        let well = controls::well(true, theme)
            .w(px(240.))
            .id("keyboard-game-program")
            .debug_selector(|| "keyboard-game-program-field".into())
            .anchor_scroll(Some(field_anchor))
            .track_focus(&input.focus_handle(cx))
            .shadow(controls::well_shadows(focused, theme))
            .role(Role::TextInput)
            .aria_label(GAME_PROGRAMS_NAME)
            .aria_value(text.clone())
            .aria_placeholder(placeholder)
            .child(controls::well_input(
                text_input("keyboard-game-input").state(input.downgrade()),
                placeholder,
                theme,
            ));
        let offered = !view.busy && !text.trim().is_empty();
        let add =
            button("keyboard-game-add".into(), "Add", offered, theme).when(offered, |button| {
                button.on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                    add_program(this, cx);
                }))
            });
        rows.push(
            controls::setting_row(
                GAME_PROGRAMS_NAME,
                vec![controls::row_line(
                    "The program's file name, matched wherever it is installed",
                    theme.text_muted,
                    theme,
                )],
                theme,
            )
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(theme.geometry.controls.button_gap)
                    .child(well)
                    .child(add),
            )
            .into_any_element(),
        );
    }
    let inset = theme.geometry.settings.section_label_inset;
    let problem = view
        .problem
        .as_ref()
        .map(|problem| note("game-status", problem.clone(), theme.danger, theme).px(inset));
    let card = div()
        .flex()
        .flex_col()
        .gap(theme.geometry.settings.section_label_gap)
        .child(controls::card(rows, theme))
        .children(problem);
    controls::section(Some(GAME.into()), card, theme)
        .debug_selector(|| "keyboard-game".into())
        .into_any_element()
}

/// The program field's state, created the first time it is wanted.
fn program_input(
    this: &mut SettingsWindow,
    cx: &mut Context<SettingsWindow>,
) -> Entity<EditableTextState> {
    if let Some(input) = &this.keyboard.game.program {
        return input.clone();
    }
    let input = cx.new(|cx| EditableTextState::new(StringStorage::default(), cx));
    input.focus_handle(cx).tab_stop(true);
    // The Add button follows the text.
    cx.subscribe(&input, |_, _, _: &TextChanged, cx| cx.notify())
        .detach();
    this.keyboard.game.program = Some(input.clone());
    input
}

/// Changes game mode's settings through `edit` and records them (#125):
/// the switch's choice, a program named or removed. The section's rows
/// dim while the record is written, and a failure is the section's note,
/// as the File search page's changes are.
fn game_change(
    this: &mut SettingsWindow,
    edit: impl FnOnce(&mut GameMode),
    cx: &mut Context<SettingsWindow>,
) {
    let mut mode = this.launcher.game_mode();
    edit(&mut mode);
    let pending = this.launcher.set_game_mode(mode);
    this.keyboard.game.busy = true;
    this.keyboard.game.problem = None;
    cx.notify();
    cx.spawn(async move |this, cx| {
        let outcome = pending.await;
        this.update(cx, |this, cx| {
            this.keyboard.game.busy = false;
            this.keyboard.game.problem = outcome.err();
            cx.notify();
        })
        .ok();
    })
    .detach();
}

/// Adds the program in the field to game mode's settings, clearing it,
/// as the File search page's pattern field is cleared by its Exclude.
fn add_program(this: &mut SettingsWindow, cx: &mut Context<SettingsWindow>) {
    let Some(input) = this.keyboard.game.program.clone() else {
        return;
    };
    let program = input.read(cx).as_str().trim().to_owned();
    if program.is_empty() {
        return;
    }
    input.update(cx, |input, cx| input.emplace("", cx));
    game_change(
        this,
        move |mode| {
            if !mode
                .programs
                .iter()
                .any(|kept| kept.eq_ignore_ascii_case(&program))
            {
                mode.programs.push(program.clone());
            }
        },
        cx,
    );
}

/// A text button with its test selector, as the File search page's are.
fn button(
    selector: String,
    label: &'static str,
    enabled: bool,
    theme: &Theme,
) -> gpui::Stateful<gpui::Div> {
    let debug = selector.clone();
    controls::button(SharedString::from(selector), label, enabled, theme)
        .debug_selector(move || debug)
        .role(Role::Button)
        .aria_label(label)
}

impl SettingsWindow {
    /// A recorder row's activation: Enter, Space or a click on its row.
    /// With no recording in progress it starts listening for that row's
    /// action; while one listens the keys are captured, so this does
    /// nothing — Enter and Space cannot be part of a binding, and the
    /// recorder keeps listening.
    fn keyboard_activate_recorder(
        &mut self,
        action: KeyboardAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.stop_propagation();
        if self.keyboard.recording.is_none() {
            self.keyboard.recording = Some(action);
            self.keyboard.rejection = None;
            let focus = self
                .keyboard
                .focuses
                .get(&action)
                .expect("every action has a row focus")
                .clone();
            window.focus(&focus, cx);
            cx.notify();
        }
    }

    /// Escape on a recorder row, or a mouse-down outside one: cancels
    /// recording, changing nothing.
    fn keyboard_cancel_recording(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.stop_propagation();
        if self.keyboard.recording.take().is_some() {
            self.keyboard.rejection = None;
            let sidebar = self.focus.clone();
            window.focus(&sidebar, cx);
            cx.notify();
        }
    }

    /// A key pressed while a recorder listens: the combination it names is
    /// the binding to record. The keys the recorder handles itself —
    /// Enter, Space and Escape for its activation and cancellation — never
    /// reach here; the sidebar's and traversal keys are swallowed in the
    /// recorder's context, and a key no rule refuses records.
    fn keyboard_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(action) = self.keyboard.recording else {
            return;
        };
        cx.stop_propagation();
        match crate::keyboard::binding_of(&event.keystroke) {
            Ok(binding) => self.keyboard_apply(action, binding, window, cx),
            Err(problem) => {
                self.keyboard.rejection = Some((action, format!("{problem}.")));
                cx.notify();
            }
        }
    }

    /// Applies `binding` as `action`'s: through the host settings, which
    /// re-make every window's key bindings before keeping and saving the
    /// choice. A refusal leaves everything as it was; the reason is the
    /// page's status, and the recorder — if one is listening — keeps
    /// listening for another try.
    /// Removes `program` from game mode's settings, recording the change
    /// (#125), as the program row's Remove button does.
    fn remove_game_program(&mut self, program: &str, cx: &mut Context<Self>) {
        let removed = program.to_owned();
        game_change(
            self,
            move |mode| mode.programs.retain(|kept| *kept != removed),
            cx,
        );
    }

    fn keyboard_apply(
        &mut self,
        action: KeyboardAction,
        binding: Binding,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let applied = crate::settings::shared(cx).update(cx, |settings, cx| {
            settings.set_keyboard(action, binding, cx)
        });
        match applied {
            Ok(()) => {
                // The change landed: the recorder is done, and focus
                // returns to the sidebar.
                self.keyboard.recording = None;
                self.keyboard.rejection = None;
                let sidebar = self.focus.clone();
                window.focus(&sidebar, cx);
                cx.notify();
            }
            Err(reason) => {
                self.keyboard.rejection = Some((action, reason));
                cx.notify();
            }
        }
    }
}
