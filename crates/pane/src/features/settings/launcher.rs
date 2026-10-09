//! The Launcher page: the choices that govern the launcher window — which
//! display it opens on, when reopening it pops back to root search, how
//! strict root search's matching is, and its layout: the window mode
//! (expanded or compact), how the pins are laid out, and whether the
//! compact window shows them.
//!
//! Every value it shows and every choice it takes goes through the host
//! settings ([`crate::settings`]), so the record's own rules — atomic
//! writes, an unreadable record never replaced, a failed save reported
//! with the shown choice rolled back — are the ones these choices live
//! by. The display and reopening choices change nothing the windows
//! render: the opening display is resolved against the display layout
//! every time the launcher opens (through [`crate::placement`], the
//! platform seam this page also reads to explain the choices), and what
//! reopening shows is applied when the launcher is next opened. The
//! layout choices are read by the launcher window as it draws, and the
//! search sensitivity is applied by the launcher's matcher on the next
//! keystroke after the choice is taken.
//!
//! The display, the reopening delay and the search sensitivity are
//! Pane-styled searchable selects ([`crate::ui::select`]); the window mode
//! and the pinned layout are segmented choices (#99, the Settings board's
//! family), two fixed choices a user scans faster than searches; showing
//! the pins in the compact window is a switch.
//!
//! What the page explains, as the General page does for its hotkey: the
//! choices the platform cannot answer — the display with the mouse where
//! the system does not tell Pane where the pointer is, the display with
//! the active window where it does not tell Pane which window is active —
//! are shown with their reason and not offered, rather than pretending
//! they succeeded; a platform that cannot choose the launcher's display
//! at all (Wayland) explains that instead; and a choice whose display
//! cannot be found falls back to the primary display, which the page
//! says.

use std::rc::Rc;

use gpui::{
    AnyElement, App, Context, Div, Entity, Role, SharedString, Stateful, Toggled, Window, div,
    prelude::*,
};
use pane_core::placement::{DisplayLayout, resolve};
use pane_core::{Launcher, OpeningMonitor, PinnedLayout, Reopening, SearchSensitivity, WindowMode};

use super::{Page, SettingsWindow, search};
use crate::ui::controls::{self, status_note as note};
use crate::ui::icon::Glyph;
use crate::ui::select::{Choice, Model, Select};
use crate::ui::theme::Theme;

/// The opening-monitor choices the page offers, in row order: the
/// preference, the row's name, declared search keywords and test
/// selector. The pointer's display is the default (see
/// [`OpeningMonitor`]), so it comes first.
const MONITORS: [(OpeningMonitor, &str, &[&str], &str); 3] = [
    (
        OpeningMonitor::Pointer,
        "Display with the mouse",
        &["pointer", "cursor"],
        "launcher-monitor-Pointer",
    ),
    (
        OpeningMonitor::Primary,
        "Primary display",
        &["main"],
        "launcher-monitor-Primary",
    ),
    (
        OpeningMonitor::ActiveWindow,
        "Display with the active window",
        &["focused", "foreground"],
        "launcher-monitor-ActiveWindow",
    ),
];

/// The reopening choices the page offers (Raycast's "Pop to Root
/// Search"), in list order: the preference, the choice's name, what it
/// does and its id, which the select's row selector ends with.
pub(crate) const REOPENINGS: [(Reopening, &str, &str, &str); 4] = [
    (
        Reopening::RootSearch,
        "Immediately",
        "Starts from an empty search",
        "RootSearch",
    ),
    (
        Reopening::After90Seconds,
        "After 90 seconds",
        "Shows what you left open if you come back within 90 seconds",
        "After90Seconds",
    ),
    (
        Reopening::After3Minutes,
        "After 3 minutes",
        "Shows what you left open if you come back within 3 minutes",
        "After3Minutes",
    ),
    (
        Reopening::RestoreView,
        "Never",
        "Shows what you left open, if it still exists",
        "RestoreView",
    ),
];

