//! Extensions in Settings (#168, ADR 0043): the sidebar's Extensions
//! group, one page per installed extension, and the group's own page, where
//! extensions are installed. Raycast manages its extensions in its
//! Settings, one page per extension, and so does Pane; the launcher has no
//! screen for them — its "Manage Extensions" command opens Settings here.
//!
//! **The group.** After Pane's own pages, the sidebar's Extensions entry
//! heads the group: its + menu installs from a folder, npm or Git, and
//! under it each installed extension has an entry of its own — its icon,
//! its title, and a mark in a word while it is paused, broken or updating
//! ([`sidebar_entries`]). The entry itself opens the group's page: the
//! installed extensions as a list, what governs them all (automatic
//! updates), what belongs to none of them (the runtime's rows, data kept
//! for an uninstalled extension), and the install sources.
//!
//! **An extension's page.** Its large icon, title, description and source
//! at the top, with the mark that needs saying; its enable switch; its
//! "Actions…" menu — Check for Update, Reload, Clear Cache, Reset
//! Confirmations, Show Source Folder, Uninstall, and what else the
//! extension list holds for it (Retry, why it is paused, development, the
//! network and the programs it used); its automatic updates' switch; its
//! preferences in a card (#143's controls, a dropdown drawn as the shared
//! searchable select); and its Commands, each with its
//! icon, its title, its alias field, its hotkey recorder, its fallback
//! switch where it takes a query, and its own enable switch. A command the
//! Shortcuts catalog does not list (a root provider, #164) shows only its
//! switch.
//!
//! **The operations are the launcher's.** The page manages nothing itself:
//! the switch and the menu's operations are the launcher's typed
//! operations ([`pane_core::Launcher::extension_operations`]: what each
//! is, whose, and whether it is on), each run through
//! [`pane_core::Launcher::run_extension_operation`], which leaves the
//! launcher on the screen the user had. The confirmations they ask for —
//! disabling or uninstalling what other extensions require, the saved-data
//! choice, deleting retained data — and the previews an install or an
//! update check shows are the launcher's own screens, drawn here in place
//! of the page from the launcher's live view, and answered here
//! ([`pane_core::Launcher::select`],
//! [`pane_core::Launcher::activate_selected`]); so every operation runs
//! through the same code with the same records. A
//! command's alias and hotkey are set as the Shortcuts page sets them
//! ([`pane_core::Launcher::set_alias`],
//! [`pane_core::Launcher::set_hotkey`]), and its switch through
//! [`pane_core::Launcher::set_command_enabled`].
//!
//! **Installing.** The + menu, the group page's install buttons, and root
//! search's install rows (which open Settings here) start the same flows:
//! a folder from the system's picker, an npm package or a Git repository
//! named in a field on the group's page; each is previewed by the launcher
//! ([`pane_core::Launcher::preview_package`],
//! [`pane_core::Launcher::preview_npm`],
//! [`pane_core::Launcher::preview_git`]), and the preview's Install row
//! installs it.
//!
//! Synchronization is by reading, not copying: every frame re-reads the
//! launcher, and wherever the launcher changes — an operation's reply, a
//! background update, build or runtime restart the changes channel
//! reports — the windows showing it redraw (see
//! [`crate::app::LauncherWindow::sync_screen`]). After any operation,
//! including a failed one, the page shows what the launcher holds, and
//! what the operation came to.

use std::cell::RefCell;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;

use gpui::{
    AnyElement, App, Context, Div, ElementId, Entity, FocusHandle, Focusable, Hsla, MouseDownEvent,
    PathPromptOptions, Role, ScrollAnchor, SharedString, Stateful, Subscription, Toggled, Window,
    anchored, deferred, div, prelude::*, px,
};
use gpui_elements::editable_text::{EditableTextState, StringStorage, TextChanged, text_input};
use pane_core::{
    ExtensionMark, ExtensionOperation, InstalledPackage, Launcher, LauncherView, OperationKind,
    PackageIdentity, PackagePreferences, PathKind, PreferenceField, PreferenceKind, Screen,
    ShortcutCommand, Status,
};

use super::{Page, SettingsWindow, search};
use crate::app::{launcher_changed_outside, row_icon};
use crate::ui::controls;
use crate::ui::extension_icon::{RowIcon, row_icon_at};
use crate::ui::icon::{Glyph, TileSize};
use crate::ui::material::popover_shadows;
use crate::ui::select::{Choice, Model, Select};
use crate::ui::theme::{Theme, pressed};
use crate::ui::virtual_list::PageWindow;

/// What the group's page is, in one line: its sidebar entry's description
/// in the search.
pub(crate) const ABOUT: &str = "Install and manage extensions";

/// The group's title: its sidebar entry, and its page's.
pub(crate) const TITLE: &str = "Extensions";

/// The scroll anchor of the preferences of the command `command` (its id
/// in `pane.json`) on the page of the extension whose identity key is
/// `package`: where "Configure Command…" takes the user. The page itself
/// is reached by the identity key, where "Configure Extension…" goes.
pub(crate) fn command_preferences_anchor(package: &str, command: &str) -> String {
    format!("preferences:{package}#{command}")
}

/// The scroll anchor of the preference kept as `key` on the page of the
/// extension whose identity key is `package`: where the sidebar's search
/// takes the user (#168).
fn preference_anchor(package: &str, key: &str) -> String {
    format!("preference:{package}\u{1f}{key}")
}

/// The scroll anchor of the command `command` (its full id) on its
/// extension's page.
fn command_anchor(command: &str) -> String {
    format!("command:{command}")
}

/// Where an install starts: the three sources Pane installs from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InstallSource {
    Folder,
    Npm,
    Git,
}

impl InstallSource {
    const ALL: [InstallSource; 3] = [
        InstallSource::Folder,
        InstallSource::Npm,
        InstallSource::Git,
    ];

    /// The install target the sidebar's search, the + menu and root
    /// search's install rows open: `install:<source>`.
    pub(crate) fn target(self) -> &'static str {
        match self {
            InstallSource::Folder => "install:folder",
            InstallSource::Npm => "install:npm",
            InstallSource::Git => "install:git",
        }
    }

    fn of_target(target: &str) -> Option<InstallSource> {
        match target {
            "install:folder" => Some(InstallSource::Folder),
            "install:npm" => Some(InstallSource::Npm),
            "install:git" => Some(InstallSource::Git),
            _ => None,
        }
    }

    /// The + menu's entry for it, and the search's.
    fn menu_title(self) -> &'static str {
        match self {
            InstallSource::Folder => "Install from Folder…",
            InstallSource::Npm => "Install from npm…",
            InstallSource::Git => "Install from Git…",
        }
    }

    /// The group page's button for it.
    fn button(self) -> &'static str {
        match self {
            InstallSource::Folder => "Folder…",
            InstallSource::Npm => "npm…",
            InstallSource::Git => "Git…",
        }
    }
}

/// The Extensions pages' own state: the text fields and the dropdowns'
/// selects of the preferences (#143), each created when it first draws and
/// kept, so what the user types or opens survives redraws; the menus; the
/// install field; and what the last operation came to.
#[derive(Default)]
pub(crate) struct State {
    /// Each text field by [`field_key`], with what saves its changes.
    fields: HashMap<String, (Entity<EditableTextState>, Subscription)>,
    /// Each dropdown preference's select by [`field_key`].
    selects: HashMap<String, PreferenceSelect>,
    /// Why the last change of a preference could not be saved, if it
    /// could not; shown as the page's status.
    problem: Option<String>,
    /// Whether the sidebar's + menu is open.
    adding: bool,
    /// Whether the extension page's Actions menu is open.
    menu: bool,
    /// The npm package or Git repository the group's page asks for, with
    /// its field, while one is asked for.
    installing: Option<Installing>,
    /// What the last operation the pages started came to, once it is done
    /// and the launcher left the flow (an install lands on root search):
    /// the launcher's status then.
    outcome: Option<Status>,
    /// The sidebar's entries of a long Extensions group, drawn only near
    /// the sidebar's view (#165).
    sidebar_window: PageWindow,
    /// An extension's long Commands section, drawn only near the page's
    /// view (#165).
    commands_window: PageWindow,
}

/// The field the group's page shows for an npm package or a Git
/// repository to install.
struct Installing {
    source: InstallSource,
    input: Entity<EditableTextState>,
}

/// The key of the text field of the preference kept as `key` of the
/// extension whose identity key is `package`.
fn field_key(package: &str, key: &str) -> String {
    format!("{package}\u{1f}{key}")
}

/// A dropdown preference's select: the shared searchable select
/// ([`crate::ui::select`]) the Settings pages' own choices use, a trigger
/// showing the value in force that opens its options with a search field.
struct PreferenceSelect {
    select: Entity<Select>,
    /// The preference's title and description it was made with: a reload
    /// changing them makes it again.
    labels: SelectLabels,
    /// The options and the value in force as the page last drew them,
    /// which the select's model reads live.
    live: Rc<RefCell<SelectLive>>,
}

/// A dropdown preference's title and description, its select's accessible
/// name and description.
#[derive(Clone, PartialEq, Eq)]
struct SelectLabels {
    title: String,
    description: String,
}

/// What a dropdown preference's select offers and marks now.
#[derive(Default)]
struct SelectLive {
    choices: Vec<Choice>,
    committed: Option<SharedString>,
}

/// A preference's text field as the page draws it: its editing state, its
/// focus and the text it holds now.
#[derive(Clone)]
pub(crate) struct FieldInput {
    input: Entity<EditableTextState>,
    focus: FocusHandle,
    text: String,
}

/// How many extensions are installed: the count the group's sidebar entry
/// shows.
fn installed(launcher: &Launcher) -> usize {
    launcher.packages().len()
}

/// The Extensions group's page, after Pane's own pages in the sidebar.
pub(crate) fn page() -> Page {
    Page {
        title: TITLE,
        about: ABOUT,
        icon: Glyph::Blocks,
        // The installed extensions, as the reference counts its plugins.
        count: Some(installed),
        render,
        search: entries,
        focus,
    }
}

