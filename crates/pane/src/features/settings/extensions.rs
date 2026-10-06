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
//! On the list, the rows are gathered into a card per installed extension
//! ([`gather`]): its enable/disable row is the card's switch, its
//! automatic updates' row a second switch, and its other operations
//! (reload, clear cache, uninstall…) short buttons — no row says more than
//! its name. A command's alias, hotkey and fallback rows are left to the
//! Shortcuts page.
//!
//! The launcher's install rows belong to the launcher *window* rather than
//! the shared screen: their folder picker and forms live in that window.
//! Clicking one summons and focuses the launcher window at that row,
//! through [`crate::app::LauncherWindow::activate_root_result`].
//!
//! Synchronization is by reading, not copying: every frame re-reads the
//! launcher, and wherever the launcher changes — an operation's reply, a
//! background update, build or runtime restart the changes channel
//! reports — the windows showing it redraw (see
//! [`crate::app::LauncherWindow::sync_screen`]). After any operation,
//! including a failed one, the page shows what the launcher holds.

use gpui::{
    AnyElement, App, Context, Div, ElementId, Hsla, Role, SharedString, Stateful, Toggled, Window,
    div, prelude::*, px,
};
use pane_core::{Launcher, Screen, Status};

use super::{Page, SettingsWindow, search};
use crate::app::{LauncherWindow, launcher_changed_outside, row_icon};
use crate::ui::controls;
use crate::ui::icon::{Glyph, IconTone, TileSize, tile_at};
use crate::ui::theme::{Theme, pressed};

/// What the page is, in one line: its sidebar entry's description in
/// the search.
pub(crate) const ABOUT: &str = "Install and manage extensions";

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
                .map(|(id, _, title)| search::Entry {
                    control: Some(id.into()),
                    title: title.into(),
                    // The page's own section label, which the rows sit under.
                    group: Some(INSTALL.into()),
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

/// The launcher's install rows: their ids, as root search builds its
/// `pane.install-*` rows, the page's titles for them under its Install
/// label, and the titles the sidebar's search finds them by. The page
/// dispatches to those rows through the launcher window, which owns their
/// pickers and forms.
const INSTALL_ROWS: [(&str, &str, &str); 3] = [
    (
        "pane.install-from-folder",
        "Folder…",
        "Install from a folder",
    ),
    ("pane.install-from-npm", "npm…", "Install from npm"),
    ("pane.install-from-git", "Git…", "Install from Git"),
];

/// The page's section labels.
const INSTALLED: &str = "Installed";
const INSTALL: &str = "Install";

/// Whether the launcher's screen is held by the extension-management
/// flow: the list itself, or one of the screens its rows open — a
/// confirmation, pause, build, network, program or runtime details. While
/// it is, the page draws the launcher's live view, so the flow's
/// confirmations show here; otherwise it reads the list without entering
/// the flow, and the launcher's screen stays wherever the user left it.
fn in_extension_flow(screen: &Screen) -> bool {
    matches!(
        screen,
        Screen::Extensions { .. }
            | Screen::Confirm { .. }
            | Screen::PauseDetails { .. }
            | Screen::BuildDetails { .. }
            | Screen::RuntimeDetails { .. }
            | Screen::NetworkDetails { .. }
            | Screen::ProgramDetails { .. }
    )
}

/// Whether the screen is one of the flow's details screens — pause, build,
/// network, program or runtime details — which offer no Cancel row of their
/// own (a confirmation's is its own), so the page offers the way out the
/// launcher window's Escape is there.
fn details_screen(screen: &Screen) -> bool {
    matches!(
        screen,
        Screen::PauseDetails { .. }
            | Screen::BuildDetails { .. }
            | Screen::RuntimeDetails { .. }
            | Screen::NetworkDetails { .. }
            | Screen::ProgramDetails { .. }
    )
}

/// One entry of the page's lists, as plain values: the launcher's row (a
/// management operation, a confirmation's answer) or an install source.
pub(crate) struct ExtensionItem {
    /// The launcher's own row id, where the entry is one of its rows.
    pub(crate) id: String,
    pub(crate) title: String,
    /// Why the entry cannot be used here, if it cannot.
    pub(crate) reason: Option<String>,
    pub(crate) icon: Option<(IconTone, Glyph)>,
}

/// One installed extension's card, as plain values: its rows of the
/// launcher's list gathered under it.
pub(crate) struct PackageCard {
    /// The launcher's enable/disable row for it: its identity's key.
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) icon: Option<(IconTone, Glyph)>,
    pub(crate) enabled: bool,
    /// What needs saying about it, in a word or two: paused, developing.
    pub(crate) badges: Vec<String>,
    /// Its automatic-updates row, and whether they are on, where it has one.
    pub(crate) auto_update: Option<(String, bool)>,
    /// Its other operations, as buttons: the row's id, the button's label,
    /// the row's own title (its accessible name and test selector) and why
    /// it cannot be used here, if it cannot.
    pub(crate) actions: Vec<(String, String, String, Option<String>)>,
}

/// What the Extensions page shows, as plain values: what [`render`] reads
/// from the launcher (#99).
pub(crate) struct ExtensionsView {
    /// The flow's screen title: shown over a confirmation's or a details
    /// screen's rows, not over the list itself (`listing`), which the
    /// titlebar already names.
    pub(crate) title: String,
    pub(crate) listing: bool,
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
    /// The installed extensions, on the list.
    pub(crate) packages: Vec<PackageCard>,
    /// The launcher's rows that belong to no extension's card: the
    /// runtime's, retained data's, a confirmation's answers.
    pub(crate) rows: Vec<ExtensionItem>,
    /// The global automatic-updates row, and whether it is on.
    pub(crate) auto_update: Option<(String, bool)>,
    pub(crate) installs: Vec<ExtensionItem>,
}

/// Which of the Extensions page's controls an element is, for the caller
/// of [`compose`] that attaches its behavior.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ExtensionsControl {
    /// The launcher's row with this id: activated as its Enter would.
    Row(String),
    /// The install source with this row id: opened in the launcher.
    Install(String),
    /// A details screen's way back.
    Back,
}

