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
    prelude::*, px,
};
use pane_core::{ApplicationUpdate, Status};

use super::{Page, SettingsWindow, search};
use crate::app::launcher_changed_outside;
use crate::ui::icon::{Glyph, IconTone};
use crate::ui::result_row::{RowContent, result_row};

/// The documentation entry's address: Pane's repository, whose README is
/// the documentation of this build.
const DOCUMENTATION: &str = "https://github.com/hoangvu12/pane";

/// The target id of the documentation entry, the control the sidebar's
/// search jumps to (see [`entries`]).
const DOCUMENTATION_ROW: &str = "documentation";

/// The About page, registered last in the window's page list: the spec's
/// section order names it the last of the seven.
pub(crate) fn page() -> Page {
    Page {
        title: "About",
        about: "Pane's version, documentation and diagnostics",
        icon: (IconTone::Command, Glyph::Gear),
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
    let typography = &theme.typography;
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
    // The update section's row, from the action the state offers. Each
    // row's subtitle is also its accessible description, as root search's
    // rows' are, so what a row explains is read, not only painted.
    let update_row = match action {
        Action::None => None,
        Action::Check => Some(
            result_row(
                RowContent {
                    title: "Check for updates".into(),
                    subtitle: Some(
                        "Read the artifact source's index for a newer version of Pane".into(),
                    ),
                    unavailable_reason: None,
                    unavailable_id: "about-check-unavailable".into(),
                    selected: false,
                    icon: Some((IconTone::Command, Glyph::Download)),
                },
                &theme,
            )
            .id("about-check-update")
            // Pressed: the selected wash, one rung above the hover one,
            // fading on the shared pointer span.
            .active(|row| row.bg(theme.row_selected))
            .transitions(|fades| fades.bg(crate::ui::motion::pointer_fade()))
            .debug_selector(|| "about-check-update".into())
            .role(Role::Button)
            .aria_label("Check for updates")
            .aria_description("Read the artifact source's index for a newer version of Pane")
            .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                this.about.started = Some(Started::Check);
                run(this.launcher.check_application_update_again(), cx);
            })),
        ),
        Action::Update(version) => {
            let row_title = format!("Update Pane to {version}");
            Some(
                result_row(
                    RowContent {
                        title: row_title.clone().into(),
                        // The root row's own explanation of what
                        // installing does, so both entry points say the
                        // same thing.
                        subtitle: Some(
                            "Your extensions and settings are kept; the new version is used \
                             the next time Pane starts"
                                .into(),
                        ),
                        unavailable_reason: None,
                        unavailable_id: "about-update-unavailable".into(),
                        selected: false,
                        icon: Some((IconTone::Command, Glyph::Download)),
                    },
                    &theme,
                )
                .id("about-update")
                // Pressed: the selected wash, one rung above the hover
                // one, fading on the shared pointer span.
                .active(|row| row.bg(theme.row_selected))
                .transitions(|fades| fades.bg(crate::ui::motion::pointer_fade()))
                .debug_selector(|| "about-update".into())
                .role(Role::Button)
                .aria_label(row_title)
                .aria_description(
                    "Your extensions and settings are kept; the new version is used the next \
                     time Pane starts",
                )
                .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                    this.about.started = Some(Started::Install);
                    run(this.launcher.install_application_update(), cx);
                })),
            )
        }
    };

    div()
        .id("about")
        .debug_selector(|| "about".into())
        .flex()
        .flex_col()
        .gap(px(4.))
        .child(
            div()
                .id("about-title")
                .debug_selector(|| "about-title".into())
                .pb(px(8.))
                .text_size(typography.search_size)
                .font_weight(typography.medium)
                .text_color(theme.text_title)
                .child("About"),
        )
        .child(
            // The version row: what this build runs, as `pane --version`
            // prints it. A labelled node, so assistive technology reads
            // the version rather than passing it by.
            div()
                .flex()
                .flex_col()
                .gap(px(2.))
                .py(px(10.))
                .child(
                    div()
                        .id("about-version")
                        .debug_selector(|| "about-version".into())
                        .role(Role::Label)
                        .aria_label(format!("Pane {}", crate::APP_VERSION))
                        .aria_description("The version of this Pane build")
                        .text_size(typography.row_title_size)
                        .font_weight(typography.medium)
                        .text_color(theme.text_title)
                        .child(format!("Pane {}", crate::APP_VERSION)),
                )
                .child(
                    div()
                        .text_size(typography.row_subtitle_size)
                        .text_color(theme.text_muted)
                        .child("The version of this Pane build"),
                ),
        )
        // The update flow: the status of the check, the offer and the
        // install, with the row that acts on it — a second surface of the
        // root rows' own flow (see the module docs).
        .child(label("Updates", &theme))
        .child(
            div()
                .id("about-update-status")
                .debug_selector(|| "about-update-status".into())
                .role(Role::Status)
                .aria_label(update_status.clone())
                .text_size(typography.row_subtitle_size)
                .text_color(update_tone)
                .child(update_status),
        )
        .when_some(update_row, |page, row| page.child(row))
        // The documentation entry: a navigation row like the launcher's
        // results, opening Pane's repository with the launcher's link
        // opener.
        .child(label("Documentation", &theme))
        .child(
            result_row(
                RowContent {
                    title: "Documentation".into(),
                    subtitle: Some("Open Pane's repository documentation".into()),
                    unavailable_reason: None,
                    unavailable_id: "about-unavailable".into(),
                    selected: false,
                    icon: Some((IconTone::Web, Glyph::Globe)),
                },
                &theme,
            )
            .id("about-documentation")
            // Pressed: the selected wash, one rung above the hover one,
            // fading on the shared pointer span.
            .active(|row| row.bg(theme.row_selected))
            .transitions(|fades| fades.bg(crate::ui::motion::pointer_fade()))
            .debug_selector(|| "about-documentation".into())
            .anchor_scroll(Some(this.search_anchor(DOCUMENTATION_ROW)))
            .role(Role::Link)
            .aria_label("Documentation")
            .aria_description("Open Pane's repository documentation")
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
            })),
        )
        .when_some(opened, |page, opened| {
            let (text, color): (SharedString, Hsla) = match &opened {
                Ok(()) => (format!("Opened {DOCUMENTATION}").into(), theme.success),
                Err(why) => (
                    format!("Could not open the documentation: {why}").into(),
                    theme.danger,
                ),
            };
            page.child(
                div()
                    .id("about-status")
                    .debug_selector(|| "about-status".into())
                    .pt(px(10.))
                    .role(Role::Status)
                    .aria_label(text.clone())
                    .text_size(typography.row_subtitle_size)
                    .text_color(color)
                    .child(text),
            )
        })
        // The diagnostics: what Pane already knows of this installation,
        // copied to this computer's clipboard by the user's explicit
        // choice (see the module docs).
        .child(label("Diagnostics", &theme))
        .child(
            result_row(
                RowContent {
                    title: "Copy diagnostics".into(),
                    subtitle: Some("Copy what Pane knows of this installation".into()),
                    unavailable_reason: None,
                    unavailable_id: "about-diagnostics-unavailable".into(),
                    selected: false,
                    icon: Some((IconTone::Command, Glyph::Copy)),
                },
                &theme,
            )
            .id("about-diagnostics")
            // Pressed: the selected wash, one rung above the hover one,
            // fading on the shared pointer span.
            .active(|row| row.bg(theme.row_selected))
            .transitions(|fades| fades.bg(crate::ui::motion::pointer_fade()))
            .debug_selector(|| "about-diagnostics".into())
            .role(Role::Button)
            .aria_label("Copy diagnostics")
            .aria_description("Copy what Pane knows of this installation")
            .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                // The report is what the launcher holds as of this click,
                // not as of the frame that drew the row.
                let report = diagnostics(&this.launcher.application_update());
                cx.write_to_clipboard(ClipboardItem::new_string(report));
                this.about.copied = true;
                cx.notify();
            })),
        )
        .when(copied, |page| {
            page.child(
                div()
                    .id("about-diagnostics-status")
                    .debug_selector(|| "about-diagnostics-status".into())
                    .pt(px(10.))
                    .role(Role::Status)
                    .aria_label("Copied the diagnostics to the clipboard")
                    .text_size(typography.row_subtitle_size)
                    .text_color(theme.success)
                    .child("Copied the diagnostics to the clipboard"),
            )
        })
        .into_any_element()
}

/// The update section's row, as the page offers it to the user.
enum Action {
    /// No row: nothing to check against, or work already running.
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

/// A small muted label above one of the page's groups of rows, as the
/// other Settings pages label theirs.
fn label(text: &'static str, theme: &crate::ui::theme::Theme) -> Stateful<Div> {
    div()
        .id(text)
        .debug_selector(move || format!("about-label-{text}"))
        .pt(px(12.))
        .pb(px(2.))
        .text_size(theme.typography.row_subtitle_size)
        .font_weight(theme.typography.medium)
        .text_color(theme.text_muted)
        .child(text)
}
