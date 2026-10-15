//! The standard List and Grid of a designed view (#240): the items a
//! view's tree names, in sections, whose search field and selection Pane
//! owns. The List document's vocabulary rides the designed tree — an item
//! has a key, a title, a subtitle, an icon, accessories and the actions
//! that activate it — and the host does what it does for a command's own
//! list: it filters the items by the text typed in the view's search field
//! (the root-search matcher, `crate::search`, unless the view handles the
//! search itself, its `onSearchText` then hearing the text through the
//! view's events, throttled by the window), keeps the selection on an
//! item by its key, raises a load-more as the selection nears the end of
//! what the tree shows while it says more is there, and fills the
//! launcher's rows with the items as it fills them for a command's list —
//! so the window draws them with the launcher's own list, virtualised,
//! accessible, selected and moved through by the same keys.
//!
//! The view's search field is the search header's (the navigation title
//! moves into the header with the List, #239's interim footer title
//! replaced): `Launcher::set_query` sets the list's text, and
//! [`crate::Screen::search_field`] names it, so the window's query field
//! shows and edits it as it does root search's. The text is partially
//! controlled: a `searchText` the tree names wins when it differs from
//! the value its previous render named and from every value the field
//! reported, so an echo of what the user typed never fights typing.
//!
//! The presentation is recomputed whenever the screen's state changes —
//! a tree landing, the search text, the selection — into the snapshot's
//! [`DesignedList`], which carries what the rows do not: the search, the
//! loading state and when it began (the loading bar's 300 ms threshold),
//! the sections' labels, the empty view's and the detail pane's subtrees
//! (the selected item's `detail`, built for it through the render
//! context's `selected`), the search-bar dropdown, and the Grid's shapes.
//! A view names at most one list or grid — the first in its tree, in
//! document order; a later one draws as a plain column of its rows, as
//! any other node's children do.

use std::future::Future;
use std::pin::Pin;

use super::presentation::Section;
use super::{Entry, Launcher, Pending, Row, Screen, State, first_index};
use crate::host_settings::SearchSensitivity;
use crate::runtime::{
    Accessory, DesignedHandler, DesignedTree, ItemLook, ListNode, Node, NodeKind,
};
use crate::search::{self, Keys, Query};

/// How long the list's loading runs before the loading bar draws (the
/// reference's late indicator, ADR 0035; the bar's look is the launcher
/// polish's, #248 — this is the threshold it rides).
pub const LOADING_MS: u64 = 300;

/// The page a list whose tree names no `pageSize` holds: how near the end
/// the selection sits for a load-more to be raised.
const PAGE: u64 = 10;

/// What the launcher keeps for the open designed view's List or Grid:
/// its search text and selection, which Pane owns, and the bookkeeping
/// of the events it raises. Held by the stack's entry, so a view below a
/// pushed one keeps its own.
#[derive(Clone, Debug, Default)]
pub(super) struct ListHeld {
    /// The search field's text: the user's, or the tree's own.
    search: String,
    // The selected item's key, `None` when no item is selected.
    selected: Option<String>,
    // The search text the tree last named: the baseline an unchanged
    // value does not move (the field's partial control).
    rendered: String,
    // The search values reported to the view since the tree was applied,
    // which an echo of the field's own value never fights.
    reported: Vec<String>,
    // How many items were listed when the last load-more was raised: the
    // next is raised once the tree has grown since.
    asked_at: Option<usize>,
    // When the list's loading began, in the clock's milliseconds.
    loading_since: Option<u64>,
}

/// One row of a designed List's or Grid's presentation: what activating
/// it does, and what the row alone cannot show.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DesignedRow {
    /// The item's key, as the selection names it.
    pub key: String,
    /// The callbacks that activate the item, in order: the first is its
    /// primary action (Enter), the second its secondary.
    pub actions: Vec<u32>,
    /// The author's own row subtree, drawn in the place of the standard
    /// row while Pane still selects and activates it.
    pub content: Option<Vec<Node>>,
    /// The detail pane's content when this item is selected and the list
    /// shows its detail.
    pub detail: Option<Box<Node>>,
    /// The cell's image (a Grid's).
    pub image: Option<crate::Icon>,
    /// The colour the cell fills with (a Grid's).
    pub color: Option<crate::Paint>,
    /// The shape of the section the cell sits in (a Grid's).
    pub shape: GridShape,
}