/// The launcher's rows gathered into cards: each installed extension's
/// rows (its enable/disable row, its operations), and the rest. `rows` are
/// the list's rows, `packages` the installed extensions (their key, title
/// and whether they are enabled). Rows that set a command's alias, hotkey
/// or fallback are left out: the Shortcuts page manages those.
pub(crate) fn gather(
    rows: &[pane_core::Row],
    packages: &[(String, String, bool)],
) -> (Vec<PackageCard>, Vec<ExtensionItem>, Option<(String, bool)>) {
    let on = |row: &pane_core::Row| {
        row.subtitle
            .as_deref()
            .is_some_and(|subtitle| subtitle.starts_with("On"))
    };
    let mut cards: Vec<PackageCard> = packages
        .iter()
        .map(|(key, title, enabled)| PackageCard {
            id: key.clone(),
            title: title.clone(),
            icon: Some(row_icon(key)),
            enabled: *enabled,
            badges: Vec::new(),
            auto_update: None,
            actions: Vec::new(),
        })
        .collect();
    let mut others = Vec::new();
    let mut global = None;
    for row in rows {
        if row.id == "updates" {
            global = Some((row.id.clone(), on(row)));
            continue;
        }
        if let Some(card) = cards.iter_mut().find(|card| card.id == row.id) {
            // The extension's own row: its state, in a word or two.
            card.badges = row
                .subtitle
                .iter()
                .flat_map(|subtitle| subtitle.split(" · "))
                .filter(|part| {
                    part.starts_with("Paused")
                        || part.starts_with("Failed")
                        || *part == "Developing"
                })
                .map(str::to_owned)
                .collect();
            continue;
        }
        let (kind, key) = row.id.split_once(':').unwrap_or((row.id.as_str(), ""));
        if matches!(
            kind,
            "hotkey" | "alias-setting" | "fallback-setting" | "unlisted-setting"
        ) {
            continue;
        }
        let reason = row.unavailable.as_ref().map(|why| why.reason().to_owned());
        match cards.iter_mut().find(|card| card.id == key) {
            Some(card) if kind == "updates" => card.auto_update = Some((row.id.clone(), on(row))),
            Some(card) => {
                let label = match kind {
                    "reload" => "Reload".to_owned(),
                    "retry" => "Retry".to_owned(),
                    "paused" => "Why paused".to_owned(),
                    "clear-cache" => "Clear cache".to_owned(),
                    "uninstall" => "Uninstall".to_owned(),
                    "network" => "Network".to_owned(),
                    // The title without the extension's name.
                    _ => row
                        .title
                        .replace(&card.title, "")
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" "),
                };
                card.actions
                    .push((row.id.clone(), label, row.title.clone(), reason));
            }
            None => others.push(ExtensionItem {
                id: row.id.clone(),
                title: row.title.clone(),
                reason,
                icon: Some(row_icon(&row.id)),
            }),
        }
    }
    (cards, others, global)
}

