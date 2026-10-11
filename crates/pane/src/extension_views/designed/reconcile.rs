//! The keyed reconciler: what the window keeps for a designed view's
//! tree, by key (#238). Each new tree is walked against the previous one
//! by path key — a parent's key composed with the child's own, or its
//! position when it gives none — and per key the window keeps: GPUI CE's
//! editing state of a text field (the text, the caret, the selection and
//! the input method's composition, with undo and the clipboard), the
//! searchable select's own open state, query and highlight, the focus of
//! every focusable control, and a scroll region's position. A key that
//! disappears loses its state; a node of a different kind under the same
//! key is new. This generalises the form's `shape`/`FormControls`
//! reconciliation (its field ids are keys, its `EditableTextState` the
//! same) and #235's per-path focus map, which the keyed state replaces.
//!
//! Hover and pressed are the style's declarative variants and the
//! element's own state, kept by the element's id — the path — so a keyed
//! node keeps them across a re-render that still draws it; the
//! accessibility ids are the paths with it.
//!
//! Keys shared by siblings, and stateful nodes without one, are matched
//! by position instead (the tree walk appends the index for those) — the
//! fallback the runtime reports in a developed package's log.
//!
//! The inputs are partially controlled: the field edits at once (IME,
//! undo and the clipboard never wait for the extension) and the value the
//! tree names is an instruction only when it differs from the value the
//! extension's previous render named — an unchanged value is no
//! instruction, and neither is one the field itself reported (its own
//! text, or a value an input or change event carried), so echoing the
//! field's value back never fights fast typing. A value that counts as
//! set replaces the text and moves the caret to its end.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use gpui::prelude::*;
use gpui::{App, Context, Entity, FocusHandle, Focusable as _, ScrollHandle, Subscription, Window};
use gpui_elements::editable_text::{EditableTextState, StringStorage, TextChanged};

use pane_core::{DesignedTree, Node, NodeKind, ViewId};

use crate::app::LauncherWindow;

use super::tree;

/// What the window keeps for the open designed view: the keyed state of
/// its tree's nodes, and the window's keystroke observer while any node
/// asks for key events.
pub(crate) struct DesignedControls {
    /// The opened view these controls are for: a designed view's id, or a
    /// Pane form's identity as one (#241) — either way one id per screen
    /// the controls follow.
    pub(super) view: ViewId,
    /// The keyed state of every stateful and focusable node the last tree
    /// named, by its path.
    pub(super) state: HashMap<String, KeyedState>,
    /// The values those controls reported since the tree landed, keyed by
    /// the nodes' keys: a form's submission reads them while the answers
    /// that carry them have not landed (#241).
    pub(super) sent: HashMap<String, String>,
    /// The window's keystroke observer, while any drawn node asks for key
    /// events (`onKey`).
    keys: Option<Subscription>,
}

impl DesignedControls {
    /// The controls of a view opening with `tree`, drawn as `render`: the
    /// state its tree asks for, and the keyboard on the first focusable
    /// control — or the one that asks for it.
    pub(super) fn new(
        view: ViewId,
        tree: &DesignedTree,
        render: u64,
        window: &mut Window,
        cx: &mut Context<LauncherWindow>,
    ) -> DesignedControls {
        let mut controls = DesignedControls {
            view,
            state: HashMap::new(),
            sent: HashMap::new(),
            keys: None,
        };
        let opened = controls.reconcile(tree, render, window, cx);
        if !opened.asked
            && let Some(first) = opened.first
            && let Some(handle) = controls
                .state
                .get(&first)
                .and_then(|state| state.focus_handle())
        {
            window.focus(&handle, cx);
        }
        controls
    }

    /// Reconciles the keyed state with `tree`, drawn as `render`: the
    /// state of the keys the tree still names is kept (a node of another
    /// kind under the same key is new), the state of the keys it no
    /// longer names goes, a field's value is placed under the partially
    /// controlled rule, and a newly asking focus node takes the keyboard.
    /// What the walk found: the first focusable control's path, and
    /// whether a focus ask was honoured.
    pub(super) fn reconcile(
        &mut self,
        tree: &DesignedTree,
        render: u64,
        window: &mut Window,
        cx: &mut Context<LauncherWindow>,
    ) -> Opened {
        let mut walk = Walk {
            visited: HashSet::new(),
            keys: false,
            first: None,
            asked: false,
        };
        let mut path = String::new();
        tree::push(&mut path, tree.root.key.as_deref(), 0);
        self.node(&tree.root, &mut path, render, &mut walk, window, cx);
        // The state of the keys the tree no longer names goes with them.
        self.state.retain(|path, _| walk.visited.contains(path));
        // The window's keystroke observer, only while a node asks for key
        // events.
        self.watch_keys(walk.keys, cx);
        Opened {
            first: walk.first,
            asked: walk.asked,
        }
    }

