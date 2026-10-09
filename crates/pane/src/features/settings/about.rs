//! The About page: the real version of the running Pane, the existing
//! application-update flow, the documentation entry, and the diagnostics
//! Pane already has.
//!
//! All of it is honest by construction. The version is
//! [`crate::APP_VERSION`], the same value `pane --version` prints and an
//! application update compares itself with. The update section is the
//! existing flow's second surface, not a second flow: its state is read
//! from the launcher ([`pane_core::Launcher::application_update`], the
//! same records root search's update rows and the status line come from),
//! and its rows run the same check
//! ([`pane_core::Launcher::check_application_update_again`]) and the same
//! install ([`pane_core::Launcher::install_application_update`]) the root
//! rows run, so the two entry points cannot disagree — whichever found
//! the offer, both show it, and installing from either swaps the same
//! program the same way. Nothing is downloaded, installed or restarted
//! except by the user's choice, the new version is used the next time
//! Pane starts, and where no release source is configured the page says
//! so instead of promising one.
//!
//! The documentation entry opens Pane's repository with the link opener
//! the app gave the launcher — the same handler quicklinks open with, not
//! a second instance. Opening runs off the window's thread, as every link
//! opening does, and what it reported is shown as the page's status.
//!
//! The diagnostics the page can copy are what Pane already knows — its
//! version, the system it runs on, where its data and its log live and
//! what its update check last found — nothing invented, nothing uploaded,
//! no credentials and no extension's settings or data. The copy is the
//! user's explicit choice, to the clipboard of this computer only; the
//! clipboard write has no failure path, so what the copy reports is its
//! completion.
//!
//! Beside the diagnostics, the Log row (#133) names the folder of Pane's
//! log, says when Pane quit unexpectedly last time — the same notice root
//! search lists, read from the launcher ([`pane_core::Launcher::log_notice`])
//! — and opens the folder with the system's file manager
//! ([`pane_core::Launcher::open_log_folder`]), which takes the notice away
//! in both places.

use std::future::Future;

use gpui::{
    AnyElement, App, ClipboardItem, Context, Div, Hsla, Role, SharedString, Stateful, Window, div,
    prelude::*,
};
use pane_core::{ApplicationUpdate, Status};

use super::{Page, SettingsWindow, search};
use crate::app::launcher_changed_outside;
use crate::ui::controls::{self, status_note};
use crate::ui::icon::Glyph;
use crate::ui::theme::Theme;

/// The documentation entry's address: Pane's repository, whose README is
/// the documentation of this build.
const DOCUMENTATION: &str = "https://github.com/pane-app/pane";

/// The target id of the documentation entry, the control the sidebar's
/// search jumps to (see [`entries`]).
const DOCUMENTATION_ROW: &str = "documentation";

/// What the page is, in one line: its sidebar entry's description in
/// the search.
pub(crate) const ABOUT: &str = "Version, updates and documentation";

/// What the rows say under their names.
pub(crate) const DOCUMENTATION_NOTE: &str = "Pane's README on GitHub";
pub(crate) const DIAGNOSTICS_NOTE: &str = "Your version, system, data folder and log folder";
pub(crate) const LOG_NOTE: &str = "Pane's own diagnostics, kept on this computer only";

/// The notice that the run before this one ended unexpectedly (#133), as
/// root search's row says it.
pub(crate) const CRASH_NOTICE: &str = pane_core::diagnostics::CRASH_NOTICE;

/// The buttons' labels.
pub(crate) const CHECK_LABEL: &str = "Check for updates";
pub(crate) const DOCUMENTATION_LABEL: &str = "Open";
pub(crate) const DIAGNOSTICS_LABEL: &str = "Copy";
pub(crate) const LOG_FOLDER_LABEL: &str = "Open log folder";
pub(crate) const COPIED: &str = "Copied to the clipboard";

/// The About page, registered last in the window's page list: the spec's
/// section order names it the last of the seven.
pub(crate) fn page() -> Page {
    Page {
        title: "About",
        about: ABOUT,
        icon: Glyph::Gear,
        count: None,
        render,
        search: entries,
        focus,
    }
}

/// The control the page offers the sidebar's search: its documentation
/// entry. The version is information, not a control, so it is not
/// offered as one.
fn entries(_launcher: &pane_core::Launcher, _cx: &App) -> Vec<search::Entry> {
    vec![search::Entry {
        control: Some(DOCUMENTATION_ROW.into()),
        title: "Documentation".into(),
        group: None,
        unavailable: None,
    }]
}

