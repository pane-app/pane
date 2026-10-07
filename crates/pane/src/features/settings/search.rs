//! The Settings window's search: one field in the sidebar that finds the
//! settings the pages registered and jumps to them.
//!
//! The field is the window's one search input, reached three ways: the
//! pointer clicks it, Tab traversal passes through it, and Cmd+F /
//! Ctrl+F focuses it from anywhere in the Settings window (the keys are
//! the reference's, bound in the window's own context so they reach
//! nothing in the launcher window behind it). It is a boxed editable
//! text field with the magnifier, as the Shortcuts page's filter is, and
//! an editable combo box to assistive technology, as root search's field
//! is: the list under it is its list.
//!
//! While it holds a query, the sidebar's list shows the search's results
//! instead of the sections; each result names the setting and where it
//! lives — "Dark", "General · Theme" — and an unavailable one says
//! why, as the control itself does on its page. The arrows move the
//! selection (Enter opens it; a click does the same), Enter on no result
//! does nothing, and the selection never moves by itself. Escape clears
//! a query; with none, it leaves the search and returns the keyboard to
//! the sections. Page navigation — the sections' keys or clicks — clears
//! the query too, so the sidebar never strays from what the keyboard
//! focus is on.
//!
//! Matching and ranking are the core's, [`pane_core::settings_matches`]:
//! the same normalize-and-rank that matches root search's results, over
//! the Settings window's own catalog. The catalog is not an index over
//! extension data: every entry is a real, registered host setting or
//! section, read live as the pages register them, so a control that
//! appears on a page (a package installed, a setting offered) is in the
//! search with it and one that goes is gone — never a placeholder, never
//! a stale row. The window recomputes the results every frame (as the
//! Shortcuts page rereads its catalog) and its watcher asks for a
//! redraw when a change arrives while the user does nothing, so the
//! results follow the launcher's packages even mid-query.
//!
//! A jump opens the entry's page and clears the query. The control is
//! focused where it takes focus — the Shortcuts page's filter field, a
//! shortcut recorder, the Launcher and Keyboard pages' selects — and
//! revealed where it does not, as the General page's theme and material
//! choices and the extension rows are: the reveal scrolls the page area
//! to the control through a GPUI [`ScrollAnchor`], after the frame that
//! paints the jumped-to page (the anchor records where the control drew
//! there), so it never scrolls on a stale position. A control that no longer
//! exists still opens its page — nothing is focused for it, nothing
//! scrolls, and the sidebar keeps the keyboard on the entry's page.
//! Where no control took the focus, the sidebar holds it, on the page
//! the jump opened: page navigation, restored.
//!
//! Nothing here animates: typing, selection, focus, dispatch and the
//! jump are immediate — the scroll is a single reveal, not a transition
//! (the motion policy's rules for query and result updates).

use std::collections::HashMap;

use gpui::{
    App, Context, Div, Entity, Focusable, KeyBinding, Role, ScrollAnchor, ScrollHandle,
    SharedString, Stateful, Subscription, Window, actions, div, prelude::*,
};
use gpui_elements::editable_text::actions::DEFAULT_INPUT_CONTEXT;
use gpui_elements::editable_text::{EditableTextState, StringStorage, TextChanged};
use pane_core::Launcher;

use super::{Page, SettingsWindow};
use crate::ui::settings_shell::{self, SidebarItem};
use crate::ui::theme::Theme;

/// The search field's key context: the results' keys are bound in it,
/// above the editable text element's own context, so they take the
/// keystrokes only while the search field is focused (the editable
/// element would otherwise take the arrows as caret movement, as it
/// should while the user edits the query).
const FIELD: &str = "SettingsSearch";

/// The field's placeholder, and its accessible name.
const PLACEHOLDER: &str = settings_shell::SEARCH_PLACEHOLDER;

actions!(
    settings_search,
    [
        FocusSearch,
        NextResult,
        PreviousResult,
        OpenResult,
        LeaveSearch
    ]
);