/// The Extensions page's composition: the flow's status above everything; then, on the list, a
/// card per installed extension — its tile, name and badges with its
/// on/off switch, its automatic updates' switch where it has one, and its
/// operations as buttons — the rest of the launcher's rows, the global
/// automatic updates and the install sources as buttons. On the flow's
/// other screens, its title over its lines of information, its answers
/// and a details screen's way back. `attach` adds each control's
/// behavior; the composition gives each its identity, its accessibility
/// and its look.
pub(crate) fn compose(
    view: &ExtensionsView,
    theme: &Theme,
    attach: impl Fn(ExtensionsControl, Stateful<Div>) -> Stateful<Div>,
) -> Stateful<Div> {
    let inset = theme.geometry.settings.section_label_inset;
    let status = view.status.as_ref().map(|(text, color)| {
        controls::field_description(text.clone(), *color, theme)
            .px(inset)
            .id("extensions-status")
            .debug_selector(|| "extensions-status".into())
            .role(Role::Status)
            .aria_label(text.clone())
    });
    let empty = view.empty.then(|| {
        controls::field_description("No extensions installed yet.", theme.text_muted, theme)
            .px(inset)
            .id("extension-empty")
            .debug_selector(|| "extension-empty".into())
    });
    let packages = view
        .packages
        .iter()
        .enumerate()
        .map(|(index, card)| package_card(index, card, theme, &attach).into_any_element())
        .collect::<Vec<_>>();
    let packages = (!packages.is_empty()).then(|| {
        controls::section(
            Some(INSTALLED.into()),
            div()
                .flex()
                .flex_col()
                .gap(theme.geometry.settings.section_label_gap)
                .children(packages),
            theme,
        )
    });
    let rows: Vec<AnyElement> = view
        .rows
        .iter()
        .enumerate()
        .map(|(index, row)| {
            let item = item(row, ("extension-row", index).into(), theme)
                .debug_selector(|| format!("extension-row-{}", row.title))
                // An unavailable row stays listed and clickable; activating
                // it shows the reason, as the launcher's does.
                .when(row.reason.is_some(), |item| item.aria_disabled(true));
            attach(ExtensionsControl::Row(row.id.clone()), item).into_any_element()
        })
        .collect();
    let details = view
        .details
        .iter()
        .enumerate()
        .map(|(index, line)| {
            controls::field_description(line.clone(), theme.text_body, theme)
                .px(inset)
                .id(("extension-detail", index))
                .debug_selector(move || format!("extension-detail-{line}"))
                .into_any_element()
        })
        .collect::<Vec<_>>();
    let back = view.back.then(|| {
        let back = controls::button("extension-back", "Back", true, theme)
            .debug_selector(|| "extension-back".into())
            .role(Role::Button)
            .aria_label("Back");
        div().flex().child(attach(ExtensionsControl::Back, back))
    });
    // The launcher's other rows: under the flow's title on its other
    // screens, unlabelled under the cards on the list.
    let list = (!rows.is_empty() || !details.is_empty() || view.back).then(|| {
        let body = div()
            .flex()
            .flex_col()
            .gap(theme.geometry.settings.section_label_gap)
            .children(details)
            .children((!rows.is_empty()).then(|| controls::list_card(rows, theme)))
            .children(back);
        let label = (!view.listing).then(|| SharedString::from(view.title.clone()));
        let section = controls::section(label, body, theme);
        if view.listing {
            section
        } else {
            section.debug_selector(|| "extensions-title".into())
        }
    });
    let updates = view.auto_update.as_ref().map(|(id, on)| {
        let switch = super::general::switch_row(
            super::general::SwitchRow {
                id: "extension-updates",
                selector: "extension-row-Update extensions automatically",
                title: "Update extensions automatically",
                on: *on,
                offered: true,
                lines: Vec::new(),
            },
            theme,
            |switch| attach(ExtensionsControl::Row(id.clone()), switch),
        );
        controls::section(
            None,
            controls::card([switch.into_any_element()], theme),
            theme,
        )
    });
    let installs = (!view.installs.is_empty()).then(|| {
        let buttons = view.installs.iter().map(|install| {
            let id = SharedString::from(format!("extension-install-{}", install.id));
            let button = controls::button(id, install.title.clone(), true, theme)
                .debug_selector(|| format!("extension-install-{}", install.title))
                .role(Role::Button)
                .aria_label(install.title.clone());
            attach(ExtensionsControl::Install(install.id.clone()), button)
        });
        let row = controls::setting_row("Install from", Vec::new(), theme).child(
            div()
                .flex_none()
                .flex()
                .gap(theme.geometry.controls.button_gap)
                .children(buttons),
        );
        controls::section(None, controls::card([row.into_any_element()], theme), theme)
            .debug_selector(|| format!("extension-section-{INSTALL}"))
    });
    let page = controls::page(theme)
        .when(view.listing, |page| {
            page.debug_selector(|| "extensions-title".into())
        })
        .children(status)
        .children(empty)
        .children(packages)
        .children(list)
        .children(updates)
        .children(installs);
    div()
        .id("extensions")
        .debug_selector(|| "extensions".into())
        .child(page)
}