/// What the sidebar's search finds here, read live: each installed
/// extension by its title, its preferences by theirs (under the
/// extension's title), the operations the extension list holds (each
/// opening its extension's page), and the install sources.
fn entries(launcher: &Launcher, _cx: &App) -> Vec<search::Entry> {
    let packages = launcher.packages();
    let mut entries = Vec::new();
    for package in &packages {
        let key = package.identity.key();
        let title = package.title();
        entries.push(search::Entry {
            control: Some(key.clone()),
            title: title.clone(),
            group: Some("Extension".into()),
            unavailable: None,
        });
        let Some(preferences) = launcher.preferences_of(&package.identity) else {
            continue;
        };
        for field in &preferences.fields {
            entries.push(search::Entry {
                control: Some(preference_anchor(&key, &field.key)),
                title: field.preference.title.clone(),
                group: Some(title.clone()),
                unavailable: None,
            });
        }
        for command in &preferences.commands {
            for field in &command.fields {
                entries.push(search::Entry {
                    control: Some(preference_anchor(&key, &field.key)),
                    title: field.preference.title.clone(),
                    group: Some(format!("{title} · {}", command.title)),
                    unavailable: None,
                });
            }
        }
    }
    entries.extend(
        launcher
            .extension_operations()
            .into_iter()
            // An extension's own switch is its entry above.
            .filter(|operation| operation.kind != OperationKind::Enable)
            .map(|operation| search::Entry {
                control: Some(operation.id),
                title: operation.title,
                group: None,
                unavailable: operation.unavailable,
            }),
    );
    if launcher.installs_packages() {
        entries.extend(InstallSource::ALL.into_iter().map(|source| search::Entry {
            control: Some(source.target().into()),
            title: source.menu_title().into(),
            group: Some("Install".into()),
            unavailable: None,
        }));
    }
    entries
}

/// The identity key of the installed extension a target names, if it names
/// one: its own key ("Configure Extension…", the search's entry), one of
/// its operations (whose owner the launcher says), or a control on its
/// page — a preference, a command's preferences, a command — by the
/// anchors this page makes.
fn extension_of(launcher: &Launcher, target: &str) -> Option<String> {
    let installed: Vec<String> = launcher
        .packages()
        .into_iter()
        .map(|package| package.identity.key())
        .collect();
    if installed.iter().any(|key| key == target) {
        return Some(target.to_owned());
    }
    let owner = match launcher
        .extension_operations()
        .into_iter()
        .find(|operation| operation.id == target)
    {
        Some(operation) => operation.owner.map(|owner| owner.key()),
        None => anchor_owner(target),
    };
    owner.filter(|key| installed.contains(key))
}

/// The identity key of the extension whose page holds the anchor `target`
/// ([`preference_anchor`], [`command_preferences_anchor`],
/// [`command_anchor`]).
fn anchor_owner(target: &str) -> Option<String> {
    if let Some(rest) = target.strip_prefix("preference:") {
        return rest.split_once('\u{1f}').map(|(key, _)| key.to_owned());
    }
    let command = target
        .strip_prefix("preferences:")
        .or_else(|| target.strip_prefix("command:"))?;
    // A command's id is its package's key, `#` and its id in `pane.json`,
    // which holds no `#`.
    command.rsplit_once('#').map(|(key, _)| key.to_owned())
}

/// A jump to `target` — the sidebar's search, [`super::open_at`] — on the
/// group: an extension's own target opens its page; an install target
/// starts that install (its field takes the keyboard, so the answer is
/// `true` for npm and Git); anything else opens the group's page. The
/// reveal then scrolls to the control where it drew.
fn focus(
    this: &mut SettingsWindow,
    target: &str,
    window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> bool {
    this.extensions.menu = false;
    this.extensions.adding = false;
    if let Some(source) = InstallSource::of_target(target) {
        this.extension = None;
        return start_install(this, source, window, cx);
    }
    this.extension = extension_of(&this.launcher, target);
    cx.notify();
    false
}

/// Whether the launcher's screen is one an extension's operation opened:
/// a confirmation, a package's preview, pause, build, network, program or
/// runtime details. While it is, the pages draw it from the launcher's
/// live view, so its confirmations show and are answered here; otherwise
/// they read the launcher, whose screen stays wherever the user left it.
pub(crate) fn in_extension_flow(screen: &Screen) -> bool {
    matches!(
        screen,
        Screen::Confirm { .. }
            | Screen::Package { .. }
            | Screen::PauseDetails { .. }
            | Screen::BuildDetails { .. }
            | Screen::RuntimeDetails { .. }
            | Screen::NetworkDetails { .. }
            | Screen::ProgramDetails { .. }
    )
}

/// Whether the screen offers no Cancel row of its own (a confirmation's
/// is its own): the details screens and a preview, where the page offers
/// the way out the launcher window's Escape is.
fn details_screen(screen: &Screen) -> bool {
    matches!(
        screen,
        Screen::PauseDetails { .. }
            | Screen::BuildDetails { .. }
            | Screen::RuntimeDetails { .. }
            | Screen::NetworkDetails { .. }
            | Screen::ProgramDetails { .. }
            | Screen::Package { .. }
    )
}

/// A status, in its tone: an operation's progress or outcome, an error.
fn status_tone(status: &Status, theme: &Theme) -> Option<(SharedString, Hsla)> {
    match status.clone() {
        Status::Idle => None,
        Status::Progress(work) => Some((work.into(), theme.warning)),
        Status::Result(answer) => Some((answer.into(), theme.success)),
        Status::Error(message) => Some((message.into(), theme.danger)),
        Status::Running => Some(("Running…".into(), theme.warning)),
    }
}

/// The operations of one installed extension, sorted out for its page
/// ([`page_operations`]).
pub(crate) struct PageOperations {
    /// Its switch.
    pub(crate) enable: Option<ExtensionOperation>,
    /// Its automatic updates' switch, where it has one.
    pub(crate) auto_update: Option<ExtensionOperation>,
    /// What its Actions menu offers, in the launcher's order.
    pub(crate) menu: Vec<ExtensionOperation>,
    /// What forgets the choices recorded for a command it no longer has.
    pub(crate) forget: Vec<ExtensionOperation>,
    /// Its commands' fallback switches, for the commands that take a query
    /// or are fallbacks.
    pub(crate) fallbacks: Vec<ExtensionOperation>,
}

/// The operations of the installed extension with `identity` among
/// `operations`, sorted out for its page by what each is (see
/// [`PageOperations`]); its commands' aliases and hotkeys are its Commands
/// section's, set as the Shortcuts page sets them.
pub(crate) fn page_operations(
    operations: Vec<ExtensionOperation>,
    identity: &PackageIdentity,
) -> PageOperations {
    let mut page = PageOperations {
        enable: None,
        auto_update: None,
        menu: Vec::new(),
        forget: Vec::new(),
        fallbacks: Vec::new(),
    };
    for operation in operations
        .into_iter()
        .filter(|operation| operation.owner.as_ref() == Some(identity))
    {
        match operation.kind {
            OperationKind::Enable => page.enable = Some(operation),
            OperationKind::AutomaticUpdates => page.auto_update = Some(operation),
            OperationKind::Fallback => page.fallbacks.push(operation),
            OperationKind::ForgetChoices => page.forget.push(operation),
            OperationKind::Hotkey | OperationKind::Alias => {}
            _ => page.menu.push(operation),
        }
    }
    page
}

/// The source of an installed extension, in words, as its page says it.
pub(crate) fn source_line(package: &InstalledPackage) -> String {
    let identity = &package.identity;
    if identity.default_id().is_some() {
        return "Default extension".into();
    }
    if let Some(name) = identity.npm_name() {
        return match &package.npm {
            Some(npm) => format!("npm · {name}@{}", npm.version),
            None => format!("npm · {name}"),
        };
    }
    if let Some(git) = &package.git {
        return format!("Git · {}", git.url);
    }
    match identity.local_folder() {
        Some(folder) => format!("Folder · {}", folder.display()),
        None => identity.to_string(),
    }
}

/// What an extension's page says under its title: its manifest's
/// description, or, for an extension of one command that gives none, that
/// command's subtitle.
fn description_of(package: &InstalledPackage) -> Option<String> {
    if let Some(description) = package.description() {
        return Some(description.to_owned());
    }
    match package.listed_commands().as_slice() {
        [only] => only
            .registration
            .subtitle
            .clone()
            .filter(|subtitle| *subtitle != package.title()),
        _ => None,
    }
}

// ------------------------------------------------------------- the sidebar

/// The sidebar's + button on the Extensions entry, and the menu it opens:
/// Install from Folder…, from npm…, from Git…. Its click is its own (the
/// entry under it does not take it). `None` where this launcher installs
/// nothing.
pub(super) fn add_button(
    this: &SettingsWindow,
    theme: &Theme,
    cx: &mut Context<SettingsWindow>,
) -> Option<AnyElement> {
    if !this.launcher.installs_packages() {
        return None;
    }
    let open = this.extensions.adding;
    let button = controls::icon_button("extensions-add", Glyph::Plus, true, theme)
        .debug_selector(|| "extensions-add".into())
        .role(Role::Button)
        .aria_label("Install an extension")
        .aria_expanded(open)
        .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
            cx.stop_propagation();
            this.extensions.adding = !this.extensions.adding;
            cx.notify();
        }));
    let menu = open.then(|| {
        let items = InstallSource::ALL
            .into_iter()
            .enumerate()
            .map(|(index, source)| {
                controls::menu_row(
                    ("extensions-add-item", index),
                    source.menu_title(),
                    None,
                    (false, false, true),
                    theme,
                )
                .debug_selector(move || format!("extensions-add-{}", source.menu_title()))
                .role(Role::MenuItem)
                .aria_label(source.menu_title())
                .on_click(cx.listener(move |this, _: &gpui::ClickEvent, window, cx| {
                    cx.stop_propagation();
                    this.extensions.adding = false;
                    open_install(this, source, window, cx);
                }))
                .into_any_element()
            })
            .collect::<Vec<_>>();
        popover(
            "extensions-add-menu",
            "Install an extension",
            items,
            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                this.extensions.adding = false;
                cx.stop_propagation();
                cx.notify();
            }),
            theme,
            cx,
        )
    });
    Some(
        div()
            .flex_none()
            .child(button)
            .children(menu)
            .into_any_element(),
    )
}