/// Registers the search's key bindings: the window-wide focus key, and —
/// in the field's own context, deeper in the focus stack than the
/// editable text element's — the results' keys. Enter and Escape are not
/// text edits here (the app leaves them unbound for every editable
/// element), so they reach the field's actions the same way the Shortcuts
/// page's inline editor's reach theirs.
pub(crate) fn bind_keys(cx: &mut App) {
    let field = format!("{FIELD} > {DEFAULT_INPUT_CONTEXT}");
    cx.bind_keys([
        // The reference's find key, bound in the window's own context so
        // it reaches the field wherever the keyboard is in Settings, and
        // nothing in another window.
        KeyBinding::new(
            if cfg!(target_os = "macos") {
                "cmd-f"
            } else {
                "ctrl-f"
            },
            FocusSearch,
            Some(super::CONTEXT),
        ),
        KeyBinding::new("down", NextResult, Some(&field)),
        KeyBinding::new("up", PreviousResult, Some(&field)),
        KeyBinding::new("enter", OpenResult, Some(&field)),
        KeyBinding::new("escape", LeaveSearch, Some(&field)),
    ]);
}

/// One entry the Settings search can find: a control on a page, or the
/// page itself (whose entry has no control). A page registers its own
/// entries through [`Page::search`] — read live, so a control that
/// appears or goes on the page is in or out of the search with it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Entry {
    /// The control's stable id, the jump target, as the page's own render
    /// keys its scroll anchor. `None` for a page's own entry, which jumps
    /// to the page and its sidebar row.
    pub(crate) control: Option<String>,
    /// What the query matches and the result shows as its title: the
    /// control's name, as its page shows it.
    pub(crate) title: String,
    /// What the query matches beside the title and the result shows under
    /// it: the group the control sits in on the page ("Theme"), or — for
    /// a page's own entry — the page's one-line description.
    pub(crate) group: Option<String>,
    /// Why the control cannot be used here, if it cannot. The result says
    /// so and stays listed, as the control does on its page; the jump
    /// still reveals it, with the page's own explanation beside it.
    pub(crate) unavailable: Option<String>,
}

/// One result of the search: an entry, and the page it is on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Hit {
    /// The page the entry is on, an index into the window's page list.
    pub(crate) page: usize,
    /// The entry itself.
    pub(crate) entry: Entry,
}

/// The search's state, held by the window as a field.
pub(crate) struct State {
    /// The search field.
    query: Entity<EditableTextState>,
    _query_changes: Subscription,
    /// The results for the query as the last frame drew them, best match
    /// first; empty while the query is blank, when the sidebar lists the
    /// sections instead.
    results: Vec<Hit>,
    /// The selected result, an index into `results`.
    selected: usize,
    /// The page area's scroll container, which the reveal scrolls.
    scroll: ScrollHandle,
    /// Each control's scroll anchor, by target id, as the pages drew them
    /// this frame: the window's page render clears them and the page
    /// fills them in again, so a reveal only ever scrolls a control that
    /// is on the page now showing.
    anchors: HashMap<String, ScrollAnchor>,
}

impl State {
    /// The search's state: an empty field, no results, nothing revealed.
    pub(crate) fn new(cx: &mut Context<SettingsWindow>) -> State {
        let query = cx.new(|cx| EditableTextState::new(StringStorage::default(), cx));
        query.focus_handle(cx).tab_stop(true);
        let _query_changes = cx.subscribe(&query, |this, _, _: &TextChanged, cx| {
            // A new query is a new search: the first result is selected,
            // and the frame redraws the sidebar with what it finds.
            this.search.selected = 0;
            cx.notify();
        });
        State {
            query,
            _query_changes,
            results: Vec::new(),
            selected: 0,
            scroll: ScrollHandle::new(),
            anchors: HashMap::new(),
        }
    }

    /// The scroll anchor for the control `target`, created when the
    /// control first draws. A page's render calls this for each control
    /// the search can reveal and attaches the anchor to it, so the anchor
    /// records where the control drew.
    pub(crate) fn anchor(&mut self, target: &str) -> ScrollAnchor {
        self.anchors
            .entry(target.to_owned())
            .or_insert_with(|| ScrollAnchor::for_handle(self.scroll.clone()))
            .clone()
    }

    /// Recomputes the results for the query as it stands, so the sidebar
    /// and its keys see the same ones: the catalog is read live (a
    /// package the launcher window installed, a choice overridden) and
    /// matched by the core, which ranks as root search does. Called from
    /// the window's render, every frame.
    pub(crate) fn refresh(&mut self, launcher: &Launcher, pages: &[Page], cx: &App) {
        let query = self.query.read(cx).as_str().to_owned();
        self.results = results(launcher, pages, &query, cx);
        self.selected = self.selected.min(self.results.len().saturating_sub(1));
    }

    /// Whether a query is showing — the sidebar lists the search's
    /// results rather than the sections. The field's text decides: a
    /// blank query is no query.
    pub(crate) fn searching(&self, cx: &App) -> bool {
        !self.query.read(cx).as_str().trim().is_empty()
    }

