//! The Launcher page: the choices that govern the launcher window — which
//! display it opens on, and what reopening it starts from.
//!
//! Every value it shows and every choice it takes goes through the host
//! settings ([`crate::settings`]), so the record's own rules — atomic
//! writes, an unreadable record never replaced, a failed save reported
//! with the shown choice rolled back — are the ones these choices live
//! by. Neither choice changes what the windows render, so choosing here
//! repaints nothing: the opening display is resolved against the display
//! layout every time the launcher opens (through [`crate::placement`],
//! the platform seam this page also reads to explain the choices), and
//! what reopening shows is applied when the launcher is next opened.
//!
//! The opening monitor is the Pane-styled searchable select
//! ([`crate::ui::select`]): the first real consumer of the shared
//! control, whose choices are few but whose search and keywords the
//! control needs exercised. The reopening choices are a segmented choice
//! (#99, the Settings board's family) — two fixed choices a user scans
//! faster than searches, the control's own rule for when a searchable
//! select is warranted — with the chosen one's description under it.
//!
//! What the page explains, as the General page does for its hotkey: the
//! choices the platform cannot answer — the pointer's display where the
//! system does not tell Pane where the pointer is, the active window's
//! display where it does not tell Pane which window is active — are
//! shown with their reason and not offered, rather than pretending they
//! succeeded; a platform that cannot choose the launcher's display at
//! all (Wayland) explains that instead; and a choice whose display is
//! disconnected falls back to the primary display, which the page says.
//! The dismissal behavior is the specification's Escape contract, not a
//! choice: backing out of what is open — a composition, a menu, a screen
//! — comes first, and hiding the launcher never quits Pane.

use std::rc::Rc;

use gpui::{
    AnyElement, App, Context, Div, Entity, Role, ScrollAnchor, SharedString, Stateful, Toggled,
    Window, div, prelude::*,
};
use pane_core::placement::{DisplayLayout, resolve};
use pane_core::{Launcher, OpeningMonitor, Reopening};

use super::{Page, SettingsWindow, search};
use crate::ui::controls::{self, status_note as note};
use crate::ui::icon::Glyph;
use crate::ui::select::{Choice, Model, Select};
use crate::ui::settings_shell;
use crate::ui::theme::Theme;

/// The opening-monitor choices the page offers, in row order: the
/// preference, the row's name, subtitle, declared search keywords and
/// test selector. The subtitle of the default names it as the
/// provisional default, so the page does not silently turn the
/// specification's proposal into a confirmed product decision.
const MONITORS: [(OpeningMonitor, &str, &str, &[&str], &str); 3] = [
    (
        OpeningMonitor::Primary,
        "Primary display",
        "The system's main display, the provisional default",
        &["main"],
        "launcher-monitor-Primary",
    ),
    (
        OpeningMonitor::Pointer,
        "Pointer's display",
        "The display the pointer is on when the launcher opens",
        &["mouse", "cursor"],
        "launcher-monitor-Pointer",
    ),
    (
        OpeningMonitor::ActiveWindow,
        "Active window's display",
        "The display of the window you are working in",
        &["focused", "foreground"],
        "launcher-monitor-ActiveWindow",
    ),
];

/// The reopening choices the page offers, in row order: the preference,
/// the row's name and subtitle, and its test selector.
pub(crate) const REOPENINGS: [(Reopening, &str, &str, &str); 2] = [
    (
        Reopening::RestoreView,
        "Restore the current view",
        "Show what the launcher was left on, when it is still valid; the provisional default",
        "launcher-reopening-RestoreView",
    ),
    (
        Reopening::RootSearch,
        "Start at root search",
        "Begin from root search with an empty query, whatever was left",
        "launcher-reopening-RootSearch",
    ),
];

/// The page's heading.
pub(crate) const TITLE: &str = "Launcher";

/// What the page is, in one line: its sidebar entry's description in
/// the search, and its heading's subtitle.
pub(crate) const ABOUT: &str = "The display the launcher opens on, and what reopening shows";

/// The opening monitor's select: its name, its description and the prefix
/// of its debug selectors.
pub(crate) const MONITOR_NAME: &str = "Display";
pub(crate) const MONITOR_DESCRIPTION: &str = "The display the launcher opens on";
pub(crate) const MONITOR_DEBUG: &str = "launcher-monitor";