/// A menu's popup: Pane's popover under its trigger, deferred so it paints
/// over the page, holding `items`; a mouse-down outside it closes it
/// (`outside`), and is consumed.
fn popover(
    id: &'static str,
    label: &'static str,
    items: Vec<AnyElement>,
    outside: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
    theme: &Theme,
    cx: &App,
) -> AnyElement {
    let material = crate::settings::visuals(cx).material;
    let list = div()
        .flex()
        .flex_col()
        .p(px(4.))
        .gap(theme.geometry.controls.list_gap)
        .children(items);
    let popup = div()
        .id(id)
        .debug_selector(move || id.into())
        .w(px(240.))
        .occlude()
        .role(Role::Menu)
        .aria_label(label)
        .on_mouse_down_out(outside)
        .child(
            div()
                .rounded(theme.geometry.popover_radius)
                .shadow(popover_shadows(theme))
                .child(material.popover(theme, list)),
        );
    deferred(
        anchored()
            .anchor(gpui::Anchor::TopLeft)
            .offset(gpui::point(px(0.), px(4.)))
            .snap_to_window_with_margin(px(4.))
            .child(popup),
    )
    .into_any_element()
}

/// The sidebar's entries for the installed extensions, under the group's
/// own entry (`group`, its index among the pages): each its icon, its
/// title and the mark that needs saying (a word: Paused, Broken,
/// Updating), selected while its page shows.
pub(super) fn sidebar_entries(
    this: &SettingsWindow,
    group: usize,
    theme: &Theme,
    cx: &mut Context<SettingsWindow>,
) -> Vec<AnyElement> {
    let packages = this.launcher.packages();
    let listed = packages.len();
    let entries = this.extensions.sidebar_window.clone();
    packages
        .into_iter()
        .enumerate()
        .map(|(index, package)| {
            let key = package.identity.key();
            let selected = this.selected == group && this.extension.as_deref() == Some(&key);
            // A long group draws only the entries near the sidebar's view,
            // and the selected one always (#165).
            entries.row(listed, key.clone(), selected, &this.sidebar_scroll, || {
                sidebar_entry_of(this, index, package, selected, theme, cx)
            })
        })
        .collect()
}

/// The sidebar entry of the installed `package`, the `index`th, selected
/// while its page shows.
fn sidebar_entry_of(
    this: &SettingsWindow,
    index: usize,
    package: InstalledPackage,
    selected: bool,
    theme: &Theme,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let key = package.identity.key();
    let title = package.title();
    let mark = this.launcher.extension_mark(&package.identity);
    let icon = crate::features::icons::row_icon_of(&this.launcher, &key, theme);
    let label = match &mark {
        Some(mark) => format!("{title}, {}", mark.word()),
        None => title.clone(),
    };
    let selector = format!("extension-entry-{title}");
    sidebar_entry(
        ("extension-entry", index),
        &icon,
        &title,
        mark.as_ref(),
        selected,
        theme,
    )
    .debug_selector(move || selector)
    .role(Role::ListBoxOption)
    .aria_label(label)
    .aria_selected(selected)
    .when(selected, |row| row.aria_active_descendant())
    .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
        this.show_extension(Some(key.clone()), cx);
    }))
    .into_any_element()
}

/// One extension's sidebar entry: the sidebar item's family (#97) — its
/// height, padding, radius and washes — indented under the group's entry,
/// with the extension's own icon in place of a glyph and its mark at its
/// right end, in the warning's tone.
fn sidebar_entry(
    id: impl Into<ElementId>,
    icon: &RowIcon,
    title: &str,
    mark: Option<&ExtensionMark>,
    selected: bool,
    theme: &Theme,
) -> Stateful<Div> {
    let settings = &theme.geometry.settings;
    let typography = &theme.typography;
    let (hover, hover_text) = if selected {
        (theme.nav_selected, theme.nav_selected_text)
    } else {
        (theme.nav_hover, theme.nav_hover_text)
    };
    let caption = typography.settings_caption_size;
    let scope = format!("extension-entry-{title}");
    let mark = mark.map(|mark| {
        let selector = format!("extension-entry-mark-{title}");
        div()
            .id("entry-mark")
            .debug_selector(move || selector)
            .flex_none()
            .text_size(caption)
            .text_color(theme.warning)
            .child(mark.word())
    });
    div()
        .flex_none()
        .w_full()
        .flex()
        .items_center()
        .gap(settings.item_gap)
        .min_h(settings.item_height)
        .pl(settings.item_padding_x + px(12.))
        .pr(settings.item_padding_x)
        .rounded(settings.item_radius)
        .cursor_pointer()
        .text_size(typography.settings_text_size)
        .font_weight(typography.medium)
        .text_color(if selected {
            theme.nav_selected_text
        } else {
            theme.nav_text
        })
        .when(selected, |row| row.bg(theme.nav_selected))
        .id(id)
        .hover(move |row| row.bg(hover).text_color(hover_text))
        .active(move |row| row.bg(pressed(hover)).text_color(hover_text))
        .child(row_icon_at(
            icon,
            TileSize::Mini,
            "entry-icon",
            &scope,
            theme,
        ))
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .truncate()
                .child(title.to_owned()),
        )
        .children(mark)
}

impl SettingsWindow {
    /// Shows the page of the installed extension whose identity key is
    /// `key`, or the group's own page with `None`: the sidebar's clicks.
    pub(super) fn show_extension(&mut self, key: Option<String>, cx: &mut Context<Self>) {
        if let Some(group) = self.pages.iter().position(|page| page.title == TITLE) {
            self.selected = group;
        }
        self.extension = key;
        self.extensions.menu = false;
        self.extensions.adding = false;
        self.clear_search(cx);
        cx.notify();
    }

    /// Test support: the field the group's page shows for an npm package
    /// or a Git repository to install, while it shows one.
    #[doc(hidden)]
    pub fn install_field(&self) -> Option<Entity<EditableTextState>> {
        self.extensions
            .installing
            .as_ref()
            .map(|installing| installing.input.clone())
    }

    /// Test support: the identity key of the extension whose page shows,
    /// if one does.
    #[doc(hidden)]
    pub fn shown_extension(&self) -> Option<String> {
        self.extension.clone()
    }
}

// -------------------------------------------------------------- the pages

/// Draws the selected page of the group: a flow screen the launcher shows
/// (a confirmation, a preview, details) in place of either page, else the
/// selected extension's page, else the group's own.
fn render(
    this: &mut SettingsWindow,
    window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let theme = crate::settings::visuals(cx).theme;
    let live = this.launcher.view();
    // An extension no longer installed (uninstalled here or elsewhere)
    // leaves its page for the group's.
    if let Some(key) = &this.extension
        && !this
            .launcher
            .packages()
            .iter()
            .any(|package| package.identity.key() == *key)
    {
        this.extension = None;
        this.extensions.menu = false;
    }
    let flow = in_extension_flow(&live.screen);
    // The flow's status while the launcher is in it; else, on a developed
    // extension's page, why its last build failed, as the development
    // thread reports it; else what the last operation came to; else a
    // preference that could not be saved.
    let status = flow
        .then(|| status_tone(&live.status, &theme))
        .flatten()
        .or_else(|| build_failure(this).map(|failure| (failure.into(), theme.danger)))
        .or_else(|| {
            this.extensions
                .outcome
                .as_ref()
                .and_then(|outcome| status_tone(outcome, &theme))
        })
        .or_else(|| {
            this.extensions
                .problem
                .clone()
                .map(|problem| (SharedString::from(problem), theme.danger))
        });
    let status = status.map(|(text, color)| {
        controls::field_description(text.clone(), color, &theme)
            .px(theme.geometry.settings.section_label_inset)
            .id("extensions-status")
            .debug_selector(|| "extensions-status".into())
            .role(Role::Status)
            .aria_label(text)
            .into_any_element()
    });
    let body = if flow {
        flow_screen(&live, &theme, cx)
    } else {
        match this.extension.clone() {
            Some(key) => extension_page(this, &key, &theme, window, cx),
            None => group_page(this, &theme, window, cx),
        }
    };
    let page = controls::page(&theme).children(status).children(body);
    div()
        .id("extensions")
        .debug_selector(|| "extensions".into())
        .child(page)
        .into_any_element()
}

/// A flow screen in place of the page: its title over its lines of
/// information, its rows (a confirmation's answers, a preview's Install,
/// a details screen's Retry), and the way back a details screen or a
/// preview offers.
fn flow_screen(
    live: &LauncherView,
    theme: &Theme,
    cx: &mut Context<SettingsWindow>,
) -> Vec<AnyElement> {
    let inset = theme.geometry.settings.section_label_inset;
    let details = live
        .details()
        .iter()
        .enumerate()
        .map(|(index, line)| {
            let selector = format!("extension-detail-{line}");
            controls::field_description(line.clone(), theme.text_body, theme)
                .px(inset)
                .id(("extension-detail", index))
                .debug_selector(move || selector)
                .into_any_element()
        })
        .collect::<Vec<_>>();
    let rows: Vec<AnyElement> = live
        .rows
        .iter()
        .enumerate()
        .map(|(index, row)| {
            let reason = row.unavailable.as_ref().map(|why| why.reason().to_owned());
            let selector = format!("extension-row-{}", row.title);
            let id = row.id.clone();
            list_entry(
                ("extension-row", index),
                None,
                &row.title,
                reason.clone(),
                theme,
            )
            .debug_selector(move || selector)
            // An unavailable row stays listed and clickable; activating it
            // shows the reason, as the launcher's does.
            .when(reason.is_some(), |item| item.aria_disabled(true))
            .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                answer(this, &id, cx);
            }))
            .into_any_element()
        })
        .collect();
    let back = details_screen(&live.screen).then(|| {
        let back = controls::button("extension-back", "Back", true, theme)
            .debug_selector(|| "extension-back".into())
            .role(Role::Button)
            .aria_label("Back")
            .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                // The way out of a details screen or a preview the page
                // shows, as the launcher window's Escape is there.
                this.launcher.back();
                launcher_changed_outside(cx);
                cx.notify();
            }));
        div().flex().child(back)
    });
    let body = div()
        .flex()
        .flex_col()
        .gap(theme.geometry.settings.section_label_gap)
        .children(details)
        .children((!rows.is_empty()).then(|| controls::list_card(rows, theme)))
        .children(back);
    vec![
        controls::section(Some(live.title.clone().into()), body, theme)
            .debug_selector(|| "extensions-title".into())
            .into_any_element(),
    ]
}