/// One section of a Grid's shape, as the window draws its cells.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GridShape {
    /// How many columns the section's cells sit in.
    pub columns: u64,
    /// The cells' width over their height; `None` for square cells.
    pub aspect_ratio: Option<crate::Finite>,
    /// How the section's images fit their cells.
    pub fit: crate::Fit,
    /// Whether the cells sit inset from the grid's edges.
    pub inset: bool,
}

/// The List's or Grid's search-bar dropdown, beside its search field.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DesignedDropdown {
    pub items: Vec<crate::DropdownItem>,
    /// The id of the item chosen.
    pub value: Option<String>,
    /// The dropdown's placeholder.
    pub placeholder: Option<String>,
    /// The callback the choice's change runs, told the chosen item's id.
    pub on_change: Option<u32>,
}

/// A designed view's List or Grid as the launcher presents it (the
/// snapshot's `list`, beside the rows it fills): what the rows alone do
/// not say.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DesignedList {
    /// The node's key, as the events raised on the list name it.
    pub key: String,
    /// Whether the view handles the search itself (`onSearchText`): Pane
    /// filters nothing then, and the text is told to the view.
    pub searching: bool,
    /// The search field's text.
    pub search: String,
    /// The field's placeholder.
    pub placeholder: Option<String>,
    /// The loading state, and when it began in the clock's milliseconds
    /// (the loading bar's threshold); `None` while it is not loading.
    pub loading: Option<u64>,
    /// The selected item's key.
    pub selected: Option<String>,
    /// The rows' own data, one per row of the launcher's view.
    pub rows: Vec<DesignedRow>,
    /// The section labels over the rows, as the launcher's own list
    /// draws them.
    pub sections: Vec<Section>,
    /// The empty view's subtree, drawn when no item is shown.
    pub empty: Option<Box<Node>>,
    /// The detail pane's content — the selected item's `detail` — shown
    /// while the list is showing its detail.
    pub detail: Option<Box<Node>>,
    /// The search-bar dropdown.
    pub dropdown: Option<DesignedDropdown>,
    /// The callback the search text runs on its change, when the view
    /// handles the search itself.
    pub on_search_text: Option<u32>,
    /// The callback the selection's change runs.
    pub on_selection_change: Option<u32>,
    /// The callback the load of the next page runs.
    pub on_load_more: Option<u32>,
    /// Whether the tree says more items follow the ones shown.
    pub has_more: bool,
    /// How many items a page holds.
    pub page_size: u64,
    /// Whether the node is a Grid, its rows cells.
    pub grid: bool,
}

/// The list or grid `tree` names, with its node: the first in document
/// order.
pub(super) fn first_list(tree: &DesignedTree) -> Option<(&Node, &ListNode)> {
    fn held(node: &Node) -> Option<(&Node, &ListNode)> {
        match &node.kind {
            NodeKind::List(list) | NodeKind::Grid(list) => Some((node, list)),
            _ => node
                .children
                .iter()
                .find_map(held)
                .or_else(|| node.fallback.as_deref().and_then(held)),
        }
    }
    held(&tree.root)
}

/// One group of a list's items: a section's, or the list's own.
struct Group<'a> {
    title: Option<String>,
    note: Option<String>,
    shape: GridShape,
    items: Vec<&'a Node>,
}

/// The groups of the list or grid `node`'s children, its search-bar
/// dropdown, and its empty view's subtree.
fn groups(node: &Node) -> (Vec<Group<'_>>, Option<&Node>, Option<&Node>) {
    let mut groups = Vec::new();
    let mut dropdown = None;
    let mut empty = None;
    for child in &node.children {
        match &child.kind {
            NodeKind::ListSection(section) => groups.push(Group {
                title: section.title.clone(),
                note: section.subtitle.clone(),
                shape: GridShape {
                    columns: section.columns.unwrap_or(crate::GRID_COLUMNS),
                    aspect_ratio: section.aspect_ratio,
                    fit: section.fit,
                    inset: section.inset,
                },
                items: child.children.iter().collect(),
            }),
            NodeKind::ListItem(_) | NodeKind::GridItem(_) => {
                if let Some(last) = groups.last_mut() {
                    last.items.push(child);
                } else {
                    groups.push(Group {
                        title: None,
                        note: None,
                        shape: GridShape::default(),
                        items: vec![child],
                    });
                }
            }
            // A dropdown is not a row; any other child is the empty view.
            NodeKind::ListDropdown(_) => dropdown = Some(child),
            _ => empty = Some(child),
        }
    }
    (groups, dropdown, empty)
}

