//! The Extensions page: the launcher's own extension management, hosted in
//! Settings.
//!
//! The page manages nothing itself; it reaches the launcher's flow. Its
//! list is the extension list "Manage extensions…" shows — the same rows
//! and lines — read through [`pane_core::Launcher::extension_list`] while
//! the launcher is on another screen, drawn live from the launcher's view
//! once the flow is entered; clicking a row enters the flow and activates
//! it exactly as the launcher window's Enter does
//! ([`pane_core::Launcher::manage_extensions`],
//! [`pane_core::Launcher::select`],
//! [`pane_core::Launcher::activate_selected`]). The confirmations that flow
//! asks for — disabling or uninstalling what other extensions require, the
//! saved-data choice, deleting retained data — are the launcher's own
//! screens, so they show here unchanged, and every operation runs through
//! the same code with the same records.
//!
//! Two kinds of rows belong to the launcher *window* rather than the shared
//! screen: a package's commands (an extension's settings are a command it
//! owns, opened in the launcher — no form is invented here) and the
//! launcher's install rows, whose folder picker and forms live in that
//! window. Clicking those summons and focuses the launcher window at that
//! row, through [`crate::app::LauncherWindow::activate_root_result`].
//!
//! Synchronization is by reading, not copying: every frame re-reads the
//! launcher, and wherever the launcher changes — an operation's reply, a
//! background update, build or runtime restart the changes channel
//! reports — the windows showing it redraw (see
//! [`crate::app::LauncherWindow::sync_screen`]). After any operation,
//! including a failed one, the page shows what the launcher holds.

use gpui::{
    AnyElement, App, Context, Div, Role, SharedString, Stateful, Window, div, prelude::*, px,
};
use pane_core::{Launcher, Screen, Status};

use super::{Page, SettingsWindow, search};
use crate::app::{LauncherWindow, launcher_changed_outside, row_icon};
use crate::ui::icon::{Glyph, IconTone};
use crate::ui::result_row::{RowContent, result_row};
use crate::ui::theme::Theme;

/// The Extensions page, first of the sections this milestone ships: the
/// spec's order names Extensions sixth of seven and About last, and only
/// About exists beside it.
pub(crate) fn page() -> Page {
    Page {
        title: "Extensions",
        about: "Install, enable, disable, update and remove extensions",
        // The blocks tile, as the launcher's own Manage extensions row.
        icon: (IconTone::Command, Glyph::Blocks),
        render,
        search: entries,
        focus,
    }
}

/// The settings the page offers the sidebar's search: the extension list
/// — the same rows "Manage extensions…" shows, read live, so a package
/// installed, disabled or removed is in or out of the search with it —
/// and the launcher's install rows, where the page offers them. These
/// are Pane's own management rows, not extension data: the search indexes
/// what the page actually shows, with each row's own honesty about why
/// it cannot be used here.
fn entries(launcher: &Launcher, _cx: &App) -> Vec<search::Entry> {
    let mut entries: Vec<search::Entry> = launcher
        .extension_list()
        .rows
        .into_iter()
        .map(|row| search::Entry {
            control: Some(row.id),
            title: row.title,
            group: None,
            unavailable: row.unavailable.as_ref().map(|why| why.reason().to_owned()),
        })
        .collect();
    if launcher.installs_packages() {
        entries.extend(
            INSTALL_ROWS
                .into_iter()
                .map(|(id, title, _)| search::Entry {
                    control: Some(id.into()),
                    title: title.into(),
                    // The page's own section label, which the rows sit under.
                    group: Some("Install".into()),
                    unavailable: None,
                }),
        );
    }
    entries
}

/// The page's rows take no keyboard focus (they are chosen with the
/// pointer, as the launcher's own lists are), so a jump reveals the row
/// and the sidebar keeps the focus: `false`.
fn focus(_: &mut SettingsWindow, _: &str, _: &mut Window, _: &mut Context<SettingsWindow>) -> bool {
    false
}

/// The launcher's install rows, as root search lists them: their ids,
/// titles and subtitles, the same values root search builds its
/// `pane.install-*` rows with. The page dispatches to those rows through
/// the launcher window, which owns their pickers and forms.
const INSTALL_ROWS: [(&str, &str, &str); 3] = [
    (
        "pane.install-from-folder",
        "Install extension from folder…",
        "Choose a local extension package to install",
    ),
    (
        "pane.install-from-npm",
        "Install extension from npm…",
        "Download an extension package published to npm",
    ),
    (
        "pane.install-from-git",
        "Install extension from Git…",
        "Fetch an extension package from a Git repository",
    ),
];