/// One entry of a list on these pages as a Settings list item named `id`:
/// its tile (with the scope its icon's selectors name) and its title,
/// and, when it cannot be used here, the reason in the warning tone, which
/// its accessible description carries too.
fn list_entry(
    id: impl Into<ElementId>,
    icon: Option<(&RowIcon, String)>,
    title: &str,
    reason: Option<String>,
    theme: &Theme,
) -> Stateful<Div> {
    let lines = reason
        .iter()
        .map(|reason| {
            controls::field_description(reason.clone(), theme.warning, theme).into_any_element()
        })
        .collect();
    let tile = icon.map(|(icon, scope)| {
        row_icon_at(icon, TileSize::Row, "extension-item-icon", &scope, theme)
    });
    controls::list_item(id, tile, title.to_owned(), lines, theme)
        .role(Role::Button)
        .aria_label(title.to_owned())
        .when_some(reason, |item, reason| item.aria_description(reason))
}

/// The group's own page: the installed extensions, each opening its page;
/// the rows that belong to none of them; automatic updates; and the
/// install sources, with the field an npm or Git install asks in.
fn group_page(
    this: &mut SettingsWindow,
    theme: &Theme,
    window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> Vec<AnyElement> {
    let packages = this.launcher.packages();
    let operations = this.launcher.extension_operations();
    // The page's own anchor: the tests' and the reveal's.
    let mut content = vec![
        div()
            .id("extensions-title")
            .debug_selector(|| "extensions-title".into())
            .into_any_element(),
    ];
    // The installed extensions.
    let mut items: Vec<AnyElement> = Vec::new();
    for (index, package) in packages.iter().enumerate() {
        let key = package.identity.key();
        let title = package.title();
        let icon = crate::features::icons::row_icon_of(&this.launcher, &key, theme);
        let mark = this.launcher.extension_mark(&package.identity);
        let anchor = this.search_anchor(&key);
        let mut lines = vec![
            controls::field_description(source_line(package), theme.text_muted, theme)
                .truncate()
                .into_any_element(),
        ];
        // What the package does, under its title as the extension's own
        // page shows it (#224): one line, cut like the source line.
        if let Some(description) = package.description() {
            lines.push(
                controls::field_description(description.to_owned(), theme.text_muted, theme)
                    .truncate()
                    .id(("extension-item-description", index))
                    .debug_selector(|| "extension-item-description".into())
                    .into_any_element(),
            );
        }
        if !package.enabled {
            lines.push(
                controls::field_description("Disabled", theme.text_muted, theme).into_any_element(),
            );
        }
        if let Some(mark) = &mark {
            lines.push(
                controls::field_description(mark.reason().to_owned(), theme.warning, theme)
                    .into_any_element(),
            );
        }
        let tile = row_icon_at(
            &icon,
            TileSize::Row,
            ("extension-item-icon", index),
            &format!("extension-{title}"),
            theme,
        );
        let selector = format!("extension-item-{title}");
        items.push(
            controls::list_item(
                ("extension-item", index),
                Some(tile),
                title.clone(),
                lines,
                theme,
            )
            .debug_selector(move || selector)
            .role(Role::Link)
            .aria_label(title)
            .anchor_scroll(Some(anchor))
            .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                this.show_extension(Some(key.clone()), cx);
            }))
            .into_any_element(),
        );
    }
    if items.is_empty() {
        content.push(
            controls::field_description("No extensions installed yet.", theme.text_muted, theme)
                .px(theme.geometry.settings.section_label_inset)
                .id("extension-empty")
                .debug_selector(|| "extension-empty".into())
                .into_any_element(),
        );
    } else {
        content.push(
            controls::section(
                Some("Installed".into()),
                controls::list_card(items, theme),
                theme,
            )
            .into_any_element(),
        );
    }
    // The operations that belong to no installed extension: the
    // runtime's, data kept for an uninstalled one; then every extension's
    // automatic updates.
    let mut global = None;
    let mut others: Vec<AnyElement> = Vec::new();
    for operation in operations {
        let installed = operation
            .owner
            .as_ref()
            .is_some_and(|owner| packages.iter().any(|package| package.identity == *owner));
        if installed {
            continue;
        }
        if operation.kind == OperationKind::AutomaticUpdates {
            global = Some(operation);
            continue;
        }
        let reason = operation.unavailable.clone();
        let icon: RowIcon = row_icon(&operation.id).into();
        let anchor = this.search_anchor(&operation.id);
        let selector = format!("extension-row-{}", operation.title);
        let title = operation.title.clone();
        others.push(
            list_entry(
                ("extension-other", others.len()),
                Some((&icon, title.clone())),
                &title,
                reason.clone(),
                theme,
            )
            .debug_selector(move || selector)
            .when(reason.is_some(), |item| item.aria_disabled(true))
            .anchor_scroll(Some(anchor))
            .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                run(this, &operation, cx);
            }))
            .into_any_element(),
        );
    }
    if !others.is_empty() {
        content.push(
            controls::section(None, controls::list_card(others, theme), theme).into_any_element(),
        );
    }
    if let Some(operation) = global {
        let anchor = this.search_anchor(&operation.id);
        let switch = switch_row(
            "extension-updates".into(),
            "Update extensions automatically",
            "extension-row-Update extensions automatically",
            operation.on.unwrap_or(false),
            theme,
        )
        .anchor_scroll(Some(anchor))
        .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
            run(this, &operation, cx);
        }));
        content.push(
            controls::section(
                None,
                controls::card([switch.into_any_element()], theme),
                theme,
            )
            .into_any_element(),
        );
    }
    if this.launcher.installs_packages() {
        content.push(install_section(this, theme, window, cx));
    }
    content
}

/// The group page's install section: a button per source, and, while an
/// npm package or a Git repository is asked for, its field and the button
/// that shows it.
fn install_section(
    this: &mut SettingsWindow,
    theme: &Theme,
    window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let mut buttons = Vec::new();
    for source in InstallSource::ALL {
        let anchor = this.search_anchor(source.target());
        let id = SharedString::from(format!("extension-install-{}", source.target()));
        let selector = format!("extension-install-{}", source.button());
        buttons.push(
            controls::button(id, source.button(), true, theme)
                .debug_selector(move || selector)
                .role(Role::Button)
                .aria_label(source.menu_title())
                .anchor_scroll(Some(anchor))
                .on_click(cx.listener(move |this, _: &gpui::ClickEvent, window, cx| {
                    start_install(this, source, window, cx);
                })),
        );
    }
    let row = controls::setting_row("Install from", Vec::new(), theme).child(
        div()
            .flex_none()
            .flex()
            .gap(theme.geometry.controls.button_gap)
            .children(buttons),
    );
    let mut rows = vec![row.into_any_element()];
    if let Some(installing) = &this.extensions.installing {
        let input = installing.input.clone();
        let focused = input.focus_handle(cx).is_focused(window);
        let text = input.read(cx).as_str().to_owned();
        let (label, placeholder) = match installing.source {
            InstallSource::Npm => (
                "npm package: its name, and a version to install that one",
                "such as @scope/name or name@1.2.3",
            ),
            _ => (
                "Git repository: its address, and @ a branch, tag or commit to install that one",
                "such as https://github.com/owner/repo@v1.0.0",
            ),
        };
        let well = controls::well(true, theme)
            .w(px(300.))
            .id("extension-install-field")
            .debug_selector(|| "extension-install-field".into())
            .track_focus(&input.focus_handle(cx))
            .shadow(controls::well_shadows(focused, theme))
            .role(Role::TextInput)
            .aria_label(label)
            .aria_value(text.clone())
            .aria_placeholder(placeholder)
            .child(controls::well_input(
                text_input("extension-install-input").state(input.downgrade()),
                placeholder,
                theme,
            ));
        let offered = !text.trim().is_empty();
        let show = controls::button("extension-install-show", "Show Package", offered, theme)
            .debug_selector(|| "extension-install-show".into())
            .role(Role::Button)
            .aria_label("Show Package")
            .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                submit_install(this, cx);
            }));
        let row = controls::setting_row(label, Vec::new(), theme).child(
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap(theme.geometry.controls.button_gap)
                .child(well)
                .child(show),
        );
        rows.push(row.into_any_element());
    }
    controls::section(Some("Install".into()), controls::card(rows, theme), theme)
        .debug_selector(|| "extension-section-Install".into())
        .into_any_element()
}

/// A switch row of these pages: `title` with its switch at its right end,
/// the whole row the switch (a click anywhere takes it), named `id`; its
/// debug selector `selector`, its switch's `<selector>-switch`.
fn switch_row(
    id: SharedString,
    title: &str,
    selector: &str,
    on: bool,
    theme: &Theme,
) -> Stateful<Div> {
    let switch = format!("{selector}-switch");
    let toggle = controls::toggle(on, theme).debug_selector(move || switch);
    let selector = selector.to_owned();
    controls::setting_row(title.to_owned(), Vec::new(), theme)
        .child(toggle)
        .id(id)
        .debug_selector(move || selector)
        .role(Role::Switch)
        .aria_label(title.to_owned())
        .aria_toggled(if on { Toggled::True } else { Toggled::False })
        .cursor_pointer()
}