/// The reopening row's name and its select's debug prefix.
pub(crate) const REOPENING_NAME: &str = "Pop to root search";
pub(crate) const REOPENING_DEBUG: &str = "launcher-reopening";

/// The search sensitivity choices the page offers, in list order: the
/// preference, the choice's name (the select's id too) and what it
/// matches. High is the default, so it comes first.
pub(crate) const SENSITIVITY_NAME: &str = "Search sensitivity";
pub(crate) const SENSITIVITY_DEBUG: &str = "launcher-sensitivity";
pub(crate) const SENSITIVITIES: [(SearchSensitivity, &str, &str); 3] = [
    (
        SearchSensitivity::High,
        "High",
        "Matches that also start the text or a word of it",
    ),
    (
        SearchSensitivity::Medium,
        "Medium",
        "Word starts and tighter placements",
    ),
    (
        SearchSensitivity::Low,
        "Low",
        "Every placement the letters can make",
    ),
];

/// The layout rows: their names, and their segments — the preference, the
/// segment's name and its test selector.
pub(crate) const WINDOW_MODE_NAME: &str = "Window mode";
pub(crate) const WINDOW_MODES: [(WindowMode, &str, &str); 2] = [
    (WindowMode::Expanded, "Expanded", "launcher-window-Expanded"),
    (WindowMode::Compact, "Compact", "launcher-window-Compact"),
];
pub(crate) const PINNED_NAME: &str = "Pinned items";
pub(crate) const PINNED_LAYOUTS: [(PinnedLayout, &str, &str); 2] = [
    (
        PinnedLayout::Horizontal,
        "Horizontal",
        "launcher-pinned-Horizontal",
    ),
    (
        PinnedLayout::Vertical,
        "Vertical",
        "launcher-pinned-Vertical",
    ),
];
/// The switch that shows the pins under the compact window's search
/// field: its name, and its id and selector.
pub(crate) const COMPACT_PINNED_NAME: &str = "Show pinned in compact window mode";
pub(crate) const COMPACT_PINNED_DEBUG: &str = "launcher-compact-pinned";

/// What the page is, in one line: its sidebar entry's description in
/// the search.
pub(crate) const ABOUT: &str = "Where the launcher opens and what it shows";

/// The opening monitor's select: its name, its description and the prefix
/// of its debug selectors.
pub(crate) const MONITOR_NAME: &str = "Display";
pub(crate) const MONITOR_DESCRIPTION: &str = "Where the launcher opens";
pub(crate) const MONITOR_DEBUG: &str = "launcher-monitor";

/// The Launcher page, registered after General in the window's page list:
/// the page of the launcher window itself.
pub(crate) fn page() -> Page {
    Page {
        title: "Launcher",
        about: ABOUT,
        icon: Glyph::Monitor,
        count: None,
        render,
        search: entries,
        focus,
    }
}

/// The Launcher page's state, held by the window as a field: the
/// opening-monitor select control.
pub(crate) struct State {
    /// The opening monitor's searchable select, the choice control the
    /// page embeds (see [`crate::ui::select`]). Everything the page
    /// shows it comes from live reads — the display layout and the host
    /// settings — and every choice it takes goes through the host
    /// settings, as the rows it replaced did.
    monitor: Entity<Select>,
    /// The reopening choice's select.
    reopening: Entity<Select>,
    /// The search sensitivity choice's select.
    sensitivity: Entity<Select>,
}