/// One installed extension's card: its tile, name and badges at the left
/// with its on/off switch at the right; its automatic updates' switch
/// where it has one; and its operations as buttons.
fn package_card(
    index: usize,
    card: &PackageCard,
    theme: &Theme,
    attach: &impl Fn(ExtensionsControl, Stateful<Div>) -> Stateful<Div>,
) -> Div {
    let controls_geometry = &theme.geometry.controls;
    let badges = card.badges.iter().map(|badge| {
        let tone = if badge == "Developing" {
            theme.accent_text
        } else {
            theme.warning
        };
        controls::field_description(badge.clone(), tone, theme).flex_none()
    });
    let label = div()
        .flex()
        .items_center()
        .gap(controls_geometry.button_gap)
        .child(controls::field_label(card.title.clone(), theme))
        .children(badges);
    let toggle = controls::toggle(card.enabled, theme)
        .debug_selector(move || format!("extension-toggle-{index}"));
    let settings = &theme.geometry.settings;
    let head = div()
        .w_full()
        .flex()
        .items_center()
        .gap(controls_geometry.row_gap)
        .min_h(settings.card_row_height)
        .px(settings.card_padding_x)
        .py(settings.card_row_padding_y)
        .children(
            card.icon
                .map(|(tone, glyph)| tile_at(TileSize::Row, tone, glyph, theme)),
        )
        .child(label.flex_1().min_w(px(0.)))
        .child(toggle)
        .id(("extension-row", index))
        .debug_selector(|| format!("extension-row-{}", card.title))
        .rounded_t(settings.card_radius)
        .hover(|row| row.bg(theme.nav_hover))
        .active(|row| row.bg(pressed(theme.nav_hover)))
        .role(Role::Switch)
        .aria_label(card.title.clone())
        .aria_toggled(if card.enabled {
            Toggled::True
        } else {
            Toggled::False
        })
        .cursor_pointer();
    let head = attach(ExtensionsControl::Row(card.id.clone()), head);
    let mut rows = vec![head.into_any_element()];
    if let Some((id, on)) = &card.auto_update {
        let switch = super::general::switch_row(
            super::general::SwitchRow {
                id: "extension-auto-update",
                selector: "extension-auto-update",
                title: "Update automatically",
                on: *on,
                offered: true,
                lines: Vec::new(),
            },
            theme,
            |switch| attach(ExtensionsControl::Row(id.clone()), switch),
        );
        rows.push(switch.into_any_element());
    }
    if !card.actions.is_empty() {
        let buttons = card.actions.iter().map(|(id, label, title, reason)| {
            let button_id = SharedString::from(format!("extension-action-{id}"));
            let button = controls::ghost_button(button_id, label.clone(), reason.is_none(), theme)
                .debug_selector(|| format!("extension-row-{title}"))
                .role(Role::Button)
                .aria_label(title.clone())
                .when_some(reason.clone(), |button, reason| {
                    // An unavailable operation stays clickable: activating
                    // it shows why, as the launcher's row does.
                    button.aria_description(reason).cursor_pointer()
                });
            attach(ExtensionsControl::Row(id.clone()), button)
        });
        rows.push(
            div()
                .flex()
                .flex_wrap()
                .gap(controls_geometry.button_gap)
                .px(theme.geometry.settings.card_padding_x)
                .py(theme.geometry.settings.card_row_padding_y)
                .children(buttons)
                .into_any_element(),
        );
    }
    controls::card(rows, theme)
}