/// Whether the launcher's screen is held by the extension-management
/// flow: the list itself, or one of the screens its rows open — a
/// confirmation, pause, build, network or runtime details. While it is,
/// the page draws the launcher's live view, so the flow's confirmations
/// show here; otherwise it reads the list without entering the flow, and
/// the launcher's screen stays wherever the user left it.
fn in_extension_flow(screen: &Screen) -> bool {
    matches!(
        screen,
        Screen::Extensions { .. }
            | Screen::Confirm { .. }
            | Screen::PauseDetails { .. }
            | Screen::BuildDetails { .. }
            | Screen::RuntimeDetails { .. }
            | Screen::NetworkDetails { .. }
    )
}

/// Whether the screen is one of the flow's details screens — pause, build,
/// network or runtime details — which offer no Cancel row of their own (a
/// confirmation's is its own), so the page offers the way out the launcher
/// window's Escape is there.
fn details_screen(screen: &Screen) -> bool {
    matches!(
        screen,
        Screen::PauseDetails { .. }
            | Screen::BuildDetails { .. }
            | Screen::RuntimeDetails { .. }
            | Screen::NetworkDetails { .. }
    )
}

/// Draws the Extensions page: the extension list — read where the launcher
/// has not entered the flow, live where it has — with the flow's title,
/// status and lines of information, then the packages' commands and the
/// launcher's install rows, which open in the launcher window.
fn render(
    this: &mut SettingsWindow,
    _window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let theme = crate::settings::visuals(cx).theme;
    let typography = &theme.typography;
    let live = this.launcher.view();
    let flow = in_extension_flow(&live.screen);
    // The list the page shows: the launcher's own rows, either read
    // without entering the flow or live from the flow the page entered —
    // the confirmation rows among them, answered here.
    let list = if flow {
        live
    } else {
        this.launcher.extension_list()
    };
    let leaving_details = details_screen(&list.screen);
    let title = list.title.clone();
    let details = list.details().to_vec();
    let rows = list.rows.clone();
    // The flow's status — an operation's progress or outcome, an error —
    // shows on the page; read mode has none (the launcher's status belongs
    // to the screen the user left it on).
    let status = if flow && !matches!(list.status, Status::Idle) {
        Some(match list.status.clone() {
            Status::Progress(work) => (SharedString::from(work), theme.warning),
            Status::Result(answer) => (SharedString::from(answer), theme.success),
            Status::Error(message) => (SharedString::from(message), theme.danger),
            Status::Running | Status::Idle => ("Running…".into(), theme.warning),
        })
    } else {
        None
    };
    // The commands of the enabled packages, each opening in the launcher
    // window: an extension's settings are a command it owns (the settings
    // sample's "Greeting" is one), not a form Pane would invent here. A
    // disabled package's commands run nowhere, so none is offered.
    let commands: Vec<(String, String, String)> = this
        .launcher
        .packages()
        .into_iter()
        .filter(|package| package.enabled)
        .flat_map(|package| {
            let package_title = package.title();
            package.commands().into_iter().map(move |command| {
                (
                    command.id,
                    command.title,
                    command.subtitle.unwrap_or_else(|| package_title.clone()),
                )
            })
        })
        .collect();
    let installs = this.launcher.installs_packages();
    // Read before the rows below consume them: whether the page has
    // anything of its own to list.
    let has_commands = !commands.is_empty();
    let empty_list = rows.is_empty() && !has_commands;

    // The page's rows of the launcher's list. The page has no keyboard
    // selection of its rows, so none is drawn as selected. Each row
    // carries the scroll anchor the search's reveal scrolls to, keyed by
    // the launcher's own row id.
    let list_rows: Vec<_> = rows
        .iter()
        .enumerate()
        .map(|(index, row)| {
            let reason = row.unavailable.as_ref().map(|why| why.reason().to_owned());
            // The row's accessible description: its subtitle and, when it
            // cannot run, the reason, together.
            let description = match (&row.subtitle, &reason) {
                (Some(subtitle), Some(reason)) => Some(format!("{subtitle}. {reason}")),
                (subtitle, reason) => subtitle.clone().or_else(|| reason.clone()),
            };
            let id = row.id.clone();
            let anchor = this.search_anchor(&row.id);
            result_row(
                RowContent {
                    title: row.title.clone().into(),
                    subtitle: row.subtitle.clone().map(SharedString::from),
                    unavailable_reason: reason.map(SharedString::from),
                    unavailable_id: ("extension-unavailable", index).into(),
                    selected: false,
                    icon: row_icon(&row.id),
                },
                &theme,
            )
            .id(("extension-row", index))
            // Pressed: the selected wash, one rung above the hover one,
            // fading on the shared pointer span.
            .active(|row| row.bg(theme.row_selected))
            .transitions(|fades| fades.bg(crate::ui::motion::pointer_fade()))
            .debug_selector(|| format!("extension-row-{}", row.title))
            .anchor_scroll(Some(anchor))
            .role(Role::Button)
            .aria_label(row.title.clone())
            // An unavailable row stays listed and clickable; activating it
            // shows the reason, as the launcher's does.
            .when(row.unavailable.is_some(), |row| row.aria_disabled(true))
            .when_some(description, |row, description| {
                row.aria_description(description)
            })
            .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                activate(this, &id, cx);
            }))
        })
        .collect();
    // The command rows: one per command of the enabled packages, opening
    // in the launcher window.
    let command_rows: Vec<_> = commands
        .into_iter()
        .enumerate()
        .map(|(index, (id, title, subtitle))| {
            result_row(
                RowContent {
                    title: title.clone().into(),
                    subtitle: Some(format!("{subtitle} · Opens in Pane's launcher").into()),
                    unavailable_reason: None,
                    unavailable_id: ("extension-command-unavailable", index).into(),
                    selected: false,
                    icon: row_icon(&id),
                },
                &theme,
            )
            .id(("extension-command", index))
            // Pressed: the selected wash, one rung above the hover one,
            // fading on the shared pointer span.
            .active(|row| row.bg(theme.row_selected))
            .transitions(|fades| fades.bg(crate::ui::motion::pointer_fade()))
            .debug_selector(|| format!("extension-command-{title}"))
            .role(Role::Button)
            .aria_label(title)
            .on_click(cx.listener(move |_, _: &gpui::ClickEvent, _, cx| {
                open_in_launcher(&id, cx);
            }))
        })
        .collect();
    // The install rows, as root search lists them, opening in the launcher
    // window where their folder picker and forms live. Each carries the
    // scroll anchor the search's reveal scrolls to.
    let install_rows: Vec<_> = INSTALL_ROWS
        .into_iter()
        .enumerate()
        .map(|(index, (id, title, subtitle))| {
            let anchor = this.search_anchor(id);
            result_row(
                RowContent {
                    title: title.into(),
                    subtitle: Some(subtitle.into()),
                    unavailable_reason: None,
                    unavailable_id: ("extension-install-unavailable", index).into(),
                    selected: false,
                    icon: row_icon(id),
                },
                &theme,
            )
            .id(("extension-install", index))
            // Pressed: the selected wash, one rung above the hover one,
            // fading on the shared pointer span.
            .active(|row| row.bg(theme.row_selected))
            .transitions(|fades| fades.bg(crate::ui::motion::pointer_fade()))
            .debug_selector(move || format!("extension-install-{title}"))
            .anchor_scroll(Some(anchor))
            .role(Role::Button)
            .aria_label(title)
            .on_click(cx.listener(move |_, _: &gpui::ClickEvent, _, cx| {
                open_in_launcher(id, cx);
            }))
        })
        .collect();

    div()
        .id("extensions")
        .debug_selector(|| "extensions".into())
        .flex()
        .flex_col()
        .gap(px(4.))
        .child(
            div()
                .id("extensions-title")
                .debug_selector(|| "extensions-title".into())
                .pb(px(8.))
                .text_size(typography.search_size)
                .font_weight(typography.medium)
                .text_color(theme.text_title)
                .child(title),
        )
        .when_some(status, |page, (text, color)| {
            page.child(
                div()
                    .id("extensions-status")
                    .debug_selector(|| "extensions-status".into())
                    .role(Role::Status)
                    .aria_label(text.clone())
                    .pb(px(4.))
                    .text_size(typography.row_subtitle_size)
                    .text_color(color)
                    .child(text),
            )
        })
        .children(details.iter().enumerate().map(|(index, line)| {
            div()
                .id(("extension-detail", index))
                .debug_selector(move || format!("extension-detail-{line}"))
                .text_size(typography.row_subtitle_size)
                .text_color(theme.text_body)
                .child(line.clone())
        }))
        .when(empty_list, |page| {
            page.child(
                div()
                    .id("extension-empty")
                    .debug_selector(|| "extension-empty".into())
                    .py(px(10.))
                    .text_size(typography.row_subtitle_size)
                    .text_color(theme.text_muted)
                    .child("No extensions are installed."),
            )
        })
        .when(leaving_details, |page| {
            // The way out of a details screen the page entered, as the
            // launcher window's Escape is there:
            // [`pane_core::Launcher::back`].
            page.child(
                result_row(
                    RowContent {
                        title: "Back".into(),
                        subtitle: Some("Return to the extension list".into()),
                        unavailable_reason: None,
                        unavailable_id: "extension-back-unavailable".into(),
                        selected: false,
                        icon: None,
                    },
                    &theme,
                )
                .id("extension-back")
                // Pressed: the selected wash, one rung above the hover
                // one, fading on the shared pointer span.
                .active(|row| row.bg(theme.row_selected))
                .transitions(|fades| fades.bg(crate::ui::motion::pointer_fade()))
                .debug_selector(|| "extension-back".into())
                .role(Role::Button)
                .aria_label("Back")
                .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                    this.launcher.back();
                    launcher_changed_outside(cx);
                    cx.notify();
                })),
            )
        })
        .children(list_rows)
        .when(has_commands, |page| {
            page.child(section("Commands", &theme))
                .children(command_rows)
        })
        .when(installs, |page| {
            page.child(section("Install", &theme))
                .children(install_rows)
        })
        .into_any_element()
}