impl State {
    /// The page's state: the opening-monitor select, wired to the host
    /// settings and the placement the page itself reads.
    pub(crate) fn new(window: &mut Window, cx: &mut Context<SettingsWindow>) -> State {
        let monitor = cx.new(|cx| {
            Select::new(
                MONITOR_NAME,
                MONITOR_DESCRIPTION,
                MONITOR_DEBUG,
                // The model, read live every render: the choices as the
                // platform answers them, the committed choice as the
                // host settings hold it, and the visuals the window
                // renders by — all three re-read each frame, so a
                // layout or a save that changed underneath the open
                // popup is what the next frame shows.
                Rc::new(|cx: &App| monitor_model(cx)),
                Rc::new(|id: &str, _window: &mut Window, cx: &mut App| {
                    // The commit path the rows it replaced took, unchanged:
                    // the host settings record the choice, write the
                    // record off the window's thread, and report a
                    // failure with the shown choice rolled back.
                    if let Some(monitor) = monitor_of(id) {
                        crate::settings::shared(cx).update(cx, |settings, cx| {
                            settings.set_opening_monitor(monitor, cx);
                        });
                    }
                }),
                window,
                cx,
            )
        });
        let reopening = super::choice_select(
            REOPENING_NAME,
            REOPENING_DEBUG,
            |_| {
                REOPENINGS
                    .iter()
                    .map(|&(_, name, does, id)| crate::ui::select::Choice {
                        subtitle: Some(does.into()),
                        ..super::choice(id, name, None)
                    })
                    .collect()
            },
            |cx| {
                let chosen = crate::settings::shared(cx).read(cx).reopening();
                REOPENINGS
                    .iter()
                    .find(|&&(reopening, ..)| reopening == chosen)
                    .map_or("RestoreView", |&(.., id)| id)
            },
            |id, cx| {
                if let Some(&(reopening, ..)) = REOPENINGS.iter().find(|&&(.., of)| of == id) {
                    crate::settings::shared(cx).update(cx, |settings, cx| {
                        settings.set_reopening(reopening, cx);
                    });
                }
            },
            window,
            cx,
        );
        let sensitivity = super::choice_select(
            SENSITIVITY_NAME,
            SENSITIVITY_DEBUG,
            |_| {
                SENSITIVITIES
                    .iter()
                    .map(|&(_, name, does)| crate::ui::select::Choice {
                        subtitle: Some(does.into()),
                        ..super::choice(name, name, None)
                    })
                    .collect()
            },
            |cx| {
                let chosen = crate::settings::shared(cx).read(cx).search_sensitivity();
                SENSITIVITIES
                    .iter()
                    .find(|&&(sensitivity, ..)| sensitivity == chosen)
                    .map_or("High", |&(_, name, _)| name)
            },
            |name, cx| {
                if let Some(&(sensitivity, ..)) =
                    SENSITIVITIES.iter().find(|&&(_, of, _)| of == name)
                {
                    crate::settings::shared(cx).update(cx, |settings, cx| {
                        settings.set_search_sensitivity(sensitivity, cx);
                    });
                }
            },
            window,
            cx,
        );
        State {
            monitor,
            reopening,
            sensitivity,
        }
    }

    /// The popup's search field, for tests that drive composition the
    /// way a platform input method does.
    #[doc(hidden)]
    pub(crate) fn field(
        &self,
        cx: &App,
    ) -> Entity<gpui_elements::editable_text::EditableTextState> {
        self.monitor.read(cx).query().clone()
    }

    /// Test support: the opening-monitor select's popup presentation as
    /// the last frame drew it — the offset from rest toward the trigger
    /// in px and the opacity; `None` when the last frame drew the popup
    /// settled (at rest while open, absent while closed). Test and debug
    /// builds only.
    #[cfg(any(test, debug_assertions))]
    pub(crate) fn popup_presentation(&self, cx: &App) -> Option<(f32, f32)> {
        self.monitor.read(cx).popup_presentation()
    }
}

/// What the select's model reads: the choices the platform can answer,
/// which of them the host settings hold, and the visuals in effect.
fn monitor_model(cx: &App) -> Model {
    let visuals = crate::settings::visuals(cx);
    let committed = crate::settings::shared(cx).read(cx).opening_monitor();
    let layout = crate::placement::shared(cx).layout();
    Model {
        theme: visuals.theme,
        material: visuals.material,
        choices: monitor_choices(&layout),
        committed: Some(monitor_name(committed).into()),
    }
}