/// An installed extension's page: its header, its Actions menu, its
/// enable switch and automatic updates, its preferences and its commands.
fn extension_page(
    this: &mut SettingsWindow,
    key: &str,
    theme: &Theme,
    window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> Vec<AnyElement> {
    let Some(package) = this
        .launcher
        .packages()
        .into_iter()
        .find(|package| package.identity.key() == key)
    else {
        return Vec::new();
    };
    let title = package.title();
    let PageOperations {
        enable,
        auto_update,
        menu: operations,
        forget,
        fallbacks,
    } = page_operations(this.launcher.extension_operations(), &package.identity);
    let mark = this.launcher.extension_mark(&package.identity);
    let mut content = vec![header(this, &package, mark.as_ref(), theme)];

    // The enable switch: the launcher's own operation for the extension,
    // so disabling one others require asks first, as there.
    let anchor = this.search_anchor(key);
    let selector = format!("extension-row-{title}");
    let switch_selector = format!("{selector}-switch");
    let toggle = controls::toggle(package.enabled, theme).debug_selector(move || switch_selector);
    let label = div()
        .flex()
        .flex_col()
        .child(controls::field_label("Enabled", theme))
        .child(controls::field_description(
            if package.enabled {
                "Its commands are offered, and its code may run."
            } else {
                "It adds no commands and runs nothing; it keeps its settings."
            },
            theme.text_muted,
            theme,
        ));
    let switch = controls::setting_row_with(label, Vec::new(), theme)
        .child(toggle)
        .id("extension-enable")
        .debug_selector(move || selector)
        .anchor_scroll(Some(anchor))
        .hover(|row| row.bg(theme.nav_hover))
        .role(Role::Switch)
        .aria_label(title.clone())
        .aria_toggled(if package.enabled {
            Toggled::True
        } else {
            Toggled::False
        })
        .cursor_pointer()
        .when_some(enable, |row, operation| {
            row.on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                run(this, &operation, cx);
            }))
        });
    let mut first = vec![switch.into_any_element()];
    if let Some(operation) = auto_update {
        let anchor = this.search_anchor(&operation.id);
        first.push(
            switch_row(
                "extension-auto-update".into(),
                "Update automatically",
                "extension-auto-update",
                operation.on.unwrap_or(false),
                theme,
            )
            .anchor_scroll(Some(anchor))
            .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                run(this, &operation, cx);
            }))
            .into_any_element(),
        );
    }
    content.push(
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap(theme.geometry.settings.section_label_gap)
            .child(actions_menu(this, &package, &operations, theme, cx))
            .child(controls::card(first, theme))
            .into_any_element(),
    );

    // Its preferences (#143).
    if let Some(preferences) = this.launcher.preferences_of(&package.identity) {
        let fields = text_fields(this, &preferences, cx);
        let selects = preference_selects(this, &preferences, window, cx);
        let anchors: HashMap<String, ScrollAnchor> = preference_targets(&preferences)
            .into_iter()
            .map(|target| {
                let anchor = this.search_anchor(&target);
                (target, anchor)
            })
            .collect();
        let drawing = PreferenceControls {
            fields: &fields,
            selects: &selects,
            anchors: &anchors,
        };
        let rows = preference_rows(&preferences, &drawing, theme, cx);
        if !rows.is_empty() {
            content.push(
                controls::section(
                    Some("Preferences".into()),
                    controls::card(rows, theme),
                    theme,
                )
                .debug_selector(|| "extension-preferences".into())
                .into_any_element(),
            );
        }
    }

    // Its commands.
    content.push(commands_section(
        this, &package, &forget, &fallbacks, theme, window, cx,
    ));
    content
}

/// The page's header: the extension's large icon, its title, what it does,
/// where it comes from, and the mark that needs saying, in full.
fn header(
    this: &SettingsWindow,
    package: &InstalledPackage,
    mark: Option<&ExtensionMark>,
    theme: &Theme,
) -> AnyElement {
    let key = package.identity.key();
    let title = package.title();
    let icon = crate::features::icons::row_icon_of(&this.launcher, &key, theme);
    let description = description_of(package).map(|description| {
        controls::field_description(description, theme.text_body, theme)
            .id("extension-page-description")
            .debug_selector(|| "extension-page-description".into())
    });
    let mark = mark.map(|mark| {
        let reason = mark.reason().to_owned();
        controls::field_description(reason.clone(), theme.warning, theme)
            .id("extension-page-mark")
            .debug_selector(|| "extension-page-mark".into())
            .role(Role::Status)
            .aria_label(reason)
    });
    let title_selector = format!("extension-page-title-{title}");
    div()
        .id("extension-page")
        .debug_selector(|| "extension-page".into())
        .w_full()
        .flex()
        .items_center()
        .gap(theme.geometry.controls.row_gap)
        .px(theme.geometry.settings.section_label_inset)
        .child(
            div()
                .id("extension-page-icon")
                .debug_selector(|| "extension-page-icon".into())
                .flex_none()
                .child(row_icon_at(
                    &icon,
                    TileSize::Slot,
                    "extension-page-tile",
                    &format!("extension-page-{title}"),
                    theme,
                )),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(
                    div()
                        .id("extension-page-title")
                        .debug_selector(move || title_selector)
                        .text_size(px(18.))
                        .font_weight(theme.typography.medium)
                        .text_color(theme.text_title)
                        .role(Role::Heading)
                        .aria_label(title.clone())
                        .child(title.clone()),
                )
                .children(description)
                .child(
                    controls::field_description(source_line(package), theme.text_muted, theme)
                        .id("extension-page-source")
                        .debug_selector(|| "extension-page-source".into())
                        .truncate(),
                )
                .children(mark),
        )
        .into_any_element()
}

/// The page's Actions menu: its button, and while it is open, the
/// operations — Check for Update (the page's own), the launcher's
/// operations for the extension, Show Source Folder (the page's own), and
/// Uninstall last.
fn actions_menu(
    this: &SettingsWindow,
    package: &InstalledPackage,
    operations: &[ExtensionOperation],
    theme: &Theme,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let open = this.extensions.menu;
    let button = controls::ghost_button("extension-menu", "Actions…", true, theme)
        .debug_selector(|| "extension-menu".into())
        .role(Role::Button)
        .aria_label(format!("Actions for {}", package.title()))
        .aria_expanded(open)
        .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
            this.extensions.menu = !this.extensions.menu;
            cx.notify();
        }));
    let menu = open.then(|| {
        let identity = package.identity.clone();
        let checkable = package.npm.is_some() || package.git.is_some();
        let mut items: Vec<AnyElement> = Vec::new();
        let entry = |index: usize, label: &str, selector: String, reason: Option<String>| {
            controls::menu_row(
                ("extension-menu-item", index),
                label.to_owned(),
                reason.clone().map(SharedString::from),
                (false, false, reason.is_none()),
                theme,
            )
            .debug_selector(move || selector)
            .role(Role::MenuItem)
            .aria_label(label.to_owned())
            .when(reason.is_some(), |row| row.aria_disabled(true))
        };
        // Check for Update: the package's source again, as its preview.
        let why_not = (!checkable).then(|| {
            if package.identity.default_id().is_some() {
                "Pane updates its default extensions itself".to_owned()
            } else {
                "A folder's extension updates when you reload it".to_owned()
            }
        });
        let for_check = identity.clone();
        items.push(
            entry(
                items.len(),
                "Check for Update",
                "extension-menu-Check for Update".into(),
                why_not,
            )
            .when(checkable, |row| {
                row.on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                    this.extensions.menu = false;
                    check_for_update(this, &for_check, cx);
                }))
            })
            .into_any_element(),
        );
        let (uninstall, others): (Vec<&ExtensionOperation>, Vec<&ExtensionOperation>) = operations
            .iter()
            .partition(|operation| operation.kind == OperationKind::Uninstall);
        for operation in others {
            let chosen = operation.clone();
            items.push(
                entry(
                    items.len(),
                    &operation.label,
                    format!("extension-row-{}", operation.title),
                    operation.unavailable.clone(),
                )
                .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                    this.extensions.menu = false;
                    run(this, &chosen, cx);
                }))
                .into_any_element(),
            );
        }
        items.push(
            entry(
                items.len(),
                "Show Source Folder",
                "extension-menu-Show Source Folder".into(),
                None,
            )
            .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                this.extensions.menu = false;
                show_source_folder(this, &identity, cx);
            }))
            .into_any_element(),
        );
        for operation in uninstall {
            let chosen = operation.clone();
            items.push(
                entry(
                    items.len(),
                    &operation.label,
                    format!("extension-row-{}", operation.title),
                    operation.unavailable.clone(),
                )
                .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                    this.extensions.menu = false;
                    run(this, &chosen, cx);
                }))
                .into_any_element(),
            );
        }
        popover(
            "extension-menu-popup",
            "Extension actions",
            items,
            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                this.extensions.menu = false;
                cx.stop_propagation();
                cx.notify();
            }),
            theme,
            cx,
        )
    });
    div()
        .flex()
        .justify_end()
        .child(div().flex_none().child(button).children(menu))
        .into_any_element()
}

