//! The File Search page (#176, #126 "The File search page"): what file
//! search indexes and how it is doing, in Pane's own controls, kept in
//! Pane's own record (`file-search.json`, not extension data).
//!
//! **Status.** Whether the index is building (and how many entries it has
//! found so far), up to date, stopped (and why) or off; how many entries it
//! holds; when and how it was last caught up — from the change journal,
//! the event history, by re-reading the folders that changed, or by
//! indexing every folder. Rebuild Index deletes it and builds it again.
//! While no enabled extension uses the index, the page says file search is
//! off and why: which extension is turned off, paused, or has its commands
//! turned off, or that none is installed.
//!
//! **What is indexed.** The roots — the home folder, and the folders the
//! user added, each with Remove — and Add Folder…; the excluded folders and
//! patterns (in `.gitignore` syntax), each with Remove, Exclude Folder…
//! and a field for a pattern; the switches for hidden entries, ignore
//! files, the default exclusions (caches, temporary folders,
//! `node_modules`, `AppData` or `~/Library`) and network and removable
//! volumes. Every control goes through
//! [`pane_core::Launcher::set_file_search_rules`], which records the rules
//! and applies them without a restart.
//!
//! **Needs attention.** What [`pane_core::Launcher::file_search_problems`]
//! lists: folders that could not be read, that macOS refused, that are not
//! watched (Linux), that churned (with Include Again) or that did not
//! answer; a walk stopped at the ceiling of entries; a stop for want of
//! free space — each with its reason and remedy.
//!
//! The page reads the launcher every frame, and the window's watcher
//! redraws it when the index's status, problems or rules change while it
//! shows ([`SettingsWindow::file_search_watched`]).

use std::future::Future;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use gpui::{
    AnyElement, App, Context, Entity, Focusable, PathPromptOptions, Role, SharedString, Window,
    div, prelude::*, px,
};
use gpui_elements::editable_text::{EditableTextState, StringStorage, TextChanged, text_input};
use pane_core::Launcher;
use pane_core::file_index::{
    CaughtUpBy, IndexState, IndexStatus, Problem, ProblemKind, ScopeRules, UserRules, count_words,
};

use super::{Page, SettingsWindow, search};
use crate::ui::controls::{self, status_note as note};
use crate::ui::icon::Glyph;
use crate::ui::theme::Theme;
use crate::ui::virtual_list::PageWindow;

/// The page's title: its sidebar entry, and its page's.
pub(crate) const TITLE: &str = "File Search";

/// What the page is, in one line: its sidebar entry's description in the
/// search.
pub(crate) const ABOUT: &str = "What file search indexes, and how the index is doing";

/// The page's debug selector, and its root element's id.
pub(crate) const SELECTOR: &str = "file-search";

/// The controls the sidebar's search finds, by their scroll anchors.
const REBUILD: &str = "file-search-rebuild";
const ADD_ROOT: &str = "file-search-add-root";
const EXCLUDE_FOLDER: &str = "file-search-exclude-folder";
const PATTERN_FIELD: &str = "file-search-pattern-field";

/// The switches: id and selector, title, and which rule each changes.
const SWITCHES: [(&str, &str, Rule); 4] = [
    (
        "file-search-hidden",
        "Include hidden files and folders",
        Rule::Hidden,
    ),
    (
        "file-search-ignore-files",
        "Leave out what .gitignore and other ignore files exclude",
        Rule::IgnoreFiles,
    ),
    (
        "file-search-default-exclusions",
        "Leave out caches, temporary folders and node_modules",
        Rule::DefaultExclusions,
    ),
    (
        "file-search-other-volumes",
        "Include network and removable drives",
        Rule::OtherVolumes,
    ),
];

/// A switch's rule.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Rule {
    Hidden,
    IgnoreFiles,
    DefaultExclusions,
    OtherVolumes,
}

impl Rule {
    fn get(self, rules: &UserRules) -> bool {
        match self {
            Rule::Hidden => rules.include_hidden,
            Rule::IgnoreFiles => rules.use_ignore_files,
            Rule::DefaultExclusions => rules.default_exclusions,
            Rule::OtherVolumes => rules.include_other_volumes,
        }
    }