    /// Reconciles one node, whose place in the tree `path` already names.
    fn node(
        &mut self,
        node: &Node,
        path: &mut String,
        render: u64,
        walk: &mut Walk,
        window: &mut Window,
        cx: &mut Context<LauncherWindow>,
    ) {
        let start = path.len();
        match &node.kind {
            // A node Pane does not know draws its fallback, in its place,
            // else its children.
            NodeKind::Unknown(_) => match node.fallback.as_deref() {
                Some(fallback) => self.node(fallback, path, render, walk, window, cx),
                None => self.children(node, path, render, walk, window, cx),
            },
            // An image's children stand in for it while it loads, under
            // its own place; an empty state's are its actions.
            NodeKind::Image(_) => {
                self.hold(node, path, render, walk, window, cx);
                path.push_str("/placeholder");
                self.children(node, path, render, walk, window, cx);
            }
            NodeKind::EmptyState(_) => {
                self.hold(node, path, render, walk, window, cx);
                path.push_str("/action");
                self.children(node, path, render, walk, window, cx);
            }
            // A List item's children are its own row subtree, drawn in
            // the place of the standard row; its `detail` is the pane's
            // content when it is selected (#240). Both keep their state
            // by key as the rest of the tree does.
            NodeKind::ListItem(_) | NodeKind::GridItem(_) => {
                self.hold(node, path, render, walk, window, cx);
                let own = path.len();
                path.push_str("/row");
                self.children(node, path, render, walk, window, cx);
                if let NodeKind::ListItem(item) = &node.kind
                    && let Some(detail) = item.detail.as_deref()
                {
                    path.truncate(own);
                    path.push_str("/detail");
                    self.node(detail, path, render, walk, window, cx);
                }
                path.truncate(own);
            }
            _ => {
                self.hold(node, path, render, walk, window, cx);
                self.children(node, path, render, walk, window, cx);
            }
        }
        path.truncate(start);
    }