/// The opening-monitor choices the select lists over `layout`, in row
/// order (#99).
pub(crate) fn monitor_choices(layout: &DisplayLayout) -> Vec<Choice> {
    MONITORS
        .iter()
        .map(|&(monitor, name, keywords, _)| Choice {
            // The choice's identity: the preference itself, as the commit
            // path and the saved choice name it.
            id: monitor_name(monitor).into(),
            label: name.into(),
            subtitle: None,
            keywords: keywords.iter().map(|&word| word.into()).collect(),
            // A choice whose answer the system does not give is listed
            // with its reason, not offered: choosing it would pretend a
            // placement that cannot be made.
            unavailable_reason: unsupported(layout, monitor).map(SharedString::from),
        })
        .collect()
}

/// The choice's stable id, the same string the commit path maps back to
/// the preference.
pub(crate) fn monitor_name(monitor: OpeningMonitor) -> &'static str {
    match monitor {
        OpeningMonitor::Primary => "Primary",
        OpeningMonitor::Pointer => "Pointer",
        OpeningMonitor::ActiveWindow => "ActiveWindow",
    }
}

/// The preference a committed choice's id names, if it names one.
fn monitor_of(id: &str) -> Option<OpeningMonitor> {
    MONITORS
        .iter()
        .map(|&(monitor, ..)| monitor)
        .find(|&monitor| monitor_name(monitor) == id)
}

impl SettingsWindow {
    /// Test support: the opening-monitor select's search field, as the
    /// search field and the alias fields are; a platform input method
    /// talks to it while composing text, and tests read what it holds.
    #[doc(hidden)]
    pub fn monitor_select_field(
        &self,
        cx: &App,
    ) -> Entity<gpui_elements::editable_text::EditableTextState> {
        self.launcher_page.field(cx)
    }

    /// Test support: the opening-monitor select's popup presentation as
    /// the last frame drew it — the offset from rest toward the trigger
    /// in px and the opacity; `None` when the last frame drew the popup
    /// settled (at rest while open, absent while closed), which is also
    /// what reduced motion ever reports. Test and debug builds only.
    #[cfg(any(test, debug_assertions))]
    #[doc(hidden)]
    pub fn monitor_select_popup(&self, cx: &App) -> Option<(f32, f32)> {
        self.launcher_page.popup_presentation(cx)
    }
}

/// The settings the page offers the sidebar's search: each choice of the
/// display, reopening and search sensitivity selects, named as the page
/// names it, in the group it sits in, saying why it cannot be used where
/// the system does not answer it — the result stays listed with its
/// reason, as the control does on the page — then the Layout card's three
/// rows. Each select's choices all jump to the one select control that
/// offers them. The reopening, sensitivity and layout choices are no
/// platform integration: they are always usable.
fn entries(_launcher: &Launcher, cx: &App) -> Vec<search::Entry> {
    let placement = crate::placement::shared(cx);
    let layout = placement.layout();
    let unavailable = placement.unavailable();
    let monitors = MONITORS.iter().map(|&(monitor, name, _, _)| {
        search::Entry {
            control: Some("launcher-monitor".into()),
            title: name.into(),
            group: Some(MONITOR_NAME.into()),
            unavailable: match &unavailable {
                // The platform cannot choose the launcher's display at all:
                // every choice says so, as the page does.
                Some(why) => Some(why.clone()),
                None => unsupported(&layout, monitor),
            },
        }
    });
    let reopenings = REOPENINGS.iter().map(|&(_, name, _, _)| search::Entry {
        control: Some(REOPENING_DEBUG.into()),
        title: name.into(),
        group: Some(REOPENING_NAME.into()),
        unavailable: None,
    });
    let sensitivities = SENSITIVITIES.iter().map(|&(_, name, _)| search::Entry {
        control: Some(SENSITIVITY_DEBUG.into()),
        title: name.into(),
        group: Some(SENSITIVITY_NAME.into()),
        unavailable: None,
    });
    let layout = [
        ("launcher-window-mode", WINDOW_MODE_NAME),
        (COMPACT_PINNED_DEBUG, COMPACT_PINNED_NAME),
        ("launcher-pinned", PINNED_NAME),
    ]
    .into_iter()
    .map(|(control, title)| search::Entry {
        control: Some(control.into()),
        title: title.into(),
        group: Some(LAYOUT.into()),
        unavailable: None,
    });
    monitors
        .chain(reopenings)
        .chain(sensitivities)
        .chain(layout)
        .collect()
}