    fn set(self, rules: &mut UserRules, on: bool) {
        match self {
            Rule::Hidden => rules.include_hidden = on,
            Rule::IgnoreFiles => rules.use_ignore_files = on,
            Rule::DefaultExclusions => rules.default_exclusions = on,
            Rule::OtherVolumes => rules.include_other_volumes = on,
        }
    }
}

/// The File Search page, registered before About in the window's page
/// list.
pub(crate) fn page() -> Page {
    Page {
        title: TITLE,
        about: ABOUT,
        icon: Glyph::Folder,
        count: None,
        render,
        search: entries,
        focus,
    }
}

/// The page's own state, held by the window as a field.
#[derive(Default)]
pub(crate) struct State {
    /// The field for a pattern to exclude, created when it first draws.
    pattern: Option<Entity<EditableTextState>>,
    /// A change the page started that has not landed yet.
    busy: bool,
    /// What the last change the page started came to, if it failed.
    problem: Option<String>,
    /// What the page last drew of the index, for the watcher.
    drawn: Option<Drawn>,
    /// The page's long lists (roots, exclusions, problems), drawn only
    /// near the page's view (#165).
    rows_window: PageWindow,
}

/// What the page draws of the index: when it differs from the launcher's
/// now, the watcher redraws the page.
#[derive(Clone, PartialEq, Eq)]
struct Drawn {
    status: IndexStatus,
    problems: Vec<Problem>,
    rules: Option<UserRules>,
}

fn drawn_now(launcher: &Launcher) -> Drawn {
    Drawn {
        status: launcher.file_index_status(),
        problems: launcher.file_search_problems(),
        rules: launcher.file_search_rules().map(|(_, rules)| rules),
    }
}

impl SettingsWindow {
    /// What the window's watcher does for this page: while it shows, a
    /// change of the index's status, problems or rules (a walk progressing,
    /// a folder taken out for churn, a stop for space) redraws it.
    pub(crate) fn file_search_watched(&mut self, cx: &mut Context<Self>) {
        if self.pages[self.selected].title != TITLE {
            return;
        }
        if self.file_search.drawn.as_ref() != Some(&drawn_now(&self.launcher)) {
            cx.notify();
        }
    }
}

/// What the sidebar's search finds on the page: Rebuild Index, Add Folder,
/// Exclude Folder, the pattern field and the switches, while this Pane
/// keeps a file index.
fn entries(launcher: &Launcher, _cx: &App) -> Vec<search::Entry> {
    if launcher.file_search_rules().is_none() {
        return Vec::new();
    }
    let entry = |control: &str, title: &str, group: &str| search::Entry {
        control: Some(control.into()),
        title: title.into(),
        group: Some(group.into()),
        unavailable: None,
    };
    let mut found = vec![
        entry(REBUILD, "Rebuild Index", "File Search"),
        entry(ADD_ROOT, "Add Folder to Index…", "Indexed folders"),
        entry(EXCLUDE_FOLDER, "Exclude Folder…", "Excluded"),
        entry(PATTERN_FIELD, "Exclude Pattern", "Excluded"),
    ];
    found.extend(
        SWITCHES
            .iter()
            .map(|&(control, title, _)| entry(control, title, "Rules")),
    );
    found
}