    /// Holds the state of `node` at `path`, if its kind keeps any: the
    /// state there already is kept while its kind matches, a node of
    /// another kind under the same key is new, and what the tree names
    /// for the node is applied onto it.
    fn hold(
        &mut self,
        node: &Node,
        path: &str,
        render: u64,
        walk: &mut Walk,
        window: &mut Window,
        cx: &mut Context<LauncherWindow>,
    ) {
        let Some(kind) = held(node) else {
            return;
        };
        walk.visited.insert(path.to_owned());
        if walk.first.is_none() && focusable(node) {
            walk.first = Some(path.to_owned());
        }
        walk.keys |= node.on_key.is_some();
        let kept = self
            .state
            .get(path)
            .is_some_and(|entry| entry.held.matches(kind));
        if !kept {
            // A node of a different kind under the same key is new: the
            // state the old kind held goes with it.
            let held = Held::new(kind, path, node, window, cx);
            let entry = self
                .state
                .entry(path.to_owned())
                .or_insert_with(KeyedState::blank);
            entry.held = held;
            entry.watching = None;
            entry.asked = false;
        }
        // What the tree now names for the node, onto its state: a field's
        // value (the partially controlled rule) and handlers, and the
        // focus and blur subscriptions it names.
        let watching = {
            let entry = self.state.get_mut(path).expect("the state is held");
            entry.render = render;
            entry.key = node.key.clone().unwrap_or_default();
            entry.on_key = node.on_key;
            let field = matches!(entry.held, Held::Field { .. });
            match &mut entry.held {
                Held::Field {
                    events, editing, ..
                } => {
                    // A date field or a picker of one path edits its value
                    // as a field does, with no events its tree names for
                    // itself.
                    let (value, wired) = match &node.kind {
                        NodeKind::TextInput(input)
                        | NodeKind::PasswordInput(input)
                        | NodeKind::TextArea(input) => (
                            input.value.clone(),
                            Some((input.on_input, input.on_change, input.throttle_ms)),
                        ),
                        NodeKind::DatePicker(date) | NodeKind::DateTimePicker(date) => {
                            (date.value.clone(), None)
                        }
                        NodeKind::FilePicker(picker) | NodeKind::FolderPicker(picker) => {
                            (picker.paths.first().cloned().unwrap_or_default(), None)
                        }
                        _ => (String::new(), None),
                    };
                    if let Some((on_input, on_change, throttle)) = wired {
                        events.on_input = on_input;
                        events.on_change = on_change;
                        events.throttle = throttle.map(Duration::from_millis);
                    }
                    place(editing, events, &value, cx);
                }
                Held::Select {
                    on_change,
                    on_input,
                    ..
                } => match &node.kind {
                    NodeKind::Select(select) => {
                        *on_change = select.on_change;
                        *on_input = select.on_input;
                    }
                    // A List's search-bar dropdown: its own change handler.
                    NodeKind::ListDropdown(dropdown) => *on_change = dropdown.on_change,
                    _ => {}
                },
                _ => {}
            }
            // The focus and blur events, subscribed while the node names
            // a handler — and for a field always, whose blur commits.
            let wants = field || node.on_focus.is_some() || node.on_blur.is_some();
            match &mut entry.watching {
                Some(watching) => {
                    watching.on_focus = node.on_focus;
                    watching.on_blur = node.on_blur;
                    wants
                }
                None => wants,
            }
        };
        if watching {
            let handle = self.state.get(path).and_then(|entry| entry.focus_handle());
            if let Some(handle) = handle {
                let (on_focus, on_blur) = (node.on_focus, node.on_blur);
                let focus_path = path.to_owned();
                let focus = cx.on_focus(&handle, window, move |this, window, cx| {
                    this.designed_focus_event(&focus_path, true, window, cx);
                });
                let blur_path = path.to_owned();
                let blur = cx.on_blur(&handle, window, move |this, window, cx| {
                    this.designed_focus_event(&blur_path, false, window, cx);
                });
                let entry = self.state.get_mut(path).expect("the state is held");
                entry.watching = Some(Watching {
                    on_focus,
                    on_blur,
                    _focus: focus,
                    _blur: blur,
                });
            }
        } else if let Some(entry) = self.state.get_mut(path) {
            entry.watching = None;
        }
        // The focus ask: honoured when it is new — the tree the user saw
        // did not name it — so an open's ask focuses its node and a later
        // ask moves the focus there again, while an unchanged one leaves
        // the focus wherever the user moved it.
        let asked = node.focus;
        if asked
            && !self.state.get(path).is_some_and(|entry| entry.asked)
            && let Some(handle) = self.state.get(path).and_then(|entry| entry.focus_handle())
        {
            window.focus(&handle, cx);
            walk.asked = true;
        }
        if let Some(entry) = self.state.get_mut(path) {
            entry.asked = asked;
        }
    }

    /// Reconciles `node`'s children, each with its own place under
    /// `path`.
    fn children(
        &mut self,
        parent: &Node,
        path: &mut String,
        render: u64,
        walk: &mut Walk,
        window: &mut Window,
        cx: &mut Context<LauncherWindow>,
    ) {
        let duplicates = tree::duplicate_keys(parent);
        for (index, child) in parent.children.iter().enumerate() {
            let start = path.len();
            tree::place_child(path, child, index, &duplicates);
            self.node(child, path, render, walk, window, cx);
            path.truncate(start);
        }
    }

    /// Subscribes the window's keystroke observer, or drops it: the
    /// observer is held only while a drawn node asks for key events.
    fn watch_keys(&mut self, wants: bool, cx: &mut Context<LauncherWindow>) {
        if wants == self.keys.is_some() {
            return;
        }
        self.keys = wants.then(|| {
            cx.observe_keystrokes(|this, event: &gpui::KeystrokeEvent, window, cx| {
                this.designed_key_event(event, window, cx);
            })
        });
    }
}

/// What one tree's walk found: the paths it visited, whether any node
/// asks for key events, the first focusable control's path, and whether a
/// focus ask was honoured.
struct Walk {
    visited: HashSet<String>,
    keys: bool,
    first: Option<String>,
    asked: bool,
}

/// What a view's opening walk found.
pub(super) struct Opened {
    /// The first focusable control's path.
    first: Option<String>,
    /// Whether a focus ask was honoured.
    asked: bool,
}