/// The one item the node `item` is: its key, title, subtitle and
/// keywords. `at` is the row's place among all the list's, for an item
/// whose tree gave no key.
fn item_of(item: &Node, at: usize) -> (String, String, Option<String>, Vec<String>) {
    match &item.kind {
        NodeKind::ListItem(held) => (
            item.key.clone().unwrap_or_else(|| at.to_string()),
            held.title.clone(),
            held.subtitle.clone(),
            held.keywords.clone(),
        ),
        NodeKind::GridItem(held) => (
            item.key.clone().unwrap_or_else(|| at.to_string()),
            held.title.clone().unwrap_or_default(),
            held.subtitle.clone(),
            Vec::new(),
        ),
        _ => (at.to_string(), String::new(), None, Vec::new()),
    }
}

/// Whether the item (`title`, `subtitle`, `keywords`) matches `query` at
/// `sensitivity`: the root-search matcher's, every word found somewhere.
fn matches(
    query: &Query,
    sensitivity: SearchSensitivity,
    title: &str,
    subtitle: Option<&str>,
    keywords: &[String],
) -> bool {
    if query.text.trim().is_empty() {
        return true;
    }
    let keys = Keys::new(title, subtitle, None).with_alternates(&[], keywords);
    !search::ranked_matches(
        query,
        std::iter::once(search::Candidate {
            keys: &keys,
            kind: search::Kind::Command,
            provider: 0,
            learned: None,
        }),
        sensitivity,
    )
    .is_empty()
}

impl Launcher {
    /// Presents the open designed view's List or Grid: the controlled
    /// search text and selection applied, the items filtered by the held
    /// search (the root-search matcher) unless the view searches itself,
    /// the launcher's rows, looks and entries filled, the snapshot's
    /// `list` built, and the view's selection named to the runtime, whose
    /// render context carries it. Called wherever the screen's state
    /// changes — a tree landing, the search text, the selection — never
    /// per frame.
    pub(super) fn present_designed_list(&self, state: &mut State) {
        let built = self.list_of(state);
        let Some((built, list)) = built else {
            // No list on the tree: the screen is the tree's alone, with
            // no rows of its own and no list presented.
            if let Screen::DesignedView(snapshot) = &mut state.view.screen {
                snapshot.list = None;
            }
            state.view.rows.clear();
            state.entries.clear();
            state.view.selected = None;
            super::looks::remember_none(state);
            return;
        };
        // The rows, their looks and their entries fill the launcher's
        // view, as a command's own list fills it: the window draws them
        // with the launcher's own list.
        let Built {
            rows,
            looks,
            selected,
            view,
        } = built;
        state.view.rows = rows;
        state.entries = vec![Entry::NoActions; state.view.rows.len()];
        state.view.selected = selected;
        super::looks::remember_designed(state, looks);
        if let Screen::DesignedView(snapshot) = &mut state.view.screen {
            snapshot.list = Some(list);
        }
        // The view's selection, named to the runtime: its render context
        // carries the key, and the view builds the detail pane for it.
        let key = state
            .view
            .selected
            .and_then(|index| state.view.rows.get(index))
            .map(|row| row.id.clone());
        if let Ok(runtime) = self.runtime() {
            runtime.set_view_selection(view, key);
        }
    }

