//! The designed view's List and Grid, drawn (#240): the launcher's own
//! rows list under the header's search field, the Grid's cells, the empty
//! view when nothing is listed, and the detail pane beside the rows — the
//! selected item's `detail`, built for it by the extension through the
//! render context's `selected`. The rows come from the launcher's
//! presentation (`Screen::DesignedView`'s `list`, filling `LauncherView`'s
//! rows), drawn by the launcher's own list machinery in `app.rs`; what
//! only the tree names is drawn here: the author's own row subtrees, the
//! empty view, the detail pane's content, and the search-bar dropdown.
//!
//! The header is the search header's (the navigation title above the
//! field, which the List's field owns — #239's footer title replaced),
//! with the loading bar under it once the list's loading has run past its
//! threshold (300 ms; the bar's look is the launcher polish's, #248 —
//! this is the flag it rides). Backspace in the empty field pops the
//! stack, as the back key does (the general order is the launcher
//! polish's, #123).

use gpui::prelude::*;
use gpui::{AnyElement, Context, Role, SharedString, div, px, relative};

use pane_core::{DesignedList, DesignedViewSnapshot, Node, Space};

use crate::app::LauncherWindow;
use crate::ui::theme::Theme;
use crate::ui::tokens;

use super::reconcile::Held;
use super::tree::{self, Draw};

/// The pane the detail draws in, of the window's width.
const DETAIL_SHARE: f32 = 0.42;

/// The Drawing the presented list's own subtrees draw with: the window's
/// keyed state, the render the events carry, on the panel.
fn draw_of<'a>(
    window: &'a LauncherWindow,
    view: &'a DesignedViewSnapshot,
    theme: &'a Theme,
) -> Option<Draw<'a>> {
    let controls = window.designed.as_ref()?;
    Some(Draw {
        theme,
        state: &controls.state,
        render: view.render,
        surface: theme.panel_solid,
    })
}

impl LauncherWindow {
    /// The designed view's list screen's body, below the header: the rows
    /// list (the launcher's own, which `rows` builds — already dimmed
    /// under the Actions panel) with the detail pane beside it, the empty
    /// view when nothing is listed, or the Grid's cells.
    pub(crate) fn render_designed_list_body(
        &mut self,
        view: &DesignedViewSnapshot,
        list: &DesignedList,
        rows: AnyElement,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if list.grid {
            return self.render_designed_grid(view, list, cx);
        }
        if list.rows.is_empty() {
            // The empty view: the list's own subtree, when the tree names
            // one; else the launcher's own empty line, which the rows
            // list's head draws.
            return match self.render_designed_empty(view, list, cx) {
                Some(empty) => empty,
                None => rows,
            };
        }
        if list.detail.is_some() {
            // The detail pane beside the rows: the selected item's own
            // content, scrolled, on the panel's surface.
            let detail = self.render_designed_detail(view, list, cx);
            div()
                .id("designed-list-and-detail")
                .flex_1()
                .min_h(px(0.))
                .flex()
                .gap(tokens::space(Space::S))
                .child(div().flex_1().min_w(px(0.)).child(rows))
                .child(
                    div()
                        .id("designed-detail-pane")
                        .flex_none()
                        .w(relative(DETAIL_SHARE))
                        .min_h(px(0.))
                        .flex()
                        .flex_col()
                        .child(detail),
                )
                .into_any_element()
        } else {
            rows
        }
    }

    /// The empty view: the list's own subtree, drawn when nothing is
    /// listed; `None` when the tree names none (the launcher's own empty
    /// line says it instead).
    pub(crate) fn render_designed_empty(
        &mut self,
        view: &DesignedViewSnapshot,
        list: &DesignedList,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let empty = list.empty.as_deref()?;
        let visuals = crate::settings::launcher_visuals(cx);
        let draw = draw_of(self, view, &visuals.theme)?;
        let (_, _, empty_path) = self.designed_list_paths(view);
        let mut path = empty_path.unwrap_or_else(|| "/empty".into());
        let element = tree::draw_node(empty, &mut path, draw, cx);
        Some(
            div()
                .id("designed-empty-view")
                .flex_1()
                .min_h(px(0.))
                .flex()
                .flex_col()
                .justify_start()
                .pt(tokens::space(Space::M))
                .child(element)
                .into_any_element(),
        )
    }