/// The keyed state of one node of the tree.
pub(crate) struct KeyedState {
    /// What the node's kind keeps.
    pub(super) held: Held,
    /// The render whose tree last named this node: the events its
    /// handlers raise carry it (the tree the user saw).
    pub(super) render: u64,
    /// The node's own key, as the events raised on it name it (empty when
    /// the tree gave it none).
    pub(super) key: String,
    /// The key-event handler the tree names on the node, when it asks for
    /// keys.
    pub(super) on_key: Option<u32>,
    /// Whether the node's focus ask was honoured for the tree on screen
    /// (an unchanged ask does not fire again).
    asked: bool,
    /// The focus and blur subscriptions, and the handlers the tree names
    /// for them.
    pub(super) watching: Option<Watching>,
}

impl KeyedState {
    /// A state with nothing held yet, for a node whose kind the walk is
    /// about to name.
    fn blank() -> KeyedState {
        KeyedState {
            held: Held::Scroll(ScrollHandle::new()),
            render: 0,
            key: String::new(),
            on_key: None,
            asked: false,
            watching: None,
        }
    }

    /// The node's focus handle, when its state holds one.
    pub(super) fn focus_handle(&self) -> Option<FocusHandle> {
        match &self.held {
            Held::Focus(focus) => Some(focus.clone()),
            Held::Canvas { focus, .. } => Some(focus.clone()),
            Held::Field { focus, .. } => Some(focus.clone()),
            Held::Select { focus, .. } => Some(focus.clone()),
            Held::Tags { focus, .. } => Some(focus.clone()),
            Held::Paths { focus, .. } => Some(focus.clone()),
            Held::Scroll(_) => None,
        }
    }
}

/// The focus and blur subscriptions of a node, and the handlers its tree
/// names for them — read as each event fires, so a re-render's new ids
/// need no new subscriptions.
pub(crate) struct Watching {
    pub(super) on_focus: Option<u32>,
    pub(super) on_blur: Option<u32>,
    _focus: Subscription,
    _blur: Subscription,
}

/// Which kind of state a node holds.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum HeldKind {
    /// A focusable control with no state of its own: its focus.
    Focus,
    /// A canvas.
    Canvas,
    /// A text field, password field or text area.
    Field,
    /// A select.
    Select,
    /// A tag picker.
    Tags,
    /// A picker of many paths.
    Paths,
    /// A scroll region.
    Scroll,
}

/// The keyed state a node's kind holds.
pub(super) enum Held {
    /// A focusable control: its focus handle.
    Focus(FocusHandle),
    /// A canvas (#242): its focus handle, where its drawing area was last
    /// laid out (to turn window positions into the canvas's own
    /// coordinates), the drag a press over it holds, and the subscription
    /// that ends one the window can no longer see the release of.
    Canvas {
        focus: FocusHandle,
        bounds: std::rc::Rc<std::cell::Cell<gpui::Bounds<gpui::Pixels>>>,
        drag: std::rc::Rc<std::cell::RefCell<CanvasDrag>>,
        _deactivation: Subscription,
    },
    /// A text field: GPUI CE's editing state — the text, the caret, the
    /// selection and the input method's composition, with undo and the
    /// clipboard — and its partially controlled bookkeeping.
    Field {
        editing: Entity<EditableTextState>,
        focus: FocusHandle,
        events: FieldEvents,
    },
    /// A select: the searchable select of `ui::select`, which keeps its
    /// own open state, query and highlight, and the change handler its
    /// tree names.
    Select {
        select: Entity<crate::ui::select::Select>,
        focus: FocusHandle,
        on_change: Option<u32>,
        /// The callback the popup's query runs as the user types it, when
        /// the tree asks for it: a dropdown whose search the extension
        /// handles (#241).
        on_input: Option<u32>,
    },
    /// A tag picker: the query its field edits, its focus, the tags
    /// chosen and the option highlighted (#241).
    Tags {
        query: Entity<EditableTextState>,
        focus: FocusHandle,
        chosen: Vec<String>,
        highlighted: Option<usize>,
    },
    /// A picker of many paths: its focus and the paths chosen (#241).
    Paths {
        focus: FocusHandle,
        paths: Vec<String>,
    },
    /// A scroll region: its position, kept by the handle its drawing
    /// tracks.
    Scroll(ScrollHandle),
}