/// A jump to `target`: the pattern field takes the keyboard; the other
/// controls are revealed, the sidebar keeping the focus.
fn focus(
    this: &mut SettingsWindow,
    target: &str,
    window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> bool {
    if target != PATTERN_FIELD {
        return false;
    }
    let input = pattern_input(this, cx);
    window.focus(&input.focus_handle(cx), cx);
    true
}

/// The pattern field's state, created the first time it is wanted.
fn pattern_input(
    this: &mut SettingsWindow,
    cx: &mut Context<SettingsWindow>,
) -> Entity<EditableTextState> {
    if let Some(input) = &this.file_search.pattern {
        return input.clone();
    }
    let input = cx.new(|cx| EditableTextState::new(StringStorage::default(), cx));
    input.focus_handle(cx).tab_stop(true);
    // The Exclude button follows the text.
    cx.subscribe(&input, |_, _, _: &TextChanged, cx| cx.notify())
        .detach();
    this.file_search.pattern = Some(input.clone());
    input
}

/// `path` for people: below the home folder `home` as `~/…`, elsewhere in
/// full.
fn shown(path: &Path, home: Option<&Path>) -> String {
    match home.and_then(|home| path.strip_prefix(home).ok()) {
        Some(below) if below.as_os_str().is_empty() => "~".into(),
        Some(below) => {
            let parts: Vec<String> = below
                .components()
                .map(|part| part.as_os_str().to_string_lossy().into_owned())
                .collect();
            format!("~/{}", parts.join("/"))
        }
        None => path.display().to_string(),
    }
}

/// How long ago `then` was, for people: "just now", "5 minutes ago".
fn ago(then: SystemTime) -> String {
    let Ok(elapsed) = SystemTime::now().duration_since(then) else {
        return "just now".into();
    };
    let seconds = elapsed.as_secs();
    let plural = |count: u64, unit: &str| {
        if count == 1 {
            format!("1 {unit} ago")
        } else {
            format!("{count} {unit}s ago")
        }
    };
    match seconds {
        0..60 => "just now".into(),
        60..3600 => plural(seconds / 60, "minute"),
        3600..86_400 => plural(seconds / 3600, "hour"),
        _ => plural(seconds / 86_400, "day"),
    }
}

/// The status line: the index's state, in words.
fn state_words(status: &IndexStatus) -> String {
    match status.state {
        IndexState::Off => "Off".into(),
        _ if status.resuming => {
            "Paused: the computer slept; indexing resumes in a few seconds".into()
        }
        IndexState::Building if status.waiting => {
            "Waiting: indexing starts once the launcher is first shown".into()
        }
        IndexState::Building if status.found > 0 => {
            format!("Indexing… {} found so far", count_words(status.found))
        }
        IndexState::Building => "Indexing…".into(),
        IndexState::Current => "Up to date".into(),
        IndexState::Stopped => match &status.reason {
            Some(reason) => format!("Stopped: {reason}"),
            None => "Stopped".into(),
        },
    }
}

/// How it was last caught up, and when.
fn caught_up_words(caught_up: Option<(CaughtUpBy, SystemTime)>) -> Option<String> {
    let (how, when) = caught_up?;
    Some(format!("Last caught up {}, {}", how.describe(), ago(when)))
}

/// A problem's kind, as its row is titled when it names no folder.
fn kind_title(kind: ProblemKind) -> &'static str {
    match kind {
        ProblemKind::Unreadable => "Folders that could not be read",
        ProblemKind::Refused => "Folders macOS did not allow",
        ProblemKind::Unwatched => "Folders not watched",
        ProblemKind::Churned => "A folder that changes constantly",
        ProblemKind::Hung => "Folders that did not answer",
        ProblemKind::Ceiling => "Too many entries",
        ProblemKind::LowSpace => "Not enough free space",
    }
}

/// The kind's word in a selector.
fn kind_name(kind: ProblemKind) -> &'static str {
    match kind {
        ProblemKind::Unreadable => "Unreadable",
        ProblemKind::Refused => "Refused",
        ProblemKind::Unwatched => "Unwatched",
        ProblemKind::Churned => "Churned",
        ProblemKind::Hung => "Hung",
        ProblemKind::Ceiling => "Ceiling",
        ProblemKind::LowSpace => "LowSpace",
    }
}

/// A folder's last name, for selectors.
fn last_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// A button of the page: `label`, named `selector`, read as `label` by
/// assistive technology, offered unless a change is in flight.
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