/// The page's Commands section: each command with its icon, its title, its
/// alias field, its hotkey recorder, its fallback switch where it takes a
/// query, and its switch; a command the Shortcuts catalog does not list (a
/// root provider) shows only its switch. Then the choices recorded for a
/// command the extension no longer has, each forgotten by a click.
#[allow(clippy::too_many_arguments)]
fn commands_section(
    this: &mut SettingsWindow,
    package: &InstalledPackage,
    forget: &[ExtensionOperation],
    fallbacks: &[ExtensionOperation],
    theme: &Theme,
    window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let shortcuts: Vec<ShortcutCommand> = this
        .launcher
        .shortcut_catalog()
        .groups
        .into_iter()
        .find(|group| group.identity.as_ref() == Some(&package.identity))
        .map(|group| group.commands)
        .unwrap_or_default();
    let commands = package.listed_commands();
    let count = commands.len();
    // A long Commands section draws only the commands near the page's
    // view, and the one whose alias or hotkey is being edited always
    // (#165).
    let window_rows = this.extensions.commands_window.clone();
    let page = this.search.scroll().clone();
    let active = super::shortcuts::active_commands(this, window);
    let mut rows = Vec::new();
    for listed in commands {
        let id = listed.registration.id.clone();
        let kept = active.contains(&id);
        let row = window_rows.row(count, id, kept, &page, || {
            command_row(this, package, listed, &shortcuts, fallbacks, theme, cx)
        });
        rows.push(row);
    }
    for (index, operation) in forget.iter().enumerate() {
        let chosen = operation.clone();
        let selector = format!("extension-row-{}", operation.title);
        rows.push(
            list_entry(
                ("extension-forget", index),
                None,
                &operation.title,
                Some("Not active; click to forget it".into()),
                theme,
            )
            .debug_selector(move || selector)
            .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                run(this, &chosen, cx);
            }))
            .into_any_element(),
        );
    }
    let body = if rows.is_empty() {
        controls::field_description("It has no commands.", theme.text_muted, theme)
            .px(theme.geometry.settings.section_label_inset)
            .into_any_element()
    } else {
        controls::card(rows, theme).into_any_element()
    };
    controls::section(Some("Commands".into()), body, theme)
        .debug_selector(|| "extension-commands".into())
        .into_any_element()
}

/// One command's row in its extension's Commands section: its icon, its
/// title, its alias field, its hotkey recorder, its fallback switch where
/// it takes a query, and its own switch (a root provider shows only its
/// switch).
fn command_row(
    this: &mut SettingsWindow,
    package: &InstalledPackage,
    listed: pane_core::ListedCommand,
    shortcuts: &[ShortcutCommand],
    fallbacks: &[ExtensionOperation],
    theme: &Theme,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let command = listed.registration;
    let id = command.id.clone();
    let anchor = this.search_anchor(&command_anchor(&id));
    let icon = crate::features::icons::row_icon_of(&this.launcher, &id, theme);
    let mut lines = Vec::new();
    if let Some(subtitle) = &command.subtitle {
        lines.push(
            controls::field_description(subtitle.clone(), theme.text_muted, theme)
                .truncate()
                .into_any_element(),
        );
    }
    // A root provider (#164) has no row in root search, so its page
    // says what it does; its switch turns its results off and on.
    if listed.mode == pane_core::CommandMode::Provider {
        let selector = format!("extension-provider-{}", command.title);
        let line = if package.enabled && listed.enabled {
            "Answers root search as you type, with no row of its own"
        } else {
            "Off: it does not answer root search"
        };
        lines.push(
            controls::field_description(line, theme.text_muted, theme)
                .debug_selector(move || selector)
                .into_any_element(),
        );
    }
    if let Some(why) = &listed.unavailable {
        lines.push(
            controls::field_description(format!("Unavailable: {why}"), theme.warning, theme)
                .into_any_element(),
        );
    }
    let label = div()
        .flex()
        .items_center()
        .gap(theme.geometry.settings.item_gap)
        .child(row_icon_at(
            &icon,
            TileSize::Mini,
            "command-icon",
            &format!("extension-command-{id}"),
            theme,
        ))
        .child(controls::field_label(command.title.clone(), theme));
    let (alias, hotkey) = match shortcuts.iter().find(|shortcut| shortcut.id == id) {
        Some(shortcut) => (
            Some(super::shortcuts::alias_column(this, shortcut, theme, cx)),
            Some(super::shortcuts::hotkey_column(this, shortcut, theme, cx)),
        ),
        None => (None, None),
    };
    // Whether text typed into root search may be sent to it below the
    // results: the launcher's own operation for it.
    let fallback = shortcut_fallback(fallbacks, &id).map(|fallback| {
        let selector = format!("extension-command-fallback-{id}");
        let operation = fallback.clone();
        let on = fallback.on.unwrap_or(false);
        div()
            .flex_none()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(2.))
            .child(controls::caption("Fallback", theme))
            .child(
                controls::toggle(on, theme)
                    .id(SharedString::from(selector.clone()))
                    .debug_selector(move || selector)
                    .role(Role::Switch)
                    .aria_label(format!("Offer {} as a fallback", command.title))
                    .aria_toggled(if on { Toggled::True } else { Toggled::False })
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                        run(this, &operation, cx);
                    })),
            )
    });
    let enabled = listed.enabled;
    let switch_selector = format!("extension-command-toggle-{id}");
    let for_click = id.clone();
    let switch = controls::toggle(enabled, theme)
        .id(SharedString::from(switch_selector.clone()))
        .debug_selector(move || switch_selector)
        .role(Role::Switch)
        .aria_label(format!("{} enabled", command.title))
        .aria_toggled(if enabled {
            Toggled::True
        } else {
            Toggled::False
        })
        .cursor_pointer()
        .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
            switch_command(this, &for_click, !enabled, cx);
        }));
    let selector = format!("extension-command-{id}");
    controls::setting_row_with(label, lines, theme)
        .items_start()
        .py(theme.geometry.controls.row_padding_y)
        .id(SharedString::from(selector.clone()))
        .debug_selector(move || selector)
        .anchor_scroll(Some(anchor))
        .children(alias)
        .children(hotkey)
        .children(fallback)
        .child(div().flex_none().pt(px(4.)).child(switch))
        .into_any_element()
}

/// The fallback switch of the command with id `command`, if it has one.
fn shortcut_fallback<'a>(
    fallbacks: &'a [ExtensionOperation],
    command: &str,
) -> Option<&'a ExtensionOperation> {
    fallbacks
        .iter()
        .find(|fallback| fallback.command.as_deref() == Some(command))
}

/// Every scroll target the preferences of an extension register: each
/// preference, and each command's group of them.
fn preference_targets(preferences: &PackagePreferences) -> Vec<String> {
    let package = preferences.identity.key();
    let mut targets: Vec<String> = preferences
        .fields
        .iter()
        .map(|field| preference_anchor(&package, &field.key))
        .collect();
    for command in &preferences.commands {
        targets.push(command_preferences_anchor(&package, &command.command));
        targets.extend(
            command
                .fields
                .iter()
                .map(|field| preference_anchor(&package, &field.key)),
        );
    }
    targets
}

/// What an extension's preference rows draw with: the text fields and
/// the selects by [`field_key`], and the scroll anchors by target.
struct PreferenceControls<'a> {
    fields: &'a HashMap<String, FieldInput>,
    selects: &'a HashMap<String, Entity<Select>>,
    anchors: &'a HashMap<String, ScrollAnchor>,
}

/// The rows of an extension's preferences: the package's, then each
/// command's under a row naming the command.
fn preference_rows(
    preferences: &PackagePreferences,
    drawing: &PreferenceControls<'_>,
    theme: &Theme,
    cx: &mut Context<SettingsWindow>,
) -> Vec<AnyElement> {
    let package = preferences.identity.key();
    let anchors = drawing.anchors;
    let mut rows: Vec<AnyElement> = preferences
        .fields
        .iter()
        .map(|field| preference_row(&package, field, drawing, theme, cx))
        .collect();
    for command in &preferences.commands {
        let selector = format!("preference-command-{}", command.command);
        let anchor = command_preferences_anchor(&package, &command.command);
        let header = controls::setting_row(command.title.clone(), Vec::new(), theme)
            .id(SharedString::from(anchor.clone()))
            .debug_selector(move || selector)
            .role(Role::Heading)
            .aria_label(command.title.clone())
            .anchor_scroll(anchors.get(&anchor).cloned());
        rows.push(header.into_any_element());
        for field in &command.fields {
            rows.push(preference_row(&package, field, drawing, theme, cx));
        }
    }
    rows
}

/// One preference's row: its title over its description and, while it is
/// required and unset, "Required" in the error tone; its control at the
/// right — a text field (a password's hidden as it is typed), a switch
/// for a checkbox, a select for a dropdown (its value in force on a
/// trigger that opens the options with a search field, the Settings
/// pages' own select, [`preference_selects`]), and a text field with
/// "Choose…" for a file, folder or application. Each change is saved as
/// it is made ([`pane_core::Launcher::set_preference`]) and applies
/// without a restart.
fn preference_row(
    package: &str,
    field: &PreferenceField,
    drawing: &PreferenceControls<'_>,
    theme: &Theme,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let (fields, anchors) = (drawing.fields, drawing.anchors);
    let preference = &field.preference;
    let key = field.key.clone();
    let mut lines = Vec::new();
    if let Some(description) = &preference.description {
        lines.push(controls::row_line(
            description.clone(),
            theme.text_muted,
            theme,
        ));
    }
    if field.missing {
        let selector = format!("preference-error-{key}");
        lines.push(
            controls::field_description("Required", theme.danger, theme)
                .debug_selector(move || selector)
                .into_any_element(),
        );
    }
    // What the control shows: the value set, else the default.
    let effective = field.value.clone().or_else(|| preference.default.clone());
    let control: AnyElement = match preference.kind {
        PreferenceKind::Checkbox => {
            let on = effective.as_deref() == Some("true");
            let selector = format!("preference-toggle-{key}");
            let label = preference
                .label
                .clone()
                .unwrap_or_else(|| preference.title.clone());
            let (owner, saved) = (package.to_owned(), key.clone());
            controls::toggle(on, theme)
                .id(SharedString::from(selector.clone()))
                .debug_selector(move || selector)
                .role(Role::Switch)
                .aria_label(label)
                .aria_toggled(if on { Toggled::True } else { Toggled::False })
                .cursor_pointer()
                .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                    save_preference(this, &owner, &saved, &(!on).to_string(), cx);
                }))
                .into_any_element()
        }
        PreferenceKind::Dropdown => match drawing.selects.get(&field_key(package, &key)) {
            Some(select) => div().flex_none().child(select.clone()).into_any_element(),
            None => div().into_any_element(),
        },
        PreferenceKind::Text
        | PreferenceKind::Password
        | PreferenceKind::File
        | PreferenceKind::Folder
        | PreferenceKind::Application
        | PreferenceKind::Applications => {
            let well = fields
                .get(&field_key(package, &key))
                .map(|input| text_well(field, input, theme));
            let choose = matches!(
                preference.kind,
                PreferenceKind::File
                    | PreferenceKind::Folder
                    | PreferenceKind::Application
                    | PreferenceKind::Applications
            )
            .then(|| {
                let selector = format!("preference-choose-{key}");
                let (owner, saved, kind) = (package.to_owned(), key.clone(), preference.kind);
                // A list's picker adds an application to it.
                let label = if preference.kind == PreferenceKind::Applications {
                    "Add…"
                } else {
                    "Choose…"
                };
                controls::ghost_button(SharedString::from(selector.clone()), label, true, theme)
                    .debug_selector(move || selector)
                    .role(Role::Button)
                    .aria_label(format!("Choose {}", preference.title))
                    .on_click(cx.listener(move |this, _: &gpui::ClickEvent, window, cx| {
                        choose_path(this, &owner, &saved, kind, window, cx);
                    }))
            });
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap(theme.geometry.controls.button_gap)
                .children(well)
                .children(choose)
                .into_any_element()
        }
    };
    let selector = format!("preference-{key}");
    controls::setting_row(preference.title.clone(), lines, theme)
        .child(control)
        .id(SharedString::from(format!(
            "preference-row-{package}-{key}"
        )))
        .debug_selector(move || selector)
        .anchor_scroll(
            anchors
                .get(&preference_anchor(package, &field.key))
                .cloned(),
        )
        .into_any_element()
}

