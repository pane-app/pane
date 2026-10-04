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
//! control needs exercised. The reopening choices keep the direct
//! choice rows — two fixed rows a user scans faster than searches, the
//! control's own rule for when a searchable select is warranted.
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
    AnyElement, App, Context, Div, Entity, Hsla, Role, ScrollAnchor, SharedString, Stateful,
    Toggled, Window, div, prelude::*, px,
};
use pane_core::placement::{DisplayLayout, resolve};
use pane_core::{Launcher, OpeningMonitor, Reopening};

use super::{Page, SettingsWindow, search};
use crate::ui::icon::{Glyph, IconTone};
use crate::ui::select::{Choice, Model, Select};
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
const REOPENINGS: [(Reopening, &str, &str, &str); 2] = [
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

/// The Launcher page, registered after General in the window's page list:
/// the page of the launcher window itself.
pub(crate) fn page() -> Page {
    Page {
        title: "Launcher",
        about: "The display the launcher opens on, and what reopening shows",
        icon: (IconTone::Command, Glyph::Monitor),
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
                "Display",
                "The display the launcher opens on",
                "launcher-monitor",
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
        choices: MONITORS
            .iter()
            .map(|&(monitor, name, subtitle, keywords, _)| Choice {
                // The choice's identity: the preference itself, as the
                // commit path and the saved choice name it.
                id: monitor_name(monitor).into(),
                label: name.into(),
                subtitle: Some(subtitle.into()),
                keywords: keywords.iter().map(|&word| word.into()).collect(),
                // A choice whose answer the system does not give is
                // listed with its reason, not offered: choosing it
                // would pretend a placement that cannot be made.
                unavailable_reason: unsupported(&layout, monitor).map(SharedString::from),
            })
            .collect(),
        committed: Some(monitor_name(committed).into()),
    }
}

/// The choice's stable id, the same string the commit path maps back to
/// the preference.
fn monitor_name(monitor: OpeningMonitor) -> &'static str {
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

/// Draws the Launcher page: the opening-monitor group, the reopening
/// group, the dismissal note, and whatever the host settings and the
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
    let unavailable = placement.unavailable();
    // Whether the opening-monitor choices are offered at all: a platform
    // that cannot choose the launcher's display explains that instead.
    let offered = unavailable.is_none();
    let layout = placement.layout();
    let visuals = crate::settings::visuals(cx);
    let theme = &visuals.theme;
    let typography = &theme.typography;
    // The select's scroll anchor, which the search's reveal scrolls to
    // (see the window's render): the whole control is what a jump to
    // any of the monitor's choices reveals.
    let anchor = this.search_anchor("launcher-monitor");

    let page = div()
        .id("launcher")
        .debug_selector(|| "launcher".into())
        .flex()
        .flex_col()
        .gap(px(4.))
        .child(
            div()
                .id("launcher-title")
                .debug_selector(|| "launcher-title".into())
                .pb(px(8.))
                .text_size(typography.search_size)
                .font_weight(typography.medium)
                .text_color(theme.text_title)
                .child("Launcher"),
        )
        // A platform that cannot choose the launcher's display at all:
        // the reason, and no choices offered below.
        .when_some(
            unavailable.map(|why| {
                note(
                    "launcher-unavailable",
                    &format!("Not available: {why}"),
                    theme.warning,
                    theme,
                )
            }),
            |page, note| page.child(note),
        )
        .when(offered, |page| {
            page.child(group(
                "Opening monitor",
                vec![monitor_select(this, anchor)],
                theme,
            ))
            // The choice's own honesty: what the launcher would open on
            // now, when that is not the display the choice names.
            .when_some(fallback_note(&layout, chosen, theme), |page, note| {
                page.child(note)
            })
        })
        .child(group(
            "Reopening",
            REOPENINGS
                .iter()
                .map(|&(preference, name, subtitle, selector)| {
                    let anchor = this.search_anchor(selector);
                    choice(
                        selector,
                        name,
                        subtitle,
                        preference == reopening,
                        true,
                        None,
                        anchor,
                        theme,
                        cx.listener(move |_, _, _, cx| {
                            crate::settings::shared(cx).update(cx, |settings, cx| {
                                settings.set_reopening(preference, cx);
                            });
                        }),
                    )
                })
                .collect(),
            theme,
        ))
        // Dismissal is not a choice: the specification's Escape contract
        // stands as it is, and this says what it is.
        .child(note(
            "launcher-dismissal",
            "Escape still backs out of what is open — a composition, a menu, an open screen — \
             before it clears the query, and only then hides the launcher. Hiding never quits \
             Pane: the Settings window stays open, and the next opening of the launcher \
             reuses the same live window.",
            theme.text_muted,
            theme,
        ))
        .when_some(status, |page, status| {
            page.child(note("launcher-status", &status, theme.danger, theme))
        });
    page.into_any_element()
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

/// What the page says when the choice and the layout resolve to a
/// fallback: the resolution's own reason, as the opening that fell back
/// is placed on an available display and says so.
fn fallback_note(
    layout: &DisplayLayout,
    choice: OpeningMonitor,
    theme: &Theme,
) -> Option<Stateful<Div>> {
    resolve(layout, choice)
        .and_then(|resolved| resolved.fallback)
        .map(|reason| note("launcher-fallback", &reason, theme.warning, theme))
}

/// One choice group: its label (the section label style) and its rows,
/// with the radio group's semantics.
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

/// The opening-monitor select, embedded as the group's one control: a
/// plain wrapper that carries the scroll anchor the search's reveal
/// scrolls to — the control itself owns its trigger and its popup (see
/// [`crate::ui::select`]). Everything it shows is read live through its
/// model and every choice it takes goes through the host settings, as
/// the radio rows it replaced did; the page around it keeps only its
/// own honesty notes.
fn monitor_select(this: &mut SettingsWindow, anchor: ScrollAnchor) -> Stateful<Div> {
    div()
        .id("launcher-monitor")
        .w_full()
        .anchor_scroll(Some(anchor))
        .child(this.launcher_page.monitor.clone())
}

/// One choice row: the reference's row chrome carrying a radio's marks
/// and semantics. `chosen` is whether the row's choice is the one in
/// effect; `offered` is whether choosing it does anything (a choice whose
/// answer this system does not give is shown with its `reason`, not
/// offered); `anchor` is the scroll anchor the search's reveal scrolls
/// to; `on_click` reports the choice to the host settings, which records
/// and saves it.
#[allow(clippy::too_many_arguments)]
fn choice(
    selector: &'static str,
    name: &'static str,
    subtitle: &'static str,
    chosen: bool,
    offered: bool,
    reason: Option<String>,
    anchor: ScrollAnchor,
    theme: &Theme,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let typography = &theme.typography;
    let geometry = &theme.geometry;
    // What the row says under its name: the reason a choice cannot be
    // answered here, where it cannot, else the choice's own subtitle.
    let description = reason.unwrap_or_else(|| subtitle.to_owned());
    let row = div()
        .flex()
        .items_center()
        .gap(geometry.row_gap)
        .min_h(geometry.row_min_height)
        .px(geometry.row_padding_x)
        .rounded(geometry.row_radius)
        .when(offered, |row| row.cursor_pointer())
        .when(!offered, |row| row.opacity(0.5).cursor_default())
        .when(chosen, |row| {
            row.bg(theme.row_selected).shadow(vec![
                gpui::BoxShadow::new(px(0.), px(0.), theme.row_selected_border)
                    .spread_radius(px(1.))
                    .inset(),
            ])
        })
        .child(
            // The radio's mark, as the extension form's choices render it.
            div()
                .flex_none()
                .w(px(18.))
                .text_size(typography.row_title_size)
                .text_color(theme.text_title)
                .child(if chosen { "◉" } else { "○" }),
        )
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
                        .child(name),
                )
                .child(
                    div()
                        .text_size(typography.row_subtitle_size)
                        .text_color(theme.text_muted)
                        .child(description.clone()),
                ),
        );
    row.id(name)
        // The pointer feedback, on the named row: the hover wash fades
        // over the shared pointer span, and the press takes the selected
        // wash — the wash the row keeps once it is chosen, so the press
        // hands over to the choice without a jump. The fade attaches only
        // while the row is unchosen, so the chosen wash both arrives and
        // leaves at once, and only the pointer's own wash fades.
        .when(offered && !chosen, |row| {
            row.hover(|row| row.bg(theme.row_hover))
                .active(|row| row.bg(theme.row_selected))
                .transitions(|fades| fades.bg(crate::ui::motion::pointer_fade()))
        })
        .debug_selector(move || selector.into())
        .anchor_scroll(Some(anchor))
        .role(Role::RadioButton)
        .aria_label(name)
        // The reason a choice cannot be used here is read as the row's
        // description, as root search's rows read their subtitles.
        .aria_description(description)
        .aria_toggled(if chosen {
            Toggled::True
        } else {
            Toggled::False
        })
        .when(!offered, |row| row.aria_disabled(true))
        .when(offered, |row| row.on_click(on_click))
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
