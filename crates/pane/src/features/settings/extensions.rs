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
    AnyElement, App, Context, Div, ElementId, Hsla, Role, ScrollAnchor, SharedString, Stateful,
    Window, div, prelude::*,
};
use pane_core::{Launcher, Screen, Status};

use super::{Page, SettingsWindow, search};
use crate::app::{LauncherWindow, launcher_changed_outside, row_icon};
use crate::ui::controls;
use crate::ui::icon::{Glyph, IconTone, TileSize, tile_at};
use crate::ui::settings_shell;
use crate::ui::theme::Theme;

/// What the page is, in one line: its sidebar entry's description in
/// the search, and its heading's subtitle.
pub(crate) const ABOUT: &str = "Install, enable, disable, update and remove extensions";

/// The page's title: its sidebar entry, and its own heading.
const TITLE: &str = "Extensions";

/// How many extensions are installed: the count the page's sidebar entry
/// shows.
fn installed(launcher: &Launcher) -> usize {
    launcher.packages().len()
}

/// The Extensions page, first of the sections this milestone ships: the
/// spec's order names Extensions sixth of seven and About last, and only
/// About exists beside it.
pub(crate) fn page() -> Page {
    Page {
        title: TITLE,
        about: ABOUT,
        // The blocks glyph, as the launcher's own Manage extensions row.
        icon: Glyph::Blocks,
        // The installed extensions, as the reference counts its plugins.
        count: Some(installed),
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

/// One entry of the page's lists, as plain values: the launcher's row (a
/// package, a management operation, a confirmation's answer), a package's
/// command, or an install source.
pub(crate) struct ExtensionItem {
    /// The launcher's own row id, where the entry is one of its rows.
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) subtitle: Option<String>,
    /// Why the entry cannot be used here, if it cannot.
    pub(crate) reason: Option<String>,
    pub(crate) icon: Option<(IconTone, Glyph)>,
}

/// What the Extensions page shows, as plain values: what [`render`] reads
/// from the launcher, and what the visual workbench's fixture supplies to
/// draw the same page (#99).
pub(crate) struct ExtensionsView {
    /// The list's own heading and the page's subtitle under it (a flow's
    /// other screens, such as a confirmation, take none).
    pub(crate) title: String,
    pub(crate) subtitle: Option<SharedString>,
    /// The flow's status — an operation's progress or outcome, an error —
    /// in its tone.
    pub(crate) status: Option<(SharedString, Hsla)>,
    /// The flow's lines of information (a confirmation's, a details
    /// screen's).
    pub(crate) details: Vec<String>,
    /// Whether nothing at all is listed.
    pub(crate) empty: bool,
    /// Whether a details screen's way back is offered.
    pub(crate) back: bool,
    pub(crate) rows: Vec<ExtensionItem>,
    pub(crate) commands: Vec<ExtensionItem>,
    pub(crate) installs: Vec<ExtensionItem>,
}

/// Which of the Extensions page's controls an element is, for the caller
/// of [`compose`] that attaches its behavior.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ExtensionsControl {
    /// The launcher's row at this index.
    Row(usize),
    /// The package command at this index.
    Command(usize),
    /// The install source at this index.
    Install(usize),
    /// A details screen's way back.
    Back,
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
    // The page's subtitle under the list's own heading; a flow's other
    // screens (a confirmation names what it asks about) take none.
    let listing = matches!(list.screen, Screen::Extensions { .. });
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
    let commands: Vec<ExtensionItem> = this
        .launcher
        .packages()
        .into_iter()
        .filter(|package| package.enabled)
        .flat_map(|package| {
            let package_title = package.title();
            package.commands().into_iter().map(move |command| {
                let subtitle = command.subtitle.unwrap_or_else(|| package_title.clone());
                ExtensionItem {
                    icon: row_icon(&command.id),
                    id: command.id,
                    title: command.title,
                    subtitle: Some(format!("{subtitle} · Opens in Pane's launcher")),
                    reason: None,
                }
            })
        })
        .collect();
    let rows: Vec<ExtensionItem> = list
        .rows
        .iter()
        .map(|row| ExtensionItem {
            id: row.id.clone(),
            title: row.title.clone(),
            subtitle: row.subtitle.clone(),
            reason: row.unavailable.as_ref().map(|why| why.reason().to_owned()),
            icon: row_icon(&row.id),
        })
        .collect();
    let installs = if this.launcher.installs_packages() {
        install_items()
    } else {
        Vec::new()
    };
    let view = ExtensionsView {
        title: list.title.clone(),
        subtitle: listing.then(|| ABOUT.into()),
        status,
        details: list.details().to_vec(),
        empty: rows.is_empty() && commands.is_empty(),
        back: details_screen(&list.screen),
        rows,
        commands,
        installs,
    };
    // Each launcher row and install row carries the scroll anchor the
    // search's reveal scrolls to, keyed by the row's own id.
    let row_anchors: Vec<ScrollAnchor> = view
        .rows
        .iter()
        .map(|row| this.search_anchor(&row.id))
        .collect();
    let install_anchors: Vec<ScrollAnchor> = view
        .installs
        .iter()
        .map(|row| this.search_anchor(&row.id))
        .collect();
    let row_ids: Vec<String> = view.rows.iter().map(|row| row.id.clone()).collect();
    let command_ids: Vec<String> = view.commands.iter().map(|row| row.id.clone()).collect();
    let install_ids: Vec<String> = view.installs.iter().map(|row| row.id.clone()).collect();
    compose(&view, &theme, |control, element| match control {
        ExtensionsControl::Row(index) => {
            let id = row_ids[index].clone();
            element
                .anchor_scroll(row_anchors.get(index).cloned())
                .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                    activate(this, &id, cx);
                }))
        }
        ExtensionsControl::Command(index) => {
            let id = command_ids[index].clone();
            element.on_click(cx.listener(move |_, _: &gpui::ClickEvent, _, cx| {
                open_in_launcher(&id, cx);
            }))
        }
        ExtensionsControl::Install(index) => {
            let id = install_ids[index].clone();
            element
                .anchor_scroll(install_anchors.get(index).cloned())
                .on_click(cx.listener(move |_, _: &gpui::ClickEvent, _, cx| {
                    open_in_launcher(&id, cx);
                }))
        }
        // The way out of a details screen the page entered, as the
        // launcher window's Escape is there: [`pane_core::Launcher::back`].
        ExtensionsControl::Back => {
            element.on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                this.launcher.back();
                launcher_changed_outside(cx);
                cx.notify();
            }))
        }
    })
    .into_any_element()
}

