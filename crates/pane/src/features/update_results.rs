//! The update results view (#256): what the last update pass that
//! recorded came to, per extension, as a screen of the launcher's —
//! reached from a failure toast's View Details, which Pane shows once the
//! next time the launcher is shown after a pass failed something, and
//! from the Extensions group in Settings, whose operation opens it (ADR
//! 0043: Settings draws it in place of the page).
//!
//! The launcher holds the screen ([`pane_core::Screen::UpdateResults`])
//! and its rows — the groups Updated, Skipped, Failed, each row the
//! extension's own, searched by what the user types through the search
//! field root search's is ([`pane_core::Launcher::set_query`]) — and the
//! window draws them here: each row the extension's icon, its title, its
//! detail and a status tag, the groups' labels over their rows. The
//! list works as any list does (see `docs/root-search.md`): the arrows
//! move the selection, Enter opens the selected extension's page in
//! Settings, and the Actions panel offers that and copying the row's
//! details, to report them to the extension's author.

use std::rc::Rc;

use gpui::{
    AnyElement, ClickEvent, ClipboardItem, Context, Div, Role, SharedString, Stateful, Window, div,
    prelude::*, px,
};
use pane_core::{Row, Screen, Status, UpdateResultsAction};

use crate::app::{LauncherWindow, Spot, section_labels};
use crate::features::announcer::section_at;
use crate::ui::result_row::{AccessoryLook, RowContent, RowMeta, result_row_with};
use crate::ui::shell::{self, SectionLabel};
use crate::ui::virtual_list::{self, ListChild, VirtualList};

/// The view's search field's placeholder.
pub(crate) const PLACEHOLDER: &str = "Search update results…";

/// What the view says while no results are recorded at all.
const NONE_YET: &str = "No update results yet. The next check that updates, skips or fails \
                        anything records them here.";

/// The update results view's state, owned by the launcher window while
/// the launcher shows the screen. The rows, their order and the
/// selection are the launcher's (the view's own state is the list's).
pub(crate) struct UpdateResultsView {
    /// The list, drawn virtually (#165): only the rows in view are laid
    /// out and painted.
    list: VirtualList,
    /// What a frame draws, as the last frame read it: its children are
    /// drawn from it as the list lays them out.
    frame: Option<Rc<ResultsFrame>>,
    /// The row selected as the last frame laid the list out, to reveal
    /// the newly selected one.
    selected: Option<usize>,
    /// Whether the status line says what one of the view's actions came
    /// to (Copy details), which goes once the user moves on.
    outcome: bool,
}

/// What a frame draws in the view's list.
struct ResultsFrame {
    /// The rows, in the groups' order, the empty groups hidden.
    rows: Vec<Row>,
    /// Their groups' labels.
    sections: Vec<SectionLabel>,
    /// The list's children: the labels and the rows.
    children: Vec<ListChild>,
    /// The selected row's index; `None` only with none listed.
    selected: Option<usize>,
}

impl LauncherWindow {
    /// Test support: whether the update results view shows.
    #[doc(hidden)]
    pub fn update_results_shown(&self) -> bool {
        self.update_results.is_some()
    }

    /// Makes the view follow the launcher's screen: it opens while the
    /// launcher shows the update results, and closes once the launcher
    /// leaves them. The screen's search field and the selection are the
    /// launcher's, driven by its own keys; nothing here takes focus.
    pub(crate) fn sync_update_results(&mut self, cx: &mut Context<Self>) {
        if !matches!(self.launcher.screen(), Screen::UpdateResults { .. }) {
            self.update_results = None;
            return;
        }
        if self.update_results.is_none() {
            let geometry = &crate::settings::launcher_visuals(cx).theme.geometry;
            self.update_results = Some(UpdateResultsView {
                list: VirtualList::new(geometry.row_min_height + geometry.row_list_gap),
                frame: None,
                selected: None,
                outcome: false,
            });
        }
    }