/// The page's keyboard controls are its three selects, the display, the
/// reopening delay and the search sensitivity: a select's trigger takes
/// focus (it is a tab stop, and Enter opens its choices), so a jump to
/// any of its choices focuses it. The Layout card's segments and switch
/// take no keyboard focus (they are chosen with the pointer, as the
/// reference's settings rows are), so a jump to one reveals it and the
/// sidebar keeps the focus: `false`.
fn focus(
    this: &mut SettingsWindow,
    target: &str,
    window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> bool {
    let select = match target {
        "launcher-monitor" => &this.launcher_page.monitor,
        REOPENING_DEBUG => &this.launcher_page.reopening,
        SENSITIVITY_DEBUG => &this.launcher_page.sensitivity,
        _ => return false,
    };
    let trigger = select.read(cx).trigger_focus();
    window.focus(&trigger, cx);
    true
}

/// What the Launcher page shows, as plain values: what [`render`] reads
/// from the host settings and the placement (#99).
pub(crate) struct LauncherView {
    /// Why the platform cannot choose the launcher's display at all, if it
    /// cannot: the page offers no opening-monitor choice then.
    pub(crate) unavailable: Option<String>,
    /// What the launcher would open on now, when that is not the display
    /// the choice names.
    pub(crate) fallback: Option<String>,
    /// The layout choices in effect.
    pub(crate) window_mode: WindowMode,
    /// Whether the compact window shows the pins under its search field.
    pub(crate) compact_pinned: bool,
    pub(crate) pinned_layout: PinnedLayout,
    /// What a save reported, if it failed.
    pub(crate) status: Option<String>,
}

/// Which of the Launcher page's controls an element is, for the caller of
/// [`compose`] that attaches its behavior: a layout choice's segment, or
/// the switch that shows the pins in the compact window.
#[derive(Clone, Copy)]
pub(crate) enum LauncherControl {
    WindowMode(WindowMode),
    Pinned(PinnedLayout),
    CompactPinned,
}

/// The layout section's label.
pub(crate) const LAYOUT: &str = "Layout";

/// Draws the Launcher page: the opening monitor's and the reopening
/// choice's selects, the layout choices, and whatever the host settings
/// and the platform report — an unsupported choice, a fallback, a save
/// that failed.
fn render(
    this: &mut SettingsWindow,
    _window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let settings = crate::settings::shared(cx);
    let (chosen, window_mode, compact_pinned, pinned_layout, status) = {
        let state = settings.read(cx);
        (
            state.opening_monitor(),
            state.window_mode(),
            state.compact_pinned(),
            state.pinned_layout(),
            state.status(),
        )
    };
    let placement = crate::placement::shared(cx);
    let layout = placement.layout();
    let view = LauncherView {
        unavailable: placement.unavailable(),
        fallback: resolve(&layout, chosen).and_then(|resolved| resolved.fallback),
        window_mode,
        compact_pinned,
        pinned_layout,
        status,
    };
    let visuals = crate::settings::visuals(cx);
    let theme = &visuals.theme;
    // The selects' scroll anchors, which the search's reveal scrolls to
    // (see the window's render): the whole control is what a jump to any
    // of its choices reveals. The monitor's select is offered only where
    // the platform can choose the launcher's display.
    let select = view.unavailable.is_none().then(|| {
        let anchor = this.search_anchor("launcher-monitor");
        div()
            .id("launcher-monitor")
            .flex_none()
            .anchor_scroll(Some(anchor))
            .child(this.launcher_page.monitor.clone())
    });
    let reopening = div()
        .id(REOPENING_DEBUG)
        .flex_none()
        .anchor_scroll(Some(this.search_anchor(REOPENING_DEBUG)))
        .child(this.launcher_page.reopening.clone());
    let sensitivity = div()
        .id(SENSITIVITY_DEBUG)
        .flex_none()
        .anchor_scroll(Some(this.search_anchor(SENSITIVITY_DEBUG)))
        .child(this.launcher_page.sensitivity.clone());
    let mode_anchor = this.search_anchor("launcher-window-mode");
    let pinned_anchor = this.search_anchor("launcher-pinned");
    let compact_pinned_anchor = this.search_anchor(COMPACT_PINNED_DEBUG);
    compose(
        &view,
        (select, Some(reopening), Some(sensitivity)),
        theme,
        |control, element| match control {
            LauncherControl::WindowMode(mode) => element
                .anchor_scroll(Some(mode_anchor.clone()))
                .on_click(cx.listener(move |_, _: &gpui::ClickEvent, _, cx| {
                    crate::settings::shared(cx).update(cx, |settings, cx| {
                        settings.set_window_mode(mode, cx);
                    });
                })),
            LauncherControl::Pinned(layout) => element
                .anchor_scroll(Some(pinned_anchor.clone()))
                .on_click(cx.listener(move |_, _: &gpui::ClickEvent, _, cx| {
                    crate::settings::shared(cx).update(cx, |settings, cx| {
                        settings.set_pinned_layout(layout, cx);
                    });
                })),
            LauncherControl::CompactPinned => element
                .anchor_scroll(Some(compact_pinned_anchor.clone()))
                .on_click(cx.listener(|_, _: &gpui::ClickEvent, _, cx| {
                    let settings = crate::settings::shared(cx);
                    let on = settings.read(cx).compact_pinned();
                    settings.update(cx, |settings, cx| {
                        settings.set_compact_pinned(!on, cx);
                    });
                })),
        },
    )
    .into_any_element()
}

/// One segmented choice at a settings row's end, named `group`: `choices`
/// (the preference, its name, its selector), `chosen` marked, each
/// attached as the `control` it is.
fn segments<T: Copy + PartialEq>(
    group: &'static str,
    choices: &[(T, &'static str, &'static str)],
    chosen: T,
    theme: &Theme,
    control: impl Fn(T) -> LauncherControl,
    attach: &impl Fn(LauncherControl, Stateful<Div>) -> Stateful<Div>,
) -> Stateful<Div> {
    let segments = choices.iter().map(|&(choice, name, selector)| {
        let on = choice == chosen;
        let segment = controls::segment(name, on, true, theme)
            .id(selector)
            .debug_selector(move || selector.into())
            .role(Role::RadioButton)
            .aria_label(name)
            .aria_toggled(if on { Toggled::True } else { Toggled::False });
        attach(control(choice), segment)
    });
    controls::row_segment_track(theme)
        .id(group)
        .role(Role::RadioGroup)
        .children(segments)
}

/// The Launcher page's composition: a card of the Display row — `selects.0`, the opening
/// monitor's searchable select (see [`crate::ui::select`]), with the
/// fallback it explains under its name, or where the platform cannot
/// choose the display at all, why — the Pop to root search row
/// (`selects.1`) and the Search sensitivity row (`selects.2`); then the
/// Layout card's window mode segments, the switch that shows the pins in
/// the compact window, and the pinned items segments. A failed save's
/// status sits above the cards. `attach` adds each control's behavior;
/// the composition gives each its identity, its accessibility and its look.
/// The page's three selects as [`compose`] arranges them: the opening
/// monitor's, the reopening choice's and the search sensitivity's.
pub(crate) type Selects = (
    Option<Stateful<Div>>,
    Option<Stateful<Div>>,
    Option<Stateful<Div>>,
);

pub(crate) fn compose(
    view: &LauncherView,
    selects: Selects,
    theme: &Theme,
    attach: impl Fn(LauncherControl, Stateful<Div>) -> Stateful<Div>,
) -> Stateful<Div> {
    let (select, reopening, sensitivity) = selects;
    let reopening = controls::setting_row(REOPENING_NAME, Vec::new(), theme)
        .debug_selector(|| "launcher-reopening-field".into())
        .children(reopening);
    let sensitivity = controls::setting_row(SENSITIVITY_NAME, Vec::new(), theme)
        .debug_selector(|| "launcher-sensitivity-field".into())
        .children(sensitivity);
    // The opening display, with the choice's own honesty: what the
    // launcher opens on now, when that is not the display the choice
    // names; or, where the platform cannot choose the display at all, why.
    let mut lines = Vec::new();
    lines.extend(view.unavailable.as_ref().map(|why| {
        note(
            "launcher-unavailable",
            format!("Not available: {why}"),
            theme.warning,
            theme,
        )
        .into_any_element()
    }));
    lines.extend(view.fallback.as_ref().map(|reason| {
        note("launcher-fallback", reason.clone(), theme.warning, theme).into_any_element()
    }));
    let display = controls::setting_row(MONITOR_NAME, lines, theme)
        .debug_selector(|| "launcher-monitor-field".into())
        .children(select);
    let card = controls::card(
        [
            display.into_any_element(),
            reopening.into_any_element(),
            sensitivity.into_any_element(),
        ],
        theme,
    );
    let mode = controls::setting_row(WINDOW_MODE_NAME, Vec::new(), theme)
        .debug_selector(|| "launcher-window-mode-field".into())
        .child(segments(
            "launcher-window-mode",
            &WINDOW_MODES,
            view.window_mode,
            theme,
            LauncherControl::WindowMode,
            &attach,
        ));
    let compact_pinned = super::general::switch_row(
        super::general::SwitchRow {
            id: COMPACT_PINNED_DEBUG,
            selector: COMPACT_PINNED_DEBUG,
            title: COMPACT_PINNED_NAME,
            on: view.compact_pinned,
            offered: true,
            lines: Vec::new(),
        },
        theme,
        |switch| attach(LauncherControl::CompactPinned, switch),
    );
    let pinned = controls::setting_row(PINNED_NAME, Vec::new(), theme)
        .debug_selector(|| "launcher-pinned-field".into())
        .child(segments(
            "launcher-pinned",
            &PINNED_LAYOUTS,
            view.pinned_layout,
            theme,
            LauncherControl::Pinned,
            &attach,
        ));
    let layout = controls::card(
        [
            mode.into_any_element(),
            compact_pinned.into_any_element(),
            pinned.into_any_element(),
        ],
        theme,
    );
    let page = controls::page(theme)
        .children(view.status.as_ref().map(|status| {
            note("launcher-status", status.clone(), theme.danger, theme)
                .px(theme.geometry.settings.section_label_inset)
        }))
        .child(controls::section(None, card, theme))
        .child(controls::section(Some(LAYOUT.into()), layout, theme));
    div()
        .id("launcher")
        .debug_selector(|| "launcher".into())
        .child(page)
}

/// Why `monitor`'s choice cannot be answered on this platform, if it
/// cannot: the layout's own `None`. The primary display is always
/// offered.
fn unsupported(layout: &DisplayLayout, monitor: OpeningMonitor) -> Option<String> {
    match monitor {
        OpeningMonitor::Primary => None,
        OpeningMonitor::Pointer if layout.pointer.is_none() => {
            Some("This system doesn't report where the mouse is".into())
        }
        OpeningMonitor::ActiveWindow if layout.active.is_none() => {
            Some("This system doesn't report the active window".into())
        }
        _ => None,
    }
}