/// The launcher's install rows, as the page lists them (see
/// [`INSTALL_ROWS`]).
pub(crate) fn install_items() -> Vec<ExtensionItem> {
    INSTALL_ROWS
        .into_iter()
        .map(|(id, title, subtitle)| ExtensionItem {
            id: id.into(),
            title: title.into(),
            subtitle: Some(subtitle.into()),
            reason: None,
            icon: row_icon(id),
        })
        .collect()
}

/// The Extensions page's composition, which the visual workbench's fixture
/// draws too: the heading block — the list's own title — then, in the
/// page's column, the flow's status and lines of information, a details
/// screen's way back, the launcher's rows as Settings list items (#99),
/// and the packages' commands and the install sources in field groups of
/// their own. `attach` adds each entry's behavior; the composition gives
/// each its identity, its accessibility and its look.
pub(crate) fn compose(
    view: &ExtensionsView,
    theme: &Theme,
    attach: impl Fn(ExtensionsControl, Stateful<Div>) -> Stateful<Div>,
) -> Stateful<Div> {
    let rows = view.rows.iter().enumerate().map(|(index, row)| {
        let item = item(row, ("extension-row", index).into(), theme)
            .debug_selector(|| format!("extension-row-{}", row.title))
            // An unavailable row stays listed and clickable; activating it
            // shows the reason, as the launcher's does.
            .when(row.reason.is_some(), |item| item.aria_disabled(true));
        attach(ExtensionsControl::Row(index), item)
    });
    let commands = view.commands.iter().enumerate().map(|(index, command)| {
        let item = item(command, ("extension-command", index).into(), theme)
            .debug_selector(|| format!("extension-command-{}", command.title));
        attach(ExtensionsControl::Command(index), item)
    });
    let installs = view.installs.iter().enumerate().map(|(index, install)| {
        let item = item(install, ("extension-install", index).into(), theme)
            .debug_selector(|| format!("extension-install-{}", install.title));
        attach(ExtensionsControl::Install(index), item)
    });
    let status = view.status.as_ref().map(|(text, color)| {
        controls::field_description(text.clone(), *color, theme)
            .id("extensions-status")
            .debug_selector(|| "extensions-status".into())
            .role(Role::Status)
            .aria_label(text.clone())
    });
    let details = (!view.details.is_empty()).then(|| {
        div()
            .flex()
            .flex_col()
            .gap(theme.geometry.controls.list_gap)
            .children(view.details.iter().enumerate().map(|(index, line)| {
                controls::field_description(line.clone(), theme.text_body, theme)
                    .id(("extension-detail", index))
                    .debug_selector(move || format!("extension-detail-{line}"))
            }))
    });
    let empty = view.empty.then(|| {
        controls::field_description("No extensions are installed.", theme.text_muted, theme)
            .id("extension-empty")
            .debug_selector(|| "extension-empty".into())
    });
    let back = view.back.then(|| {
        let back = controls::button("Back", true, theme)
            .id("extension-back")
            .debug_selector(|| "extension-back".into())
            .role(Role::Button)
            .aria_label("Back")
            .aria_description("Return to the extension list");
        div().flex().child(attach(ExtensionsControl::Back, back))
    });
    let list = (!view.rows.is_empty()).then(|| {
        div()
            .flex()
            .flex_col()
            .gap(theme.geometry.controls.list_gap)
            .children(rows)
    });
    let commands = (!view.commands.is_empty()).then(|| section("Commands", commands, theme));
    let installs = (!view.installs.is_empty()).then(|| section("Install", installs, theme));
    let column = controls::column(theme)
        .child(
            settings_shell::page_header(view.title.clone(), view.subtitle.clone(), theme)
                .id("extensions-title")
                .debug_selector(|| "extensions-title".into()),
        )
        .children(status)
        .children(details)
        .children(empty)
        .children(back)
        .children(list)
        .children(commands)
        .children(installs);
    div()
        .id("extensions")
        .debug_selector(|| "extensions".into())
        .child(column)
}