    /// The open designed view's list, as the tree names it: the rows it
    /// fills the view with, and what they alone do not say; `None` with
    /// no list on the tree (the snapshot's `list` cleared) or no designed
    /// view open.
    fn list_of(&self, state: &mut State) -> Option<(Built, DesignedList)> {
        {
            let Screen::DesignedView(snapshot) = &state.view.screen else {
                return None;
            };
            let stack = state.designed_view.as_mut()?;
            let Some((node, list)) = first_list(&snapshot.tree) else {
                // No list on this tree: the screen is the tree's alone.
                return None;
            };
            let top = stack.top_mut();
            let view = top.id;
            let held = &mut top.list;
            // The search text the tree names: an instruction only when it
            // differs from the value its previous render named, from the
            // field's own text and from every value the field reported
            // (the partial control, as a text field's value is placed).
            if let Some(named) = &list.search_text
                && *named != held.rendered
                && *named != held.search
                && !held.reported.iter().any(|reported| reported == named)
            {
                held.search = named.clone();
            }
            held.reported.clear();
            held.rendered = list.search_text.clone().unwrap_or_default();
            // When the list's loading began: the first tree that says it,
            // kept while it says it.
            if list.is_loading {
                held.loading_since.get_or_insert_with(|| state.clock.now());
            } else {
                held.loading_since = None;
            }
            // The selection the tree names, by its key; the row it is
            // among is found once the rows are built.
            if let Some(named) = &list.selected_key {
                held.selected = Some(named.clone());
            }
            let searching = list.on_search_text.is_some();
            let query = Query::new(&held.search);
            let sensitivity = state.sensitivity;
            let selected_key = held.selected.clone();
            // The items of each group, filtered by the held search (a
            // self-searching list is never filtered: its view answers the
            // tree for the text); the sections' labels over what is left.
            let (groups, dropdown, empty) = groups(node);
            let mut presented: Vec<DesignedRow> = Vec::new();
            let mut rows: Vec<Row> = Vec::new();
            let mut looks: Vec<(String, ItemLook)> = Vec::new();
            let mut sections = Vec::new();
            let grid = matches!(&node.kind, NodeKind::Grid(_));
            for group in groups {
                let before = rows.len();
                for item in group.items {
                    let at = rows.len();
                    let (key, title, subtitle, keywords) = item_of(item, at);
                    if !searching
                        && !matches(&query, sensitivity, &title, subtitle.as_deref(), &keywords)
                    {
                        continue;
                    }
                    let mut designed = designed_row(item, group.shape);
                    designed.key = key.clone();
                    presented.push(designed);
                    rows.push(Row {
                        id: key.clone(),
                        title,
                        subtitle,
                        unavailable: None,
                    });
                    looks.push((
                        key,
                        ItemLook {
                            icon: item_icon(item),
                            title_tooltip: item_tooltip(item, true),
                            subtitle_tooltip: item_tooltip(item, false),
                            accessories: item_accessories(item),
                        },
                    ));
                }
                if rows.len() > before && (group.title.is_some() || group.note.is_some()) {
                    sections.push(Section {
                        label: group.title.unwrap_or_default(),
                        note: group.note,
                        first: before,
                    });
                }
            }
            // The selection's row: the selected key's, else the place the
            // selection was at (the list tree's rule), else the first.
            let selected = selected_key
                .and_then(|key| rows.iter().position(|row| row.id == key))
                .or_else(|| {
                    let last = rows.len().checked_sub(1)?;
                    Some(state.view.selected?.min(last))
                })
                .or_else(|| first_index(&rows));
            let selected_key = selected
                .and_then(|index| presented.get(index))
                .map(|row| row.key.clone());
            let detail = selected
                .and_then(|index| presented.get(index))
                .and_then(|row| row.detail.clone());
            Some((
                Built {
                    rows,
                    looks,
                    selected,
                    view,
                },
                DesignedList {
                    key: node.key.clone().unwrap_or_default(),
                    searching,
                    search: held.search.clone(),
                    placeholder: list.search_placeholder.clone(),
                    loading: held.loading_since,
                    selected: selected_key,
                    rows: presented,
                    sections,
                    empty: empty.cloned().map(Box::new),
                    detail,
                    dropdown: dropdown.map(dropdown_of),
                    on_search_text: list.on_search_text,
                    on_selection_change: list.on_selection_change,
                    on_load_more: list.on_load_more,
                    has_more: list.has_more,
                    page_size: list.page_size.unwrap_or(PAGE),
                    grid,
                },
            ))
        }
    }