/// Draws the page (see the module docs).
fn render(
    this: &mut SettingsWindow,
    window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let visuals = crate::settings::visuals(cx);
    let theme = &visuals.theme;
    let drawn = drawn_now(&this.launcher);
    this.file_search.drawn = Some(drawn.clone());
    let page = controls::page(theme);
    let Some((effective, rules)) = this.launcher.file_search_rules() else {
        return div()
            .id(SELECTOR)
            .debug_selector(|| SELECTOR.into())
            .child(
                page.child(
                    note(
                        "file-search-unavailable",
                        "This Pane keeps no file index, so file search is not available",
                        theme.text_muted,
                        theme,
                    )
                    .px(theme.geometry.settings.section_label_inset),
                ),
            )
            .into_any_element();
    };
    let status = drawn.status;
    let busy = this.file_search.busy;
    let home = effective.home.clone();
    let mut sections: Vec<AnyElement> = Vec::new();
    if let Some(problem) = &this.file_search.problem {
        sections.push(
            note("file-search-status", problem.clone(), theme.danger, theme)
                .px(theme.geometry.settings.section_label_inset)
                .into_any_element(),
        );
    }
    sections.push(status_section(this, &status, busy, theme, cx));
    sections.push(roots_section(this, &effective, &rules, busy, theme, cx));
    sections.push(excluded_section(
        this,
        &rules,
        home.as_deref(),
        busy,
        theme,
        window,
        cx,
    ));
    sections.push(rules_section(&rules, busy, theme, cx));
    if !drawn.problems.is_empty() {
        sections.push(problems_section(
            this,
            &drawn.problems,
            home.as_deref(),
            busy,
            theme,
            cx,
        ));
    }
    div()
        .id(SELECTOR)
        .debug_selector(|| SELECTOR.into())
        .child(page.children(sections))
        .into_any_element()
}

/// The Status card: the state, the entries, when it last caught up, why it
/// is off, and Rebuild Index.
fn status_section(
    this: &mut SettingsWindow,
    status: &IndexStatus,
    busy: bool,
    theme: &Theme,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let tone = match status.state {
        IndexState::Stopped => theme.warning,
        _ => theme.text_muted,
    };
    let mut lines =
        vec![note("file-search-state", state_words(status), tone, theme).into_any_element()];
    if status.state != IndexState::Off || status.entries > 0 {
        lines.push(
            note(
                "file-search-entries",
                format!("{} files and folders indexed", count_words(status.entries)),
                theme.text_muted,
                theme,
            )
            .into_any_element(),
        );
    }
    if let Some(caught_up) = caught_up_words(status.caught_up) {
        lines.push(
            note("file-search-caught-up", caught_up, theme.text_muted, theme).into_any_element(),
        );
    }
    if cfg!(target_os = "macos") && status.caught_up.is_none() && status.state != IndexState::Off {
        lines.push(
            note(
                "file-search-macos-prompts",
                "macOS will ask whether Pane may read your Desktop, Documents and Downloads \
                 folders; Pane indexes only the ones you allow",
                theme.text_muted,
                theme,
            )
            .into_any_element(),
        );
    }
    let mut rows = Vec::new();
    let rebuild_anchor = this.search_anchor(REBUILD);
    let rebuild = button(REBUILD.into(), "Rebuild Index", !busy, theme)
        .anchor_scroll(Some(rebuild_anchor))
        .when(!busy, |button| {
            button.on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                let pending = this.launcher.rebuild_file_index();
                apply_and_report(this, pending, cx);
            }))
        });
    rows.push(
        controls::setting_row("File search", lines, theme)
            .child(rebuild)
            .into_any_element(),
    );
    if status.state == IndexState::Off {
        rows.push(off_row(this, status, theme).into_any_element());
    }
    controls::section(None, controls::card(rows, theme), theme)
        .debug_selector(|| "file-search-section-Status".into())
        .into_any_element()
}