/// One entry as a Settings list item named `id`: its tile, its title over
/// its subtitle and, when it cannot be used here, the reason in the
/// warning tone — all of which its accessible description carries too.
fn item(entry: &ExtensionItem, id: ElementId, theme: &Theme) -> Stateful<Div> {
    let mut lines = Vec::new();
    if let Some(subtitle) = &entry.subtitle {
        lines.push(
            controls::field_description(subtitle.clone(), theme.text_muted, theme)
                .truncate()
                .into_any_element(),
        );
    }
    if let Some(reason) = &entry.reason {
        lines.push(
            controls::field_description(reason.clone(), theme.warning, theme).into_any_element(),
        );
    }
    let tile = entry
        .icon
        .map(|(tone, glyph)| tile_at(TileSize::Row, tone, glyph, theme));
    // The entry's accessible description: its subtitle and, when it cannot
    // run, the reason, together.
    let description = match (&entry.subtitle, &entry.reason) {
        (Some(subtitle), Some(reason)) => Some(format!("{subtitle}. {reason}")),
        (subtitle, reason) => subtitle.clone().or_else(|| reason.clone()),
    };
    controls::list_item(tile, entry.title.clone(), lines, theme)
        .id(id)
        .role(Role::Button)
        .aria_label(entry.title.clone())
        .when_some(description, |item, description| {
            item.aria_description(description)
        })
}

/// One of the page's own groups of entries: its field label over them.
fn section(label: &'static str, items: impl Iterator<Item = Stateful<Div>>, theme: &Theme) -> Div {
    controls::field(theme)
        .child(
            controls::field_label(label, theme)
                .debug_selector(move || format!("extension-section-{label}")),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap(theme.geometry.controls.list_gap)
                .children(items),
        )
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