    /// The page area's scroll container, which the reveal scrolls: the
    /// window's page render tracks it.
    pub(crate) fn scroll(&self) -> &ScrollHandle {
        &self.scroll
    }

    /// Drops the controls' scroll anchors, as the window's page render
    /// does before the page about to draw fills them in again — so a
    /// reveal only ever scrolls a control on the page now showing.
    pub(crate) fn clear_anchors(&mut self) {
        self.anchors.clear();
    }
}

/// The results for `query` over what the pages register now: each page's
/// own entry (its title and description) and the controls it registers,
/// matched and ranked by the core. A blank query has no results — the
/// sidebar lists the sections, and only a query the user typed searches.
fn results(launcher: &Launcher, pages: &[Page], query: &str, cx: &App) -> Vec<Hit> {
    if query.trim().is_empty() {
        return Vec::new();
    }
    let registered = catalog(launcher, pages, cx);
    let entries = registered
        .iter()
        .map(|(page, entry)| pane_core::SettingsEntry {
            title: entry.title.clone(),
            group: entry.group.clone(),
            page: pages[*page].title.into(),
        })
        .collect::<Vec<_>>();
    pane_core::settings_matches(query, &entries)
        .into_iter()
        .map(|index| Hit {
            page: registered[index].0,
            entry: registered[index].1.clone(),
        })
        .collect()
}

/// Everything the pages register for the search: each page's own entry,
/// then the controls the page registers. Read live on every search, so a
/// control that appears on a page (a package installed, a setting
/// offered) is in the next catalog and one that goes is gone — the search
/// never lists a control the page no longer shows.
fn catalog(launcher: &Launcher, pages: &[Page], cx: &App) -> Vec<(usize, Entry)> {
    let mut registered = Vec::new();
    for (index, page) in pages.iter().enumerate() {
        registered.push((
            index,
            Entry {
                control: None,
                title: page.title.into(),
                group: Some(page.about.into()),
                unavailable: None,
            },
        ));
        registered.extend(
            (page.search)(launcher, cx)
                .into_iter()
                .map(|entry| (index, entry)),
        );
    }
    registered
}

impl SettingsWindow {
    /// The scroll anchor for the control `target`, created when the
    /// control first draws. A page's render calls this for each control
    /// the search can reveal and attaches the anchor to it, so the
    /// anchor records where the control drew.
    pub(crate) fn search_anchor(&mut self, target: &str) -> ScrollAnchor {
        self.search.anchor(target)
    }

    /// Test support: the search field, as the window's other field
    /// accessors; a platform input method talks to it while composing
    /// text, and tests read what it holds.
    #[doc(hidden)]
    pub fn search_field(&self) -> Entity<EditableTextState> {
        self.search.query.clone()
    }