/// The page's entry takes no keyboard focus (it is a link row, chosen
/// with the pointer), so a jump reveals it and the sidebar keeps the
/// focus: `false`.
fn focus(_: &mut SettingsWindow, _: &str, _: &mut Window, _: &mut Context<SettingsWindow>) -> bool {
    false
}

/// The About page's state, held by the window: what opening the
/// documentation entry last reported, what copying the diagnostics last
/// reported, and the update work this page started and is waiting to hear
/// the end of.
#[derive(Default)]
pub(crate) struct State {
    opened: Option<Result<(), String>>,
    /// Whether the diagnostics were copied: the copy's completion, shown
    /// as the page's status. The clipboard write has no failure path, so
    /// there is nothing else to report.
    copied: bool,
    /// The check or install this page started, and is waiting to hear the
    /// end of. The launcher begins it when the spawned future is first
    /// polled — one frame after the click — so the page keeps its own
    /// word for that frame; the launcher's state, read every frame, takes
    /// over as soon as the work has begun. Cleared when the answer lands.
    started: Option<Started>,
}

/// Which update action this page started.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Started {
    /// "Check for updates", clicked.
    Check,
    /// "Update Pane to <version>", clicked.
    Install,
}

impl State {
    /// Records what opening the documentation entry reported.
    pub(crate) fn record(&mut self, opened: Result<(), String>) {
        self.opened = Some(opened);
    }
}

/// Draws the About page: the pane's name and the running version, the
/// update flow, the documentation entry and the diagnostics copy, with
/// what each of them last reported.
fn render(
    this: &mut SettingsWindow,
    _window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let theme = crate::settings::visuals(cx).theme;
    let opened = this.about.opened.clone();
    let copied = this.about.copied;
    // The update flow's state, read from the same records the root rows
    // and the status line come from (see the module docs), and the work
    // this page itself started (see [`State::started`]).
    let update = this.launcher.application_update();
    let started = this.about.started;
    // Whether a check or an install is running: the launcher's own state
    // for one it began, or this page's word for the one it just started.
    let checking = started == Some(Started::Check) || matches!(update, ApplicationUpdate::Checking);
    let installing =
        started == Some(Started::Install) || matches!(update, ApplicationUpdate::Installing { .. });
    // While an install runs, the status line is its progress — the same
    // line the launcher window shows — so the page follows it.
    let status = this.launcher.view().status;

    // What the update section says, in which tone, and the row it offers
    // the user, if any.
    let (update_status, update_tone, action): (SharedString, Hsla, Action) = match &update {
        // No source: the state is explained, never glossed over as a
        // check that would run or a release that exists.
        ApplicationUpdate::Unconfigured => (
            "This build has no update source.".into(),
            theme.text_muted,
            Action::None,
        ),
        // A check running — or the one this page started, which the
        // launcher has not begun yet: what it answers is the next thing
        // the page says.
        ApplicationUpdate::Checking => (checking_text(), theme.warning, Action::None),
        ApplicationUpdate::Unchecked
        | ApplicationUpdate::Current
        | ApplicationUpdate::Failed(_)
            if checking =>
        {
            (checking_text(), theme.warning, Action::None)
        }
        // An install running: the status line's own progress, and what
        // the install is doing before the first bytes of its package
        // arrive.
        ApplicationUpdate::Installing { version } => (
            install_progress(&status, version),
            theme.warning,
            Action::None,
        ),
        // The offer this page's clicked row named, before the launcher
        // has begun installing it.
        ApplicationUpdate::Offered { version, .. } if installing => (
            install_progress(&status, version),
            theme.warning,
            Action::None,
        ),
        // The states a check answers.
        ApplicationUpdate::Unchecked => ("Not checked yet".into(), theme.text_muted, Action::Check),
        ApplicationUpdate::Current => ("Pane is up to date".into(), theme.success, Action::Check),
        ApplicationUpdate::Failed(why) => (
            format!("Couldn't check for updates: {why}").into(),
            theme.danger,
            Action::Check,
        ),
        // The offer: the same version, and the same explanation of what
        // installing does, the root row offers — with why installing it
        // last failed, if it did, since the offer stays, ready to be
        // chosen again.
        ApplicationUpdate::Offered { version, failure } => match failure {
            Some(why) => (
                format!("Couldn't update to {version}: {why}").into(),
                theme.danger,
                Action::Update(version.clone()),
            ),
            None => (
                format!("Pane {version} is available").into(),
                theme.success,
                Action::Update(version.clone()),
            ),
        },
        ApplicationUpdate::Installed { version } => (
            format!("Pane {version} is installed and starts next time").into(),
            theme.success,
            Action::None,
        ),
    };
    let view = AboutView {
        version: crate::APP_VERSION.to_owned(),
        update_status,
        update_tone,
        action,
        opened: opened.map(|opened| match opened {
            Ok(()) => (format!("Opened {DOCUMENTATION}").into(), theme.success),
            Err(why) => (
                format!("Couldn't open the documentation: {why}").into(),
                theme.danger,
            ),
        }),
        copied,
        // Pane's log, and whether the run before ended unexpectedly: the
        // same notice root search lists (#133).
        log: this.launcher.log_notice().map(log_view),
    };
    let documentation = this.search_anchor(DOCUMENTATION_ROW);
    compose(&view, &theme, |control, element| match control {
        AboutControl::Check => {
            element.on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                this.about.started = Some(Started::Check);
                run(this.launcher.check_application_update_again(), cx);
            }))
        }
        AboutControl::Update => {
            element.on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                this.about.started = Some(Started::Install);
                run(this.launcher.install_application_update(), cx);
            }))
        }
        AboutControl::Documentation => {
            element
                .anchor_scroll(Some(documentation.clone()))
                .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                    // A system handler may take a moment to start, so the
                    // opening happens off the window's thread; what it
                    // reports becomes the page's status.
                    let links = this.launcher.link_opener();
                    cx.spawn(async move |this, cx| {
                        let opened = cx
                            .background_executor()
                            .spawn(async move { links.open(DOCUMENTATION) })
                            .await;
                        this.update(cx, |this, cx| {
                            this.about.record(opened);
                            cx.notify();
                        })
                        .ok();
                    })
                    .detach();
                }))
        }
        AboutControl::Diagnostics => {
            element.on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                // The report is what the launcher holds as of this click,
                // not as of the frame that drew the button.
                let report = diagnostics(
                    &this.launcher.application_update(),
                    this.launcher.log_notice().as_ref(),
                );
                cx.write_to_clipboard(ClipboardItem::new_string(report));
                this.about.copied = true;
                cx.notify();
            }))
        }
        AboutControl::LogFolder => {
            element.on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                // The folder opens with the system's file manager, off the
                // window's thread; the notice goes, here and in root
                // search, and the status line says whether it opened.
                let opening = this.launcher.open_log_folder();
                launcher_changed_outside(cx);
                cx.notify();
                cx.spawn(async move |this, cx| {
                    opening.await;
                    cx.update(launcher_changed_outside);
                    this.update(cx, |_, cx| cx.notify()).ok();
                })
                .detach();
            }))
        }
    })
    .into_any_element()
}