    /// This module's piece of [`Launcher::set_query`] for the open
    /// designed view's List: `text` becomes the search field's. A view
    /// that handles the search itself is told it through the returned
    /// future, raised on the list's key; a host-filtered list is filtered
    /// at once. `None` with no designed list on screen.
    pub(super) fn designed_list_searched(
        &self,
        state: &mut State,
        text: &str,
    ) -> Option<Pin<Box<dyn Future<Output = ()> + Send>>> {
        let event = {
            let Screen::DesignedView(snapshot) = &state.view.screen else {
                return None;
            };
            let list = snapshot.list.clone()?;
            let stack = state.designed_view.as_mut()?;
            let held = &mut stack.top_mut().list;
            held.search = text.to_owned();
            list.searching.then(|| {
                held.reported.retain(|reported| reported != text);
                held.reported.push(text.to_owned());
                (list.on_search_text, list.key.clone(), text.to_owned())
            })
        };
        // The rows and the snapshot's list reflect the new text at once.
        self.present_designed_list(state);
        let (callback, key, text) = event?;
        let callback = callback?;
        let payload = value_payload(&text);
        // The send happens on a thread of its own, started now: a caller
        // can hold the returned future un-polled while it waits for what
        // the search does (the crash tests hold theirs until after), so
        // the send cannot wait for the future's first poll. The thread
        // also escapes this function's caller's state lock — it is
        // `set_query`'s, still held here — which the send takes itself to
        // name the view the event is raised on: the thread waits the lock
        // out instead of deadlocking with it, as the selection's and the
        // back-out's sends do on threads of their own.
        let key = (!key.is_empty()).then_some(key);
        let launcher = self.clone();
        let started = std::thread::Builder::new()
            .name("pane-designed-search".into())
            .spawn(move || {
                // [diag-ff22] Diagnostic scaffolding for the query changes
                // that never re-render: the search-text event's send, and
                // its outcome. fix-forward-23 removes it with the root
                // fix.
                crate::diagnostic!(
                    "[diag-ff22] search send: callback={callback} key={key:?} payload={payload}"
                );
                let sent = launcher.send_designed_seen(
                    DesignedHandler::Input,
                    callback,
                    key.as_deref(),
                    None,
                    payload,
                );
                futures::executor::block_on(sent);
                crate::diagnostic!("[diag-ff22] search sent");
                launcher.changed();
            });
        // The future the caller holds completes when the search's answer
        // has landed: the thread's end.
        Some(Box::pin(async move {
            if let Ok(finished) = started {
                let _ = finished.join();
            }
        }))
    }

    /// This module's piece of [`Launcher::select`] for the open designed
    /// view's List or Grid: the selection moves to the row at `index`,
    /// the selection-change event the tree names is raised on the list's
    /// key (told the selected item's key), and a load-more is raised when
    /// the selection nears the end of what is shown while the tree says
    /// more is there. Both events are delivered in the background, as the
    /// pop event is: their answers land through the change channel.
    pub(super) fn designed_list_selected(&self, state: &mut State, index: usize) {
        let events: Vec<(DesignedHandler, u32, Option<String>, String)> = {
            let Screen::DesignedView(snapshot) = &state.view.screen else {
                return;
            };
            let Some(list) = snapshot.list.clone() else {
                return;
            };
            let Some(row) = list.rows.get(index) else {
                return;
            };
            let key = row.key.clone();
            let Some(stack) = state.designed_view.as_mut() else {
                return;
            };
            let held = &mut stack.top_mut().list;
            if held.selected.as_deref() == Some(key.as_str()) {
                // The selection did not move; nothing is asked.
                state.view.selected = Some(index);
                return;
            }
            held.selected = Some(key.clone());
            state.view.selected = Some(index);
            // A load-more, when the selection nears the end of what is
            // shown and the tree says more is there: once for each growth
            // of the list, so the view is not asked again before it
            // answers.
            let near = list.rows.len().saturating_sub(index) <= list.page_size as usize;
            let more = list.has_more
                && list.on_load_more.is_some()
                && near
                && held.asked_at.is_none_or(|asked| asked < list.rows.len());
            if more {
                held.asked_at = Some(list.rows.len());
            }
            let mut events = Vec::new();
            if let Some(callback) = list.on_selection_change {
                events.push((DesignedHandler::Selection, callback, value_payload(&key)));
            }
            if more && let Some(callback) = list.on_load_more {
                events.push((DesignedHandler::More, callback, "{}".to_owned()));
            }
            let key = (!list.key.is_empty()).then_some(list.key.clone());
            events
                .into_iter()
                .map(|(handler, callback, payload)| (handler, callback, key.clone(), payload))
                .collect()
        };
        self.present_designed_list(state);
        if events.is_empty() {
            return;
        }
        let launcher = self.clone();
        let started = std::thread::Builder::new()
            .name("pane-designed-list".into())
            .spawn(move || {
                for (handler, callback, key, payload) in events {
                    let sent = launcher.send_designed_seen(
                        handler,
                        callback,
                        key.as_deref(),
                        None,
                        payload,
                    );
                    futures::executor::block_on(sent);
                }
                launcher.changed();
            });
        // A thread that could not start leaves the events unsent; the
        // selection itself has moved, and the view is asked again when it
        // moves next.
        let _ = started;
    }

