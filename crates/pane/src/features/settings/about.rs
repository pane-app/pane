//! The About page: the one Settings page this milestone ships, holding
//! the real version of the running Pane and the documentation entry.
//!
//! Both are honest by construction: the version is
//! [`crate::APP_VERSION`], the same value `pane --version` prints and an
//! application update compares itself with, and the documentation entry
//! opens Pane's repository with the link opener the app gave the
//! launcher — the same handler quicklinks open with, not a second
//! instance. Opening runs off the window's thread, as every link
//! opening does, and what it reported is shown as the page's status.

use gpui::{AnyElement, Context, Hsla, Role, SharedString, Window, div, prelude::*, px};

use super::{Page, SettingsWindow};
use crate::ui;
use crate::ui::icon::{Glyph, IconTone};
use crate::ui::result_row::{RowContent, result_row};

/// The documentation entry's address: Pane's repository, whose README is
/// the documentation of this build.
const DOCUMENTATION: &str = "https://github.com/hoangvu12/pane";

/// The About page, registered first in the window's page list.
pub(crate) fn page() -> Page {
    Page {
        title: "About",
        icon: (IconTone::Command, Glyph::Gear),
        render,
    }
}

/// The About page's state, held by the window: what opening the
/// documentation entry last reported, if anything.
#[derive(Default)]
pub(crate) struct State {
    opened: Option<Result<(), String>>,
}

impl State {
    /// Records what opening the documentation entry reported.
    pub(crate) fn record(&mut self, opened: Result<(), String>) {
        self.opened = Some(opened);
    }
}

/// Draws the About page: the pane's name, the running version and the
/// documentation entry, with what opening the entry last reported.
fn render(
    this: &mut SettingsWindow,
    _window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let theme = ui::visuals().theme.clone();
    let typography = &theme.typography;
    let opened = this.about.opened.clone();
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
        .child(
            // The documentation entry: a navigation row like the
            // launcher's results, opening Pane's repository with the
            // launcher's link opener.
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
            .debug_selector(|| "about-documentation".into())
            .role(Role::Link)
            .aria_label("Documentation")
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
        .into_any_element()
}