/// What the About page shows, as plain values: what [`render`] reads from
/// the launcher and the page's own state (#99).
pub(crate) struct AboutView {
    /// The running version, as `pane --version` prints it.
    pub(crate) version: String,
    /// What the update section says, and in which tone.
    pub(crate) update_status: SharedString,
    pub(crate) update_tone: Hsla,
    /// The update section's button, if it offers one.
    pub(crate) action: Action,
    /// What opening the documentation last reported, in its tone.
    pub(crate) opened: Option<(SharedString, Hsla)>,
    /// Whether the diagnostics were copied.
    pub(crate) copied: bool,
    /// Pane's log (#133), when this launcher keeps one.
    pub(crate) log: Option<LogView>,
}

/// Pane's log, as the About page shows it: its folder, and whether the run
/// before this one ended unexpectedly (the notice root search lists too).
pub(crate) struct LogView {
    pub(crate) folder: String,
    pub(crate) quit_unexpectedly: bool,
}

/// The Log row's view of what the launcher knows of Pane's log.
fn log_view(notice: pane_core::LogNotice) -> LogView {
    LogView {
        folder: notice.folder.display().to_string(),
        quit_unexpectedly: notice.quit_unexpectedly,
    }
}

/// Which of the About page's controls an element is, for the caller of
/// [`compose`] that attaches its behavior.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AboutControl {
    /// "Check for updates".
    Check,
    /// "Update Pane to <version>".
    Update,
    /// The documentation's button.
    Documentation,
    /// "Copy diagnostics".
    Diagnostics,
    /// "Open log folder".
    LogFolder,
}