/// Why file search is off, and what turns it on.
fn off_row(this: &SettingsWindow, status: &IndexStatus, theme: &Theme) -> gpui::Div {
    let mut lines = Vec::new();
    let reason = status
        .reason
        .clone()
        .unwrap_or_else(|| "no enabled extension uses file search".into());
    lines.push(
        note(
            "file-search-off",
            format!("File search is off: {reason}"),
            theme.warning,
            theme,
        )
        .into_any_element(),
    );
    let packages = this.launcher.file_search_packages();
    if packages.is_empty() {
        lines.push(controls::row_line(
            "Install an extension that uses file search, such as Files, under Extensions",
            theme.text_muted,
            theme,
        ));
    }
    for (title, why) in packages {
        if let Some(why) = why {
            lines.push(
                note(
                    format!("file-search-off-{title}"),
                    format!("{title} uses file search, but {why}: turn it on under Extensions"),
                    theme.text_muted,
                    theme,
                )
                .into_any_element(),
            );
        }
    }
    controls::setting_row("Nothing is indexed or watched", lines, theme)
}

/// The Indexed folders card: the home folder, each added folder with
/// Remove, and Add Folder….
fn roots_section(
    this: &mut SettingsWindow,
    effective: &ScopeRules,
    rules: &UserRules,
    busy: bool,
    theme: &Theme,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let home = effective.home.as_deref();
    let mut rows = Vec::new();
    // A long list draws only the rows near the page's view (#165).
    let (window_rows, page) = (
        this.file_search.rows_window.clone(),
        this.search.scroll().clone(),
    );
    let listed = effective.roots.len();
    for root in &effective.roots {
        let key = format!("root:{}", root.display());
        rows.push(window_rows.row(listed, key, false, &page, || {
            let added = rules.added_roots.contains(root);
            let name = last_name(root);
            let what = if Some(root.as_path()) == home {
                "Your home folder"
            } else if added {
                "A folder you added"
            } else {
                "Indexed by default"
            };
            let mut row = controls::setting_row(
                shown(root, home),
                vec![controls::row_line(what, theme.text_muted, theme)],
                theme,
            )
            .debug_selector({
                let name = name.clone();
                move || format!("file-search-root-{name}")
            });
            if added {
                let root = root.clone();
                row = row.child(
                    button(
                        format!("file-search-remove-root-{name}"),
                        "Remove",
                        !busy,
                        theme,
                    )
                    .when(!busy, |button| {
                        button.on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                            let root = root.clone();
                            change(
                                this,
                                move |rules| rules.added_roots.retain(|r| *r != root),
                                cx,
                            );
                        }))
                    }),
                );
            }
            row.into_any_element()
        }));
    }
    let anchor = this.search_anchor(ADD_ROOT);
    let add = button(ADD_ROOT.into(), "Add Folder…", !busy, theme)
        .anchor_scroll(Some(anchor))
        .when(!busy, |button| {
            button.on_click(cx.listener(|this, _: &gpui::ClickEvent, window, cx| {
                pick_folder(this, "Add", window, cx, |rules, folder| {
                    if !rules.added_roots.contains(&folder) {
                        rules.added_roots.push(folder);
                    }
                });
            }))
        });
    rows.push(
        controls::setting_row(
            "Add a folder",
            vec![controls::row_line(
                "Index a folder outside your home folder, such as a second drive's projects",
                theme.text_muted,
                theme,
            )],
            theme,
        )
        .child(add)
        .into_any_element(),
    );
    controls::section(
        Some("Indexed folders".into()),
        controls::card(rows, theme),
        theme,
    )
    .debug_selector(|| "file-search-section-Folders".into())
    .into_any_element()
}