/// A small muted label above one of the page's own groups of rows.
fn section(label: &'static str, theme: &Theme) -> Stateful<Div> {
    div()
        .id(label)
        .debug_selector(move || format!("extension-section-{label}"))
        .pt(px(12.))
        .pb(px(2.))
        .text_size(theme.typography.row_subtitle_size)
        .font_weight(theme.typography.medium)
        .text_color(theme.text_muted)
        .child(label)
}

/// Activates the extension-list row with `id` through the launcher's own
/// flow, as the launcher window's Enter does. If the launcher is not in
/// the flow, it is entered first — the same screen the "Manage
/// extensions…" root result opens — then the row is selected and
/// activated; the confirmation the row asks for, if it asks, shows on this
/// page, drawn from the launcher's view. The launcher's screen holds the
/// flow, so the launcher window is told to redraw — without taking focus;
/// the flow runs here — both now and when the activation's reply lands,
/// as the launcher window's own `show_until_done` does.
fn activate(this: &mut SettingsWindow, id: &str, cx: &mut Context<SettingsWindow>) {
    let launcher = &this.launcher;
    if !in_extension_flow(&launcher.view().screen) {
        launcher.manage_extensions();
    }
    let Some(index) = launcher.view().rows.iter().position(|row| row.id == id) else {
        // The row left the list between the frame that drew it and this
        // click (a background change): the page redraws with what the
        // launcher holds now, and nothing is activated.
        cx.notify();
        return;
    };
    launcher.select(index);
    let pending = launcher.activate_selected();
    launcher_changed_outside(cx);
    cx.notify();
    cx.spawn(async move |this, cx| {
        pending.await;
        cx.update(launcher_changed_outside);
        this.update(cx, |_, cx| cx.notify()).ok();
    })
    .detach();
}

/// Opens the root result with `id` — a package's command, or one of the
/// launcher's install rows — in the launcher window, summoned and focused:
/// those flows belong to that window, whose forms, folder picker and key
/// capture already live there. The page stays where it is; the launcher
/// window's updates tell it to redraw with what the launcher holds.
fn open_in_launcher(id: &str, cx: &mut Context<SettingsWindow>) {
    for window in cx.windows() {
        let Some(launcher) = window.downcast::<LauncherWindow>() else {
            continue;
        };
        launcher
            .update(cx, |launcher, window, cx| {
                launcher.activate_root_result(id, window, cx);
            })
            .ok();
    }
    cx.notify();
}