    /// The dropdown of the open designed view's List changed: the choice
    /// is told to the tree's `onChange`, raised on the list's key. For
    /// the window, which the dropdown's commit reaches, and the tests.
    pub fn set_designed_dropdown(&self, value: &str) -> impl Future<Output = ()> + Send + 'static {
        let state = self.lock();
        let event = match &state.view.screen {
            Screen::DesignedView(snapshot) => snapshot.list.as_ref().map(|list| {
                (
                    list.dropdown
                        .as_ref()
                        .and_then(|dropdown| dropdown.on_change),
                    list.key.clone(),
                    value.to_owned(),
                )
            }),
            _ => None,
        };
        drop(state);
        let launcher = self.clone();
        async move {
            if let Some((Some(callback), key, value)) = event {
                let payload = value_payload(&value);
                launcher
                    .send_designed_seen(
                        DesignedHandler::Change,
                        callback,
                        (!key.is_empty()).then_some(key.as_str()),
                        None,
                        payload,
                    )
                    .await;
            }
        }
    }

    /// The work Enter does on the open designed view's List or Grid: the
    /// selected item's first action (or its only one), pressed on its
    /// key. `None` when no designed list is on screen, no item is
    /// selected, or the item has no action to run.
    pub(super) fn designed_list_press(&self, state: &mut State) -> Option<Pending> {
        let Screen::DesignedView(snapshot) = &state.view.screen else {
            return None;
        };
        let list = snapshot.list.as_ref()?;
        let index = state.view.selected?;
        let row = list.rows.get(index)?;
        let callback = *row.actions.first()?;
        let key = row.key.clone();
        state.sent_from = None;
        // [diag-ff27] Diagnostic scaffolding for the drive that hangs
        // between its typing and its Enter: the press of a designed
        // list's row, whose event the item's answer rides. fix-forward-28
        // removes it with the root fix.
        crate::diagnostic!("[diag-ff27] press dispatched: callback={callback} key={key:?}");
        Some(Pending::DesignedEvent {
            handler: DesignedHandler::Press,
            callback,
            key: (!key.is_empty()).then_some(key),
            payload: "{}".to_owned(),
        })
    }

    /// Whether the open designed view's list search field holds text: what
    /// Escape clears before the stack pops.
    pub fn designed_list_search_typed(&self) -> bool {
        search_typed(&self.lock())
    }

    /// The launcher's clock, in milliseconds: what the window times the
    /// loading bar's threshold against.
    pub fn now_ms(&self) -> u64 {
        self.lock().clock.now()
    }

    /// Opens `url` with the system's handler, as a computed result's link
    /// row does: a designed view's Markdown links, which Pane opens itself
    /// (#240).
    pub fn open_link(&self, url: String) -> impl Future<Output = ()> + Send + 'static {
        let epoch = self.lock().screen_epoch;
        let launcher = self.clone();
        async move {
            launcher.open_url(epoch, url).await;
        }
    }

    /// Clears the open designed view's list search field, as the back key
    /// does before it pops the stack (#240): the field empties and a
    /// host-filtered list is listed whole again; a view that handles the
    /// search itself is told the empty text, its answer drawing what it
    /// lists for none.
    pub(super) fn clear_designed_list_search(&self, state: &mut State) {
        let event = {
            let Screen::DesignedView(snapshot) = &state.view.screen else {
                return;
            };
            let Some(list) = snapshot.list.clone() else {
                return;
            };
            let stack = state.designed_view.as_mut();
            if let Some(stack) = stack {
                let held = &mut stack.top_mut().list;
                held.search.clear();
                if list.searching {
                    // The empty text is reported, so the view echoing it
                    // back sets nothing.
                    held.reported.retain(|reported| !reported.is_empty());
                    held.reported.push(String::new());
                }
            }
            list.searching
                .then_some((list.on_search_text, list.key.clone()))
        };
        self.present_designed_list(state);
        let Some((Some(callback), key)) = event else {
            return;
        };
        let launcher = self.clone();
        let started = std::thread::Builder::new()
            .name("pane-designed-list".into())
            .spawn(move || {
                // [diag-ff23] Diagnostic scaffolding for the back-out's
                // clear, whose answer never returns: the send and its
                // completion. fix-forward-24 removes it with the root
                // fix.
                crate::diagnostic!("[diag-ff23] clear send: callback={callback} key={key:?}");
                let sent = launcher.send_designed_seen(
                    DesignedHandler::Input,
                    callback,
                    (!key.is_empty()).then_some(key.as_str()),
                    None,
                    value_payload(""),
                );
                futures::executor::block_on(sent);
                crate::diagnostic!("[diag-ff23] clear sent");
                launcher.changed();
            });
        let _ = started;
    }
}