    /// Focuses the search field: the sidebar's search, and the Cmd+F /
    /// Ctrl+F the window binds to this action anywhere in it (the window's
    /// render attaches the action, so this is the window module's seam).
    pub(crate) fn search_focus(
        &mut self,
        _: &FocusSearch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.search.query.focus_handle(cx), cx);
        cx.notify();
    }

    /// Moves the result selection down, as far as the last result: the
    /// field's Down key, which the deeper binding takes from the editable
    /// element only while the field is focused.
    fn search_next(&mut self, _: &NextResult, _: &mut Window, cx: &mut Context<Self>) {
        if !self.search.results.is_empty() {
            self.search.selected = (self.search.selected + 1).min(self.search.results.len() - 1);
            cx.notify();
        }
    }

    /// Moves the result selection up, as far as the first.
    fn search_previous(&mut self, _: &PreviousResult, _: &mut Window, cx: &mut Context<Self>) {
        if !self.search.results.is_empty() {
            self.search.selected = self.search.selected.saturating_sub(1);
            cx.notify();
        }
    }

    /// Opens the selected result, as the field's Enter does.
    fn search_open_action(&mut self, _: &OpenResult, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(hit) = self.search.results.get(self.search.selected).cloned() {
            self.search_open(&hit, window, cx);
        }
    }

    /// Leaves the search from the field, as Escape does: a query clears
    /// first — as it does in every search of Pane's — and an empty field
    /// returns the keyboard to the sections, page navigation restored.
    fn search_leave(&mut self, _: &LeaveSearch, window: &mut Window, cx: &mut Context<Self>) {
        if self.search.query.read(cx).as_str().is_empty() {
            window.focus(&self.focus, cx);
            cx.notify();
        } else {
            self.clear_search(cx);
        }
    }

    /// Clears the search query, if one shows: the sidebar returns to the
    /// sections. Opening a result calls this, and so does page
    /// navigation — the sections' keys and clicks — so the sidebar never
    /// lists results the keyboard has left behind.
    pub(crate) fn clear_search(&mut self, cx: &mut Context<Self>) {
        if !self.search.query.read(cx).as_str().is_empty() {
            self.search
                .query
                .update(cx, |query, cx| query.emplace("", cx));
        }
    }

    /// Opens the result `hit`, as Enter on it and a click on its row do:
    /// the query clears (the sidebar returns to the sections) and the
    /// entry's page opens. The control is focused where it takes focus;
    /// where it does not — and where it no longer exists — the sidebar
    /// keeps the window's keyboard focus, on the page the jump opened.
    /// Where the control drew, the page area scrolls to it: after the
    /// frame that paints the page, since that is where its anchor
    /// records the control's place.
    fn search_open(&mut self, hit: &Hit, window: &mut Window, cx: &mut Context<Self>) {
        self.clear_search(cx);
        self.selected = hit.page;
        // A jump lands its page at once: it is going somewhere specific,
        // and the reveal's scroll must not wait out an arrival first (the
        // page would arrive, then jump).
        self.section_arrival = None;
        self.drawn_section = None;
        cx.notify();
        let control = hit.entry.control.clone();
        let focus = self.pages[hit.page].focus;
        let took = control
            .as_deref()
            .is_some_and(|target| focus(self, target, window, cx));
        if !took {
            window.focus(&self.focus, cx);
        }
        if let Some(target) = control {
            // The reveal. This frame has not painted the entry's page
            // yet; the anchor records where the control drew when that
            // paint lays the page out, so the scroll waits for it.
            let this = cx.entity();
            window.on_next_frame(move |window, _cx| {
                window.on_next_frame(move |window, cx| {
                    // The page has painted; the anchors are the ones it
                    // drew, at rest, since the jump starts no arrival.
                    // Ask for the scroll.
                    SettingsWindow::reveal_when_settled(&this, &target, window, cx);
                });
            });
        }
    }

    /// Shows the page titled `page`, scrolled to the control `target`, as
    /// opening a search result there does (see [`super::open_at`]); the
    /// sidebar keeps the window's keyboard focus. Nothing changes for a
    /// page that is not registered.
    pub(crate) fn show_at(
        &mut self,
        page: &str,
        target: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.pages.iter().position(|shown| shown.title == page) else {
            return;
        };
        self.clear_search(cx);
        self.selected = index;
        // As a jump does: the page lands at once, and the reveal scrolls
        // once the page has painted.
        self.section_arrival = None;
        self.drawn_section = None;
        window.focus(&self.focus, cx);
        cx.notify();
        let this = cx.entity();
        let target = target.to_owned();
        window.on_next_frame(move |window, _cx| {
            window.on_next_frame(move |window, cx| {
                SettingsWindow::reveal_when_settled(&this, &target, window, cx);
            });
        });
    }

    /// What the window's watcher does for the search: nothing unless a
    /// query is showing, in which case results that differ from the ones
    /// drawn ask for a redraw — the next frame's refresh shows them, so a
    /// setting that appeared or went while the window sat idle is found
    /// or gone from the search.
    pub(crate) fn search_watched(&mut self, cx: &mut Context<Self>) {
        let query = self.search.query.read(cx).as_str().to_owned();
        if query.trim().is_empty() {
            return;
        }
        if results(&self.launcher, &self.pages, &query, cx) != self.search.results {
            cx.notify();
        }
    }

    /// The reveal's scroll, on the frame after the jump painted the
    /// entry's page, so the anchors are the ones that paint drew. Where
    /// the page is still arriving from the section change the jump made,
    /// each frame re-checks until the arrival settles: the shift the
    /// arrival draws is a layout offset, so an anchor recorded mid-flight
    /// names a place the control does not keep, and the scroll lands the
    /// rest — the position the control stays at. Then the control's
    /// anchor scrolls — it runs one frame later, on the position that
    /// paint recorded — and the window repaints, so the scrolled place
    /// shows.
    fn reveal_when_settled(this: &Entity<Self>, target: &str, window: &mut Window, cx: &mut App) {
        if this.update(cx, |this, _| this.section_arrival.is_some()) {
            let this = this.clone();
            let target = target.to_owned();
            window.on_next_frame(move |window, cx| {
                SettingsWindow::reveal_when_settled(&this, &target, window, cx);
            });
            return;
        }
        let anchor = this.update(cx, |this, _| this.search.anchors.get(target).cloned());
        if let Some(anchor) = anchor {
            anchor.scroll_to(window, cx);
            let this = this.clone();
            window.on_next_frame(move |_, cx| {
                this.update(cx, |_, cx| cx.notify());
            });
        }
    }
}