impl Held {
    /// The state a node of `kind` at `path` starts from: a field holding
    /// the value its tree names, a select over its options and choice.
    fn new(
        kind: HeldKind,
        path: &str,
        node: &Node,
        window: &mut Window,
        cx: &mut Context<LauncherWindow>,
    ) -> Held {
        match kind {
            HeldKind::Focus => Held::Focus(cx.focus_handle().tab_stop(true)),
            HeldKind::Canvas => {
                let focus = cx.focus_handle().tab_stop(true);
                // A drag ends when the window is deactivated, since the
                // release may then never reach it: the custom view's rule,
                // kept (see `canvas`).
                let path = path.to_owned();
                let deactivation = cx.observe_window_activation(window, move |this, window, cx| {
                    if !window.is_window_active() {
                        this.designed_canvas_deactivated(&path, window, cx);
                    }
                });
                Held::Canvas {
                    focus,
                    bounds: Default::default(),
                    drag: Default::default(),
                    _deactivation: deactivation,
                }
            }
            HeldKind::Field => {
                let editing = cx.new(|cx| EditableTextState::new(StringStorage::default(), cx));
                let focus = editing.read(cx).focus_handle(cx);
                let focus = focus.tab_stop(true);
                let mut events = FieldEvents::default();
                if let NodeKind::TextInput(input) = &node.kind {
                    // The field starts from the value its tree names,
                    // which is also the baseline an unchanged value does
                    // not move, its caret at the value's end.
                    if !input.value.is_empty() {
                        let len = input.value.len();
                        editing.update(cx, |editing, cx| {
                            editing.emplace(&input.value, cx);
                            editing.move_to(len, cx);
                        });
                    }
                    events.rendered = input.value.clone();
                    events.committed = input.value.clone();
                }
                // The field's changes drive its input events and the
                // frame that draws its live value; the subscription lives
                // with the editing state it listens to.
                let path = path.to_owned();
                cx.subscribe_in(
                    &editing,
                    window,
                    move |this, _, _: &TextChanged, window, cx| {
                        this.designed_input_changed(&path, window, cx);
                    },
                )
                .detach();
                Held::Field {
                    editing,
                    focus,
                    events,
                }
            }
            HeldKind::Select => {
                let entity = cx.entity();
                let place = path.to_owned();
                let model = std::rc::Rc::new(move |cx: &App| {
                    entity.read(cx).designed_select_model(&place, cx)
                });
                let entity = cx.entity();
                let place = path.to_owned();
                let commit =
                    std::rc::Rc::new(move |value: &str, window: &mut Window, cx: &mut App| {
                        let place = place.clone();
                        entity.update(cx, |this, cx| {
                            this.designed_select_committed(&place, value, window, cx);
                        });
                    });
                let entity = cx.entity();
                let place = path.to_owned();
                // The query of a dropdown whose search the extension
                // handles, told to it as the user types it (#241).
                let query =
                    std::rc::Rc::new(move |text: &str, window: &mut Window, cx: &mut App| {
                        let place = place.clone();
                        entity.update(cx, |this, cx| {
                            this.designed_select_queried(&place, text, window, cx);
                        });
                    });
                let name = select_label(node).unwrap_or_else(|| "select".into());
                let debug = format!("designed-select-{}", super::components::short(&name));
                let searching = !matches!(&node.kind, NodeKind::Select(select) if !select.search);
                let select = cx.new(|cx| {
                    let select =
                        crate::ui::select::Select::new(name, "", debug, model, commit, window, cx)
                            .on_query(query);
                    if searching {
                        select
                    } else {
                        select.unfiltered()
                    }
                });
                let focus = select.read(cx).trigger_focus();
                Held::Select {
                    select,
                    focus,
                    on_change: None,
                    on_input: None,
                }
            }
            HeldKind::Scroll => Held::Scroll(ScrollHandle::new()),
            HeldKind::Tags => {
                let query = cx.new(|cx| EditableTextState::new(StringStorage::default(), cx));
                let focus = query.read(cx).focus_handle(cx);
                let focus = focus.tab_stop(true);
                // The query's changes move the highlight and redraw; the
                // picker's value is its chosen tags, committed as one.
                let path = path.to_owned();
                cx.subscribe_in(
                    &query,
                    window,
                    move |this, _, _: &TextChanged, window, cx| {
                        let _ = window;
                        if let Some(controls) = this.designed.as_mut()
                            && let Some(entry) = controls.state.get_mut(&path)
                            && let Held::Tags { highlighted, .. } = &mut entry.held
                        {
                            *highlighted = None;
                        }
                        cx.notify();
                    },
                )
                .detach();
                Held::Tags {
                    query,
                    focus,
                    chosen: tags_of(node),
                    highlighted: None,
                }
            }
            HeldKind::Paths => Held::Paths {
                focus: cx.focus_handle().tab_stop(true),
                paths: paths_of(node),
            },
        }
    }