/// The Excluded card: the excluded folders and patterns, each with Remove;
/// Exclude Folder…; and the field for a pattern.
fn excluded_section(
    this: &mut SettingsWindow,
    rules: &UserRules,
    home: Option<&Path>,
    busy: bool,
    theme: &Theme,
    window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let mut rows = Vec::new();
    // A long list draws only the rows near the page's view (#165).
    let (window_rows, page) = (
        this.file_search.rows_window.clone(),
        this.search.scroll().clone(),
    );
    let listed = rules.excluded_folders.len() + rules.excluded_patterns.len();
    for folder in &rules.excluded_folders {
        let name = last_name(folder);
        let removed = folder.clone();
        let key = format!("excluded:{}", folder.display());
        rows.push(window_rows.row(listed, key, false, &page, || {
            controls::setting_row(
                shown(folder, home),
                vec![controls::row_line(
                    "An excluded folder",
                    theme.text_muted,
                    theme,
                )],
                theme,
            )
            .debug_selector({
                let name = name.clone();
                move || format!("file-search-excluded-{name}")
            })
            .child(
                button(
                    format!("file-search-remove-excluded-{name}"),
                    "Remove",
                    !busy,
                    theme,
                )
                .when(!busy, |button| {
                    button.on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                        let removed = removed.clone();
                        change(
                            this,
                            move |rules| rules.excluded_folders.retain(|f| *f != removed),
                            cx,
                        );
                    }))
                }),
            )
            .into_any_element()
        }));
    }
    for pattern in &rules.excluded_patterns {
        let removed = pattern.clone();
        let selector = format!("file-search-pattern-{pattern}");
        rows.push(
            window_rows.row(listed, format!("pattern:{pattern}"), false, &page, || {
                controls::setting_row(
                    pattern.clone(),
                    vec![controls::row_line(
                        "An excluded pattern",
                        theme.text_muted,
                        theme,
                    )],
                    theme,
                )
                .debug_selector(move || selector)
                .child(
                    button(
                        format!("file-search-remove-pattern-{pattern}"),
                        "Remove",
                        !busy,
                        theme,
                    )
                    .when(!busy, |button| {
                        button.on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                            let removed = removed.clone();
                            change(
                                this,
                                move |rules| rules.excluded_patterns.retain(|p| *p != removed),
                                cx,
                            );
                        }))
                    }),
                )
                .into_any_element()
            }),
        );
    }
    let anchor = this.search_anchor(EXCLUDE_FOLDER);
    let exclude = button(EXCLUDE_FOLDER.into(), "Exclude Folder…", !busy, theme)
        .anchor_scroll(Some(anchor))
        .when(!busy, |button| {
            button.on_click(cx.listener(|this, _: &gpui::ClickEvent, window, cx| {
                pick_folder(this, "Exclude", window, cx, |rules, folder| {
                    if !rules.excluded_folders.contains(&folder) {
                        rules.excluded_folders.push(folder);
                    }
                });
            }))
        });
    rows.push(
        controls::setting_row(
            "Exclude a folder",
            vec![controls::row_line(
                "Leave a folder out, with everything in it",
                theme.text_muted,
                theme,
            )],
            theme,
        )
        .child(exclude)
        .into_any_element(),
    );
    let input = pattern_input(this, cx);
    let focused = input.focus_handle(cx).is_focused(window);
    let text = input.read(cx).as_str().to_owned();
    let placeholder = "such as *.log or build/";
    let anchor = this.search_anchor(PATTERN_FIELD);
    let well = controls::well(true, theme)
        .w(px(200.))
        .id(PATTERN_FIELD)
        .debug_selector(|| PATTERN_FIELD.into())
        .anchor_scroll(Some(anchor))
        .track_focus(&input.focus_handle(cx))
        .shadow(controls::well_shadows(focused, theme))
        .role(Role::TextInput)
        .aria_label("Exclude Pattern")
        .aria_value(text.clone())
        .aria_placeholder(placeholder)
        .child(controls::well_input(
            text_input("file-search-pattern-input").state(input.downgrade()),
            placeholder,
            theme,
        ));
    let offered = !busy && !text.trim().is_empty();
    let add = button("file-search-add-pattern".into(), "Exclude", offered, theme).when(
        offered,
        |button| {
            button.on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                add_pattern(this, cx);
            }))
        },
    );
    rows.push(
        controls::setting_row(
            "Exclude a pattern",
            vec![controls::row_line(
                "Names or paths to leave out, written as in .gitignore",
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
    controls::section(Some("Excluded".into()), controls::card(rows, theme), theme)
        .debug_selector(|| "file-search-section-Excluded".into())
        .into_any_element()
}

/// The Rules card: the four switches.
fn rules_section(
    rules: &UserRules,
    busy: bool,
    theme: &Theme,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let rows = SWITCHES.iter().map(|&(id, title, rule)| {
        let on = rule.get(rules);
        super::general::switch_row(
            super::general::SwitchRow {
                id,
                selector: id,
                title,
                on,
                offered: !busy,
                lines: Vec::new(),
            },
            theme,
            |row| {
                row.when(!busy, |row| {
                    row.on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                        change(this, move |rules| rule.set(rules, !on), cx);
                    }))
                })
            },
        )
        .into_any_element()
    });
    let rows: Vec<AnyElement> = rows.collect();
    controls::section(Some("Rules".into()), controls::card(rows, theme), theme)
        .debug_selector(|| "file-search-section-Rules".into())
        .into_any_element()
}