    /// The view's list, under its search field: the groups' labels over
    /// their rows, drawn virtually, or what the view says while it lists
    /// nothing.
    pub(crate) fn render_update_results(&mut self, cx: &mut Context<Self>) -> Stateful<Div> {
        // The view is read with the screen; the render of a frame the
        // sync missed (the screen changed on another thread since) makes
        // it here, with nothing to redraw beyond this frame.
        self.sync_update_results(cx);
        let theme = crate::settings::launcher_visuals(cx).theme;
        let (view, listing) = self.launcher.presented_list();
        let sections = section_labels(&listing);
        let rows = view.rows;
        let selected = view.selected;
        let children = virtual_list::children(false, rows.len(), &sections);
        let frame = Rc::new(ResultsFrame {
            rows,
            sections,
            children,
            selected,
        });
        if let Some(shown) = self.update_results.as_mut() {
            if shown.list.count() != frame.children.len() {
                shown.list.reset(frame.children.len());
            }
            if shown.selected != selected {
                if let Some(row) = selected {
                    shown
                        .list
                        .reveal(virtual_list::child_of_row(false, &frame.sections, row));
                }
                shown.selected = selected;
            }
            shown.frame = Some(frame.clone());
        }
        let list = shell::result_list(&theme).aria_label("Update results");
        if frame.children.is_empty() {
            // No results are recorded at all, or the search lists none of
            // them.
            let query = view.screen.search_field().unwrap_or("");
            let note = if query.trim().is_empty() {
                NONE_YET.to_owned()
            } else {
                format!("No results for “{}”", query.trim())
            };
            list.child(
                div()
                    .id("results-empty")
                    .debug_selector(|| "results-empty".into())
                    .pt(theme.geometry.list_padding_top)
                    .px(theme.geometry.list_padding_x)
                    .pb(theme.geometry.list_padding_bottom)
                    .text_size(theme.typography.row_subtitle_size)
                    .text_color(theme.text_muted)
                    .child(note),
            )
        } else {
            let state = self
                .update_results
                .as_ref()
                .expect("the view follows the screen")
                .list
                .state()
                .clone();
            list.child(
                gpui::list(
                    state,
                    cx.processor(|this, index, _: &mut Window, cx| {
                        this.render_results_child(index, cx)
                    }),
                )
                .flex_1()
                .min_h(px(0.))
                .w_full()
                .pt(theme.geometry.list_padding_top)
                .pb(theme.geometry.list_padding_bottom),
            )
        }
    }

    /// The view's list child at `index` of the frame laid out, as the
    /// list draws it: a group's label, or a row — the extension's icon,
    /// its title, its detail and a status tag — with the gap after it
    /// and the list's side padding.
    fn render_results_child(&mut self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(frame) = self
            .update_results
            .as_ref()
            .and_then(|shown| shown.frame.clone())
        else {
            return div().into_any_element();
        };
        let theme = crate::settings::launcher_visuals(cx).theme;
        let child = match frame.children.get(index) {
            Some(&ListChild::Label(at)) => {
                let section = &frame.sections[at];
                let debug = format!("section-{}", section.label);
                shell::section_label(section.label.clone(), section.note.clone(), &theme)
                    .debug_selector(move || debug)
                    .into_any_element()
            }
            Some(&ListChild::Row(at)) => match frame.rows.get(at) {
                Some(row) => {
                    let selected = frame.selected == Some(at);
                    let drawn = self.render_result_row(row, at, selected, &frame.sections, cx);
                    drawn.into_any_element()
                }
                None => div().into_any_element(),
            },
            Some(ListChild::Head) | None => div().into_any_element(),
        };
        let last = index + 1 >= frame.children.len();
        virtual_list::item(
            child,
            last,
            theme.geometry.row_list_gap,
            theme.geometry.list_padding_x,
        )
        .into_any_element()
    }