/// The About page's composition: one card of rows (#99) — the version, the updates (the
/// status and the button the state offers), the documentation and the
/// diagnostics, each action a Settings button at its row's end — with
/// what each last reported under its name. `attach` adds each button's
/// behavior; the composition gives each its identity, its accessibility
/// and its look.
pub(crate) fn compose(
    view: &AboutView,
    theme: &Theme,
    attach: impl Fn(AboutControl, Stateful<Div>) -> Stateful<Div>,
) -> Stateful<Div> {
    // The version: what this build runs, as `pane --version` prints it. A
    // labelled node, so assistive technology reads the version rather than
    // passing it by.
    let version = controls::setting_row("Version", Vec::new(), theme).child(
        div()
            .flex_none()
            .text_size(theme.typography.settings_text_size)
            .font_family(theme.typography.mono_family.clone())
            .text_color(theme.text_body)
            .id("about-version")
            .debug_selector(|| "about-version".into())
            .role(Role::Label)
            .aria_label(format!("Pane {}", view.version))
            .child(view.version.clone()),
    );
    // The update flow: the status of the check, the offer and the install,
    // with the button that acts on it — a second surface of the root rows'
    // own flow (see the module docs).
    let update_button = match &view.action {
        Action::None => None,
        Action::Check => Some(attach(
            AboutControl::Check,
            action_button(
                "about-check-update",
                CHECK_LABEL,
                "Looks for a newer version of Pane",
                theme,
            ),
        )),
        Action::Update(version) => {
            let title = format!("Update to {version}");
            Some(attach(
                AboutControl::Update,
                action_button(
                    "about-update",
                    &title,
                    "Keeps your extensions and settings. The new version starts next time.",
                    theme,
                ),
            ))
        }
    };
    let status = status_note(
        "about-update-status",
        view.update_status.clone(),
        view.update_tone,
        theme,
    );
    let updates = controls::setting_row("Updates", vec![status.into_any_element()], theme)
        .children(update_button);
    // The documentation: Pane's repository, opened with the launcher's
    // link opener, and what the opening reported.
    let mut documentation_lines = vec![controls::row_line(
        DOCUMENTATION_NOTE,
        theme.text_muted,
        theme,
    )];
    documentation_lines.extend(view.opened.as_ref().map(|(text, color)| {
        status_note("about-status", text.clone(), *color, theme).into_any_element()
    }));
    let documentation =
        controls::setting_row("Documentation", documentation_lines, theme).child(attach(
            AboutControl::Documentation,
            controls::button("about-documentation", DOCUMENTATION_LABEL, true, theme)
                .debug_selector(|| "about-documentation".into())
                .role(Role::Link)
                .aria_label("Open documentation")
                .aria_description(DOCUMENTATION_NOTE),
        ));
    // The diagnostics: what Pane already knows of this installation,
    // copied to this computer's clipboard by the user's explicit choice
    // (see the module docs).
    let mut diagnostics_lines = vec![controls::row_line(
        DIAGNOSTICS_NOTE,
        theme.text_muted,
        theme,
    )];
    diagnostics_lines.extend(view.copied.then(|| {
        status_note("about-diagnostics-status", COPIED, theme.success, theme).into_any_element()
    }));
    let diagnostics = controls::setting_row("Diagnostics", diagnostics_lines, theme).child(attach(
        AboutControl::Diagnostics,
        action_button(
            "about-diagnostics",
            DIAGNOSTICS_LABEL,
            "Copies your version, system, data folder and log folder",
            theme,
        ),
    ));
    // Pane's log beside the diagnostics (#133): where it is, the notice
    // that Pane quit unexpectedly last time, and the folder opened with the
    // system's file manager.
    let log = view.log.as_ref().map(|log| {
        let mut lines = vec![
            controls::row_line(LOG_NOTE, theme.text_muted, theme),
            controls::row_line(log.folder.clone(), theme.text_muted, theme),
        ];
        if log.quit_unexpectedly {
            lines.push(
                status_note("about-crash-notice", CRASH_NOTICE, theme.warning, theme)
                    .into_any_element(),
            );
        }
        controls::setting_row("Log", lines, theme).child(attach(
            AboutControl::LogFolder,
            action_button(
                "about-log-folder",
                LOG_FOLDER_LABEL,
                "Opens the folder of Pane's log in the file manager",
                theme,
            ),
        ))
    });
    let mut rows = vec![
        version.into_any_element(),
        updates.into_any_element(),
        documentation.into_any_element(),
    ];
    rows.extend(log.map(|log| log.into_any_element()));
    rows.push(diagnostics.into_any_element());
    let card = controls::card(rows, theme);
    let page = controls::page(theme).child(controls::section(None, card, theme));
    div()
        .id("about")
        .debug_selector(|| "about".into())
        .child(page)
}