/// The Needs attention card: each problem with its reason and remedy, and
/// Include Again on a folder taken out for churn.
fn problems_section(
    this: &mut SettingsWindow,
    problems: &[Problem],
    home: Option<&Path>,
    busy: bool,
    theme: &Theme,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    // A long list (a hundred folders that could not be read, say) draws
    // only the rows near the page's view (#165).
    let (window_rows, page) = (
        this.file_search.rows_window.clone(),
        this.search.scroll().clone(),
    );
    let listed = problems.len();
    let rows: Vec<AnyElement> = problems
        .iter()
        .enumerate()
        .map(|(at, problem)| {
            let key = format!("problem:{at}:{:?}", problem.folder);
            window_rows.row(listed, key, false, &page, || {
                problem_row(problem, home, busy, theme, cx)
            })
        })
        .collect();
    controls::section(
        Some("Needs attention".into()),
        controls::card(rows, theme),
        theme,
    )
    .debug_selector(|| "file-search-section-Problems".into())
    .into_any_element()
}

/// One row of the Needs attention card: what needs attention, why and
/// what to do, with Include Again for a folder taken out for churn.
fn problem_row(
    problem: &Problem,
    home: Option<&Path>,
    busy: bool,
    theme: &Theme,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let kind = kind_name(problem.kind);
    let (title, selector) = match &problem.folder {
        Some(folder) => (
            shown(folder, home),
            format!("file-search-problem-{kind}-{}", last_name(folder)),
        ),
        None => (
            kind_title(problem.kind).to_owned(),
            format!("file-search-problem-{kind}"),
        ),
    };
    let lines = vec![
        controls::row_line(problem.reason.clone(), theme.warning, theme),
        controls::row_line(problem.remedy.clone(), theme.text_muted, theme),
    ];
    let description = format!("{}. {}", problem.reason, problem.remedy);
    let row = controls::setting_row(title.clone(), lines, theme)
        .id(SharedString::from(selector.clone()))
        .debug_selector(move || selector)
        .role(Role::Status)
        .aria_label(title)
        .aria_description(description);
    match (&problem.kind, &problem.folder) {
        (ProblemKind::Churned, Some(folder)) => {
            let folder = folder.clone();
            let name = last_name(&folder);
            row.child(
                button(
                    format!("file-search-include-again-{name}"),
                    "Include Again",
                    !busy,
                    theme,
                )
                .when(!busy, |button| {
                    button.on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                        let pending = this.launcher.include_in_file_search(folder.clone());
                        apply_and_report(this, pending, cx);
                    }))
                }),
            )
            .into_any_element()
        }
        _ => row.into_any_element(),
    }
}

// ------------------------------------------------------------ the behavior

/// Changes the user's rules as `edit` says and applies them: recorded,
/// and the index brought to them without a restart.
fn change(
    this: &mut SettingsWindow,
    edit: impl FnOnce(&mut UserRules),
    cx: &mut Context<SettingsWindow>,
) {
    let Some((_, mut rules)) = this.launcher.file_search_rules() else {
        return;
    };
    edit(&mut rules);
    let pending = this.launcher.set_file_search_rules(rules);
    apply_and_report(this, pending, cx);
}