/// The search field: the shared editable text element in the sidebar's
/// search well with the magnifier (see
/// [`settings_shell::search_field`]). The well is the field's
/// accessibility node — an editable combo box, as root search's field
/// is, with the results list below it as its list — and carries the key
/// context the results' keys are bound in.
pub(super) fn field(
    this: &SettingsWindow,
    theme: &Theme,
    cx: &mut Context<SettingsWindow>,
) -> Stateful<Div> {
    let input = &this.search.query;
    let query = input.read(cx).as_str().to_owned();
    settings_shell::search_field(input, PLACEHOLDER, theme)
        .id("settings-search-field")
        .debug_selector(|| "settings-search-field".into())
        .key_context(FIELD)
        .track_focus(&input.focus_handle(cx))
        .role(Role::EditableComboBox)
        .aria_label(PLACEHOLDER)
        .aria_value(query)
        .aria_placeholder(PLACEHOLDER)
        .on_action(cx.listener(SettingsWindow::search_next))
        .on_action(cx.listener(SettingsWindow::search_previous))
        .on_action(cx.listener(SettingsWindow::search_open_action))
        .on_action(cx.listener(SettingsWindow::search_leave))
}

/// The results the sidebar lists while a query shows: one row per result,
/// each naming the setting and where it lives, the selected one the
/// field's Enter opens and its arrows move; or, when nothing matches,
/// the no-results line that says so.
pub(super) fn result_rows(
    this: &SettingsWindow,
    theme: &Theme,
    cx: &mut Context<SettingsWindow>,
) -> Vec<gpui::AnyElement> {
    if this.search.results.is_empty() {
        let message = format!(
            "No settings match “{}”",
            this.search.query.read(cx).as_str().trim()
        );
        return vec![
            div()
                .id("settings-search-empty")
                .debug_selector(|| "settings-search-empty".into())
                // A live status, so assistive technology announces that the
                // query found nothing (plain text children are invisible to
                // the tree without a role).
                .role(Role::Status)
                .aria_label(message.clone())
                .px(theme.geometry.settings.item_padding_x)
                .text_size(theme.typography.row_kind_size)
                .text_color(theme.text_muted)
                .child(message)
                .into_any_element(),
        ];
    }
    this.search
        .results
        .iter()
        .enumerate()
        .map(|(index, hit)| {
            let selected = index == this.search.selected;
            let page = this.pages[hit.page].title;
            let subtitle = match &hit.entry.group {
                Some(group) => format!("{page} · {group}"),
                None => page.to_owned(),
            };
            let description = match hit.entry.unavailable.as_deref() {
                Some(reason) => format!("{subtitle}. {reason}"),
                None => subtitle.clone(),
            };
            let title = hit.entry.title.clone();
            let hit_for_click = hit.clone();
            // The sidebar's own item, as the sections it stands in for:
            // the result's name with its page's glyph, where it lives
            // under it, and why it cannot be used here, if it cannot.
            settings_shell::sidebar_item(
                ("settings-search-result", index),
                SidebarItem {
                    label: hit.entry.title.clone().into(),
                    glyph: this.pages[hit.page].icon,
                    detail: Some(subtitle.into()),
                    reason: hit.entry.unavailable.clone().map(SharedString::from),
                    count: None,
                    selected,
                },
                theme,
            )
            .debug_selector(move || format!("settings-search-result-{title}"))
            .role(Role::ListBoxOption)
            .aria_label(hit.entry.title.clone())
            .aria_description(description)
            .aria_selected(selected)
            .when(selected, |row| row.aria_active_descendant())
            .when(hit.entry.unavailable.is_some(), |row| {
                row.aria_disabled(true)
            })
            .on_click(cx.listener(move |this, _: &gpui::ClickEvent, window, cx| {
                this.search_open(&hit_for_click, window, cx);
            }))
            .into_any_element()
        })
        .collect()
}
