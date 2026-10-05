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
//! version, the system it runs on, where its data lives and what its
//! update check last found — nothing invented, nothing uploaded, no
//! credentials and no extension's settings or data. The copy is the
//! user's explicit choice, to the clipboard of this computer only; the
//! clipboard write has no failure path, so what the copy reports is its
//! completion.

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
use crate::ui::settings_shell;
use crate::ui::theme::Theme;

/// The documentation entry's address: Pane's repository, whose README is
/// the documentation of this build.
const DOCUMENTATION: &str = "https://github.com/hoangvu12/pane";

/// The target id of the documentation entry, the control the sidebar's
/// search jumps to (see [`entries`]).
const DOCUMENTATION_ROW: &str = "documentation";

/// The page's heading.
pub(crate) const TITLE: &str = "About";

/// What the page is, in one line: its sidebar entry's description in
/// the search, and its heading's subtitle.
pub(crate) const ABOUT: &str = "Pane's version, documentation and diagnostics";

/// What the page's sections say under their labels.
pub(crate) const VERSION_NOTE: &str = "The version of this Pane build";
pub(crate) const DOCUMENTATION_NOTE: &str = "Open Pane's repository documentation";
pub(crate) const DIAGNOSTICS_NOTE: &str = "Copy what Pane knows of this installation";

/// The buttons' labels.
pub(crate) const CHECK_LABEL: &str = "Check for updates";
pub(crate) const DOCUMENTATION_LABEL: &str = "Open documentation";
pub(crate) const DIAGNOSTICS_LABEL: &str = "Copy diagnostics";
pub(crate) const COPIED: &str = "Copied the diagnostics to the clipboard";

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
            "No artifact source is configured for this Pane, so there is no release to check \
             for an update."
                .into(),
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
        ApplicationUpdate::Unchecked => (
            "Pane has not checked for an update yet.".into(),
            theme.text_muted,
            Action::Check,
        ),
        ApplicationUpdate::Current => ("Pane is up to date".into(), theme.success, Action::Check),
        ApplicationUpdate::Failed(why) => (
            format!("Could not check for a Pane update: {why}").into(),
            theme.danger,
            Action::Check,
        ),
        // The offer: the same version, and the same explanation of what
        // installing does, the root row offers — with why installing it
        // last failed, if it did, since the offer stays, ready to be
        // chosen again.
        ApplicationUpdate::Offered { version, failure } => match failure {
            Some(why) => (
                format!("Could not update Pane to {version}: {why}").into(),
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
            format!("Installed Pane {version}; the new version is used the next time Pane starts")
                .into(),
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
                format!("Could not open the documentation: {why}").into(),
                theme.danger,
            ),
        }),
        copied,
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
                let report = diagnostics(&this.launcher.application_update());
                cx.write_to_clipboard(ClipboardItem::new_string(report));
                this.about.copied = true;
                cx.notify();
            }))
        }
    })
    .into_any_element()
}

/// What the About page shows, as plain values: what [`render`] reads from
/// the launcher and the page's own state, and what the visual workbench's
/// fixture supplies to draw the same page (#99).
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
}