/// Waits for `pending`, a change the page started, redrawing now and when
/// it lands; a failure is the page's status.
fn apply_and_report(
    this: &mut SettingsWindow,
    pending: impl Future<Output = Result<(), String>> + 'static,
    cx: &mut Context<SettingsWindow>,
) {
    this.file_search.busy = true;
    this.file_search.problem = None;
    cx.notify();
    cx.spawn(async move |this, cx| {
        let outcome = pending.await;
        this.update(cx, |this, cx| {
            this.file_search.busy = false;
            this.file_search.problem = outcome.err();
            cx.notify();
        })
        .ok();
    })
    .detach();
}

/// Asks for a folder with the system's picker (its button reading
/// `prompt`), and changes the rules with it as `edit` says.
fn pick_folder(
    _this: &mut SettingsWindow,
    prompt: &'static str,
    window: &mut Window,
    cx: &mut Context<SettingsWindow>,
    edit: fn(&mut UserRules, PathBuf),
) {
    let picked = cx.prompt_for_paths(PathPromptOptions {
        files: false,
        directories: true,
        multiple: false,
        prompt: Some(prompt.into()),
    });
    cx.spawn_in(window, async move |this, cx| {
        let folder = match picked.await {
            Ok(Ok(Some(paths))) => paths.into_iter().next(),
            Ok(Ok(None)) | Err(_) => None,
            Ok(Err(error)) => {
                this.update(cx, |this, cx| {
                    this.file_search.problem =
                        Some(format!("Could not open a folder picker: {error:#}"));
                    cx.notify();
                })
                .ok();
                None
            }
        };
        if let Some(folder) = folder {
            this.update(cx, |this, cx| {
                change(this, move |rules| edit(rules, folder), cx);
            })
            .ok();
        }
    })
    .detach();
}

/// Excludes the pattern the field holds, and empties the field.
fn add_pattern(this: &mut SettingsWindow, cx: &mut Context<SettingsWindow>) {
    let Some(input) = this.file_search.pattern.clone() else {
        return;
    };
    let pattern = input.read(cx).as_str().trim().to_owned();
    if pattern.is_empty() {
        return;
    }
    input.update(cx, |input, cx| input.emplace("", cx));
    change(
        this,
        move |rules| {
            if !rules.excluded_patterns.contains(&pattern) {
                rules.excluded_patterns.push(pattern);
            }
        },
        cx,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_and_times_read_for_people() {
        let home = Path::new("/home/me");
        assert_eq!(shown(Path::new("/home/me"), Some(home)), "~");
        assert_eq!(
            shown(&home.join("Projects").join("app"), Some(home)),
            "~/Projects/app"
        );
        assert_eq!(
            shown(Path::new("/mnt/drive"), Some(home)),
            Path::new("/mnt/drive").display().to_string()
        );
        let now = SystemTime::now();
        assert_eq!(ago(now), "just now");
        assert_eq!(
            ago(now - std::time::Duration::from_secs(5 * 60)),
            "5 minutes ago"
        );
        assert_eq!(
            ago(now - std::time::Duration::from_secs(3600)),
            "1 hour ago"
        );
    }

    #[test]
    fn the_state_reads_as_the_index_is() {
        let mut status = IndexStatus {
            state: IndexState::Building,
            found: 12_345,
            ..IndexStatus::default()
        };
        assert_eq!(state_words(&status), "Indexing… 12,345 found so far");
        status.waiting = true;
        assert!(state_words(&status).starts_with("Waiting"));
        status.state = IndexState::Stopped;
        status.reason = Some("Another Pane is using file search on this computer".into());
        assert_eq!(
            state_words(&status),
            "Stopped: Another Pane is using file search on this computer"
        );
        status.state = IndexState::Current;
        assert_eq!(state_words(&status), "Up to date");
        status.resuming = true;
        assert!(state_words(&status).starts_with("Paused: the computer slept"));
        status.resuming = false;
        assert_eq!(
            caught_up_words(Some((CaughtUpBy::Journal, SystemTime::now()))).as_deref(),
            Some("Last caught up from the change journal, just now")
        );
    }
}