    /// The detail pane's content: the selected item's `detail`, drawn as
    /// any node of the tree is, in a scrolled column on the card surface.
    pub(crate) fn render_designed_detail(
        &mut self,
        view: &DesignedViewSnapshot,
        list: &DesignedList,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(detail) = list.detail.as_deref() else {
            return div().into_any_element();
        };
        let visuals = crate::settings::launcher_visuals(cx);
        let Some(draw) = draw_of(self, view, &visuals.theme) else {
            return div().into_any_element();
        };
        let (item_paths, _, _) = self.designed_list_paths(view);
        let mut path = list
            .selected
            .as_deref()
            .and_then(|key| item_paths.iter().find(|(held, _)| held == key))
            .map(|(_, path)| format!("{path}/detail"))
            .unwrap_or_else(|| "/detail".into());
        let content = tree::draw_node(detail, &mut path, draw, cx);
        div()
            .id("designed-detail")
            .key_context("DesignedDetail")
            .flex_1()
            .min_h(px(0.))
            .flex()
            .flex_col()
            .overflow_y_scroll()
            .p(tokens::space(Space::M))
            .child(content)
            .into_any_element()
    }

    /// The author's own row subtree of the presented row `key`, drawn in
    /// the place of the standard row; `None` when the row is the standard
    /// one.
    pub(crate) fn render_designed_row(
        &mut self,
        view: &DesignedViewSnapshot,
        key: &str,
        selected: bool,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let list = screen_list_of(self)?;
        let row = list.rows.iter().find(|row| row.key == key)?;
        let content = row.content.as_ref()?;
        if content.is_empty() {
            return None;
        }
        let visuals = crate::settings::launcher_visuals(cx);
        let draw = draw_of(self, view, &visuals.theme)?;
        let (item_paths, _, _) = self.designed_list_paths(view);
        let mut path = item_paths
            .iter()
            .find(|(held, _)| held == key)
            .map(|(_, path)| format!("{path}/row"))
            .unwrap_or_else(|| format!("/{key}/row"));
        let duplicates = duplicate_keys_of(content);
        let mut drawn = Vec::new();
        for (index, child) in content.iter().enumerate() {
            let start = path.len();
            tree::place_child_owned(&mut path, child, index, &duplicates);
            drawn.push(tree::draw_node(child, &mut path, draw, cx));
            path.truncate(start);
        }
        let wash = visuals.theme.selection_wash;
        Some(
            div()
                .id(format!("designed-row-{key}"))
                .flex()
                .flex_col()
                .min_w(px(0.))
                .when(selected, |row| row.bg(wash))
                .children(drawn)
                .into_any_element(),
        )
    }

    /// The Grid: the tree's cells in sections, each section's cells in
    /// its own columns, the selected cell washed as the launcher's rows
    /// are. Not virtualised: a grid's cells are image-sized, and the
    /// scroll holds them whole.
    pub(crate) fn render_designed_grid(
        &mut self,
        view: &DesignedViewSnapshot,
        list: &DesignedList,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let visuals = crate::settings::launcher_visuals(cx);
        let theme = visuals.theme;
        let geometry = &theme.geometry;
        // The cells' titles and subtitles are the launcher's rows', which
        // the designed rows sit beside.
        let titles: Vec<(String, Option<String>)> = self
            .launcher
            .view()
            .rows
            .iter()
            .map(|row| (row.title.clone(), row.subtitle.clone()))
            .collect();
        // The cells' titles and subtitles are the launcher's rows', which
        // the designed rows sit beside.

        let mut sections = Vec::new();
        let mut at = 0;
        let mut rest = list.rows.len();
        for section in &list.sections {
            // The section's rows run to the next section's first row.
            let end = list
                .sections
                .iter()
                .map(|next| next.first)
                .filter(|first| *first > section.first)
                .min()
                .unwrap_or(list.rows.len());
            let rows = &list.rows[section.first.min(list.rows.len())..end.min(list.rows.len())];
            rest = rest.saturating_sub(rows.len());
            at += rows.len();
            if rows.is_empty() {
                continue;
            }
            sections.push(self.render_grid_section(view, list, &titles, section, rows, cx));
        }
        if at < list.rows.len() {
            // Cells with no section above them (a grid whose sections
            // carry no titles).
            let rows = &list.rows[at..];
            let section = pane_core::Section {
                label: String::new(),
                note: None,
                first: at,
            };
            sections.push(self.render_grid_section(view, list, &titles, &section, rows, cx));
        }
        let empty = if list.rows.is_empty() {
            self.render_designed_empty(view, list, cx)
        } else {
            None
        };
        div()
            .id("designed-grid-view")
            .key_context("DesignedGrid")
            .flex_1()
            .min_h(px(0.))
            .flex()
            .flex_col()
            .overflow_y_scroll()
            .pt(geometry.list_padding_top)
            .pb(geometry.list_padding_bottom)
            .children(sections)
            .when_some(empty, |grid, empty| grid.child(empty))
            .into_any_element()
    }