/// A text preference's well: its editable text, a password's drawn as
/// dots (the editable glyphs are transparent under them), in the error
/// state's ring while it is required and unset.
fn text_well(field: &PreferenceField, input: &FieldInput, theme: &Theme) -> Stateful<Div> {
    let preference = &field.preference;
    let secret = preference.kind.is_secret();
    let key = &field.key;
    let selector = format!("preference-field-{key}");
    let ring = controls::well_shadows(true, theme);
    let shown = if secret {
        "\u{2022}".repeat(input.text.chars().count())
    } else {
        input.text.clone()
    };
    let placeholder = preference.placeholder.clone().unwrap_or_default();
    let editable = controls::well_input(
        text_input(SharedString::from(format!("preference-input-{key}")))
            .state(input.input.downgrade()),
        placeholder.clone(),
        theme,
    );
    let well = controls::well(true, theme)
        .w(px(240.))
        .id(SharedString::from(selector.clone()))
        .debug_selector(move || selector)
        .track_focus(&input.focus)
        .role(Role::TextInput)
        .aria_label(preference.title.clone())
        .aria_value(shown.clone())
        .aria_placeholder(placeholder)
        .when(field.missing, |well| {
            well.aria_description("Required")
                .shadow(controls::error_ring(theme))
        })
        .focus(move |well| well.shadow(ring));
    if !secret {
        return well.child(editable);
    }
    well.child(
        div()
            .relative()
            .flex_1()
            .min_w(px(0.))
            .child(editable.text_color(gpui::transparent_black()))
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .flex()
                    .items_center()
                    .text_size(theme.typography.settings_text_size)
                    .text_color(theme.text_title)
                    .child(shown),
            ),
    )
}

/// The text fields of the text, password, file, folder and application
/// preferences of `preferences`, by [`field_key`]: each created the first
/// time its preference draws, holding the value set then, and saving each
/// change as it is made.
fn text_fields(
    this: &mut SettingsWindow,
    preferences: &PackagePreferences,
    cx: &mut Context<SettingsWindow>,
) -> HashMap<String, FieldInput> {
    let mut drawn = HashMap::new();
    let package = preferences.identity.key();
    let all = preferences.fields.iter().chain(
        preferences
            .commands
            .iter()
            .flat_map(|command| &command.fields),
    );
    for field in all {
        if matches!(
            field.preference.kind,
            PreferenceKind::Checkbox | PreferenceKind::Dropdown
        ) {
            continue;
        }
        let id = field_key(&package, &field.key);
        let input = match this.extensions.fields.get(&id) {
            Some((input, _)) => input.clone(),
            None => {
                let input = cx.new(|cx| EditableTextState::new(StringStorage::default(), cx));
                input.focus_handle(cx).tab_stop(true);
                if let Some(value) = field.value.as_deref().filter(|value| !value.is_empty()) {
                    input.update(cx, |input, cx| input.emplace(value, cx));
                }
                let (owner, key) = (package.clone(), field.key.clone());
                let changes = cx.subscribe(&input, move |this, input, _: &TextChanged, cx| {
                    let text = input.read(cx).as_str().to_owned();
                    save_preference(this, &owner, &key, &text, cx);
                });
                this.extensions
                    .fields
                    .insert(id.clone(), (input.clone(), changes));
                input
            }
        };
        let focus = input.focus_handle(cx);
        let text = input.read(cx).as_str().to_owned();
        drawn.insert(id, FieldInput { input, focus, text });
    }
    drawn
}