    /// Whether this state is of `kind`.
    fn matches(&self, kind: HeldKind) -> bool {
        matches!(
            (self, kind),
            (Held::Focus(_), HeldKind::Focus)
                | (Held::Canvas { .. }, HeldKind::Canvas)
                | (Held::Field { .. }, HeldKind::Field)
                | (Held::Select { .. }, HeldKind::Select)
                | (Held::Tags { .. }, HeldKind::Tags)
                | (Held::Paths { .. }, HeldKind::Paths)
                | (Held::Scroll(_), HeldKind::Scroll)
        )
    }
}

/// The events one field's tree names, and the bookkeeping of its input
/// events: what was reported to the extension, what is in flight, and
/// what waits for it.
#[derive(Default)]
pub(crate) struct FieldEvents {
    /// The callback the field's value runs as the user types it, when the
    /// tree asks for it.
    pub(super) on_input: Option<u32>,
    /// The callback a commit of the field runs.
    pub(super) on_change: Option<u32>,
    /// The least time between the field's input events, when it asks for
    /// one.
    pub(super) throttle: Option<Duration>,
    /// The value the tree last named for this key: the baseline an
    /// unchanged value does not move.
    pub(super) rendered: String,
    /// The values reported to the extension since the tree was applied —
    /// input and change events' — which an echo of the field's own value
    /// never fights.
    pub(super) reported: Vec<String>,
    /// The value the field last committed, so a commit reports a change
    /// only when the value moved since the last one.
    pub(super) committed: String,
    /// Whether an input event is in flight.
    pub(super) in_flight: bool,
    /// The latest value waiting for the in-flight event, or for the
    /// throttle's time.
    pub(super) pending: Option<String>,
    /// When the last input event was sent, if one was.
    pub(super) sent: Option<Instant>,
    /// Whether a timer is armed to send the waiting value.
    pub(super) armed: bool,
    /// Whether the field's text is being placed by the extension's value,
    /// not typed by the user: the change of it reports no input event.
    pub(super) placing: bool,
}

impl FieldEvents {
    /// Notes that `value` was reported to the extension, so a later
    /// render naming it back is its echo, not an instruction.
    pub(super) fn reported(&mut self, value: &str) {
        self.reported.retain(|reported| reported != value);
        self.reported.push(value.to_owned());
        let excess = self.reported.len().saturating_sub(16);
        self.reported.drain(0..excess);
    }
}

/// Places `value`, the value the tree names for the field, onto its
/// editing state under the partially controlled rule: a value that
/// differs from both the previous render's and the field's own reported
/// values replaces the text and moves the caret to its end; any other is
/// no instruction, and the field keeps what the user typed.
fn place(
    editing: &Entity<EditableTextState>,
    events: &mut FieldEvents,
    value: &str,
    cx: &mut Context<LauncherWindow>,
) {
    let live = editing.read(cx).as_str().to_owned();
    let set = value != events.rendered
        && value != live
        && !events.reported.iter().any(|reported| reported == value);
    // The values the field reported answered the tree that named the old
    // value; this render has them now.
    events.reported.clear();
    if set {
        // The extension set the value: its text replaces the field's and
        // its caret moves to the end. The change of it is the extension's
        // own echo, not the user's typing, so it raises no input event.
        events.placing = true;
        let len = value.len();
        editing.update(cx, |editing, cx| {
            editing.emplace(value, cx);
            editing.move_to(len, cx);
        });
        events.committed = value.to_owned();
        // The set value supersedes whatever typing waited to be told.
        events.pending = None;
    }
    events.rendered = value.to_owned();
}

/// The tags a tag picker node starts with.
fn tags_of(node: &Node) -> Vec<String> {
    match &node.kind {
        NodeKind::TagPicker(picker) => picker.tags.clone(),
        _ => Vec::new(),
    }
}

/// The paths a picker-of-many node starts with.
fn paths_of(node: &Node) -> Vec<String> {
    match &node.kind {
        NodeKind::FilePicker(picker) | NodeKind::FolderPicker(picker) => picker.paths.clone(),
        _ => Vec::new(),
    }
}