/// The dismissal note: what Escape does, which is no choice.
pub(crate) const DISMISSAL: &str = "Escape still backs out of what is open — a composition, a \
                                     menu, an open screen — before it clears the query, and \
                                     only then hides the launcher. Hiding never quits Pane: the \
                                     Settings window stays open, and the next opening of the \
                                     launcher reuses the same live window.";

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
        State { monitor }
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
/// order — which the visual workbench's fixture lists too (#99).
pub(crate) fn monitor_choices(layout: &DisplayLayout) -> Vec<Choice> {
    MONITORS
        .iter()
        .map(|&(monitor, name, subtitle, keywords, _)| Choice {
            // The choice's identity: the preference itself, as the commit
            // path and the saved choice name it.
            id: monitor_name(monitor).into(),
            label: name.into(),
            subtitle: Some(subtitle.into()),
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

/// The settings the page offers the sidebar's search: each choice of both
/// groups, named as the page names it, in the group it sits in, saying
/// why it cannot be used where the system does not answer it — the result
/// stays listed with its reason, as the control does on the page. The
/// opening monitor's choices all jump to the one select control that
/// offers them; the reopening choices to their rows. The reopening
/// choices are no platform integration: they are always usable.
fn entries(_launcher: &Launcher, cx: &App) -> Vec<search::Entry> {
    let placement = crate::placement::shared(cx);
    let layout = placement.layout();
    let unavailable = placement.unavailable();
    let monitors = MONITORS.iter().map(|&(monitor, name, _, _, _)| {
        search::Entry {
            control: Some("launcher-monitor".into()),
            title: name.into(),
            group: Some("Opening monitor".into()),
            unavailable: match &unavailable {
                // The platform cannot choose the launcher's display at all:
                // every choice says so, as the page does.
                Some(why) => Some(why.clone()),
                None => unsupported(&layout, monitor),
            },
        }
    });
    let reopenings = REOPENINGS
        .iter()
        .map(|&(_, name, _, selector)| search::Entry {
            control: Some(selector.into()),
            title: name.into(),
            group: Some("Reopening".into()),
            unavailable: None,
        });
    monitors.chain(reopenings).collect()
}

/// The page's one keyboard control is the opening-monitor select: its
/// trigger takes focus (it is a tab stop, and Enter opens its choices),
/// so a jump to any of the monitor's choices focuses it. The reopening
/// rows take no keyboard focus (they are chosen with the pointer, as
/// the reference's settings rows are), so a jump to one reveals it and
/// the sidebar keeps the focus: `false`.
fn focus(
    this: &mut SettingsWindow,
    target: &str,
    window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> bool {
    if target != "launcher-monitor" {
        return false;
    }
    let trigger = this.launcher_page.monitor.read(cx).trigger_focus();
    window.focus(&trigger, cx);
    true
}

/// What the Launcher page shows, as plain values: what [`render`] reads
/// from the host settings and the placement, and what the visual
/// workbench's fixture supplies to draw the same page (#99).
pub(crate) struct LauncherView {
    /// Why the platform cannot choose the launcher's display at all, if it
    /// cannot: the page offers no opening-monitor choice then.
    pub(crate) unavailable: Option<String>,
    /// What the launcher would open on now, when that is not the display
    /// the choice names.
    pub(crate) fallback: Option<String>,
    /// The reopening choice in effect.
    pub(crate) reopening: Reopening,
    /// What a save reported, if it failed.
    pub(crate) status: Option<String>,
}

/// Which of the Launcher page's controls an element is, for the caller of
/// [`compose`] that attaches its behavior: a reopening choice's segment.
#[derive(Clone, Copy)]
pub(crate) enum LauncherControl {
    Reopening(Reopening),
}

/// Draws the Launcher page: the opening monitor's select, the reopening
/// choice, the dismissal note, and whatever the host settings and the
/// platform report — an unsupported choice, a fallback, a save that
/// failed.
fn render(
    this: &mut SettingsWindow,
    _window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let settings = crate::settings::shared(cx);
    let (chosen, reopening, status) = {
        let state = settings.read(cx);
        (state.opening_monitor(), state.reopening(), state.status())
    };
    let placement = crate::placement::shared(cx);
    let layout = placement.layout();
    let view = LauncherView {
        unavailable: placement.unavailable(),
        fallback: resolve(&layout, chosen).and_then(|resolved| resolved.fallback),
        reopening,
        status,
    };
    let visuals = crate::settings::visuals(cx);
    let theme = &visuals.theme;
    // The select's scroll anchor, which the search's reveal scrolls to
    // (see the window's render): the whole control is what a jump to any
    // of the monitor's choices reveals. The select is offered only where
    // the platform can choose the launcher's display.
    let select = view.unavailable.is_none().then(|| {
        let anchor = this.search_anchor("launcher-monitor");
        div()
            .id("launcher-monitor")
            .w_full()
            .anchor_scroll(Some(anchor))
            .child(this.launcher_page.monitor.clone())
    });
    let anchors: Vec<ScrollAnchor> = REOPENINGS
        .iter()
        .map(|&(_, _, _, selector)| this.search_anchor(selector))
        .collect();
    compose(&view, select, theme, |control, element| match control {
        LauncherControl::Reopening(preference) => {
            let index = REOPENINGS
                .iter()
                .position(|&(choice, ..)| choice == preference)
                .unwrap_or_default();
            element
                .anchor_scroll(anchors.get(index).cloned())
                .on_click(cx.listener(move |_, _: &gpui::ClickEvent, _, cx| {
                    crate::settings::shared(cx).update(cx, |settings, cx| {
                        settings.set_reopening(preference, cx);
                    });
                }))
        }
    })
    .into_any_element()
}

/// The Launcher page's composition, which the visual workbench's fixture
/// draws too: the heading block, then in the page's column the platform's
/// reason where it cannot choose the display; else `select` (the opening
/// monitor's searchable select, a Settings field of its own, see
/// [`crate::ui::select`]) with the fallback it explains; the reopening
/// choice's field group — a segmented choice, the chosen one's
/// description under it; the dismissal note; and a failed save's status.
/// `attach` adds each reopening segment's behavior; the composition gives
/// each its identity, its accessibility and its look.
pub(crate) fn compose(
    view: &LauncherView,
    select: Option<Stateful<Div>>,
    theme: &Theme,
    attach: impl Fn(LauncherControl, Stateful<Div>) -> Stateful<Div>,
) -> Stateful<Div> {
    let segments = REOPENINGS
        .iter()
        .map(|&(preference, name, subtitle, selector)| {
            let chosen = preference == view.reopening;
            let segment = controls::segment(name, chosen, true, theme)
                .id(name)
                .debug_selector(move || selector.into())
                .role(Role::RadioButton)
                .aria_label(name)
                // What the choice does is read as its description, as a
                // field's description is.
                .aria_description(subtitle)
                .aria_toggled(if chosen {
                    Toggled::True
                } else {
                    Toggled::False
                });
            attach(LauncherControl::Reopening(preference), segment)
        });
    let description = REOPENINGS
        .iter()
        .find(|&&(preference, ..)| preference == view.reopening)
        .map_or("", |&(_, _, subtitle, _)| subtitle);
    let reopening = controls::field(theme)
        .debug_selector(|| "launcher-reopening-field".into())
        .child(controls::field_label("Reopening", theme))
        .child(
            controls::segment_track(theme)
                .id("launcher-reopening")
                .debug_selector(|| "launcher-reopening-track".into())
                .role(Role::RadioGroup)
                .aria_label("Reopening")
                .children(segments),
        )
        .child(controls::field_description(
            description,
            theme.text_muted,
            theme,
        ));
    let column =
        controls::column(theme)
            .child(
                settings_shell::page_header(TITLE, Some(ABOUT.into()), theme)
                    .id("launcher-title")
                    .debug_selector(|| "launcher-title".into()),
            )
            // A platform that cannot choose the launcher's display at all:
            // the reason, and no choices offered below.
            .children(view.unavailable.as_ref().map(|why| {
                note(
                    "launcher-unavailable",
                    format!("Not available: {why}"),
                    theme.warning,
                    theme,
                )
            }))
            // The opening monitor, with the choice's own honesty: what the
            // launcher would open on now, when that is not the display the
            // choice names.
            .children(select.map(|select| {
                controls::field(theme)
                    .debug_selector(|| "launcher-monitor-field".into())
                    .child(select)
                    .children(view.fallback.as_ref().map(|reason| {
                        note("launcher-fallback", reason.clone(), theme.warning, theme)
                    }))
            }))
            .child(reopening)
            // Dismissal is not a choice: the specification's Escape contract
            // stands as it is, and this says what it is.
            .child(note(
                "launcher-dismissal",
                DISMISSAL,
                theme.text_muted,
                theme,
            ))
            .children(
                view.status
                    .as_ref()
                    .map(|status| note("launcher-status", status.clone(), theme.danger, theme)),
            );
    div()
        .id("launcher")
        .debug_selector(|| "launcher".into())
        .child(column)
}

/// Why `monitor`'s choice cannot be answered on this platform, if it
/// cannot: the layout's own `None`. The primary display is always
/// offered.
fn unsupported(layout: &DisplayLayout, monitor: OpeningMonitor) -> Option<String> {
    match monitor {
        OpeningMonitor::Primary => None,
        OpeningMonitor::Pointer if layout.pointer.is_none() => Some(
            "This system does not tell Pane where the pointer is outside its own windows, so \
             the launcher opens on the primary display instead"
                .into(),
        ),
        OpeningMonitor::ActiveWindow if layout.active.is_none() => Some(
            "This system does not tell Pane which window is active, so the launcher opens on \
             the primary display instead"
                .into(),
        ),
        _ => None,
    }
}