    /// One section of the Grid: its label over its cells, wrapped into
    /// the section's columns.
    fn render_grid_section(
        &mut self,
        view: &DesignedViewSnapshot,
        list: &DesignedList,
        titles: &[(String, Option<String>)],
        section: &pane_core::Section,
        rows: &[pane_core::DesignedRow],
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let visuals = crate::settings::launcher_visuals(cx);
        let theme = visuals.theme;
        let mut cells = Vec::new();
        for row in rows {
            let index = list
                .rows
                .iter()
                .position(|held| held.key == row.key)
                .unwrap_or(0);
            let selected = list.selected.as_deref() == Some(row.key.as_str());
            cells.push(self.render_designed_cell(view, list, titles, index, selected, cx));
        }
        let shape = rows.first().map(|row| row.shape).unwrap_or_default();
        let label = (!section.label.is_empty()).then(|| {
            crate::ui::shell::section_label(
                SharedString::from(section.label.clone()),
                section.note.clone().map(SharedString::from),
                &theme,
            )
        });
        let grid = div()
            .id(format!("designed-grid-{}", section.first))
            .flex()
            .flex_wrap()
            .gap(tokens::space(Space::S))
            .when(shape.inset, |grid| grid.px(tokens::space(Space::Xs)))
            .children(cells);
        div()
            .id(format!("designed-grid-section-{}", section.first))
            .flex()
            .flex_col()
            .min_w(px(0.))
            .gap(tokens::space(Space::Xs))
            .when_some(label, |column, label| column.child(label))
            .child(grid)
            .into_any_element()
    }

    /// One cell of the Grid: its image, colour or the author's own
    /// subtree, with its title and subtitle under it, the selected cell
    /// washed, a click selecting and activating it.
    fn render_designed_cell(
        &mut self,
        view: &DesignedViewSnapshot,
        list: &DesignedList,
        titles: &[(String, Option<String>)],
        index: usize,
        selected: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(row) = list.rows.get(index) else {
            return div().into_any_element();
        };
        let visuals = crate::settings::launcher_visuals(cx);
        let theme = visuals.theme;
        let draw = draw_of(self, view, &theme);
        let shape = row.shape;
        let columns = shape.columns.max(1) as f32;
        let width = relative(1. / columns);
        let drawn = draw.as_ref().zip(row.image.as_ref()).map(|(draw, image)| {
            let path = format!("designed-cell-{}", row.key);
            super::components::grid_image(image, shape.fit, &path, draw)
        });
        let drawn = match drawn {
            Some(drawn) => Some(drawn),
            None => row.color.map(|paint| {
                let fill = tokens::paint_color(&paint, &theme);
                div()
                    .id(format!("designed-cell-color-{}", row.key))
                    .size_full()
                    .rounded(theme.geometry.row_radius)
                    .bg(fill)
                    .into_any_element()
            }),
        };
        // The author's own cell content, drawn as the row subtree it is.
        let drawn = match drawn {
            Some(drawn) => Some(drawn),
            None => self.render_designed_row(view, &row.key, selected, cx),
        };
        let key = row.key.clone();
        let title: Option<SharedString> = titles
            .get(index)
            .map(|(title, _)| title.clone())
            .filter(|title| !title.is_empty())
            .map(SharedString::from);
        let subtitle: Option<String> = titles.get(index).and_then(|(_, subtitle)| subtitle.clone());
        let aspect = shape.aspect_ratio;
        let cell = div()
            .id(format!("designed-cell-{}", row.key))
            .debug_selector(move || format!("designed-cell-{}", key))
            .flex()
            .flex_col()
            .min_w(px(0.))
            .w(width)
            .gap(tokens::space(Space::Xs))
            .when_some(
                aspect.map(|pane_core::Finite(ratio)| ratio),
                |cell, ratio| cell.aspect_ratio(ratio),
            )
            .map(|cell| match drawn {
                Some(drawn) => cell.child(
                    div()
                        .id(format!("designed-cell-image-{}", row.key))
                        .flex_1()
                        .min_h(px(0.))
                        .overflow_hidden()
                        .rounded(theme.geometry.row_radius)
                        .child(drawn),
                ),
                None => cell,
            })
            .when(title.is_some() || subtitle.is_some(), |cell| {
                cell.child(
                    div()
                        .flex()
                        .flex_col()
                        .min_w(px(0.))
                        .gap(tokens::space(Space::Xs))
                        .when_some(title, |cell, title| {
                            cell.child(
                                div()
                                    .id(format!("designed-cell-title-{}", row.key))
                                    .min_w(px(0.))
                                    .truncate()
                                    .text_size(theme.typography.row_subtitle_size)
                                    .text_color(if selected {
                                        theme.text_title
                                    } else {
                                        theme.text_body
                                    })
                                    .role(Role::Label)
                                    .map(|label| label.aria_label(title.clone()))
                                    .child(title),
                            )
                        })
                        .when_some(subtitle, |cell, subtitle| {
                            cell.child(
                                div()
                                    .id(format!("designed-cell-subtitle-{}", row.key))
                                    .min_w(px(0.))
                                    .truncate()
                                    .text_size(theme.typography.row_kind_size)
                                    .text_color(theme.text_muted)
                                    .child(subtitle),
                            )
                        }),
                )
            });
        let pressable = !row.actions.is_empty();
        let selected_wash = theme.selection_wash;
        let cell = cell
            .map(|cell| {
                cell.when(pressable, |cell| {
                    cell.cursor_pointer()
                        .when(selected, |cell| cell.bg(selected_wash))
                })
            })
            .role(Role::ListBoxOption)
            .map(|cell| {
                cell.aria_label(
                    titles
                        .get(index)
                        .map(|(title, _)| title.clone())
                        .filter(|title| !title.is_empty())
                        .unwrap_or_else(|| "cell".to_owned()),
                )
            })
            .aria_selected(selected);
        if !pressable {
            return cell.into_any_element();
        }
        cell.on_click(
            cx.listener(move |this, event: &gpui::ClickEvent, window, cx| {
                if event.click_count() <= 1 {
                    // A double click's second click runs nothing more.
                    this.launcher.select(index);
                    this.announcer.user_moved();
                    this.activate_selected(window, cx);
                    this.motion.pointer_open();
                }
            }),
        )
        .into_any_element()
    }