/// The value a discrete control's tree names, keyed by its key: a
/// Which kind of state a node holds, `None` for one that holds none: a
/// field, a select, a scroll region, or a control that is focusable or
/// names a handler.
fn held(node: &Node) -> Option<HeldKind> {
    match &node.kind {
        NodeKind::TextInput(_)
        | NodeKind::PasswordInput(_)
        | NodeKind::TextArea(_)
        | NodeKind::DatePicker(_)
        | NodeKind::DateTimePicker(_) => Some(HeldKind::Field),
        // A picker of one path is a field its text edits; one of many
        // holds the paths it chose.
        NodeKind::FilePicker(picker) | NodeKind::FolderPicker(picker) => Some(if picker.multiple {
            HeldKind::Paths
        } else {
            HeldKind::Field
        }),
        NodeKind::Select(select) => (!select.options.is_empty()).then_some(HeldKind::Select),
        // A List's search-bar dropdown is the searchable select, keyed by
        // its place in the tree (#240).
        NodeKind::ListDropdown(dropdown) => {
            (!dropdown.items.is_empty()).then_some(HeldKind::Select)
        }
        NodeKind::TagPicker(_) => Some(HeldKind::Tags),
        NodeKind::Scroll { .. } => Some(HeldKind::Scroll),
        NodeKind::Canvas(canvas) => {
            (canvas.handlers.any() || node.on_key.is_some()).then_some(HeldKind::Canvas)
        }
        _ if focusable(node)
            || node.handles(pane_core::DesignedHandler::Submit)
            || node.on_focus.is_some()
            || node.on_blur.is_some()
            || node.on_key.is_some() =>
        {
            Some(HeldKind::Focus)
        }
        _ => None,
    }
}

/// Whether a node of this kind is a focusable control, as its drawing
/// takes the keyboard.
pub(super) fn focusable(node: &Node) -> bool {
    match &node.kind {
        NodeKind::Button(button) => button.on_press.is_some() && button.enabled,
        NodeKind::Link(link) => link.on_press.is_some(),
        NodeKind::RichRow(row) => row.on_press.is_some(),
        NodeKind::Toggle(toggle) => toggle.on_change.is_some(),
        NodeKind::Checkbox(checkbox) => checkbox.on_change.is_some(),
        NodeKind::Segmented(segmented) => segmented.on_change.is_some(),
        NodeKind::Slider(slider) => slider.on_change.is_some(),
        NodeKind::TextInput(_) | NodeKind::PasswordInput(_) | NodeKind::TextArea(_) => true,
        NodeKind::Select(select) => !select.options.is_empty(),
        NodeKind::Canvas(canvas) => canvas.handlers.any() || node.on_key.is_some(),
        _ => false,
    }
}

/// A select node's label, or its chosen option's, as its control names
/// itself.
fn select_label(node: &Node) -> Option<String> {
    let NodeKind::Select(select) = &node.kind else {
        return None;
    };
    select.label.clone().or_else(|| {
        select
            .value
            .as_deref()
            .and_then(|value| {
                select
                    .options
                    .iter()
                    .find(|option| option.value == value)
                    .map(|option| option.label.clone().unwrap_or_else(|| option.value.clone()))
            })
            .or_else(|| select.options.first().map(|option| option.value.clone()))
    })
}

/// The drag a press over a canvas holds (#242, the custom view's): what
/// the window keeps while the button pressed over the canvas may still be
/// down. Moves are coalesced as the custom view's were: while one is in
/// flight, only the latest further move waits, and it is sent when the
/// moves in flight are answered.
pub(crate) struct CanvasDrag {
    /// Whether the primary button was pressed over the canvas and is
    /// still held: pointer moves and the release are sent only while it
    /// is.
    pub pressed: bool,
    /// Where the pointer was last seen during the press, in the canvas's
    /// coordinates: where a drag the window can no longer see the release
    /// of ends.
    pub last: (f32, f32),
    /// Pointer moves sent to the view and not answered yet.
    pub in_flight: u32,
    /// The latest move of a drag that has not been sent, with the
    /// modifiers held when it happened: sent when the moves in flight are
    /// answered.
    pub waiting: Option<((f32, f32), gpui::Modifiers)>,
}

impl Default for CanvasDrag {
    fn default() -> CanvasDrag {
        CanvasDrag {
            pressed: false,
            last: (0., 0.),
            in_flight: 0,
            waiting: None,
        }
    }
}