/// One of the page's buttons, named `selector`: `title` is what it says
/// (and its accessible name), `description` what it does.
fn action_button(
    selector: &'static str,
    title: &str,
    description: &'static str,
    theme: &Theme,
) -> Stateful<Div> {
    controls::button(selector, title.to_owned(), true, theme)
        .debug_selector(move || selector.into())
        .role(Role::Button)
        .aria_label(if title == DIAGNOSTICS_LABEL {
            "Copy diagnostics".to_owned()
        } else {
            title.to_owned()
        })
        .aria_description(description)
}

/// The update section's button, as the page offers it to the user.
pub(crate) enum Action {
    /// No button: nothing to check against, or work already running.
    None,
    /// "Check for updates": the check the user asks for.
    Check,
    /// "Update Pane to <version>", the offered update the user chooses to
    /// install.
    Update(String),
}

/// Runs the update action `pending` the page's row started: the launcher
/// window redraws with whatever the action leaves (an update's row
/// appears in root search; the status line follows the install), the page
/// redraws now — with the action it started — and again when the answer
/// lands, as the launcher window's own `show_until_done` does.
fn run(pending: impl Future<Output = ()> + 'static, cx: &mut Context<SettingsWindow>) {
    launcher_changed_outside(cx);
    cx.notify();
    cx.spawn(async move |this, cx| {
        pending.await;
        cx.update(launcher_changed_outside);
        this.update(cx, |this, cx| {
            this.about.started = None;
            cx.notify();
        })
        .ok();
    })
    .detach();
}

/// "Checking for updates…", as the page says it while one runs.
fn checking_text() -> SharedString {
    "Checking for updates…".into()
}

/// What the page says while an update installs: the status line's own
/// progress — the same line the launcher window shows — with what the
/// install is doing before the first bytes of its package arrive.
fn install_progress(status: &Status, version: &str) -> SharedString {
    match status {
        Status::Progress(work) => work.clone().into(),
        _ => format!("Downloading Pane {version}…").into(),
    }
}

/// The report the diagnostics copy holds: what Pane already knows of this
/// installation — its version, the system it runs on, where its data
/// lives, where its log is (and whether the run before ended
/// unexpectedly) and what its update check last found — and nothing else
/// (see the module docs). The folders are redacted as the log redacts
/// (#133): the home folder is `~`, and the user's and the computer's names
/// are left out, so the report can be pasted into a public bug report.
fn diagnostics(update: &ApplicationUpdate, log: Option<&pane_core::LogNotice>) -> String {
    let folder =
        |path: &std::path::Path| pane_core::diagnostics::redacted(&path.display().to_string());
    let mut report = format!("Pane {}", crate::APP_VERSION);
    if let Some(target) = pane_core::Target::current() {
        report.push_str(&format!("\nBuilt for {}", target.id()));
    }
    match crate::data_dir() {
        Some(dir) => report.push_str(&format!("\nData folder: {}", folder(&dir))),
        None => report.push_str("\nData folder: none"),
    }
    match log {
        Some(log) => {
            report.push_str(&format!("\nLog folder: {}", folder(&log.folder)));
            if log.quit_unexpectedly {
                report.push_str("\nLast run: Pane quit unexpectedly");
            }
        }
        None => report.push_str("\nLog folder: none"),
    }
    report.push_str(&format!("\nUpdate check: {}", update_line(update)));
    report
}

/// What the update check last found, as the one line of the diagnostics
/// report that says it.
fn update_line(update: &ApplicationUpdate) -> String {
    match update {
        ApplicationUpdate::Unconfigured => "no artifact source is configured".into(),
        ApplicationUpdate::Unchecked => "not run yet".into(),
        ApplicationUpdate::Checking => "running".into(),
        ApplicationUpdate::Current => "Pane is up to date".into(),
        ApplicationUpdate::Offered {
            version,
            failure: None,
        } => format!("Pane {version} is available"),
        ApplicationUpdate::Offered {
            version,
            failure: Some(why),
        } => format!("Pane {version} is available; installing it failed: {why}"),
        ApplicationUpdate::Installing { version } => format!("installing Pane {version}"),
        ApplicationUpdate::Installed { version } => {
            format!("installed Pane {version}; used the next time Pane starts")
        }
        ApplicationUpdate::Failed(why) => format!("could not check: {why}"),
    }
}