    /// The paths of the presented list's items (keyed as its rows name
    /// them), its search-bar dropdown and its empty view, by the tree
    /// walk's grammar: where the tree holds what only the tree names.
    pub(super) fn designed_list_paths(
        &self,
        view: &DesignedViewSnapshot,
    ) -> (Vec<(String, String)>, Option<String>, Option<String>) {
        let Some((list_path, node)) = tree::find_list(&view.tree) else {
            return (Vec::new(), None, None);
        };
        tree::list_paths(node, &list_path)
    }

    /// Opens the presented list's search-bar dropdown (Ctrl+P, #497), its
    /// popup taking the keyboard; `false` when the screen shows none.
    pub(crate) fn open_designed_dropdown(
        &mut self,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let pane_core::Screen::DesignedView(view) = self.launcher.screen() else {
            return false;
        };
        if view
            .list
            .as_ref()
            .is_none_or(|list| list.dropdown.is_none())
        {
            return false;
        }
        let (_, dropdown_path, _) = self.designed_list_paths(&view);
        let path = dropdown_path.unwrap_or_else(|| "/dropdown".into());
        let select = self
            .designed
            .as_ref()
            .and_then(|controls| controls.state.get(&path))
            .and_then(|state| match &state.held {
                Held::Select { select, .. } => Some(select.clone()),
                _ => None,
            });
        let Some(select) = select else {
            return false;
        };
        // Deferred: the select's model reads this window, which is being
        // updated while the key's action runs.
        window.defer(cx, move |window, cx| {
            select.update(cx, |select, cx| select.open(window, cx));
        });
        true
    }

    /// The search-bar dropdown of the presented list, drawn in the header
    /// beside the search field; `None` when the tree names none. The
    /// dropdown is the searchable select, keyed by its place in the tree,
    /// so its open state, query and highlight survive a re-render that
    /// still draws it.
    pub(crate) fn render_designed_dropdown(
        &mut self,
        view: &DesignedViewSnapshot,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let list = screen_list_of(self)?;
        list.dropdown.as_ref()?;
        let controls = self.designed.as_ref()?;
        let (_, dropdown_path, _) = self.designed_list_paths(view);
        let path = dropdown_path.unwrap_or_else(|| "/dropdown".into());
        let entity = controls
            .state
            .get(&path)
            .and_then(|state| match &state.held {
                Held::Select { select, .. } => Some(select.clone()),
                _ => None,
            })?;
        let _ = cx;
        Some(
            div()
                .id("designed-dropdown")
                .flex_none()
                .flex()
                .child(entity.clone())
                .into_any_element(),
        )
    }
}

/// The current screen's presented list, when it names one.
pub(super) fn screen_list_of(window: &LauncherWindow) -> Option<DesignedList> {
    let pane_core::Screen::DesignedView(view) = window.launcher.screen() else {
        return None;
    };
    view.list.clone()
}

/// The keys the children share with a sibling, as `tree::duplicate_keys`
/// reads a parent's: matched by position instead.
fn duplicate_keys_of(children: &[Node]) -> std::collections::HashSet<String> {
    let mut counts: std::collections::HashMap<&str, u32> = std::collections::HashMap::new();
    for child in children {
        if let Some(key) = child.key.as_deref() {
            *counts.entry(key).or_default() += 1;
        }
    }
    counts
        .into_iter()
        .filter(|(_, count)| *count > 1)
        .map(|(key, _)| key.to_owned())
        .collect()
}