/// The selects of the dropdown preferences of `preferences`, by
/// [`field_key`]: each created the first time its preference draws (made
/// again when a reload changed its title or description), and told the
/// options and the value in force (the value set, else the default) each
/// time it draws. A choice is saved as it is made, as every preference's
/// change is ([`save_preference`]).
///
/// Its debug selectors are the select's own under
/// `preference-select-<key>`: the trigger is `preference-select-<key>`,
/// an option's row `preference-select-<key>-<value>`, the popup's search
/// field `preference-select-<key>-query`.
fn preference_selects(
    this: &mut SettingsWindow,
    preferences: &PackagePreferences,
    window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> HashMap<String, Entity<Select>> {
    let mut drawn = HashMap::new();
    let package = preferences.identity.key();
    let all = preferences.fields.iter().chain(
        preferences
            .commands
            .iter()
            .flat_map(|command| &command.fields),
    );
    for field in all {
        let preference = &field.preference;
        if preference.kind != PreferenceKind::Dropdown {
            continue;
        }
        let id = field_key(&package, &field.key);
        let labels = SelectLabels {
            title: preference.title.clone(),
            description: preference.description.clone().unwrap_or_default(),
        };
        let fresh = this
            .extensions
            .selects
            .get(&id)
            .is_none_or(|kept| kept.labels != labels);
        if fresh {
            let live = Rc::new(RefCell::new(SelectLive::default()));
            let read = live.clone();
            let settings = cx.entity().downgrade();
            let (owner, key) = (package.clone(), field.key.clone());
            let SelectLabels {
                title: name,
                description,
            } = labels.clone();
            let debug = format!("preference-select-{}", field.key);
            let select = cx.new(|cx| {
                Select::new(
                    name,
                    description,
                    debug,
                    Rc::new(move |cx: &App| {
                        let visuals = crate::settings::visuals(cx);
                        let live = read.borrow();
                        Model {
                            theme: visuals.theme,
                            material: visuals.material,
                            choices: live.choices.clone(),
                            committed: live.committed.clone(),
                        }
                    }),
                    Rc::new(move |value: &str, _: &mut Window, cx: &mut App| {
                        settings
                            .update(cx, |this, cx| {
                                save_preference(this, &owner, &key, value, cx);
                            })
                            .ok();
                    }),
                    window,
                    cx,
                )
            });
            this.extensions.selects.insert(
                id.clone(),
                PreferenceSelect {
                    select,
                    labels,
                    live,
                },
            );
        }
        let kept = &this.extensions.selects[&id];
        let effective = field.value.clone().or_else(|| preference.default.clone());
        *kept.live.borrow_mut() = SelectLive {
            choices: preference
                .options
                .iter()
                .map(|option| Choice {
                    id: option.value.clone().into(),
                    label: option.title.clone().into(),
                    subtitle: None,
                    keywords: vec![option.value.clone().into()],
                    unavailable_reason: None,
                })
                .collect(),
            committed: effective
                .filter(|value| {
                    preference
                        .options
                        .iter()
                        .any(|option| &option.value == value)
                })
                .map(SharedString::from),
        };
        drawn.insert(id, kept.select.clone());
    }
    drawn
}

// ------------------------------------------------------------ the behavior

/// Saves `value` as the preference kept as `key` of the extension whose
/// identity key is `package`, as the user changed it on its page; the
/// launcher window redraws (a row may no longer need setup), and a value
/// that cannot be saved says why on the page.
fn save_preference(
    this: &mut SettingsWindow,
    package: &str,
    key: &str,
    value: &str,
    cx: &mut Context<SettingsWindow>,
) {
    let Some(identity) = identity_of(&this.launcher, package) else {
        cx.notify();
        return;
    };
    let saved = this.launcher.set_preference(&identity, key, Some(value));
    launcher_changed_outside(cx);
    cx.notify();
    cx.spawn(async move |this, cx| {
        let problem = saved.await.err();
        this.update(cx, |this, cx| {
            this.extensions.problem = problem;
            cx.notify();
        })
        .ok();
        cx.update(launcher_changed_outside);
    })
    .detach();
}

/// Asks for the path of the file, folder or application (`kind`)
/// preference kept as `key` of the extension whose identity key is
/// `package`, and sets its field to it, which saves it.
fn choose_path(
    _this: &mut SettingsWindow,
    package: &str,
    key: &str,
    kind: PreferenceKind,
    window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) {
    let pick = match kind {
        PreferenceKind::Folder => PathKind::Folder,
        PreferenceKind::Application | PreferenceKind::Applications => PathKind::Application,
        _ => PathKind::File,
    };
    let picked = cx.prompt_for_paths(path_prompt(pick));
    let (package, key) = (package.to_owned(), key.to_owned());
    cx.spawn_in(window, async move |this, cx| {
        let Ok(Ok(Some(paths))) = picked.await else {
            return;
        };
        let Some(path) = paths.into_iter().next() else {
            return;
        };
        let path = path.to_string_lossy().into_owned();
        this.update(cx, |this, cx| {
            let field = this
                .extensions
                .fields
                .get(&field_key(&package, &key))
                .map(|(input, _)| input.clone());
            // A list of applications gains the application's file name;
            // any other path preference becomes the path.
            let value = if kind == PreferenceKind::Applications {
                let file = std::path::Path::new(&path)
                    .file_name()
                    .map_or_else(|| path.clone(), |name| name.to_string_lossy().into_owned());
                let listed = field
                    .as_ref()
                    .map(|input| input.read(cx).as_str().trim().to_owned())
                    .unwrap_or_default();
                appended(&listed, &file)
            } else {
                path
            };
            if let Some(input) = field {
                input.update(cx, |input, cx| input.emplace(&value, cx));
            }
            save_preference(this, &package, &key, &value, cx);
        })
        .ok();
    })
    .detach();
}

/// The list of applications `listed` (file names, separated by commas)
/// with `file` added at its end, unless it names it already (ignoring
/// case).
fn appended(listed: &str, file: &str) -> String {
    let names: Vec<&str> = listed
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .collect();
    if names.iter().any(|name| name.eq_ignore_ascii_case(file)) {
        return names.join(", ");
    }
    names
        .into_iter()
        .chain(std::iter::once(file))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The system's picker for a path of `kind`, as a file, folder or
/// application preference's "Choose…" opens it, here and on the launcher's
/// Setup screen.
pub(crate) fn path_prompt(kind: PathKind) -> PathPromptOptions {
    PathPromptOptions {
        files: kind != PathKind::Folder,
        // A macOS application is a folder (its bundle).
        directories: kind == PathKind::Folder
            || (kind == PathKind::Application && cfg!(target_os = "macos")),
        multiple: false,
        prompt: Some("Choose".into()),
    }
}

/// Why the last build of the extension whose page shows failed, while it
/// is developed and its last build did: "Hello did not build: <its first
/// error>".
fn build_failure(this: &SettingsWindow) -> Option<String> {
    let key = this.extension.as_deref()?;
    let package = this
        .launcher
        .packages()
        .into_iter()
        .find(|package| package.identity.key() == key)?;
    let failure = this.launcher.development(&package.identity)?.failure?;
    Some(format!(
        "{} did not build: {}",
        package.title(),
        failure.summary
    ))
}

/// The installed package whose identity key is `key`.
fn identity_of(launcher: &Launcher, key: &str) -> Option<PackageIdentity> {
    launcher
        .packages()
        .into_iter()
        .map(|package| package.identity)
        .find(|identity| identity.key() == key)
}

/// Waits for `pending`, an operation the pages started, redrawing both
/// windows now and when it lands, and keeps what it came to as the page's
/// status once the launcher is off the screens an operation opens (an
/// install lands on root search, with its outcome there).
fn keep_outcome_of(pending: impl Future<Output = ()> + 'static, cx: &mut Context<SettingsWindow>) {
    launcher_changed_outside(cx);
    cx.notify();
    cx.spawn(async move |this, cx| {
        pending.await;
        cx.update(launcher_changed_outside);
        this.update(cx, |this, cx| {
            let view = this.launcher.view();
            if !in_extension_flow(&view.screen) && !matches!(view.status, Status::Idle) {
                this.extensions.outcome = Some(view.status);
            }
            cx.notify();
        })
        .ok();
    })
    .detach();
}

/// Runs `operation` through the launcher, which stays on the screen the
/// user had ([`Launcher::run_extension_operation`]); the confirmation or
/// details screen it asks for, if it asks, shows on the page, drawn from
/// the launcher's view.
fn run(
    this: &mut SettingsWindow,
    operation: &ExtensionOperation,
    cx: &mut Context<SettingsWindow>,
) {
    this.extensions.outcome = None;
    let pending = this.launcher.run_extension_operation(operation);
    keep_outcome_of(pending, cx);
}

/// Answers the screen an operation opened by its row with `id` (a
/// confirmation's answer, a preview's Install, a details screen's Retry),
/// activated as the launcher window's Enter does.
fn answer(this: &mut SettingsWindow, id: &str, cx: &mut Context<SettingsWindow>) {
    this.extensions.outcome = None;
    let launcher = &this.launcher;
    let Some(index) = launcher.view().rows.iter().position(|row| row.id == id) else {
        // The row left the screen between the frame that drew it and this
        // click (a background change): the page redraws with what the
        // launcher holds now, and nothing is activated.
        cx.notify();
        return;
    };
    launcher.select(index);
    let pending = launcher.activate_selected();
    keep_outcome_of(pending, cx);
}

/// Keeps what an operation that answers for itself came to as the page's
/// status, and redraws both windows.
fn answered(
    pending: impl Future<Output = Result<String, String>> + 'static,
    cx: &mut Context<SettingsWindow>,
) {
    launcher_changed_outside(cx);
    cx.notify();
    cx.spawn(async move |this, cx| {
        let outcome = pending.await;
        this.update(cx, |this, cx| {
            this.extensions.outcome = Some(match outcome {
                Ok(done) => Status::Result(done),
                Err(why) => Status::Error(why),
            });
            cx.notify();
        })
        .ok();
        cx.update(launcher_changed_outside);
    })
    .detach();
}

/// Turns the command with id `command` on or off, as its switch on the page
/// does; what it came to is the page's status.
fn switch_command(
    this: &mut SettingsWindow,
    command: &str,
    enabled: bool,
    cx: &mut Context<SettingsWindow>,
) {
    let pending = this.launcher.set_command_enabled(command, enabled);
    answered(pending, cx);
}

/// Checks the extension with `identity` for an update: its source's
/// preview, drawn on the page.
fn check_for_update(
    this: &mut SettingsWindow,
    identity: &PackageIdentity,
    cx: &mut Context<SettingsWindow>,
) {
    this.extensions.outcome = None;
    if let Some(pending) = this.launcher.check_for_update(identity) {
        keep_outcome_of(pending, cx);
    }
}

/// Shows the extension's folder in the system's file manager; what it
/// came to is the page's status.
fn show_source_folder(
    this: &mut SettingsWindow,
    identity: &PackageIdentity,
    cx: &mut Context<SettingsWindow>,
) {
    let pending = this.launcher.show_source_folder(identity);
    answered(pending, cx);
}

/// Opens the group's page and starts installing from `source`: the + menu.
fn open_install(
    this: &mut SettingsWindow,
    source: InstallSource,
    window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) {
    this.show_extension(None, cx);
    start_install(this, source, window, cx);
}

/// Starts installing from `source` on the group's page: the system's
/// folder picker, whose folder is previewed; or the field for an npm
/// package or a Git repository, which takes the keyboard. Whether a field
/// took it.
fn start_install(
    this: &mut SettingsWindow,
    source: InstallSource,
    window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> bool {
    this.extensions.outcome = None;
    match source {
        InstallSource::Folder => {
            this.extensions.installing = None;
            let picked = cx.prompt_for_paths(PathPromptOptions {
                files: false,
                directories: true,
                multiple: false,
                prompt: Some("Install".into()),
            });
            cx.spawn_in(window, async move |this, cx| {
                let folder = match picked.await {
                    Ok(Ok(Some(paths))) => paths.into_iter().next(),
                    Ok(Ok(None)) | Err(_) => None,
                    Ok(Err(error)) => {
                        this.update(cx, |this, cx| {
                            this.extensions.outcome = Some(Status::Error(format!(
                                "Could not open a folder picker: {error:#}"
                            )));
                            cx.notify();
                        })
                        .ok();
                        None
                    }
                };
                if let Some(folder) = folder {
                    this.update(cx, |this, cx| {
                        let pending = this.launcher.preview_package(&folder);
                        keep_outcome_of(pending, cx);
                    })
                    .ok();
                }
            })
            .detach();
            cx.notify();
            false
        }
        InstallSource::Npm | InstallSource::Git => {
            let input = match this.extensions.installing.take() {
                Some(installing) => installing.input,
                None => {
                    let input = cx.new(|cx| EditableTextState::new(StringStorage::default(), cx));
                    input.focus_handle(cx).tab_stop(true);
                    // The button that shows the package follows the text.
                    cx.subscribe(&input, |_, _, _: &TextChanged, cx| cx.notify())
                        .detach();
                    input
                }
            };
            window.focus(&input.focus_handle(cx), cx);
            this.extensions.installing = Some(Installing { source, input });
            cx.notify();
            true
        }
    }
}

/// Shows the npm package or Git repository the field names: the
/// launcher's preview, drawn on the page, whose Install row installs it.
fn submit_install(this: &mut SettingsWindow, cx: &mut Context<SettingsWindow>) {
    let Some(installing) = &this.extensions.installing else {
        return;
    };
    let spec = installing.input.read(cx).as_str().trim().to_owned();
    if spec.is_empty() {
        return;
    }
    let pending: Pin<Box<dyn Future<Output = ()>>> = match installing.source {
        InstallSource::Npm => Box::pin(this.launcher.preview_npm(&spec)),
        InstallSource::Git | InstallSource::Folder => Box::pin(this.launcher.preview_git(&spec)),
    };
    this.extensions.installing = None;
    this.extensions.outcome = None;
    keep_outcome_of(pending, cx);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An applications list's picker adds the application once (#166).
    #[test]
    fn an_application_is_added_to_a_list_once() {
        assert_eq!(appended("", "KeePass.exe"), "KeePass.exe");
        assert_eq!(
            appended("KeePass.exe, ", "1Password.exe"),
            "KeePass.exe, 1Password.exe"
        );
        assert_eq!(
            appended("KeePass.exe,1Password.exe", "keepass.EXE"),
            "KeePass.exe, 1Password.exe"
        );
    }
}