/// The About page's composition, which the visual workbench's fixture
/// draws too: the heading block, then in the page's column a field group
/// per section (#99) — the version, the updates (the status and the
/// button the state offers), the documentation and the diagnostics, each
/// action a Settings button — with what each last reported. `attach` adds
/// each button's behavior; the composition gives each its identity, its
/// accessibility and its look.
pub(crate) fn compose(
    view: &AboutView,
    theme: &Theme,
    attach: impl Fn(AboutControl, Stateful<Div>) -> Stateful<Div>,
) -> Stateful<Div> {
    // The version: what this build runs, as `pane --version` prints it. A
    // labelled node, so assistive technology reads the version rather than
    // passing it by.
    let name = format!("Pane {}", view.version);
    let version = controls::field(theme)
        .child(
            controls::field_label(name.clone(), theme)
                .id("about-version")
                .debug_selector(|| "about-version".into())
                .role(Role::Label)
                .aria_label(name)
                .aria_description(VERSION_NOTE),
        )
        .child(controls::field_description(
            VERSION_NOTE,
            theme.text_muted,
            theme,
        ));
    // The update flow: the status of the check, the offer and the install,
    // with the button that acts on it — a second surface of the root rows'
    // own flow (see the module docs). Each button's explanation is also its
    // accessible description, as root search's rows' subtitles are.
    let update_button = match &view.action {
        Action::None => None,
        Action::Check => Some(attach(
            AboutControl::Check,
            action_button(
                "about-check-update",
                CHECK_LABEL,
                "Read the artifact source's index for a newer version of Pane",
                theme,
            ),
        )),
        Action::Update(version) => {
            let title = format!("Update Pane to {version}");
            Some(attach(
                AboutControl::Update,
                action_button(
                    "about-update",
                    &title,
                    "Your extensions and settings are kept; the new version is used the next \
                     time Pane starts",
                    theme,
                ),
            ))
        }
    };
    let updates = controls::field(theme)
        .child(label("Updates", theme))
        .child(
            controls::field_description(view.update_status.clone(), view.update_tone, theme)
                .id("about-update-status")
                .debug_selector(|| "about-update-status".into())
                .role(Role::Status)
                .aria_label(view.update_status.clone()),
        )
        .children(update_button.map(|button| div().flex().child(button)));
    // The documentation: Pane's repository, opened with the launcher's
    // link opener, and what the opening reported.
    let documentation = controls::field(theme)
        .child(label("Documentation", theme))
        .child(controls::field_description(
            DOCUMENTATION_NOTE,
            theme.text_muted,
            theme,
        ))
        .child(
            div().flex().child(attach(
                AboutControl::Documentation,
                controls::button(DOCUMENTATION_LABEL, true, theme)
                    .id("about-documentation")
                    .debug_selector(|| "about-documentation".into())
                    .role(Role::Link)
                    .aria_label(DOCUMENTATION_LABEL)
                    .aria_description(DOCUMENTATION_NOTE),
            )),
        )
        .children(
            view.opened
                .as_ref()
                .map(|(text, color)| status_note("about-status", text.clone(), *color, theme)),
        );
    // The diagnostics: what Pane already knows of this installation,
    // copied to this computer's clipboard by the user's explicit choice
    // (see the module docs).
    let diagnostics = controls::field(theme)
        .child(label("Diagnostics", theme))
        .child(controls::field_description(
            DIAGNOSTICS_NOTE,
            theme.text_muted,
            theme,
        ))
        .child(div().flex().child(attach(
            AboutControl::Diagnostics,
            action_button(
                "about-diagnostics",
                DIAGNOSTICS_LABEL,
                DIAGNOSTICS_NOTE,
                theme,
            ),
        )))
        .children(
            view.copied
                .then(|| status_note("about-diagnostics-status", COPIED, theme.success, theme)),
        );
    let column = controls::column(theme)
        .child(
            settings_shell::page_header(TITLE, Some(ABOUT.into()), theme)
                .id("about-title")
                .debug_selector(|| "about-title".into()),
        )
        .child(version)
        .child(updates)
        .child(documentation)
        .child(diagnostics);
    div()
        .id("about")
        .debug_selector(|| "about".into())
        .child(column)
}

/// One of the page's buttons, named `selector`: `title` is what it says
/// (and its accessible name), `description` what it does.
fn action_button(
    selector: &'static str,
    title: &str,
    description: &'static str,
    theme: &Theme,
) -> Stateful<Div> {
    controls::button(title.to_owned(), true, theme)
        .id(selector)
        .debug_selector(move || selector.into())
        .role(Role::Button)
        .aria_label(title.to_owned())
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

/// "Checking for a Pane update…", as the page says it while one runs.
fn checking_text() -> SharedString {
    "Checking for a Pane update…".into()
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
/// lives and what its update check last found — and nothing else (see the
/// module docs).
fn diagnostics(update: &ApplicationUpdate) -> String {
    let mut report = format!("Pane {}", crate::APP_VERSION);
    if let Some(target) = pane_core::Target::current() {
        report.push_str(&format!("\nBuilt for {}", target.id()));
    }
    match crate::data_dir() {
        Some(dir) => report.push_str(&format!("\nData folder: {}", dir.display())),
        None => report.push_str("\nData folder: none"),
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

/// A section's field label (#99), as the board labels its fields.
fn label(text: &'static str, theme: &Theme) -> Stateful<Div> {
    controls::field_label(text, theme)
        .id(text)
        .debug_selector(move || format!("about-label-{text}"))
}