/// What one presentation built, before it fills the state.
struct Built {
    rows: Vec<Row>,
    looks: Vec<(String, ItemLook)>,
    selected: Option<usize>,
    view: crate::ViewId,
}

/// The designed row the item `node` is, with its section's `shape`: its
/// actions, its detail, its own subtree, and the cell a Grid's item is.
fn designed_row(node: &Node, shape: GridShape) -> DesignedRow {
    let (actions, image, color, detail) = match &node.kind {
        NodeKind::ListItem(item) => {
            let mut actions = Vec::new();
            if let Some(on_press) = item.on_press {
                actions.push(on_press);
            }
            actions.extend(item.actions.iter().map(|action| action.on_press));
            (actions, None, None, item.detail.clone())
        }
        NodeKind::GridItem(item) => {
            let mut actions = Vec::new();
            if let Some(on_press) = item.on_press {
                actions.push(on_press);
            }
            (actions, item.image.clone(), item.color, None)
        }
        _ => (Vec::new(), None, None, None),
    };
    DesignedRow {
        key: String::new(),
        actions,
        content: (!node.children.is_empty()).then(|| node.children.clone()),
        detail,
        image,
        color,
        shape,
    }
}

/// The item's icon, as its row draws it.
fn item_icon(node: &Node) -> Option<crate::Icon> {
    match &node.kind {
        NodeKind::ListItem(item) => item.icon.clone(),
        NodeKind::GridItem(item) => item.image.clone(),
        _ => None,
    }
}

/// The item's tooltip: `title` for the title's, else the subtitle's.
fn item_tooltip(node: &Node, title: bool) -> Option<String> {
    match &node.kind {
        NodeKind::ListItem(item) => {
            if title {
                item.title_tooltip.clone()
            } else {
                item.subtitle_tooltip.clone()
            }
        }
        _ => None,
    }
}

/// The item's accessories, as its row draws them.
fn item_accessories(node: &Node) -> Vec<Accessory> {
    match &node.kind {
        NodeKind::ListItem(item) => item.accessories.clone(),
        _ => Vec::new(),
    }
}

/// The dropdown the node holds, for the snapshot's list.
fn dropdown_of(node: &Node) -> DesignedDropdown {
    let NodeKind::ListDropdown(dropdown) = &node.kind else {
        return DesignedDropdown::default();
    };
    DesignedDropdown {
        items: dropdown.items.clone(),
        value: dropdown.value.clone(),
        placeholder: dropdown.placeholder.clone(),
        on_change: dropdown.on_change,
    }
}

/// A value as an event's payload names it: `{"value": …}`, a string
/// escaped as JSON.
fn value_payload(value: &str) -> String {
    serde_json::json!({ "value": value }).to_string()
}

/// Whether the open designed view's list search field holds text: what
/// Escape clears before the stack pops.
pub(super) fn search_typed(state: &State) -> bool {
    let Screen::DesignedView(snapshot) = &state.view.screen else {
        return false;
    };
    snapshot
        .list
        .as_ref()
        .is_some_and(|list| !list.search.trim().is_empty())
}