    /// The view's row at `index`: the extension's icon, its title, its
    /// detail as the subtitle, and its group as a status tag at the
    /// right. A click selects the row, as a click on any list's does, and
    /// opens its extension's page, as Enter does.
    fn render_result_row(
        &self,
        row: &Row,
        index: usize,
        selected: bool,
        sections: &[SectionLabel],
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let theme = crate::settings::launcher_visuals(cx).theme;
        let icon = crate::features::icons::row_icon_of(&self.launcher, &row.id, &theme);
        // The rows are a list whose hovering does not move the selection:
        // an unselected row under the pointer takes the hover wash, fading
        // once it leaves (#245, `crate::app::hover_wash`).
        let hover = self
            .motion
            .hover
            .look(Spot::Row(index), cx.background_executor().now());
        // The row's status tag: the group its row is listed under.
        let (tag, tone) = match section_at(sections, index).as_deref() {
            Some("Skipped") => ("Skipped", theme.text_muted),
            Some("Failed") => ("Failed", theme.danger),
            _ => ("Updated", theme.success),
        };
        result_row_with(
            RowContent {
                title: row.title.clone().into(),
                subtitle: row.subtitle.clone().map(SharedString::from),
                unavailable_reason: None,
                selected,
                unavailable_id: ("unavailable", index).into(),
                hover,
                icon: Some(icon),
            },
            RowMeta {
                accessories: vec![AccessoryLook {
                    text: tag.into(),
                    tag: true,
                    color: tone,
                    icon: None,
                    tooltip: row.subtitle.clone().map(SharedString::from),
                }],
                ..RowMeta::default()
            },
            &theme,
        )
        .id(("results-row", index))
        .debug_selector(move || format!("results-row-{}", row.title))
        .role(Role::ListBoxOption)
        .aria_label(row.title.clone())
        .aria_selected(selected)
        .aria_position_in_set(index + 1)
        .when(selected, |row| row.aria_active_descendant())
        .on_hover(cx.listener(move |this, over: &bool, _, cx| {
            this.motion.hover.set(Spot::Row(index), *over, cx);
        }))
        .when_some(row.subtitle.clone(), |element, subtitle| {
            element.aria_description(subtitle)
        })
        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
            this.launcher.select(index);
            this.announcer.user_moved();
            this.moved_on_in_results(cx);
            this.activate_selected(window, cx);
            this.motion.pointer_open();
        }))
    }

    /// The user moved on: the status line is at rest again, its Copy
    /// details outcome gone.
    fn moved_on_in_results(&mut self, cx: &mut Context<Self>) {
        if let Some(shown) = self.update_results.as_mut()
            && std::mem::take(&mut shown.outcome)
        {
            self.launcher.show_status(Status::Idle);
        }
        cx.notify();
    }

    /// Runs `action`, one of the view's Actions panel entries, on the row
    /// it opened for (by the extension's identity key): opening its
    /// extension's page in Settings, as Enter does; copying its details to
    /// the clipboard; or checking that extension alone for an update and
    /// updating what it finds — Retry on a Failed row, Update Now on a
    /// Skipped row whose only reason is the user's switch — an asked pass
    /// whose toast follows it as any asked pass's.
    pub(crate) fn run_update_results_action(
        &mut self,
        action: UpdateResultsAction,
        target: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let view = self.launcher.view();
        let Some(index) = view.rows.iter().position(|row| row.id == target) else {
            return;
        };
        match action {
            UpdateResultsAction::ShowExtension => {
                self.launcher.select(index);
                self.announcer.user_moved();
                self.moved_on_in_results(cx);
                self.activate_selected(window, cx);
            }
            UpdateResultsAction::Retry | UpdateResultsAction::UpdateNow => {
                self.moved_on_in_results(cx);
                let pending = self.launcher.check_extension_update_of(target);
                self.show_until_done(pending, window, cx);
            }
            UpdateResultsAction::CopyDetails => {
                let copied = format!(
                    "{}: {}",
                    view.rows[index].title,
                    view.rows[index].subtitle.clone().unwrap_or_default()
                );
                cx.write_to_clipboard(ClipboardItem::new_string(copied));
                self.launcher
                    .show_status(Status::Result("Copied the details".into()));
                if let Some(shown) = self.update_results.as_mut() {
                    shown.outcome = true;
                }
                cx.notify();
            }
        }
    }
}