/// One entry as a Settings list item named `id`: its tile and its title,
/// and, when it cannot be used here, the reason in the warning tone, which
/// its accessible description carries too.
fn item(entry: &ExtensionItem, id: ElementId, theme: &Theme) -> Stateful<Div> {
    let lines = entry
        .reason
        .iter()
        .map(|reason| {
            controls::field_description(reason.clone(), theme.warning, theme).into_any_element()
        })
        .collect();
    let tile = entry
        .icon
        .map(|(tone, glyph)| tile_at(TileSize::Row, tone, glyph, theme));
    controls::list_item(id, tile, entry.title.clone(), lines, theme)
        .role(Role::Button)
        .aria_label(entry.title.clone())
        .when_some(entry.reason.clone(), |item, reason| {
            item.aria_description(reason)
        })
}

/// Draws the Extensions page: the extension list — read where the launcher
/// has not entered the flow, live where it has — gathered into a card per
/// installed extension, with the flow's title, status and lines of
/// information on its other screens, then the launcher's install sources,
/// which open in the launcher window.
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
    // Whether the list itself shows, or one of the flow's other screens (a
    // confirmation names what it asks about).
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
    let packages: Vec<(String, String, bool)> = if listing {
        this.launcher
            .packages()
            .iter()
            .map(|package| (package.identity.key(), package.title(), package.enabled))
            .collect()
    } else {
        Vec::new()
    };
    let (packages, rows, auto_update) = if listing {
        gather(&list.rows, &packages)
    } else {
        // A confirmation's answers, or a details screen's rows, as they
        // are.
        let rows = list
            .rows
            .iter()
            .map(|row| ExtensionItem {
                id: row.id.clone(),
                title: row.title.clone(),
                reason: row.unavailable.as_ref().map(|why| why.reason().to_owned()),
                icon: None,
            })
            .collect();
        (Vec::new(), rows, None)
    };
    let installs = if listing && this.launcher.installs_packages() {
        install_items()
    } else {
        Vec::new()
    };
    let view = ExtensionsView {
        title: list.title.clone(),
        listing,
        status,
        // The list's explanatory lines stay in the launcher: the cards say
        // enough. A confirmation's or a details screen's lines are what
        // that screen is for.
        details: if listing {
            Vec::new()
        } else {
            list.details().to_vec()
        },
        empty: listing && packages.is_empty() && rows.is_empty(),
        back: details_screen(&list.screen),
        packages,
        rows,
        auto_update,
        installs,
    };
    // Each row's scroll anchor, keyed by the row's own id, which the
    // search's reveal scrolls to.
    let ids = view
        .packages
        .iter()
        .flat_map(|card| {
            std::iter::once(card.id.clone())
                .chain(card.auto_update.iter().map(|(id, _)| id.clone()))
                .chain(card.actions.iter().map(|(id, ..)| id.clone()))
        })
        .chain(view.rows.iter().map(|row| row.id.clone()))
        .chain(view.auto_update.iter().map(|(id, _)| id.clone()))
        .chain(view.installs.iter().map(|row| row.id.clone()));
    let anchors: std::collections::HashMap<String, gpui::ScrollAnchor> = ids
        .map(|id| {
            let anchor = this.search_anchor(&id);
            (id, anchor)
        })
        .collect();
    compose(&view, &theme, |control, element| match control {
        ExtensionsControl::Row(id) => {
            element
                .anchor_scroll(anchors.get(&id).cloned())
                .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                    activate(this, &id, cx);
                }))
        }
        ExtensionsControl::Install(id) => element
            .anchor_scroll(anchors.get(&id).cloned())
            .on_click(cx.listener(move |_, _: &gpui::ClickEvent, _, cx| {
                open_in_launcher(&id, cx);
            })),
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
        .map(|(id, title, _)| ExtensionItem {
            id: id.into(),
            title: title.into(),
            reason: None,
            icon: Some(row_icon(id)),
        })
        .collect()
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
