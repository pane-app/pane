//! A designed view, as an author writes it: a [`View`] whose `render`
//! answers a tree of [`Node`]s, built with [`column`], [`row`], [`text`],
//! [`button`] and every other node of the UI component set, whose
//! controls' `on_click` closures run with the view's state mutably
//! through [`Cx::listener`]. The SDK answers Pane's `render` with the
//! tree as the versioned JSON document of `docs/designed-tree.md` (ADR
//! 0036's envelope), names each listener by a per-render callback id, and
//! runs the closure Pane hands back — so an author never sees the JSON
//! or the ids.
//!
//! A command whose `pane.json` entry says `"mode": "designed"` opens a
//! view as its screen: [`Command::open_designed_view`] answers with the
//! view's state (its [`Command::DesignedView`]), and Pane never calls the
//! command's `render`. Pane asks for the tree again after each event, so
//! the view's `render` reads the state the event's listener changed. A
//! view that changes by itself asks for that: the answer of its render
//! names when Pane asks again ([`IntoNode::refresh_after`], #236), and
//! data on its way is shown as a loading state whose arrival asks for a
//! drawing itself ([`Pending`], #243) — see the timer sample
//! (`sample-timer`).
//!
//! ```ignore
//! struct Counter {
//!     count: u32,
//! }
//!
//! impl pane_extension::view::View for Counter {
//!     fn render(&mut self, cx: &mut Cx<Self>) -> impl IntoAnswer {
//!         column()
//!             .gap(Space::M)
//!             .child(text(format!("Count: {}", self.count)).style(TextStyle::Title))
//!             .child(
//!                 row().gap(Space::S).children([
//!                     button("Increment").on_click(cx.listener(|this: &mut Self| {
//!                         this.count += 1;
//!                     })),
//!                     button("Reset")
//!                         .tone(Tone::Destructive)
//!                         .on_click(cx.listener(|this: &mut Self| this.count = 0)),
//!                 ]),
//!             )
//!     }
//! }
//!
//! impl pane_extension::Command for Sample {
//!     type DesignedView = Counter;
//!
//!     async fn open_designed_view(
//!         command: String,
//!         launch: LaunchRecord,
//!     ) -> Result<Counter, String> {
//!         Ok(Counter { count: 0 })
//!     }
//! }
//! ```
//!
//! Every node carries a [`Style`] — how it takes space, and the surface
//! it draws, with `hover` and `pressed` variants Pane applies itself —
//! set through the same-named methods on every builder. A colour a node
//! draws with is the tone token ([`crate::icon::Tone`]), a raw
//! colour, or a light and dark pair, each corrected for contrast by Pane
//! unless it is given as [`Paint::exact`]. Icons and images come from
//! [`crate::Icon`], the one icon model the list tree shares.
//!
//! The listeners of the last two renders are kept: a press of what the
//! user could see is delivered even if the view has rendered since, and
//! an older event is dropped. The component set's version is written into
//! every document; a Pane that renders another major refuses it naming
//! both versions, and one that knows less draws what it understands.
//!
//! A view's answer may navigate (#239): [`Cx::push`] pushes a view above
//! this one, [`Cx::replace`] replaces this view with one, [`Cx::pop`] and
//! [`Cx::pop_with`] pop it — with a result answered to the view below,
//! whose own `on_pop` (given to [`Cx::push_with`]) runs with it. The back
//! key pops a view without asking, delivering the pop event with no
//! result. [`Container::navigation_title`] names the view, shown where a
//! screen's title is.

use alloc::borrow::ToOwned as _;
use alloc::boxed::Box;
use alloc::format;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::{OnceCell, RefCell};
use core::fmt::Write as _;
use core::future::Future;
use core::pin::Pin;
use core::sync::atomic::{AtomicU64, Ordering};
use core::time::Duration;

use crate::exports::pane::extension::command as wit;
use crate::icon;
pub use crate::icon::{Color, Icon};
use crate::pane::extension::view::ask_to_render;
use crate::pane::extension::view::measure_text as wit_measure_text;
use wit::{GuestView, Outcome, Rendered, UiEvent};
use wit_bindgen::spawn_local;

/// The version of the UI component set this SDK writes
/// (`docs/designed-tree.md`).
const COMPONENT_SET: &str = "1.3";

/// The callback id of the pop event, the event Pane sends a view when the
/// one above it popped: an id no tree names (this SDK's ids start at 1),
/// its payload `{"pop": <result>}`, the result the pop answered (`null`
/// when the back key popped, so no view's result reached the one below).
const POP_CALLBACK: u32 = 0;

/// The id of the view being rendered, as its context named it: what
/// [`Pending`]'s work asks to be drawn again when it answers (#243). Set
/// for the duration of the view's `render`, read where the work is
/// started inside it — a component's code runs on one thread, one view at
/// a time, so one slot serves every view of the command.
static ASKING_VIEW: AtomicU64 = AtomicU64::new(0);

/// A designed view: the screen a `"mode": "designed"` command opens. Pane
/// asks for its tree when the view opens and after each event, with a
/// [`Cx`] through which the tree's listeners are named; the view's state
/// is the author's, kept in the view's resource for as long as it is
/// open. A view that changes by itself asks for that: the answer of its
/// render names when Pane asks again ([`IntoNode::refresh_after`],
/// #236), so timers, clocks, progress and polling need no change to the
/// extension runtime.
pub trait View: Sized + 'static {
    /// The view's tree, as it is now. The controls' `on_click` listeners
    /// are named through `cx`; the tree is drawn whole again after each
    /// event, so what the view's state now says is what the user sees.
    /// The answer may ask Pane to draw it again after some time.
    fn render(&mut self, cx: &mut Cx<Self>) -> impl IntoAnswer;
}

/// The context of one render: where the tree's listeners are named, each
/// by a fresh callback id, kept by the view until the next-but-one render,
/// and whether the render answers the refresh the view asked for (where
/// an interval's work runs).
pub struct Cx<'a, V: View> {
    listeners: &'a mut Vec<Run<V>>,
    refreshed: bool,
    selected: Option<String>,
}

impl<V: View> Cx<'_, V> {
    /// The listener `run` becomes, which a press of the control it is
    /// given to runs with the view's state mutably: `cx.listener(|this:
    /// &mut V| ...)`, handed to [`Button::on_click`] and its kind. The
    /// view re-renders after the press, as it does after every event.
    pub fn listener(&mut self, run: impl FnOnce(&mut V) + 'static) -> Listener {
        let id = self.listeners.len() as u32 + 1;
        self.listeners.push(Run::Listener(Box::new(run)));
        Listener(id)
    }

    /// The listener `run` becomes, told the text the event carried with
    /// the view's state mutably: `cx.value_listener(|this: &mut V, value:
    /// &str| ...)`. A field's value as the user typed or committed it,
    /// or a key as it was pressed, is the text: handed to
    /// [`TextInput::on_input`], [`TextInput::on_change`] and the key
    /// listeners.
    pub fn value_listener(&mut self, run: impl FnOnce(&mut V, &str) + 'static) -> ValueListener {
        let id = self.listeners.len() as u32 + 1;
        self.listeners.push(Run::Value(Box::new(run)));
        ValueListener(id)
    }

    /// The listener `run` becomes, told the canvas event that reached it
    /// with the view's state mutably: `cx.canvas_listener(|this: &mut V,
    /// event: CanvasEvent| ...)`, handed to a canvas's
    /// [`on_pointer_down`](CanvasBuilder::on_pointer_down) and its kind.
    pub fn canvas_listener(
        &mut self,
        run: impl FnOnce(&mut V, CanvasEvent) + 'static,
    ) -> CanvasListener {
        let id = self.listeners.len() as u32 + 1;
        self.listeners.push(Run::Canvas(Box::new(run)));
        CanvasListener(id)
    }

    /// The listener `run` becomes, told the values the form was submitted
    /// with: `cx.form_listener(|this: &mut V, values: &FormValues| ...)`,
    /// handed to [`Form::on_submit`](crate::view::form::Form::on_submit)
    /// (#241). Each field's value is keyed by its key.
    pub fn form_listener(
        &mut self,
        run: impl FnOnce(&mut V, &crate::form::FormValues) + 'static,
    ) -> FormListener {
        let id = self.listeners.len() as u32 + 1;
        self.listeners.push(Run::Form(Box::new(run)));
        FormListener(id)
    }

    /// A press that pushes a view above this one: `open` builds the
    /// pushed view's state from this view's, this view staying below it
    /// (its state kept, its tree shown again when the pushed view pops).
    /// Handed to [`Button::on_click`], as a listener is.
    pub fn push(&mut self, open: impl FnOnce(&mut V) -> V + 'static) -> Listener {
        self.pushed(Box::new(open), None)
    }

    /// A press that pushes a view above this one, `on_pop` answering this
    /// view when it pops: with the result the pop answered (or `None`, a
    /// pop that carried none, as the back key's), this view re-rendering
    /// after. The Raycast-style `Action.Push`'s `onPop`.
    pub fn push_with(
        &mut self,
        open: impl FnOnce(&mut V) -> V + 'static,
        on_pop: impl FnOnce(&mut V, Option<&str>) + 'static,
    ) -> Listener {
        self.pushed(Box::new(open), Some(Box::new(on_pop)))
    }

    /// A press that pushes a view above this one, `on_pop` kept to answer
    /// this view when it pops.
    fn pushed(
        &mut self,
        open: Box<dyn FnOnce(&mut V) -> V>,
        on_pop: Option<Box<dyn FnOnce(&mut V, Option<&str>)>>,
    ) -> Listener {
        let id = self.listeners.len() as u32 + 1;
        self.listeners.push(Run::Push { open, on_pop });
        Listener(id)
    }

    /// A press that replaces this view with the state `open` builds: this
    /// view is dropped, never used again, the views below it staying.
    pub fn replace(&mut self, open: impl FnOnce(&mut V) -> V + 'static) -> Listener {
        let id = self.listeners.len() as u32 + 1;
        self.listeners.push(Run::Replace(Box::new(open)));
        Listener(id)
    }

    /// A press that pops this view: the view below's tree shows again and
    /// it is told the view above popped, with no result.
    pub fn pop(&mut self) -> Listener {
        let id = self.listeners.len() as u32 + 1;
        self.listeners.push(Run::Pop(None));
        Listener(id)
    }

    /// A press that pops this view, answering `result` — built from this
    /// view's state — to the view below, whose own `on_pop` runs with it
    /// before it re-renders.
    pub fn pop_with(&mut self, result: impl FnOnce(&mut V) -> String + 'static) -> Listener {
        let id = self.listeners.len() as u32 + 1;
        self.listeners.push(Run::Pop(Some(Box::new(result))));
        Listener(id)
    }

    /// Whether this render answers the refresh the view asked for by
    /// [`IntoNode::refresh_after`]: `false` on the view's first render,
    /// on the one after an event, and on the one a loading state's data
    /// asked for itself (see [`Pending`]). This is where an interval's
    /// work runs — counting a tick, advancing a clock — so that it runs
    /// once per interval and never twice for one drawing.
    pub fn refreshed(&self) -> bool {
        self.refreshed
    }

    /// The selected item's key of the view's List or Grid, as the render's
    /// context names it (#240); `None` when it says none. The detail pane's
    /// content is built for the selected item alone, so an item's `detail`
    /// is built only when its key is this one.
    pub fn selected(&self) -> Option<&str> {
        self.selected.as_deref()
    }
}

/// A named listener of the tree: what [`Cx::listener`], [`Cx::push`],
/// [`Cx::replace`], [`Cx::pop`] and their kinds answer, handed to
/// [`Button::on_click`] and its kind.
#[derive(Clone, Debug)]
pub struct Listener(u32);

/// A named listener of the tree that the event's text reaches: what
/// [`Cx::value_listener`] answers, handed to [`TextInput::on_input`],
/// [`TextInput::on_change`] and the key listeners. The text is the
/// event's payload's `value` (a field's, a control's) or `key` (a key
/// pressed).
#[derive(Debug)]
pub struct ValueListener(u32);

/// A named listener of a canvas's input: what [`Cx::canvas_listener`]
/// answers, handed to a canvas's pointer, wheel, double-click, secondary
/// and resize handlers, told the event that reached it.
#[derive(Debug)]
pub struct CanvasListener(u32);

/// What a listener of the tree does when its node is pressed: run it and
/// answer nothing next (the view re-renders, as it always does), or
/// answer the navigation one of [`Cx`]'s push, replace and pop listeners
/// builds.
enum Run<V> {
    /// Runs the listener; nothing next.
    Listener(Box<dyn FnOnce(&mut V)>),
    /// Runs the listener with the event's text; nothing next.
    Value(Box<dyn FnOnce(&mut V, &str)>),
    /// Runs the listener with the canvas event the payload names; nothing
    /// next.
    Canvas(Box<dyn FnOnce(&mut V, CanvasEvent)>),
    /// Runs the listener with the form's submitted values; nothing next
    /// (#241).
    Form(Box<dyn FnOnce(&mut V, &crate::form::FormValues)>),
    /// Builds the state of a view pushed above this one, `on_pop` (when
    /// given) kept to answer this view when it pops.
    Push {
        open: Box<dyn FnOnce(&mut V) -> V>,
        on_pop: Option<Box<dyn FnOnce(&mut V, Option<&str>)>>,
    },
    /// Builds the state of the view replacing this one.
    Replace(Box<dyn FnOnce(&mut V) -> V>),
    /// Pops this view, answering the result the closure builds (an empty
    /// one when there is none).
    Pop(Option<Box<dyn FnOnce(&mut V) -> String>>),
}

/// One node of the tree: a layout primitive or UI component, with the
/// style every node carries and the properties its own kind reads. Built
/// with the constructors of this module, and given its identity with
/// [`Node::key`]. The `kind`, `key`, `navigation_title` and `focus`
/// fields are `pub(crate)`: `crate::form`'s field builders read and
/// change them on the nodes this module hands them.
#[derive(Debug)]
pub struct Node {
    pub(crate) kind: NodeKind,
    pub(crate) style: Style,
    /// Where in a `stack` this node is placed; `None` for the stack's own
    /// placement. Nowhere else is it read.
    pub(crate) place: Option<Place>,
    /// How far this node sits from its place in a `stack`.
    pub(crate) offset: Option<(Length, Length)>,
    pub(crate) key: Option<String>,
    pub(crate) name: Option<String>,
    /// What names the view, when this node's the tree's root: shown where
    /// a screen's title is. Ignored on any other node.
    pub(crate) navigation_title: Option<String>,
    pub(crate) requires: Option<u64>,
    pub(crate) fallback: Option<Box<Node>>,
    pub(crate) children: Vec<Node>,
    /// Whether the node asks for the keyboard (see [`Node::focus`]).
    pub(crate) focus: bool,
    /// The listener a focus of this node runs, it taking the keyboard.
    pub(crate) on_focus: Option<Listener>,
    /// The listener a blur of this node runs, it losing the keyboard.
    pub(crate) on_blur: Option<Listener>,
    /// The listener a key pressed while this node is focused runs, told
    /// the key as a key sequence spells it.
    pub(crate) on_key: Option<ValueListener>,
}

impl Node {
    pub(crate) fn of(kind: NodeKind) -> Node {
        Node {
            kind,
            style: Style::default(),
            place: None,
            offset: None,
            key: None,
            name: None,
            navigation_title: None,
            requires: None,
            fallback: None,
            children: Vec::new(),
            focus: false,
            on_focus: None,
            on_blur: None,
            on_key: None,
        }
    }

    /// The node's stable identity among its siblings, which Pane keeps
    /// node state under.
    pub fn key(mut self, key: impl Into<String>) -> Node {
        self.key = Some(key.into());
        self
    }

    /// The name assistive technology reads the node by.
    pub fn name(mut self, name: impl Into<String>) -> Node {
        self.name = Some(name.into());
        self
    }

    /// What names the view, when this node is the tree's root: shown where
    /// a screen's title is. Ignored on any other node.
    pub fn navigation_title(mut self, title: impl Into<String>) -> Node {
        self.navigation_title = Some(title.into());
        self
    }

    /// The minimum minor version of the UI component set this node needs.
    pub fn requires(mut self, requires: u64) -> Node {
        self.requires = Some(requires);
        self
    }

    /// The node drawn instead when Pane does not know this one.
    pub fn fallback(mut self, fallback: impl IntoNode) -> Node {
        self.fallback = Some(Box::new(fallback.into_node()));
        self
    }

    /// A child of this node's, after its children.
    pub fn child(mut self, child: impl IntoNode) -> Node {
        self.children.push(child.into_node());
        self
    }

    /// The children of this node's, after its children.
    pub fn children(mut self, children: impl IntoIterator<Item = impl IntoNode>) -> Node {
        self.children
            .extend(children.into_iter().map(IntoNode::into_node));
        self
    }

    /// Asks for the keyboard: a node whose ask is new — the tree the user
    /// saw did not name it — is focused, so a view's opening ask is an
    /// auto-focus and a later one a focus moved from code. An unchanged
    /// ask leaves the focus wherever the user moved it.
    pub fn focus(mut self) -> Node {
        self.focus = true;
        self
    }

    /// The listener a focus of this node runs, it taking the keyboard.
    pub fn on_focus(mut self, listener: Listener) -> Node {
        self.on_focus = Some(listener);
        self
    }

    /// The listener a blur of this node runs, it losing the keyboard.
    pub fn on_blur(mut self, listener: Listener) -> Node {
        self.on_blur = Some(listener);
        self
    }

    /// The listener a key pressed while this node is focused runs, told
    /// the key as a key sequence spells it (Tab, Enter and Escape stay
    /// with Pane).
    pub fn on_key(mut self, listener: ValueListener) -> Node {
        self.on_key = Some(listener);
        self
    }
}

/// What a node is, and the properties its own kind reads.
#[derive(Debug)]
pub enum NodeKind {
    Column(Layout),
    Row(Layout),
    Stack(Place),
    Scroll(Orientation),
    Spacer,
    Divider(Orientation),
    Text(Text),
    Button {
        label: String,
        tone: Option<Tone>,
        icon: Option<Icon>,
        keys: Option<Vec<String>>,
        on_click: Option<Listener>,
        enabled: bool,
    },
    Link {
        label: String,
        on_click: Option<Listener>,
        color: Option<Paint>,
    },
    Icon(IconNodePayload),
    IconTile(IconNodePayload),
    Image {
        image: Icon,
        size: Option<IconSize>,
        fit: Fit,
    },
    RichRow(RichRow),
    Keycap(String),
    KeySequence(Vec<String>),
    Tag(TagPayload),
    Badge(TagPayload),
    Toggle(TogglePayload),
    Checkbox(TogglePayload),
    Segmented(SegmentedPayload),
    Select(SegmentedPayload),
    Slider {
        value: f32,
        min: f32,
        max: f32,
        step: f32,
        on_click: Option<Listener>,
        label: Option<String>,
    },
    Progress {
        value: f32,
        label: Option<String>,
    },
    Loading {
        label: Option<String>,
    },
    Markdown(String),
    Card(Layout),
    SectionHeader(SectionHeaderPayload),
    MetadataList(MetadataListPayload),
    EmptyState(EmptyStatePayload),
    TextInput(TextInputPayload),
    PasswordInput(TextInputPayload),
    TextArea(TextInputPayload),
    /// A standard List (#240): its items in sections, whose search field
    /// and selection Pane owns.
    List(ListNode),
    /// A standard Grid (#240): a List's behaviour with cells.
    Grid(ListNode),
    /// A section of a List's or Grid's items.
    ListSection(SectionNode),
    /// One item of a List.
    ListItem(ListItem),
    /// One cell of a Grid.
    GridItem(GridItem),
    /// A List's search-bar dropdown.
    ListDropdown(DropdownNode),
    /// A Detail (#240): a scrolled column of what a record is.
    Detail(Layout),
    /// A canvas: a leaf the view draws into with drawing operations
    /// (#242).
    Canvas(CanvasNode),
    /// A form: a column of fields whose submission is an action (#241).
    Form(FormPayload),
    /// One date field, "YYYY-MM-DD".
    DatePicker(DatePayload),
    /// One date and time field, "YYYY-MM-DD HH:MM".
    DateTimePicker(DatePayload),
    /// One tag picker: a multi-select of its options.
    TagPicker(TagPickerPayload),
    /// One file picker: a path typed or chosen with the system's dialog.
    FilePicker(PathPayload),
    /// One folder picker: a path typed or chosen with the system's dialog.
    FolderPicker(PathPayload),
}

/// The layout of a `column`, `row` or `card`: its gap, padding,
/// alignment, distribution and whether children wrap.
#[derive(Clone, Debug, Default)]
pub struct Layout {
    gap: Option<Space>,
    padding: Padding,
    align: Option<Align>,
    justify: Option<Justify>,
    wrap: bool,
}

/// The padding inside a node: each side's space token, `None` for none.
#[derive(Clone, Copy, Debug, Default)]
pub struct Padding {
    /// The space above the node's content.
    pub top: Option<Space>,
    /// The space to its right.
    pub right: Option<Space>,
    /// The space below it.
    pub bottom: Option<Space>,
    /// The space to its left.
    pub left: Option<Space>,
}

/// The style every node carries: how it takes space (its [`Sizing`]) and
/// the surface it draws (its [`Surface`]), with the `hover` and `pressed`
/// variants Pane applies itself, without calling the extension. Nothing
/// is set until an author sets it.
#[derive(Clone, Debug, Default)]
pub struct Style {
    pub(crate) sizing: Sizing,
    pub(crate) surface: Surface,
    pub(crate) hover: Option<Surface>,
    pub(crate) pressed: Option<Surface>,
}

/// How a node takes space: its grow, shrink, basis, sizes and aspect
/// ratio.
#[derive(Clone, Debug, Default)]
pub struct Sizing {
    pub(crate) grow: Option<f32>,
    pub(crate) shrink: Option<f32>,
    pub(crate) basis: Option<Length>,
    pub(crate) width: Option<Length>,
    pub(crate) height: Option<Length>,
    pub(crate) min_width: Option<Length>,
    pub(crate) max_width: Option<Length>,
    pub(crate) min_height: Option<Length>,
    pub(crate) max_height: Option<Length>,
    pub(crate) aspect_ratio: Option<f32>,
}

/// The surface a node draws: its background, border, corner radius and
/// opacity. A variant of it restates any of them.
#[derive(Clone, Debug, Default)]
pub struct Surface {
    pub(crate) background: Option<Paint>,
    pub(crate) border: Option<Border>,
    pub(crate) radius: Option<Radius>,
    pub(crate) opacity: Option<f32>,
}

/// The border a node draws: how wide, and in which colour. Either alone
/// draws a hairline of the colour, or a border-wide edge in the theme's
/// own colour.
#[derive(Clone, Debug, Default)]
pub struct Border {
    width: Option<Length>,
    color: Option<Paint>,
}

/// One length: a space token, pixels, or a fraction of the parent.
#[derive(Clone, Copy, Debug)]
pub enum Length {
    /// The named distance.
    Token(Space),
    /// This many pixels.
    Px(f32),
    /// This fraction of the parent's size.
    Fraction(f32),
}

/// A corner radius: a token, or pixels.
#[derive(Clone, Copy, Debug)]
pub enum Radius {
    /// A control's small radius.
    S,
    /// A row's medium radius.
    M,
    /// A card's large radius.
    L,
    /// A pill or a disc.
    Full,
    /// This many pixels.
    Px(f32),
}

/// How big an icon or an image is: a token, or pixels.
#[derive(Clone, Copy, Debug)]
pub enum IconSize {
    S,
    M,
    L,
    Xl,
    /// This many pixels.
    Px(f32),
}

/// One colour a node draws with: a tone or a raw colour (corrected for
/// contrast), a light and dark pair, or one given exactly, correction
/// off.
#[derive(Clone, Debug)]
pub enum Paint {
    /// The colour, or the tone as its name, corrected for contrast
    /// against what it is drawn on.
    Color(Color),
    /// One colour for the light theme and one for the dark.
    Pair { light: Color, dark: Color },
    /// The colour drawn exactly as it is, contrast correction off.
    Exact(Color),
    /// One exact colour for the light theme and one for the dark.
    ExactPair { light: Color, dark: Color },
}

impl From<Color> for Paint {
    fn from(color: Color) -> Paint {
        Paint::Color(color)
    }
}

impl From<crate::icon::Tone> for Paint {
    fn from(tone: crate::icon::Tone) -> Paint {
        Paint::Color(Color::Tone(tone))
    }
}

/// Where in a `stack` a child is placed: one of its nine places.
#[derive(Clone, Copy, Debug)]
pub enum Place {
    TopStart,
    Top,
    TopEnd,
    Start,
    Center,
    End,
    BottomStart,
    Bottom,
    BottomEnd,
}

/// Which way a `scroll` scrolls, or a `divider` runs.
#[derive(Clone, Copy, Debug)]
pub enum Orientation {
    Vertical,
    Horizontal,
}

/// How an image fits the box it is given.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Fit {
    #[default]
    Contain,
    Cover,
    Fill,
}

/// One text node: what it says, plain or in spans, and how it is drawn.
#[derive(Debug)]
pub struct Text {
    content: TextContent,
    style: Option<TextStyle>,
    level: Option<TextLevel>,
    color: Option<Paint>,
    size: Option<f32>,
    weight: Option<f32>,
    truncate: bool,
}

/// What a text says: one string, or runs of styled and linked spans.
#[derive(Debug)]
pub enum TextContent {
    Plain(String),
    Spans(Vec<Span>),
}

/// One span of a text: a run of its content, styled, and a link when it
/// carries an `on_click` listener.
#[derive(Clone, Debug)]
pub struct Span {
    text: String,
    style: Option<TextStyle>,
    level: Option<TextLevel>,
    color: Option<Paint>,
    code: bool,
    on_click: Option<Listener>,
}

/// A rich row: the launcher's own result row as a component.
#[derive(Debug, Default)]
pub struct RichRow {
    title: String,
    subtitle: Option<String>,
    icon: Option<Icon>,
    accessories: Vec<Accessory>,
    on_click: Option<Listener>,
}

/// One accessory at a rich row's end.
#[derive(Debug)]
pub struct Accessory {
    text: String,
    tag: bool,
    color: Option<Paint>,
}

/// One option of a segmented control, select or tag picker: its value,
/// what is drawn for it, and the section it belongs to (#241).
#[derive(Debug)]
pub struct Choice {
    value: String,
    label: Option<String>,
    /// The section the option belongs to, drawn as a group's header.
    pub(crate) section: Option<String>,
}

/// One row of a metadata list: a label and its value, link or tags.
#[derive(Debug, Default)]
pub struct MetadataItem {
    label: Option<String>,
    value: Option<String>,
    on_click: Option<Listener>,
    tags: Vec<String>,
    separator: bool,
}

/// What a canvas node holds (#242): its drawing operations, what it is to
/// assistive technology, and the handlers its input names.
#[derive(Debug, Default)]
pub struct CanvasNode {
    /// The operations it paints, in order.
    pub ops: Vec<Draw>,
    /// What the one node the canvas is to assistive technology says.
    pub a11y: CanvasA11y,
    /// The handlers the canvas's input names.
    pub handlers: CanvasHandlers,
}

/// What the one node a canvas is to assistive technology says.
#[derive(Debug, Default)]
pub struct CanvasA11y {
    /// What kind of control the canvas is; a generic one when `None`.
    pub role: Option<CanvasRole>,
    /// What names the canvas.
    pub label: Option<String>,
    /// What the canvas currently holds.
    pub value: Option<String>,
}

/// What kind of control a canvas is to assistive technology.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CanvasRole {
    /// A color chooser, whose value names its chosen color.
    ColorWell,
    /// A value between bounds, adjustable by increment and decrement.
    Slider,
    /// A picture, whose label names what it shows.
    Image,
    /// A figure: a picture with its caption in the label.
    Figure,
    /// A group of things the label names.
    Group,
    /// Whatever else the canvas is.
    Generic,
}

/// The handlers a canvas's input names, each the listener its tree runs
/// when that input happens. Keys ride the node's own `on_key` as every
/// focusable node's do; `on_increment`, `on_decrement` and `on_activate`
/// are the semantic handlers the up and down arrows and Space run — told
/// nothing — so a control-like canvas needs no key parsing, while the
/// pointer, wheel and resize handlers are told the event that reached
/// them (`Cx::canvas_listener`).
#[derive(Debug, Default)]
pub struct CanvasHandlers {
    /// The up arrow.
    pub on_increment: Option<Listener>,
    /// The down arrow.
    pub on_decrement: Option<Listener>,
    /// Space.
    pub on_activate: Option<Listener>,
    /// The primary button pressed over the canvas.
    pub on_pointer_down: Option<CanvasListener>,
    /// That button released.
    pub on_pointer_up: Option<CanvasListener>,
    /// The pointer moved while that button is held — a drag, coalesced to
    /// the latest while one is in flight.
    pub on_pointer_move: Option<CanvasListener>,
    /// The pointer entering the canvas's hover.
    pub on_pointer_enter: Option<CanvasListener>,
    /// The pointer leaving it.
    pub on_pointer_leave: Option<CanvasListener>,
    /// The wheel turned over the canvas.
    pub on_wheel: Option<CanvasListener>,
    /// The primary button pressed twice over the canvas.
    pub on_double_click: Option<CanvasListener>,
    /// The secondary button pressed over the canvas.
    pub on_secondary: Option<CanvasListener>,
    /// The canvas's size changing.
    pub on_resize: Option<CanvasListener>,
}

/// One drawing operation of a canvas: what it paints, or how it moves the
/// state the painting that follows paints in. Coordinates are logical
/// pixels in the canvas's own space, its origin its top-left corner.
#[derive(Clone, Debug)]
pub enum Draw {
    /// A rectangle, filled and/or stroked, its corners rounded by `radius`.
    Rect {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        radius: f32,
        fill: Option<Paint>,
        stroke: Option<CanvasStroke>,
    },
    /// A circle, `x` and `y` its center.
    Circle {
        x: f32,
        y: f32,
        radius: f32,
        fill: Option<Paint>,
        stroke: Option<CanvasStroke>,
    },
    /// One line of text, its top-left corner at `x`, `y`.
    Text {
        x: f32,
        y: f32,
        content: String,
        style: Option<TextStyle>,
        level: Option<TextLevel>,
        color: Option<Paint>,
        size: Option<f32>,
        weight: Option<f32>,
    },
    /// An image of the icon model, at a position and size.
    Image {
        image: Icon,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    },
    /// Move the current path's point, starting a new sub-path.
    MoveTo { x: f32, y: f32 },
    /// A straight line to a point.
    LineTo { x: f32, y: f32 },
    /// A quadratic curve to a point, through a control point.
    QuadTo { cx: f32, cy: f32, x: f32, y: f32 },
    /// A cubic curve to a point, through two control points.
    CubicTo {
        c1x: f32,
        c1y: f32,
        c2x: f32,
        c2y: f32,
        x: f32,
        y: f32,
    },
    /// An arc of a circle, `x` and `y` its center, from `start` to `end`
    /// radians, drawn the short way unless `ccw`.
    Arc {
        x: f32,
        y: f32,
        radius: f32,
        start: f32,
        end: f32,
        ccw: bool,
    },
    /// Close the current sub-path with a line to where it began.
    Close,
    /// Fill the current path, which it ends.
    Fill { color: Paint },
    /// Stroke the current path, which it ends.
    Stroke { stroke: CanvasStroke },
    /// Clip what follows to this rectangle, intersected with the clips
    /// before it.
    Clip {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    },
    /// Move the origin the operations that follow draw at.
    Translate { x: f32, y: f32 },
    /// Scale the space the operations that follow draw in, from the origin.
    Scale { x: f32, y: f32 },
    /// Rotate the space the operations that follow draw in, around the
    /// origin, clockwise by this many degrees.
    Rotate { degrees: f32 },
}

/// One stroke: its colour, its width, and the shapes of its ends and
/// corners.
#[derive(Clone, Debug)]
pub struct CanvasStroke {
    pub color: Paint,
    /// One pixel when the tree gives none.
    pub width: f32,
    pub cap: Option<Cap>,
    pub join: Option<Join>,
}

/// How a stroke's ends are drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cap {
    Butt,
    Round,
    Square,
}

/// How a stroke's corners are drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Join {
    Miter,
    Round,
    Bevel,
}

impl Draw {
    /// A rectangle at `x`, `y` of `width` and `height`.
    pub fn rect(x: f32, y: f32, width: f32, height: f32) -> Draw {
        Draw::Rect {
            x,
            y,
            width,
            height,
            radius: 0.,
            fill: None,
            stroke: None,
        }
    }

    /// A rectangle with its corners rounded by `radius`.
    pub fn rounded(x: f32, y: f32, width: f32, height: f32, radius: f32) -> Draw {
        Draw::Rect {
            x,
            y,
            width,
            height,
            radius,
            fill: None,
            stroke: None,
        }
    }

    /// A circle, `x` and `y` its center.
    pub fn circle(x: f32, y: f32, radius: f32) -> Draw {
        Draw::Circle {
            x,
            y,
            radius,
            fill: None,
            stroke: None,
        }
    }

    /// One line of text. Place it with [`Draw::at`].
    pub fn text(content: impl Into<String>) -> Draw {
        Draw::Text {
            x: 0.,
            y: 0.,
            content: content.into(),
            style: None,
            level: None,
            color: None,
            size: None,
            weight: None,
        }
    }

    /// An image of the icon model. Place and size it with [`Draw::at`] and
    /// [`Draw::size`].
    pub fn image(image: Icon) -> Draw {
        Draw::Image {
            image,
            x: 0.,
            y: 0.,
            width: 16.,
            height: 16.,
        }
    }

    /// A fill of the current path.
    pub fn fill(color: impl Into<Paint>) -> Draw {
        Draw::Fill {
            color: color.into(),
        }
    }

    /// A stroke of the current path, one pixel wide.
    pub fn stroke(color: impl Into<Paint>) -> Draw {
        Draw::Stroke {
            stroke: CanvasStroke {
                color: color.into(),
                width: 1.,
                cap: None,
                join: None,
            },
        }
    }

    /// Clip what follows to a rectangle.
    pub fn clip(x: f32, y: f32, width: f32, height: f32) -> Draw {
        Draw::Clip {
            x,
            y,
            width,
            height,
        }
    }

    /// Move the origin the operations that follow draw at.
    pub fn translate(x: f32, y: f32) -> Draw {
        Draw::Translate { x, y }
    }

    /// Scale the space the operations that follow draw in, from the origin.
    pub fn scale(x: f32, y: f32) -> Draw {
        Draw::Scale { x, y }
    }

    /// Rotate the space the operations that follow draw in, around the
    /// origin, clockwise by this many degrees.
    pub fn rotate(degrees: f32) -> Draw {
        Draw::Rotate { degrees }
    }

    /// This rectangle or circle filled.
    pub fn filled(mut self, paint: impl Into<Paint>) -> Draw {
        match &mut self {
            Draw::Rect { fill, .. } | Draw::Circle { fill, .. } => {
                *fill = Some(paint.into());
            }
            _ => {}
        }
        self
    }

    /// This rectangle or circle stroked, one pixel wide.
    pub fn stroked(mut self, paint: impl Into<Paint>) -> Draw {
        match &mut self {
            Draw::Rect { stroke, .. } | Draw::Circle { stroke, .. } => {
                *stroke = Some(CanvasStroke {
                    color: paint.into(),
                    width: 1.,
                    cap: None,
                    join: None,
                });
            }
            _ => {}
        }
        self
    }

    /// This text or image's top-left corner.
    pub fn at(mut self, x: f32, y: f32) -> Draw {
        match &mut self {
            Draw::Text {
                x: own, y: own_y, ..
            }
            | Draw::Image {
                x: own, y: own_y, ..
            } => {
                *own = x;
                *own_y = y;
            }
            _ => {}
        }
        self
    }

    /// This image's width and height.
    pub fn size(mut self, width: f32, height: f32) -> Draw {
        if let Draw::Image {
            width: own,
            height: own_height,
            ..
        } = &mut self
        {
            *own = width;
            *own_height = height;
        }
        self
    }

    /// This text's token style.
    pub fn style(mut self, style: TextStyle) -> Draw {
        if let Draw::Text { style: own, .. } = &mut self {
            *own = Some(style);
        }
        self
    }

    /// This text's token level.
    pub fn level(mut self, level: TextLevel) -> Draw {
        if let Draw::Text { level: own, .. } = &mut self {
            *own = Some(level);
        }
        self
    }

    /// This text's colour.
    pub fn color(mut self, paint: impl Into<Paint>) -> Draw {
        if let Draw::Text { color: own, .. } = &mut self {
            *own = Some(paint.into());
        }
        self
    }

    /// This text's size in pixels.
    pub fn text_size(mut self, size: f32) -> Draw {
        if let Draw::Text { size: own, .. } = &mut self {
            *own = Some(size);
        }
        self
    }

    /// This text's weight, 100 to 900.
    pub fn weight(mut self, weight: f32) -> Draw {
        if let Draw::Text { weight: own, .. } = &mut self {
            *own = Some(weight);
        }
        self
    }

    /// This stroke's width.
    pub fn width(mut self, width: f32) -> Draw {
        match &mut self {
            Draw::Stroke { stroke } => stroke.width = width,
            Draw::Rect {
                stroke: Some(stroke),
                ..
            }
            | Draw::Circle {
                stroke: Some(stroke),
                ..
            } => stroke.width = width,
            _ => {}
        }
        self
    }

    /// This stroke's end caps.
    pub fn cap(mut self, cap: Cap) -> Draw {
        match &mut self {
            Draw::Stroke { stroke } => stroke.cap = Some(cap),
            Draw::Rect {
                stroke: Some(stroke),
                ..
            }
            | Draw::Circle {
                stroke: Some(stroke),
                ..
            } => stroke.cap = Some(cap),
            _ => {}
        }
        self
    }

    /// This stroke's corner joins.
    pub fn join(mut self, join: Join) -> Draw {
        match &mut self {
            Draw::Stroke { stroke } => stroke.join = Some(join),
            Draw::Rect {
                stroke: Some(stroke),
                ..
            }
            | Draw::Circle {
                stroke: Some(stroke),
                ..
            } => stroke.join = Some(join),
            _ => {}
        }
        self
    }
}

/// Measures `text` in `style` (#242, `pane:extension/view.measure-text`):
/// the width and height it occupies when the canvas draws it, laid out
/// exactly, at the size and weight the tree's text styles resolve to. A
/// canvas's drawing calls this while the view renders, so the text it
/// draws fits what it says.
pub fn measure_text(text: &str, style: TextMeasure) -> (f32, f32) {
    let style = style.json();
    let held = wit_measure_text(text, &style);
    (held.width, held.height)
}

/// How text is measured ([`measure_text`]): a token style, a size in
/// pixels, a weight from 100 to 900.
#[derive(Debug, Default)]
pub struct TextMeasure {
    pub style: Option<TextStyle>,
    pub size: Option<f32>,
    pub weight: Option<f32>,
}

impl TextMeasure {
    /// The style as `measure-text` reads it: the same JSON a canvas text
    /// operation names.
    fn json(&self) -> String {
        let mut style = String::new();
        if let Some(name) = self.style.map(text_style_name) {
            let _ = write!(style, "\"style\":\"{name}\",");
        }
        if let Some(size) = self.size {
            let _ = write!(style, "\"size\":{size},");
        }
        if let Some(weight) = self.weight {
            let _ = write!(style, "\"weight\":{weight},");
        }
        style.pop();
        format!("{{{style}}}")
    }
}

/// The input a canvas receives, as its listener is told it
/// ([`Cx::canvas_listener`]): the payload of the canvas's event, read
/// into what it names. A canvas's pointer events carry the point in the
/// canvas's own space and the modifiers held.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CanvasEvent {
    /// The primary button pressed over the canvas.
    PointerDown {
        x: f32,
        y: f32,
        /// How many clicks this press is: two is a double one.
        clicks: f32,
        ctrl: bool,
        alt: bool,
        shift: bool,
    },
    /// That button released.
    PointerUp { x: f32, y: f32 },
    /// The pointer moved while that button is held.
    PointerMove { x: f32, y: f32 },
    /// The pointer entering the canvas's hover, carrying no point.
    PointerEnter,
    /// The pointer leaving it.
    PointerLeave,
    /// The wheel turned over the canvas, its delta in pixels or lines.
    Wheel {
        x: f32,
        y: f32,
        dx: f32,
        dy: f32,
        /// Whether the delta is in pixels (else lines).
        pixel: bool,
    },
    /// The primary button pressed twice over the canvas.
    DoubleClick { x: f32, y: f32 },
    /// The secondary button pressed over the canvas.
    Secondary { x: f32, y: f32 },
    /// The canvas's size changing.
    Resize { width: f32, height: f32 },
}

impl CanvasEvent {
    /// The event `payload` names, when it reads: an event of another kind,
    /// or a payload that does not name one, answers `None`.
    pub fn of(payload: &str) -> Option<CanvasEvent> {
        let kind = field_text(payload, "event")?;
        let at = || (field_number(payload, "x"), field_number(payload, "y"));
        match kind.as_str() {
            "pointer-down" => {
                let (x, y) = at();
                Some(CanvasEvent::PointerDown {
                    x: x?,
                    y: y?,
                    clicks: field_number(payload, "clicks").unwrap_or(1.),
                    ctrl: field_bool(payload, "ctrl"),
                    alt: field_bool(payload, "alt"),
                    shift: field_bool(payload, "shift"),
                })
            }
            "pointer-up" => {
                let (x, y) = at();
                Some(CanvasEvent::PointerUp { x: x?, y: y? })
            }
            "pointer-move" => {
                let (x, y) = at();
                Some(CanvasEvent::PointerMove { x: x?, y: y? })
            }
            "pointer-enter" => Some(CanvasEvent::PointerEnter),
            "pointer-leave" => Some(CanvasEvent::PointerLeave),
            "wheel" => Some(CanvasEvent::Wheel {
                x: field_number(payload, "x")?,
                y: field_number(payload, "y")?,
                dx: field_number(payload, "dx")?,
                dy: field_number(payload, "dy")?,
                pixel: field_text(payload, "unit").as_deref() == Some("pixel"),
            }),
            "double-click" => {
                let (x, y) = at();
                Some(CanvasEvent::DoubleClick { x: x?, y: y? })
            }
            "secondary" => {
                let (x, y) = at();
                Some(CanvasEvent::Secondary { x: x?, y: y? })
            }
            "resize" => Some(CanvasEvent::Resize {
                width: field_number(payload, "width")?,
                height: field_number(payload, "height")?,
            }),
            _ => None,
        }
    }
}

/// The string a payload's `"name"` field holds, when it names one.
fn field_text(payload: &str, name: &str) -> Option<String> {
    let at = payload.find(&format!("\"{name}\""))?;
    let rest = payload[at + name.len() + 2..].trim_start();
    let rest = rest.strip_prefix(':')?.trim_start();
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(unescape(&rest[..end]))
}

/// The number a payload's `"name"` field holds, when it names one.
fn field_number(payload: &str, name: &str) -> Option<f32> {
    let at = payload.find(&format!("\"{name}\""))?;
    let rest = payload[at + name.len() + 2..].trim_start();
    let rest = rest.strip_prefix(':')?.trim_start();
    let end = rest
        .find([',', '}'])
        .unwrap_or_else(|| rest.rfind('"').unwrap_or(rest.len()));
    let text = rest[..end].trim();
    let text = text.strip_suffix('}').unwrap_or(text);
    text.parse().ok()
}

/// Whether a payload's `"name"` field holds `true`.
fn field_bool(payload: &str, name: &str) -> bool {
    field_number(payload, name).is_some_and(|held| held > 0.5)
        || field_text(payload, name).is_some_and(|held| held == "true")
}

/// A payload string's content, unescaped.
fn unescape(text: &str) -> String {
    let mut result = String::new();
    let mut characters = text.chars();
    while let Some(character) = characters.next() {
        match character {
            '\\' => match characters.next() {
                Some('"') => result.push('"'),
                Some('\\') => result.push('\\'),
                Some('/') => result.push('/'),
                Some('n') => result.push('\n'),
                Some('r') => result.push('\r'),
                Some('t') => result.push('\t'),
                Some('u') => {
                    let digits: String = characters.by_ref().take(4).collect();
                    if let Ok(code) = u32::from_str_radix(&digits, 16)
                        && let Some(character) = char::from_u32(code)
                    {
                        result.push(character);
                    }
                }
                _ => return result,
            },
            other => result.push(other),
        }
    }
    result
}

/// A space token: the named distances of the UI component set, resolved
/// onto Pane's spacing rhythm.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Space {
    Xs,
    S,
    M,
    L,
    Xl,
    Xxl,
}

/// How a container's children are laid out along its cross axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Start,
    Center,
    End,
    Stretch,
    Baseline,
}

/// How a container's children share its main axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Justify {
    Start,
    Center,
    End,
    SpaceBetween,
    SpaceAround,
}

/// The style of a text: its size, weight and family.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextStyle {
    Heading,
    Title,
    Body,
    Caption,
    Mono,
    SmallMono,
}

/// The level of a text: its colour, through the alpha of the text colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextLevel {
    Primary,
    Secondary,
    Tertiary,
    Quaternary,
}

/// The tone of a button. The tone a *colour* is drawn in is
/// [`crate::icon::Tone`], the token of the UI component set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Default,
    Secondary,
    Ghost,
    Accent,
    Destructive,
}

/// Anything that becomes one [`Node`] of the tree: the builders
/// ([`column`] and its kind) and a `Node` itself.
pub trait IntoNode {
    fn into_node(self) -> Node;

    /// Pane draws this tree again after `after`: the view's own ask, the
    /// render it answers named by [`Cx::refreshed`] (#236). The answer of
    /// a render is one: the ask belongs to its root, and a tree that is a
    /// child of another cannot ask.
    fn refresh_after(self, after: Duration) -> Answer
    where
        Self: Sized,
    {
        Answer {
            node: self.into_node(),
            refresh: Some(Refresh::After(after)),
            error: None,
        }
    }
}

/// What a view's `render` answers: the tree it drew, and when Pane asks
/// for it again. Every tree an [`IntoNode`] builds is one with no ask;
/// [`IntoNode::refresh_after`] — the view's own ask, an interval — is the
/// one that asks. Data on its way asks for nothing ([`loading`]): its
/// arrival asks for a drawing itself ([`Pending`], #243).
pub struct Answer {
    node: Node,
    refresh: Option<Refresh>,
    /// An error this render answers instead of its tree, shown to the
    /// user with the view keeping its last good one.
    error: Option<String>,
}

/// When Pane asks for a view's tree again.
enum Refresh {
    /// After `after`: the view's own ask, whose render [`Cx::refreshed`]
    /// names.
    After(Duration),
}

impl Answer {
    /// Pane draws this answer's tree again after `after`, whatever it
    /// asked for before.
    pub fn refresh_after(mut self, after: Duration) -> Answer {
        self.refresh = Some(Refresh::After(after));
        self
    }

    /// This render answers `message` as its error: shown to the user, the
    /// view keeping its last good tree, as an event the extension
    /// refused. A `Result` answer carries one too
    /// ([`IntoAnswer` for `Result`]).
    pub fn error(message: impl Into<String>) -> Answer {
        Answer {
            node: Node::of(NodeKind::Column(Layout::default())),
            refresh: None,
            error: Some(message.into()),
        }
    }
}

/// What a view's `render` answers: every tree (anything [`IntoNode`])
/// is one, as is [`Answer`].
pub trait IntoAnswer {
    /// The tree drawn, and when Pane asks for it again.
    fn into_answer(self) -> Answer;
}

impl<N: IntoNode> IntoAnswer for N {
    fn into_answer(self) -> Answer {
        Answer {
            node: self.into_node(),
            refresh: None,
            error: None,
        }
    }
}

/// A view's `render` answering an error: the message is shown to the user,
/// the view keeping its last good tree.
impl<N: IntoNode> IntoAnswer for Result<N, String> {
    fn into_answer(self) -> Answer {
        match self {
            Ok(node) => Answer {
                node: node.into_node(),
                refresh: None,
                error: None,
            },
            Err(message) => Answer::error(message),
        }
    }
}

impl IntoAnswer for Answer {
    fn into_answer(self) -> Answer {
        self
    }
}

/// Data a view is still waiting for, shown as a loading state until it
/// arrives (#236): [`Pending::loading`] holds the work, and each render
/// reads it through [`Pending::ready`] — `None` while the work has not
/// answered, so the view renders its loading state (a plain tree, the
/// answer's own), and its
/// answer from then on. The work keeps running between Pane's calls
/// (#243): the first `ready` that finds it pending starts it as a task of
/// the render's own, which Pane drives while a view of the command is
/// open, so it is awaited in the background; when it answers, the SDK
/// asks Pane to draw the view again, and that drawing shows the answer —
/// the moment it arrived, with no timer to wait for. The work is a real
/// future: it must either await something the host answers (a file, a
/// service, the clock) or finish on its own, for nothing schedules a
/// future that wakes no one.
///
/// The work runs once; a view that wants it again builds another.
pub struct Pending<T> {
    /// The work, until the first render starts it.
    work: Option<Pin<Box<dyn Future<Output = T>>>>,
    /// Where the work's answer is kept, shared with the task that waits
    /// for it: set once, read from then on.
    answer: Rc<OnceCell<T>>,
}

impl<T: 'static> Pending<T> {
    /// Data answered by `work`, which the view waits for: a future the
    /// view's `open_designed_view` can start, of a file read, a service
    /// called, or work of its own.
    pub fn loading(work: impl Future<Output = T> + 'static) -> Pending<T> {
        Pending {
            work: Some(Box::pin(work)),
            answer: Rc::new(OnceCell::new()),
        }
    }

    /// The work's answer, once it has answered: `None` while it is
    /// pending — the loading state, whose arrival asks for this render
    /// again. The first call starts the work, running it beyond the
    /// render that reads it (see [`Pending`]); the answer is written
    /// from the task that awaits the work, before it asks to be drawn
    /// again, so a `Some` is always the whole answer.
    pub fn ready(&mut self) -> Option<&T> {
        if let Some(work) = self.work.take() {
            let answer = self.answer.clone();
            // The view whose render started the work: the one that asks
            // to be drawn again when it answers, however many views the
            // command's component holds.
            let view = ASKING_VIEW.load(Ordering::Relaxed);
            spawn_local(async move {
                let arrived = work.await;
                answer.set(arrived).ok();
                ask_to_render(view);
            });
        }
        self.answer.get()
    }
}

impl IntoNode for Node {
    fn into_node(self) -> Node {
        self
    }
}

/// The methods every node's builder carries, so a tree reads alike
/// whichever node it builds: its children, its [`Style`], and its place in
/// a `stack`.
macro_rules! styled {
    ($builder:ident) => {
        impl $builder {
            /// A child of this node's, after its children.
            pub fn child(self, child: impl IntoNode) -> Self {
                let node = self.0;
                $builder(node.child(child))
            }

            /// The children of this node's, after its children.
            pub fn children(self, children: impl IntoIterator<Item = impl IntoNode>) -> Self {
                let node = self.0;
                $builder(node.children(children))
            }

            /// How much this node grows to fill the space its parent
            /// shares out.
            pub fn grow(mut self, grow: f32) -> Self {
                self.0.style.sizing.grow = Some(grow);
                self
            }

            /// How much this node shrinks when its parent runs out of
            /// space.
            pub fn shrink(mut self, shrink: f32) -> Self {
                self.0.style.sizing.shrink = Some(shrink);
                self
            }

            /// This node's flex basis.
            pub fn basis(mut self, basis: Length) -> Self {
                self.0.style.sizing.basis = Some(basis);
                self
            }

            /// This node's width.
            pub fn width(mut self, width: Length) -> Self {
                self.0.style.sizing.width = Some(width);
                self
            }

            /// This node's height.
            pub fn height(mut self, height: Length) -> Self {
                self.0.style.sizing.height = Some(height);
                self
            }

            /// This node's least width.
            pub fn min_width(mut self, width: Length) -> Self {
                self.0.style.sizing.min_width = Some(width);
                self
            }

            /// This node's most width.
            pub fn max_width(mut self, width: Length) -> Self {
                self.0.style.sizing.max_width = Some(width);
                self
            }

            /// This node's least height.
            pub fn min_height(mut self, height: Length) -> Self {
                self.0.style.sizing.min_height = Some(height);
                self
            }

            /// This node's most height.
            pub fn max_height(mut self, height: Length) -> Self {
                self.0.style.sizing.max_height = Some(height);
                self
            }

            /// This node's width over its height.
            pub fn aspect_ratio(mut self, ratio: f32) -> Self {
                self.0.style.sizing.aspect_ratio = Some(ratio);
                self
            }

            /// This node's background colour.
            pub fn background(mut self, background: impl Into<Paint>) -> Self {
                self.0.style.surface.background = Some(background.into());
                self
            }

            /// This node's border: how wide, and in which colour.
            pub fn border(mut self, border: Border) -> Self {
                self.0.style.surface.border = Some(border);
                self
            }

            /// This node's corner radius.
            pub fn radius(mut self, radius: Radius) -> Self {
                self.0.style.surface.radius = Some(radius);
                self
            }

            /// This node's opacity, 0 to 1.
            pub fn opacity(mut self, opacity: f32) -> Self {
                self.0.style.surface.opacity = Some(opacity);
                self
            }

            /// The surface this node draws while the pointer is over it,
            /// restating any of its properties.
            pub fn hover(mut self, surface: Surface) -> Self {
                self.0.style.hover = Some(surface);
                self
            }

            /// The surface this node draws while it is pressed, restating
            /// any of its properties.
            pub fn pressed(mut self, surface: Surface) -> Self {
                self.0.style.pressed = Some(surface);
                self
            }

            /// The node's stable identity among its siblings, which Pane
            /// keeps node state under.
            pub fn key(mut self, key: impl Into<String>) -> Self {
                self.0.key = Some(key.into());
                self
            }

            /// The name assistive technology reads the node by.
            pub fn name(mut self, name: impl Into<String>) -> Self {
                self.0.name = Some(name.into());
                self
            }

            /// The node drawn instead when Pane does not know this one.
            pub fn fallback(mut self, fallback: impl IntoNode) -> Self {
                self.0.fallback = Some(Box::new(fallback.into_node()));
                self
            }

            /// Where in a `stack` this node is placed, when the stack's
            /// own placement is not where it sits.
            pub fn place(mut self, place: Place) -> Self {
                self.0.place = Some(place);
                self
            }

            /// How far this node sits from its place in a `stack`, in its
            /// placement's directions.
            pub fn offset(mut self, x: Length, y: Length) -> Self {
                self.0.offset = Some((x, y));
                self
            }

            /// Asks for the keyboard: a node whose ask is new — the tree
            /// the user saw did not name it — is focused (see
            /// [`Node::focus`]).
            pub fn focus(mut self) -> Self {
                self.0.focus = true;
                self
            }

            /// The listener a focus of this node runs, it taking the
            /// keyboard.
            pub fn on_focus(mut self, listener: Listener) -> Self {
                self.0.on_focus = Some(listener);
                self
            }

            /// The listener a blur of this node runs, it losing the
            /// keyboard.
            pub fn on_blur(mut self, listener: Listener) -> Self {
                self.0.on_blur = Some(listener);
                self
            }

            /// The listener a key pressed while this node is focused runs,
            /// told the key as a key sequence spells it (Tab, Enter and
            /// Escape stay with Pane).
            pub fn on_key(mut self, listener: ValueListener) -> Self {
                self.0.on_key = Some(listener);
                self
            }
        }
    };
}

/// A builder of nodes: the style methods of `styled!`, and the
/// [`IntoNode`] every builder answers.
macro_rules! builder {
    ($builder:ident) => {
        styled!($builder);
        impl IntoNode for $builder {
            fn into_node(self) -> Node {
                self.0
            }
        }
    };
}

pub(crate) use builder;
// Form's builders live in their own module: the macro this one expands to
// must be in scope there too.
pub(crate) use styled;

/// A column: children below each other.
pub fn column() -> Container {
    Container(Node::of(NodeKind::Column(Layout::default())))
}

/// A row: children beside each other.
pub fn row() -> Container {
    Container(Node::of(NodeKind::Row(Layout::default())))
}

/// A card of children, on Pane's own card surface.
pub fn card() -> Container {
    Container(Node::of(NodeKind::Card(Layout::default())))
}

/// A stack: children drawn over each other, each placed by its `place`
/// method or the stack's `align`.
pub fn stack() -> Stack {
    Stack(Node::of(NodeKind::Stack(Place::TopStart)))
}

/// A scrolling region, vertical.
pub fn scroll() -> Scroll {
    Scroll(Node::of(NodeKind::Scroll(Orientation::Vertical)))
}

/// Space: it grows to fill what it is given.
pub fn spacer() -> Spacer {
    Spacer(Node::of(NodeKind::Spacer))
}

/// A hairline rule, horizontal.
pub fn divider() -> Divider {
    Divider(Node::of(NodeKind::Divider(Orientation::Horizontal)))
}

/// A loading indicator, indeterminate.
pub fn loading() -> Loading {
    Loading(Node::of(NodeKind::Loading { label: None }))
}

/// A canvas: a leaf this view draws into with drawing operations
/// ([`Draw`]), taking input ([`CanvasBuilder::on_pointer_down`] and its
/// kind) and named to assistive technology. Its size comes from its style
/// as any node's does — a `.width()` and `.height()`, or the space the
/// layout gives it, which the render context names and a change of which
/// is the resize event.
pub fn canvas() -> CanvasBuilder {
    CanvasBuilder(Node::of(NodeKind::Canvas(CanvasNode {
        ops: Vec::new(),
        a11y: CanvasA11y::default(),
        handlers: CanvasHandlers::default(),
    })))
}

/// One line of text.
pub fn text(text: impl Into<String>) -> TextBuilder {
    TextBuilder(Node::of(NodeKind::Text(Text {
        content: TextContent::Plain(text.into()),
        style: None,
        level: None,
        color: None,
        size: None,
        weight: None,
        truncate: false,
    })))
}

/// A text of spans, each styled and a link when it carries a listener.
pub fn spans(spans: impl IntoIterator<Item = Span>) -> TextBuilder {
    TextBuilder(Node::of(NodeKind::Text(Text {
        content: TextContent::Spans(spans.into_iter().collect()),
        style: None,
        level: None,
        color: None,
        size: None,
        weight: None,
        truncate: false,
    })))
}

/// One span of a text.
pub fn span(text: impl Into<String>) -> Span {
    Span {
        text: text.into(),
        style: None,
        level: None,
        color: None,
        code: false,
        on_click: None,
    }
}

/// A button with `label`, which a press runs the listener it is given.
pub fn button(label: impl Into<String>) -> Button {
    Button(Node::of(NodeKind::Button {
        label: label.into(),
        tone: None,
        icon: None,
        keys: None,
        on_click: None,
        enabled: true,
    }))
}

/// A link with `label`, which a press runs the listener it is given.
pub fn link(label: impl Into<String>) -> Link {
    Link(Node::of(NodeKind::Link {
        label: label.into(),
        on_click: None,
        color: None,
    }))
}

/// One icon, by name, file, URL or system reference.
pub fn icon(icon: Icon) -> IconNode {
    IconNode(Node::of(NodeKind::Icon(IconNodePayload {
        icon,
        size: None,
    })))
}

/// One icon on Pane's tile.
pub fn icon_tile(icon: Icon) -> IconNode {
    IconNode(Node::of(NodeKind::IconTile(IconNodePayload {
        icon,
        size: None,
    })))
}

/// One image, with the node's children standing in for it while it loads.
pub fn image(image: Icon) -> Image {
    Image(Node::of(NodeKind::Image {
        image,
        size: None,
        fit: Fit::Contain,
    }))
}

/// A rich row: a title, with an optional subtitle, icon and accessories.
pub fn rich_row(title: impl Into<String>) -> RichRowBuilder {
    RichRowBuilder(Node::of(NodeKind::RichRow(RichRow {
        title: title.into(),
        subtitle: None,
        icon: None,
        accessories: Vec::new(),
        on_click: None,
    })))
}

/// One keycap: the key its cap shows.
pub fn keycap(key: impl Into<String>) -> Keycap {
    Keycap(Node::of(NodeKind::Keycap(key.into())))
}

/// A key sequence: the keys its caps show, in order.
pub fn key_sequence(keys: impl IntoIterator<Item = impl Into<String>>) -> KeySequence {
    KeySequence(Node::of(NodeKind::KeySequence(
        keys.into_iter().map(Into::into).collect(),
    )))
}

/// One tag: a short label in a chip.
pub fn tag(text: impl Into<String>) -> Tag {
    Tag(Node::of(NodeKind::Tag(TagPayload {
        text: text.into(),
        color: None,
    })))
}

/// One badge: a short count in a filled chip.
pub fn badge(text: impl Into<String>) -> Tag {
    Tag(Node::of(NodeKind::Badge(TagPayload {
        text: text.into(),
        color: None,
    })))
}

/// One toggle: on or off.
pub fn toggle(on: bool) -> Toggle {
    Toggle(Node::of(NodeKind::Toggle(TogglePayload {
        on,
        on_click: None,
        label: None,
        field: FieldPayload::default(),
    })))
}

/// One checkbox: checked or not.
pub fn checkbox(checked: bool) -> Toggle {
    Toggle(Node::of(NodeKind::Checkbox(TogglePayload {
        on: checked,
        on_click: None,
        label: None,
        field: FieldPayload::default(),
    })))
}

/// A segmented control over `choices`, the first chosen until one is.
pub fn segmented(choices: impl IntoIterator<Item = Choice>) -> Segmented {
    Segmented(Node::of(NodeKind::Segmented(SegmentedPayload::of(
        choices.into_iter().collect(),
    ))))
}

/// A select over `choices`, the first drawn until one is chosen.
pub fn select(choices: impl IntoIterator<Item = Choice>) -> Segmented {
    Segmented(Node::of(NodeKind::Select(SegmentedPayload::of(
        choices.into_iter().collect(),
    ))))
}

/// One option of a segmented control, select or tag picker, named `value`
/// and drawn as `label` when one is given.
pub fn choice(value: impl Into<String>) -> Choice {
    Choice {
        value: value.into(),
        label: None,
        section: None,
    }
}

/// One slider, `value` between 0 and 1 until bounds are given.
pub fn slider(value: f32) -> Slider {
    Slider(Node::of(NodeKind::Slider {
        value,
        min: 0.,
        max: 1.,
        step: 0.1,
        on_click: None,
        label: None,
    }))
}

/// One progress bar, `value` how far along it is, 0 to 1.
pub fn progress(value: f32) -> Progress {
    Progress(Node::of(NodeKind::Progress { value, label: None }))
}

/// One markdown node, `markdown` its source.
pub fn markdown(markdown: impl Into<String>) -> Markdown {
    Markdown(Node::of(NodeKind::Markdown(markdown.into())))
}

/// A section header: a title over a group.
pub fn section_header(title: impl Into<String>) -> SectionHeader {
    SectionHeader(Node::of(NodeKind::SectionHeader(SectionHeaderPayload {
        title: title.into(),
        note: None,
    })))
}

/// A metadata list: rows of a label and its value, link or tags.
pub fn metadata_list(items: impl IntoIterator<Item = MetadataItem>) -> MetadataList {
    MetadataList(Node::of(NodeKind::MetadataList(MetadataListPayload {
        items: items.into_iter().collect(),
    })))
}

/// One row of a metadata list: a label and its value.
pub fn metadata(label: impl Into<String>, value: impl Into<String>) -> MetadataItem {
    MetadataItem {
        label: Some(label.into()),
        value: Some(value.into()),
        on_click: None,
        tags: Vec::new(),
        separator: false,
    }
}

/// One row of a metadata list: a label with tag chips.
pub fn metadata_tags(
    label: impl Into<String>,
    tags: impl IntoIterator<Item = impl Into<String>>,
) -> MetadataItem {
    MetadataItem {
        label: Some(label.into()),
        value: None,
        on_click: None,
        tags: tags.into_iter().map(Into::into).collect(),
        separator: false,
    }
}

/// A separator row of a metadata list.
pub fn metadata_separator() -> MetadataItem {
    MetadataItem {
        label: None,
        value: None,
        on_click: None,
        tags: Vec::new(),
        separator: true,
    }
}

/// An empty state: an icon, a title and a description, with the node's
/// children as its actions.
pub fn empty_state(title: impl Into<String>) -> EmptyState {
    EmptyState(Node::of(NodeKind::EmptyState(EmptyStatePayload {
        title: title.into(),
        description: None,
        icon: None,
    })))
}

/// One text input, starting at `value`. The field edits at once, its
/// state kept by its key: `on_input` hears its value as the user types
/// it (coalesced by Pane, throttled by [`TextInput::throttle`]),
/// `on_change` on its commits, and a value that differs from the field's
/// value in the previous render replaces its text.
pub fn text_input(value: impl Into<String>) -> TextInput {
    TextInput(Node::of(NodeKind::TextInput(TextInputPayload {
        value: value.into(),
        ..TextInputPayload::default()
    })))
}

/// One password field, starting empty (see [`text_input`]).
pub fn password_input() -> TextInput {
    TextInput(Node::of(NodeKind::PasswordInput(
        TextInputPayload::default(),
    )))
}

/// One text area, starting at `value` (see [`text_input`]); its Enter
/// inserts a newline, its commits are blurs'.
pub fn text_area(value: impl Into<String>) -> TextInput {
    TextInput(Node::of(NodeKind::TextArea(TextInputPayload {
        value: value.into(),
        ..TextInputPayload::default()
    })))
}

/// A standard List (#240): its items in sections — [`item`] and
/// [`section`] children, a [`dropdown`] child its search-bar dropdown,
/// an [`empty_state`] child its empty view — whose search field and
/// selection Pane owns. Pane filters the items by the text typed in the
/// field with the root-search matcher, unless `on_search_text` handles
/// the search itself (then the events are throttled);
/// `on_selection_change` hears the selection move, told the selected
/// item's key, and `on_load_more` is raised as the selection nears the
/// end while `has_more` says more is there.
pub fn list() -> List {
    List(Node::of(NodeKind::List(ListNode::default())))
}

/// A standard Grid (#240): a List's behaviour with cells — [`cell`]
/// children in [`section`]s, each section with its own columns (1–8,
/// 5 by default), aspect ratio, fit and inset. The arrows move the
/// selection by cell and row, Ctrl+Up and Ctrl+Down by section.
pub fn grid() -> Grid {
    Grid(Node::of(NodeKind::Grid(ListNode::default())))
}

/// One section of a List's or Grid's items, titled `title`, with an
/// optional `subtitle` beside it.
pub fn section(title: impl Into<String>) -> Section {
    Section(Node::of(NodeKind::ListSection(SectionNode {
        title: Some(title.into()),
        ..SectionNode::default()
    })))
}

/// One item of a List, keyed `key` (#240): the item's identity — what
/// the selection names and the detail pane is built for — with the List
/// document's vocabulary on its builder (its [`Item::title`] the first
/// line of the row, a subtitle, keywords, an icon, accessories and the
/// actions that activate it), and the detail pane's content when it is
/// selected. Its children are its own row subtree, drawn in the place of
/// the standard row while Pane still selects and activates it.
pub fn item(key: impl Into<String>) -> Item {
    let mut node = Node::of(NodeKind::ListItem(ListItem::default()));
    node.key = Some(key.into());
    Item(node)
}

/// One cell of a Grid (#240): an image, a colour or a subtree, with a
/// title and a subtitle under it.
pub fn cell() -> GridCell {
    GridCell(Node::of(NodeKind::GridItem(GridItem::default())))
}

/// The search-bar dropdown of a List (#240): its items and the one
/// chosen, changed by the user's choice, which the list's `on_change`
/// hears. A child of the list.
pub fn dropdown() -> Dropdown {
    Dropdown(Node::of(NodeKind::ListDropdown(DropdownNode::default())))
}

/// A Detail (#240): a scrolled column of what a record or an article is
/// — Markdown, a metadata panel, a loading state, actions, as children.
pub fn detail() -> Detail {
    Detail(Node::of(NodeKind::Detail(Layout::default())))
}

/// A List (see [`list`]).
#[derive(Debug)]
pub struct List(Node);

/// A Grid (see [`grid`]).
#[derive(Debug)]
pub struct Grid(Node);

/// A section of items (see [`section`]).
#[derive(Debug)]
pub struct Section(Node);

/// An item of a List (see [`item`]).
#[derive(Debug)]
pub struct Item(Node);

/// A cell of a Grid (see [`cell`]).
#[derive(Debug)]
pub struct GridCell(Node);

/// A search-bar dropdown (see [`dropdown`]).
#[derive(Debug)]
pub struct Dropdown(Node);

/// A Detail (see [`detail`]).
#[derive(Debug)]
pub struct Detail(Node);

builder!(List);
builder!(Grid);
builder!(Section);
builder!(Item);
builder!(GridCell);
builder!(Dropdown);
builder!(Detail);

impl List {
    /// What names the view, when this node is the tree's root: shown where
    /// a screen's title is (the header's title row, above the search
    /// field, #240).
    pub fn navigation_title(mut self, title: impl Into<String>) -> List {
        self.0.navigation_title = Some(title.into());
        self
    }

    /// The search field's placeholder.
    pub fn search_placeholder(mut self, placeholder: impl Into<String>) -> List {
        self.0.list_mut().search_placeholder = Some(placeholder.into());
        self
    }

    /// The search text the view sets: the field's value, which wins
    /// while the user has not typed a newer one.
    pub fn search_text(mut self, text: impl Into<String>) -> List {
        self.0.list_mut().search_text = Some(text.into());
        self
    }

    /// The item the view selects, by its key.
    pub fn selected_key(mut self, key: impl Into<String>) -> List {
        self.0.list_mut().selected_key = Some(key.into());
        self
    }

    /// The loading state: the loading bar, drawn once loading has run
    /// past its threshold (300 ms).
    pub fn is_loading(mut self, loading: bool) -> List {
        self.0.list_mut().is_loading = loading;
        self
    }

    /// Whether the detail pane shows beside the items.
    pub fn is_showing_detail(mut self, showing: bool) -> List {
        self.0.list_mut().is_showing_detail = showing;
        self
    }

    /// Whether more items follow the ones shown, whose loading the list
    /// asks for as the selection nears the end.
    pub fn has_more(mut self, has_more: bool) -> List {
        self.0.list_mut().has_more = has_more;
        self
    }

    /// How many items a page holds.
    pub fn page_size(mut self, size: u64) -> List {
        self.0.list_mut().page_size = Some(size);
        self
    }

    /// The listener the search text runs on its every change: the list is
    /// then not filtered by Pane, and the text is told to the view
    /// (throttled).
    pub fn on_search_text(mut self, listener: ValueListener) -> List {
        self.0.list_mut().on_search_text = Some(listener);
        self
    }

    /// The listener the selection's change runs, told the selected item's
    /// key.
    pub fn on_selection_change(mut self, listener: ValueListener) -> List {
        self.0.list_mut().on_selection_change = Some(listener);
        self
    }

    /// The listener the load of the next page runs, as the selection
    /// nears the end of what is shown.
    pub fn on_load_more(mut self, listener: Listener) -> List {
        self.0.list_mut().on_load_more = Some(listener);
        self
    }
}

impl Grid {
    /// What names the view, when this node is the tree's root (see
    /// [`List::navigation_title`]).
    pub fn navigation_title(mut self, title: impl Into<String>) -> Grid {
        self.0.navigation_title = Some(title.into());
        self
    }

    /// The search field's placeholder (see [`List::search_placeholder`]).
    pub fn search_placeholder(mut self, placeholder: impl Into<String>) -> Grid {
        self.0.list_mut().search_placeholder = Some(placeholder.into());
        self
    }

    /// The search text the view sets (see [`List::search_text`]).
    pub fn search_text(mut self, text: impl Into<String>) -> Grid {
        self.0.list_mut().search_text = Some(text.into());
        self
    }

    /// The cell the view selects, by its key.
    pub fn selected_key(mut self, key: impl Into<String>) -> Grid {
        self.0.list_mut().selected_key = Some(key.into());
        self
    }

    /// The loading state (see [`List::is_loading`]).
    pub fn is_loading(mut self, loading: bool) -> Grid {
        self.0.list_mut().is_loading = loading;
        self
    }

    /// Whether more cells follow the ones shown (see
    /// [`List::has_more`]).
    pub fn has_more(mut self, has_more: bool) -> Grid {
        self.0.list_mut().has_more = has_more;
        self
    }

    /// How many cells a page holds (see [`List::page_size`]).
    pub fn page_size(mut self, size: u64) -> Grid {
        self.0.list_mut().page_size = Some(size);
        self
    }

    /// The listener the search text runs on its every change (see
    /// [`List::on_search_text`]).
    pub fn on_search_text(mut self, listener: ValueListener) -> Grid {
        self.0.list_mut().on_search_text = Some(listener);
        self
    }

    /// The listener the selection's change runs (see
    /// [`List::on_selection_change`]).
    pub fn on_selection_change(mut self, listener: ValueListener) -> Grid {
        self.0.list_mut().on_selection_change = Some(listener);
        self
    }

    /// The listener the load of the next page runs (see
    /// [`List::on_load_more`]).
    pub fn on_load_more(mut self, listener: Listener) -> Grid {
        self.0.list_mut().on_load_more = Some(listener);
        self
    }
}

impl Node {
    /// The list node of a List or Grid, changed.
    fn list_mut(&mut self) -> &mut ListNode {
        match &mut self.kind {
            NodeKind::List(list) | NodeKind::Grid(list) => list,
            _ => unreachable!("a list or grid node"),
        }
    }
}

impl Section {
    /// The section's second line, beside its title.
    pub fn subtitle(mut self, subtitle: impl Into<String>) -> Section {
        match &mut self.0.kind {
            NodeKind::ListSection(section) => section.subtitle = Some(subtitle.into()),
            _ => unreachable!("a section node"),
        }
        self
    }

    /// How many columns the section's cells sit in (a Grid's), 1–8.
    pub fn columns(mut self, columns: u64) -> Section {
        match &mut self.0.kind {
            NodeKind::ListSection(section) => section.columns = Some(columns),
            _ => unreachable!("a section node"),
        }
        self
    }

    /// How the section's images fit their cells (a Grid's).
    pub fn fit(mut self, fit: Fit) -> Section {
        match &mut self.0.kind {
            NodeKind::ListSection(section) => section.fit = fit,
            _ => unreachable!("a section node"),
        }
        self
    }

    /// Whether the section's cells sit inset from the grid's edges (a
    /// Grid's).
    pub fn inset(mut self, inset: bool) -> Section {
        match &mut self.0.kind {
            NodeKind::ListSection(section) => section.inset = inset,
            _ => unreachable!("a section node"),
        }
        self
    }
}

impl Item {
    /// The item's first line, its title.
    pub fn title(mut self, title: impl Into<String>) -> Item {
        self.0.item_mut().title = title.into();
        self
    }

    /// The item's second line, under its title.
    pub fn subtitle(mut self, subtitle: impl Into<String>) -> Item {
        self.0.item_mut().subtitle = Some(subtitle.into());
        self
    }

    /// The item's icon (`crate::icon`).
    pub fn icon(mut self, icon: Icon) -> Item {
        self.0.item_mut().look.icon = Some(icon);
        self
    }

    /// The item's accessory, after its accessories (see
    /// `crate::icon::Accessory`).
    pub fn accessory(mut self, accessory: crate::icon::Accessory) -> Item {
        self.0.item_mut().look.accessories.push(accessory);
        self
    }

    /// The tooltip shown while the pointer rests on the item's title.
    pub fn title_tooltip(mut self, tooltip: impl Into<String>) -> Item {
        self.0.item_mut().look.title_tooltip = Some(tooltip.into());
        self
    }

    /// The tooltip shown while the pointer rests on the item's subtitle.
    pub fn subtitle_tooltip(mut self, tooltip: impl Into<String>) -> Item {
        self.0.item_mut().look.subtitle_tooltip = Some(tooltip.into());
        self
    }

    /// A word the search matches as the item's subtitle is, after its
    /// keywords.
    pub fn keyword(mut self, keyword: impl Into<String>) -> Item {
        self.0.item_mut().keywords.push(keyword.into());
        self
    }

    /// The listener that activates the item (its primary action).
    pub fn on_press(mut self, listener: Listener) -> Item {
        self.0.item_mut().on_press = Some(listener);
        self
    }

    /// The item's action, after its actions: the second is its secondary
    /// action.
    pub fn action(mut self, action: ListAction) -> Item {
        self.0.item_mut().actions.push(action);
        self
    }

    /// The detail pane's content when this item is selected and the list
    /// shows its detail — built for the selected item through the render
    /// context's `selected`.
    pub fn detail(mut self, detail: impl IntoNode) -> Item {
        self.0.item_mut().detail = Some(Box::new(detail.into_node()));
        self
    }
}

impl Node {
    /// The item node of an item, changed.
    fn item_mut(&mut self) -> &mut ListItem {
        match &mut self.kind {
            NodeKind::ListItem(item) => item,
            _ => unreachable!("an item node"),
        }
    }
}

impl GridCell {
    /// The cell's title, under it.
    pub fn title(mut self, title: impl Into<String>) -> GridCell {
        self.0.cell_mut().title = Some(title.into());
        self
    }

    /// The cell's subtitle, under its title.
    pub fn subtitle(mut self, subtitle: impl Into<String>) -> GridCell {
        self.0.cell_mut().subtitle = Some(subtitle.into());
        self
    }

    /// The cell's image.
    pub fn image(mut self, image: Icon) -> GridCell {
        self.0.cell_mut().image = Some(image);
        self
    }

    /// The colour the cell fills with.
    pub fn color(mut self, color: impl Into<Paint>) -> GridCell {
        self.0.cell_mut().color = Some(color.into());
        self
    }

    /// The listener that activates the cell.
    pub fn on_press(mut self, listener: Listener) -> GridCell {
        self.0.cell_mut().on_press = Some(listener);
        self
    }
}

impl Node {
    /// The cell node of a grid cell, changed.
    fn cell_mut(&mut self) -> &mut GridItem {
        match &mut self.kind {
            NodeKind::GridItem(cell) => cell,
            _ => unreachable!("a grid cell node"),
        }
    }
}

impl Dropdown {
    /// The item chosen, by its id.
    pub fn value(mut self, value: impl Into<String>) -> Dropdown {
        match &mut self.0.kind {
            NodeKind::ListDropdown(dropdown) => dropdown.value = Some(value.into()),
            _ => unreachable!("a dropdown node"),
        }
        self
    }

    /// The dropdown's placeholder, shown while no choice is made.
    pub fn placeholder(mut self, placeholder: impl Into<String>) -> Dropdown {
        match &mut self.0.kind {
            NodeKind::ListDropdown(dropdown) => dropdown.placeholder = Some(placeholder.into()),
            _ => unreachable!("a dropdown node"),
        }
        self
    }

    /// The listener the choice's change runs, told the chosen item's id.
    pub fn on_change(mut self, listener: ValueListener) -> Dropdown {
        match &mut self.0.kind {
            NodeKind::ListDropdown(dropdown) => dropdown.on_change = Some(listener),
            _ => unreachable!("a dropdown node"),
        }
        self
    }

    /// The dropdown's item, after its items.
    pub fn item(mut self, item: DropdownItem) -> Dropdown {
        match &mut self.0.kind {
            NodeKind::ListDropdown(dropdown) => dropdown.items.push(item),
            _ => unreachable!("a dropdown node"),
        }
        self
    }
}

impl Detail {
    /// What names the view, when this node is the tree's root (see
    /// [`List::navigation_title`]).
    pub fn navigation_title(mut self, title: impl Into<String>) -> Detail {
        self.0.navigation_title = Some(title.into());
        self
    }
}

/// `fit`'s name, as the tree writes it.
fn fit_name(fit: Fit) -> &'static str {
    match fit {
        Fit::Contain => "contain",
        Fit::Cover => "cover",
        Fit::Fill => "fill",
    }
}

/// The payload the icon and icon-tile kinds read.
#[derive(Debug)]
pub struct IconNodePayload {
    icon: Icon,
    size: Option<IconSize>,
}

/// The payload the list and grid kinds read (#240): the List's own
/// properties — its search field and selection Pane owns — while its
/// children (the sections, the items or cells, the dropdown, the empty
/// view) arrive as nodes.
#[derive(Debug, Default)]
pub struct ListNode {
    /// The search field's placeholder.
    pub(crate) search_placeholder: Option<String>,
    /// The search text the view sets: the field's value, which wins while
    /// the user has not typed a newer one.
    pub(crate) search_text: Option<String>,
    /// The item the view selects, by its key.
    pub(crate) selected_key: Option<String>,
    /// The loading state: the loading bar, drawn once loading has run past
    /// its threshold (300 ms).
    pub(crate) is_loading: bool,
    /// Whether the detail pane shows beside the items.
    pub(crate) is_showing_detail: bool,
    /// Whether more items follow the ones shown, whose loading the list
    /// asks for as the selection nears the end.
    pub(crate) has_more: bool,
    /// How many items a page holds; clamped to 1–100.
    pub(crate) page_size: Option<u64>,
    /// The listener the search text runs on its every change, when the view
    /// handles the search itself — the list is then not filtered by Pane
    /// and the events are throttled.
    pub(crate) on_search_text: Option<ValueListener>,
    /// The listener the selection's change runs, told the selected item's
    /// key.
    pub(crate) on_selection_change: Option<ValueListener>,
    /// The listener the load of the next page runs, as the selection nears
    /// the end of what is shown.
    pub(crate) on_load_more: Option<Listener>,
}

/// The payload the list-section kind reads: a group of items under its
/// title and subtitle.
#[derive(Debug, Default)]
pub struct SectionNode {
    pub(crate) title: Option<String>,
    pub(crate) subtitle: Option<String>,
    /// How many columns the section's cells sit in (a Grid's), 1–8.
    pub(crate) columns: Option<u64>,
    /// The cells' width over their height (a Grid's).
    pub(crate) aspect_ratio: Option<f32>,
    /// How the section's images fit their cells (a Grid's).
    pub(crate) fit: Fit,
    /// Whether the cells sit inset from the grid's edges (a Grid's).
    pub(crate) inset: bool,
}

/// The payload the list-item kind reads (#240): the List document's
/// vocabulary on the designed tree — a title, a subtitle, keywords, an
/// icon, accessories and the actions that activate it — and the detail
/// pane's content when the item is selected.
#[derive(Debug, Default)]
pub struct ListItem {
    pub(crate) title: String,
    pub(crate) subtitle: Option<String>,
    /// Its icon, tooltips and accessories (`crate::icon`).
    pub(crate) look: crate::icon::Look,
    /// Words the search matches as the subtitle is.
    pub(crate) keywords: Vec<String>,
    /// The listener that activates the item (its primary action).
    pub(crate) on_press: Option<Listener>,
    /// Its further actions in order: the second is the item's secondary
    /// action, Ctrl+Enter.
    pub(crate) actions: Vec<ListAction>,
    /// The detail pane's content when this item is selected and the list
    /// shows its detail.
    pub(crate) detail: Option<Box<Node>>,
}

/// One of a List item's actions: what the footer calls it, and the
/// listener that runs it.
#[derive(Debug)]
pub struct ListAction {
    title: Option<String>,
    on_press: Listener,
}

impl ListAction {
    /// An action titled `title` that runs `listener` when it is chosen
    /// (the item's secondary action when it is its second).
    pub fn new(title: impl Into<String>, on_press: Listener) -> ListAction {
        ListAction {
            title: Some(title.into()),
            on_press,
        }
    }
}

/// The payload the grid-item kind reads: its title and subtitle, and the
/// image, colour or subtree it shows.
#[derive(Debug, Default)]
pub struct GridItem {
    pub(crate) title: Option<String>,
    pub(crate) subtitle: Option<String>,
    pub(crate) image: Option<Icon>,
    /// A colour the cell fills with.
    pub(crate) color: Option<Paint>,
    /// The listener that activates the cell.
    pub(crate) on_press: Option<Listener>,
}

/// The payload the list-dropdown kind reads: its items and the one
/// chosen, changed by the user's choice.
#[derive(Debug, Default)]
pub struct DropdownNode {
    /// The id of the item chosen.
    pub(crate) value: Option<String>,
    /// The dropdown's placeholder, shown while no choice is made.
    pub(crate) placeholder: Option<String>,
    /// The listener the choice's change runs, told the chosen item's id.
    pub(crate) on_change: Option<ValueListener>,
    pub(crate) items: Vec<DropdownItem>,
}

/// One item of a List's search-bar dropdown, named `value` and drawn as
/// `label` when one is given.
#[derive(Debug)]
pub struct DropdownItem {
    value: String,
    label: Option<String>,
}

impl DropdownItem {
    /// An item named `value`.
    pub fn new(value: impl Into<String>) -> DropdownItem {
        DropdownItem {
            value: value.into(),
            label: None,
        }
    }

    /// What is drawn for this item.
    pub fn label(mut self, label: impl Into<String>) -> DropdownItem {
        self.label = Some(label.into());
        self
    }
}

/// The payload the tag and badge kinds read.
#[derive(Debug)]
pub struct TagPayload {
    pub(crate) text: String,
    pub(crate) color: Option<Paint>,
}

/// The payload the toggle and checkbox kinds read.
#[derive(Debug)]
pub struct TogglePayload {
    pub(crate) on: bool,
    pub(crate) on_click: Option<Listener>,
    pub(crate) label: Option<String>,
    /// What the field says around its control as a form field (#241).
    pub(crate) field: FieldPayload,
}

/// The payload the segmented and select kinds read.
#[derive(Debug)]
pub struct SegmentedPayload {
    pub(crate) options: Vec<Choice>,
    pub(crate) value: Option<String>,
    pub(crate) on_click: Option<Listener>,
    /// The callback the popup's query runs as the user types it, when the
    /// dropdown's search is the extension's (#241).
    pub(crate) on_input: Option<ValueListener>,
    /// Whether the popup filters its choices: `false` for a dropdown whose
    /// search the extension handles.
    pub(crate) search: bool,
    /// Shown while no option is chosen (#241).
    pub(crate) placeholder: Option<String>,
    pub(crate) label: Option<String>,
    /// What the field says around its control as a form field (#241).
    pub(crate) field: FieldPayload,
}

/// The payload the section-header kind reads.
#[derive(Debug)]
pub struct SectionHeaderPayload {
    title: String,
    note: Option<String>,
}

/// The payload the metadata-list kind reads.
#[derive(Debug)]
pub struct MetadataListPayload {
    items: Vec<MetadataItem>,
}

/// The payload the empty-state kind reads.
#[derive(Debug)]
pub struct EmptyStatePayload {
    title: String,
    description: Option<String>,
    icon: Option<Icon>,
}

/// What every field of a form carries (#241): its title, the note under
/// it, the error the view's last answer set, and whether its last
/// submitted value is kept as the package's settings and prefilled the
/// next time the field appears.
#[derive(Debug, Default)]
pub(crate) struct FieldPayload {
    pub(crate) title: Option<String>,
    pub(crate) info: Option<String>,
    pub(crate) error: Option<String>,
    pub(crate) remember: bool,
}

/// The payload the text-input, password-input and text-area kinds read.
#[derive(Debug, Default)]
pub struct TextInputPayload {
    pub(crate) value: String,
    /// The value the field starts from when the view names none of its
    /// own: a form field's `default` (#241).
    pub(crate) default: Option<String>,
    pub(crate) placeholder: Option<String>,
    pub(crate) on_input: Option<ValueListener>,
    pub(crate) on_change: Option<ValueListener>,
    /// The least time between this field's input events.
    pub(crate) throttle: Option<Duration>,
    pub(crate) label: Option<String>,
    /// What the field says around its control as a form field (#241).
    pub(crate) field: FieldPayload,
}

impl SegmentedPayload {
    /// The payload over `options`, none chosen.
    pub(crate) fn of(options: Vec<Choice>) -> SegmentedPayload {
        SegmentedPayload {
            options,
            ..SegmentedPayload::default()
        }
    }
}

impl Default for SegmentedPayload {
    /// The control a tree gives no properties: it offers nothing, and a
    /// select's popup searches as one always does.
    fn default() -> SegmentedPayload {
        SegmentedPayload {
            options: Vec::new(),
            value: None,
            on_click: None,
            on_input: None,
            search: true,
            placeholder: None,
            label: None,
            field: FieldPayload::default(),
        }
    }
}

impl Default for TogglePayload {
    fn default() -> TogglePayload {
        TogglePayload {
            on: false,
            on_click: None,
            label: None,
            field: FieldPayload::default(),
        }
    }
}

/// The payload the `form` kind reads (#241): its submission, and what its
/// submit button says.
#[derive(Debug, Default)]
pub struct FormPayload {
    /// The listener a submission runs, told every field's value.
    pub(crate) on_submit: Option<FormListener>,
    pub(crate) submit_label: Option<String>,
}

/// The payload the date-picker and date-time-picker kinds read.
#[derive(Debug, Default)]
pub struct DatePayload {
    /// The field's value ("YYYY-MM-DD" or "YYYY-MM-DD HH:MM").
    pub(crate) value: String,
    /// The value the field starts from when the view names none of its
    /// own.
    pub(crate) default: Option<String>,
    pub(crate) placeholder: Option<String>,
    pub(crate) on_change: Option<ValueListener>,
    /// What the field says around its control as a form field (#241).
    pub(crate) field: FieldPayload,
}

/// The payload the tag-picker kind reads.
#[derive(Debug, Default)]
pub struct TagPickerPayload {
    /// The tags chosen, by their options' values.
    pub(crate) tags: Vec<String>,
    /// The tags offered.
    pub(crate) options: Vec<Choice>,
    /// The value the picker starts from when the view names none of its
    /// own.
    pub(crate) default: Vec<String>,
    pub(crate) on_change: Option<Listener>,
    /// What the field says around its control as a form field (#241).
    pub(crate) field: FieldPayload,
}

/// The payload the file-picker and folder-picker kinds read.
#[derive(Debug, Default)]
pub struct PathPayload {
    /// The paths chosen.
    pub(crate) paths: Vec<String>,
    /// The path the picker starts from when the view names none of its
    /// own.
    pub(crate) default: Option<String>,
    /// Whether the picker chooses several paths.
    pub(crate) multiple: bool,
    pub(crate) on_change: Option<ValueListener>,
    /// What the field says around its control as a form field (#241).
    pub(crate) field: FieldPayload,
}

/// A form's submission listener, as the tree's `form` node names it
/// (#241): what [`Cx::form_listener`] answers, told the values the form
/// was submitted with.
#[derive(Debug)]
pub struct FormListener(pub(crate) u32);

/// A `column`, `row` or `card` being built.
#[derive(Debug)]
pub struct Container(Node);

/// A `stack` being built: its children drawn over each other.
#[derive(Debug)]
pub struct Stack(Node);

/// A `scroll` being built: a scrolling region.
#[derive(Debug)]
pub struct Scroll(Node);

/// A `spacer` being built: space.
#[derive(Debug)]
pub struct Spacer(Node);

/// A `divider` being built: a hairline rule.
#[derive(Debug)]
pub struct Divider(Node);

/// A `text` being built.
#[derive(Debug)]
pub struct TextBuilder(Node);

/// A `button` being built.
#[derive(Debug)]
pub struct Button(Node);

/// A `link` being built.
#[derive(Debug)]
pub struct Link(Node);

/// An `icon` or `icon-tile` being built.
#[derive(Debug)]
pub struct IconNode(Node);

/// An `image` being built.
#[derive(Debug)]
pub struct Image(Node);

/// A `rich-row` being built.
#[derive(Debug)]
pub struct RichRowBuilder(Node);

/// A `keycap` being built: one key.
#[derive(Debug)]
pub struct Keycap(Node);

/// A `key-sequence` being built: keys side by side.
#[derive(Debug)]
pub struct KeySequence(Node);

/// A `tag` or `badge` being built.
#[derive(Debug)]
pub struct Tag(Node);

/// A `toggle` or `checkbox` being built.
#[derive(Debug)]
pub struct Toggle(Node);

/// A `segmented` control or `select` being built.
#[derive(Debug)]
pub struct Segmented(Node);

/// A `slider` being built.
#[derive(Debug)]
pub struct Slider(Node);

/// A `progress` bar being built.
#[derive(Debug)]
pub struct Progress(Node);

/// A `loading` indicator being built.
#[derive(Debug)]
pub struct Loading(Node);

/// A `markdown` node being built.
#[derive(Debug)]
pub struct Markdown(Node);

/// A `section-header` being built.
#[derive(Debug)]
pub struct SectionHeader(Node);

/// A `metadata-list` being built.
#[derive(Debug)]
pub struct MetadataList(Node);

/// An `empty-state` being built.
#[derive(Debug)]
pub struct EmptyState(Node);

/// A `text-input`, `password-input` or `text-area` being built.
#[derive(Debug)]
pub struct TextInput(Node);

/// A canvas being built.
#[derive(Debug)]
pub struct CanvasBuilder(Node);

builder!(Container);
builder!(Stack);
builder!(Scroll);
builder!(Spacer);
builder!(Divider);
builder!(TextBuilder);
builder!(Button);
builder!(Link);
builder!(IconNode);
builder!(Image);
builder!(RichRowBuilder);
builder!(Keycap);
builder!(KeySequence);
builder!(Tag);
builder!(Toggle);
builder!(Segmented);
builder!(Slider);
builder!(Progress);
builder!(Loading);
builder!(Markdown);
builder!(SectionHeader);
builder!(MetadataList);
builder!(EmptyState);
builder!(TextInput);
builder!(CanvasBuilder);

impl CanvasBuilder {
    /// The canvas node of a canvas builder, changed.
    fn canvas_mut(&mut self) -> &mut CanvasNode {
        match &mut self.0.kind {
            NodeKind::Canvas(canvas) => canvas,
            _ => unreachable!("a canvas node"),
        }
    }

    /// The operations it paints, in order (later ones over earlier ones).
    pub fn ops(mut self, ops: impl IntoIterator<Item = Draw>) -> CanvasBuilder {
        let canvas = self.canvas_mut();
        canvas.ops.clear();
        canvas.ops.extend(ops);
        self
    }

    /// What kind of control the canvas is to assistive technology; a
    /// generic one when none is given.
    pub fn role(mut self, role: CanvasRole) -> CanvasBuilder {
        self.canvas_mut().a11y.role = Some(role);
        self
    }

    /// What names the canvas to assistive technology.
    pub fn label(mut self, label: impl Into<String>) -> CanvasBuilder {
        self.canvas_mut().a11y.label = Some(label.into());
        self
    }

    /// What the canvas currently holds, as a colour well names its chosen
    /// colour.
    pub fn value(mut self, value: impl Into<String>) -> CanvasBuilder {
        self.canvas_mut().a11y.value = Some(value.into());
        self
    }

    /// The up arrow runs `listener`, the semantic handler a control-like
    /// canvas needs no key parsing for.
    pub fn on_increment(mut self, listener: Listener) -> CanvasBuilder {
        self.canvas_mut().handlers.on_increment = Some(listener);
        self
    }

    /// The down arrow runs `listener` (see [`CanvasBuilder::on_increment`]).
    pub fn on_decrement(mut self, listener: Listener) -> CanvasBuilder {
        self.canvas_mut().handlers.on_decrement = Some(listener);
        self
    }

    /// Space runs `listener` (see [`CanvasBuilder::on_increment`]).
    pub fn on_activate(mut self, listener: Listener) -> CanvasBuilder {
        self.canvas_mut().handlers.on_activate = Some(listener);
        self
    }

    /// The primary button pressed over the canvas runs `listener`.
    pub fn on_pointer_down(mut self, listener: CanvasListener) -> CanvasBuilder {
        self.canvas_mut().handlers.on_pointer_down = Some(listener);
        self
    }

    /// That button released runs `listener`.
    pub fn on_pointer_up(mut self, listener: CanvasListener) -> CanvasBuilder {
        self.canvas_mut().handlers.on_pointer_up = Some(listener);
        self
    }

    /// The pointer moved while that button is held — a drag, coalesced to
    /// the latest while one is in flight — runs `listener`.
    pub fn on_pointer_move(mut self, listener: CanvasListener) -> CanvasBuilder {
        self.canvas_mut().handlers.on_pointer_move = Some(listener);
        self
    }

    /// The pointer entering the canvas's hover runs `listener`.
    pub fn on_pointer_enter(mut self, listener: CanvasListener) -> CanvasBuilder {
        self.canvas_mut().handlers.on_pointer_enter = Some(listener);
        self
    }

    /// The pointer leaving the canvas's hover runs `listener`.
    pub fn on_pointer_leave(mut self, listener: CanvasListener) -> CanvasBuilder {
        self.canvas_mut().handlers.on_pointer_leave = Some(listener);
        self
    }

    /// The wheel turned over the canvas runs `listener`.
    pub fn on_wheel(mut self, listener: CanvasListener) -> CanvasBuilder {
        self.canvas_mut().handlers.on_wheel = Some(listener);
        self
    }

    /// The primary button pressed twice over the canvas runs `listener`.
    pub fn on_double_click(mut self, listener: CanvasListener) -> CanvasBuilder {
        self.canvas_mut().handlers.on_double_click = Some(listener);
        self
    }

    /// The secondary button pressed over the canvas runs `listener`.
    pub fn on_secondary(mut self, listener: CanvasListener) -> CanvasBuilder {
        self.canvas_mut().handlers.on_secondary = Some(listener);
        self
    }

    /// The canvas's size changing runs `listener`, told the size it was
    /// laid out at.
    pub fn on_resize(mut self, listener: CanvasListener) -> CanvasBuilder {
        self.canvas_mut().handlers.on_resize = Some(listener);
        self
    }
}

/// The properties of a column, a row or a card, as it is built.
macro_rules! lays_out {
    ($builder:ident) => {
        impl $builder {
            /// The gap between this container's children.
            pub fn gap(mut self, gap: Space) -> Self {
                self.layout(|layout| layout.gap = Some(gap));
                self
            }

            /// The padding inside this container, one token for all sides.
            pub fn padding(self, padding: Space) -> Self {
                self.padding_each(Padding {
                    top: Some(padding),
                    right: Some(padding),
                    bottom: Some(padding),
                    left: Some(padding),
                })
            }

            /// The padding inside this container, each side's.
            pub fn padding_each(mut self, padding: Padding) -> Self {
                self.layout(|layout| layout.padding = padding);
                self
            }

            /// How this container's children are laid out along its cross
            /// axis.
            pub fn align(mut self, align: Align) -> Self {
                self.layout(|layout| layout.align = Some(align));
                self
            }

            /// How this container's children share its main axis.
            pub fn justify(mut self, justify: Justify) -> Self {
                self.layout(|layout| layout.justify = Some(justify));
                self
            }

            /// Whether this container's children wrap onto further lines.
            pub fn wrap(mut self) -> Self {
                self.layout(|layout| layout.wrap = true);
                self
            }

            fn layout(&mut self, change: impl FnOnce(&mut Layout)) {
                if let Some(layout) = layout_of(&mut self.0) {
                    change(layout);
                }
            }
        }
    };
}

/// The layout of a node that has one, mutable.
fn layout_of(node: &mut Node) -> Option<&mut Layout> {
    match &mut node.kind {
        NodeKind::Column(layout) | NodeKind::Row(layout) | NodeKind::Card(layout) => Some(layout),
        _ => None,
    }
}

lays_out!(Container);

impl Stack {
    /// Where in this stack a child that says none is placed.
    pub fn align(mut self, place: Place) -> Stack {
        if let NodeKind::Stack(own) = &mut self.0.kind {
            *own = place;
        }
        self
    }
}

impl Container {
    /// The navigation title of the view this container is the root of:
    /// what names the view where a screen's title is shown. Only the
    /// tree's root names one; a title on any other node is ignored.
    pub fn navigation_title(mut self, title: impl Into<String>) -> Container {
        self.0.navigation_title = Some(title.into());
        self
    }
}

impl Scroll {
    /// This region scrolls vertically.
    pub fn vertical(mut self) -> Scroll {
        if let NodeKind::Scroll(orientation) = &mut self.0.kind {
            *orientation = Orientation::Vertical;
        }
        self
    }

    /// This region scrolls horizontally.
    pub fn horizontal(mut self) -> Scroll {
        if let NodeKind::Scroll(orientation) = &mut self.0.kind {
            *orientation = Orientation::Horizontal;
        }
        self
    }
}

impl Divider {
    /// This rule runs vertically.
    pub fn vertical(mut self) -> Divider {
        if let NodeKind::Divider(orientation) = &mut self.0.kind {
            *orientation = Orientation::Vertical;
        }
        self
    }
}

impl TextBuilder {
    /// This text in `style`: its size, weight and family.
    pub fn style(mut self, style: TextStyle) -> TextBuilder {
        self.with_text(|text| text.style = Some(style));
        self
    }

    /// This text at `level`: its colour, through the alpha of the text
    /// colour.
    pub fn level(mut self, level: TextLevel) -> TextBuilder {
        self.with_text(|text| text.level = Some(level));
        self
    }

    /// The colour this text is drawn in, a tone or a raw colour.
    pub fn color(mut self, color: impl Into<Paint>) -> TextBuilder {
        self.with_text(|text| text.color = Some(color.into()));
        self
    }

    /// This text's size in pixels.
    pub fn size(mut self, size: f32) -> TextBuilder {
        self.with_text(|text| text.size = Some(size));
        self
    }

    /// This text's weight, 100 to 900.
    pub fn weight(mut self, weight: f32) -> TextBuilder {
        self.with_text(|text| text.weight = Some(weight));
        self
    }

    /// This text is drawn on one line with an ellipsis.
    pub fn truncate(mut self) -> TextBuilder {
        self.with_text(|text| text.truncate = true);
        self
    }

    /// This text with `span` after its content.
    pub fn span(mut self, span: Span) -> TextBuilder {
        self.with_text(|text| {
            let content = match &mut text.content {
                TextContent::Plain(plain) => {
                    let plain = plain.clone();
                    TextContent::Spans(alloc::vec![
                        Span {
                            text: plain,
                            style: text.style,
                            level: text.level,
                            color: text.color.clone(),
                            code: false,
                            on_click: None,
                        },
                        span,
                    ])
                }
                TextContent::Spans(spans) => {
                    spans.push(span);
                    TextContent::Spans(spans.clone())
                }
            };
            text.content = content;
        });
        self
    }

    fn with_text(&mut self, change: impl FnOnce(&mut Text)) {
        if let NodeKind::Text(text) = &mut self.0.kind {
            change(text);
        }
    }
}

impl Span {
    /// This span in `style`: its size, weight and family.
    pub fn style(mut self, style: TextStyle) -> Span {
        self.style = Some(style);
        self
    }

    /// This span at `level`.
    pub fn level(mut self, level: TextLevel) -> Span {
        self.level = Some(level);
        self
    }

    /// The colour this span is drawn in.
    pub fn color(mut self, color: impl Into<Paint>) -> Span {
        self.color = Some(color.into());
        self
    }

    /// This span is drawn in the mono family, as code is.
    pub fn code(mut self) -> Span {
        self.code = true;
        self
    }

    /// This span is a link, a press of it running `listener`.
    pub fn on_click(mut self, listener: Listener) -> Span {
        self.on_click = Some(listener);
        self
    }
}

impl IntoNode for Span {
    fn into_node(self) -> Node {
        text(String::new()).span(self).into_node()
    }
}

impl Button {
    /// This button in `tone`.
    pub fn tone(mut self, tone: Tone) -> Button {
        if let NodeKind::Button { tone: own, .. } = &mut self.0.kind {
            *own = Some(tone);
        }
        self
    }

    /// The icon drawn before this button's label.
    pub fn icon(mut self, icon: Icon) -> Button {
        if let NodeKind::Button { icon: own, .. } = &mut self.0.kind {
            *own = Some(icon);
        }
        self
    }

    /// The keys drawn after this button's label, one keycap each.
    pub fn keys(mut self, keys: impl IntoIterator<Item = impl Into<String>>) -> Button {
        if let NodeKind::Button { keys: own, .. } = &mut self.0.kind {
            *own = Some(keys.into_iter().map(Into::into).collect());
        }
        self
    }

    /// Whether this button can be pressed.
    pub fn enabled(mut self, enabled: bool) -> Button {
        if let NodeKind::Button { enabled: own, .. } = &mut self.0.kind {
            *own = enabled;
        }
        self
    }

    /// A press of this button runs `listener` ([`Cx::listener`]), with the
    /// view's state mutably. Without one, the button cannot be pressed.
    pub fn on_click(mut self, listener: Listener) -> Button {
        if let NodeKind::Button { on_click: own, .. } = &mut self.0.kind {
            *own = Some(listener);
        }
        self
    }
}

impl Link {
    /// A press of this link runs `listener`.
    pub fn on_click(mut self, listener: Listener) -> Link {
        if let NodeKind::Link { on_click: own, .. } = &mut self.0.kind {
            *own = Some(listener);
        }
        self
    }

    /// The colour this link is drawn in.
    pub fn color(mut self, color: impl Into<Paint>) -> Link {
        if let NodeKind::Link { color: own, .. } = &mut self.0.kind {
            *own = Some(color.into());
        }
        self
    }
}

impl IconNode {
    /// How big this icon is.
    pub fn size(mut self, size: IconSize) -> IconNode {
        if let NodeKind::Icon(own) | NodeKind::IconTile(own) = &mut self.0.kind {
            own.size = Some(size);
        }
        self
    }
}

impl Image {
    /// The node drawn while this image loads or cannot be read, after any
    /// it already has: its children are its placeholder.
    pub fn placeholder(mut self, placeholder: impl IntoNode) -> Image {
        self.0.children.push(placeholder.into_node());
        self
    }

    /// How big this image is.
    pub fn size(mut self, size: IconSize) -> Image {
        if let NodeKind::Image { size: own, .. } = &mut self.0.kind {
            *own = Some(size);
        }
        self
    }

    /// How this image fits its box.
    pub fn fit(mut self, fit: Fit) -> Image {
        if let NodeKind::Image { fit: own, .. } = &mut self.0.kind {
            *own = fit;
        }
        self
    }
}

impl RichRowBuilder {
    /// This row's subtitle.
    pub fn subtitle(mut self, subtitle: impl Into<String>) -> RichRowBuilder {
        if let NodeKind::RichRow(row) = &mut self.0.kind {
            row.subtitle = Some(subtitle.into());
        }
        self
    }

    /// This row's icon.
    pub fn icon(mut self, icon: Icon) -> RichRowBuilder {
        if let NodeKind::RichRow(row) = &mut self.0.kind {
            row.icon = Some(icon);
        }
        self
    }

    /// One accessory at this row's end: `text`.
    pub fn accessory(mut self, text: impl Into<String>) -> RichRowBuilder {
        if let NodeKind::RichRow(row) = &mut self.0.kind {
            row.accessories.push(Accessory {
                text: text.into(),
                tag: false,
                color: None,
            });
        }
        self
    }

    /// One tag accessory at this row's end.
    pub fn tag(mut self, text: impl Into<String>) -> RichRowBuilder {
        if let NodeKind::RichRow(row) = &mut self.0.kind {
            row.accessories.push(Accessory {
                text: text.into(),
                tag: true,
                color: None,
            });
        }
        self
    }

    /// A press of this row runs `listener`.
    pub fn on_click(mut self, listener: Listener) -> RichRowBuilder {
        if let NodeKind::RichRow(row) = &mut self.0.kind {
            row.on_click = Some(listener);
        }
        self
    }
}

impl Tag {
    /// The colour this tag or badge is drawn in.
    pub fn color(mut self, color: impl Into<Paint>) -> Tag {
        match &mut self.0.kind {
            NodeKind::Tag(tag) | NodeKind::Badge(tag) => tag.color = Some(color.into()),
            _ => {}
        }
        self
    }
}

impl Toggle {
    /// The label drawn beside this toggle or checkbox, naming it to
    /// assistive technology.
    pub fn label(mut self, label: impl Into<String>) -> Toggle {
        match &mut self.0.kind {
            NodeKind::Toggle(toggle) | NodeKind::Checkbox(toggle) => {
                toggle.label = Some(label.into())
            }
            _ => {}
        }
        self
    }

    /// A change of this toggle or checkbox runs `listener`, told the
    /// value it now is.
    pub fn on_click(mut self, listener: Listener) -> Toggle {
        match &mut self.0.kind {
            NodeKind::Toggle(toggle) | NodeKind::Checkbox(toggle) => {
                toggle.on_click = Some(listener)
            }
            _ => {}
        }
        self
    }
}

impl Segmented {
    /// The chosen option's value.
    pub fn value(mut self, value: impl Into<String>) -> Segmented {
        match &mut self.0.kind {
            NodeKind::Segmented(control) | NodeKind::Select(control) => {
                control.value = Some(value.into())
            }
            _ => {}
        }
        self
    }

    /// This control's label, naming it to assistive technology.
    pub fn label(mut self, label: impl Into<String>) -> Segmented {
        match &mut self.0.kind {
            NodeKind::Segmented(control) | NodeKind::Select(control) => {
                control.label = Some(label.into())
            }
            _ => {}
        }
        self
    }

    /// A change of this control's choice runs `listener`, told the value
    /// chosen.
    pub fn on_click(mut self, listener: Listener) -> Segmented {
        match &mut self.0.kind {
            NodeKind::Segmented(control) | NodeKind::Select(control) => {
                control.on_click = Some(listener)
            }
            _ => {}
        }
        self
    }
}

impl Choice {
    /// What is drawn for this option.
    pub fn label(mut self, label: impl Into<String>) -> Choice {
        self.label = Some(label.into());
        self
    }

    /// The section the option belongs to, drawn as a group's header above
    /// the choices of one section (#241).
    pub fn section(mut self, section: impl Into<String>) -> Choice {
        self.section = Some(section.into());
        self
    }
}

impl Slider {
    /// This slider's lower bound.
    pub fn min(mut self, min: f32) -> Slider {
        if let NodeKind::Slider { min: own, .. } = &mut self.0.kind {
            *own = min;
        }
        self
    }

    /// This slider's upper bound.
    pub fn max(mut self, max: f32) -> Slider {
        if let NodeKind::Slider { max: own, .. } = &mut self.0.kind {
            *own = max;
        }
        self
    }

    /// How far one adjustment of this slider moves.
    pub fn step(mut self, step: f32) -> Slider {
        if let NodeKind::Slider { step: own, .. } = &mut self.0.kind {
            *own = step;
        }
        self
    }

    /// This slider's label, naming it to assistive technology.
    pub fn label(mut self, label: impl Into<String>) -> Slider {
        if let NodeKind::Slider { label: own, .. } = &mut self.0.kind {
            *own = Some(label.into());
        }
        self
    }

    /// A change of this slider runs `listener`, told the value it now is.
    pub fn on_click(mut self, listener: Listener) -> Slider {
        if let NodeKind::Slider { on_click: own, .. } = &mut self.0.kind {
            *own = Some(listener);
        }
        self
    }
}

impl Progress {
    /// This bar's label, naming it to assistive technology.
    pub fn label(mut self, label: impl Into<String>) -> Progress {
        if let NodeKind::Progress { label: own, .. } = &mut self.0.kind {
            *own = Some(label.into());
        }
        self
    }
}

impl Loading {
    /// This indicator's label, naming it to assistive technology.
    pub fn label(mut self, label: impl Into<String>) -> Loading {
        if let NodeKind::Loading { label: own } = &mut self.0.kind {
            *own = Some(label.into());
        }
        self
    }
}

impl Markdown {
    /// This node's markdown source.
    pub fn markdown(mut self, markdown: impl Into<String>) -> Markdown {
        if let NodeKind::Markdown(own) = &mut self.0.kind {
            *own = markdown.into();
        }
        self
    }
}

impl SectionHeader {
    /// The note beside this header's title.
    pub fn note(mut self, note: impl Into<String>) -> SectionHeader {
        if let NodeKind::SectionHeader(header) = &mut self.0.kind {
            header.note = Some(note.into());
        }
        self
    }
}

impl MetadataItem {
    /// This row's value.
    pub fn value(mut self, value: impl Into<String>) -> MetadataItem {
        self.value = Some(value.into());
        self
    }

    /// A press of this row's value runs `listener`, making it a link.
    pub fn on_click(mut self, listener: Listener) -> MetadataItem {
        self.on_click = Some(listener);
        self
    }
}

impl EmptyState {
    /// One action of this state's, after its children: a button usually.
    pub fn action(mut self, action: impl IntoNode) -> EmptyState {
        self.0.children.push(action.into_node());
        self
    }

    /// This state's description.
    pub fn description(mut self, description: impl Into<String>) -> EmptyState {
        if let NodeKind::EmptyState(empty) = &mut self.0.kind {
            empty.description = Some(description.into());
        }
        self
    }

    /// This state's icon.
    pub fn icon(mut self, icon: Icon) -> EmptyState {
        if let NodeKind::EmptyState(empty) = &mut self.0.kind {
            empty.icon = Some(icon);
        }
        self
    }
}

impl TextInput {
    /// This field's placeholder.
    pub fn placeholder(mut self, placeholder: impl Into<String>) -> TextInput {
        match &mut self.0.kind {
            NodeKind::TextInput(input)
            | NodeKind::PasswordInput(input)
            | NodeKind::TextArea(input) => input.placeholder = Some(placeholder.into()),
            _ => {}
        }
        self
    }

    /// This field's label, naming it to assistive technology.
    pub fn label(mut self, label: impl Into<String>) -> TextInput {
        match &mut self.0.kind {
            NodeKind::TextInput(input)
            | NodeKind::PasswordInput(input)
            | NodeKind::TextArea(input) => input.label = Some(label.into()),
            _ => {}
        }
        self
    }

    /// This field's value as the user types it runs `listener`, told it —
    /// only when the field asks: Pane coalesces the events to the latest
    /// while one is in flight.
    pub fn on_input(mut self, listener: ValueListener) -> TextInput {
        match &mut self.0.kind {
            NodeKind::TextInput(input)
            | NodeKind::PasswordInput(input)
            | NodeKind::TextArea(input) => input.on_input = Some(listener),
            _ => {}
        }
        self
    }

    /// A commit of this field (Enter, a blur) runs `listener`, told its
    /// value.
    pub fn on_change(mut self, listener: ValueListener) -> TextInput {
        match &mut self.0.kind {
            NodeKind::TextInput(input)
            | NodeKind::PasswordInput(input)
            | NodeKind::TextArea(input) => input.on_change = Some(listener),
            _ => {}
        }
        self
    }

    /// The least time between this field's input events, when it asks for
    /// them: the events between are dropped, the latest kept for the
    /// time's end.
    pub fn throttle(mut self, throttle: Duration) -> TextInput {
        match &mut self.0.kind {
            NodeKind::TextInput(input)
            | NodeKind::PasswordInput(input)
            | NodeKind::TextArea(input) => input.throttle = Some(throttle),
            _ => {}
        }
        self
    }
}

impl Surface {
    /// This surface's background colour.
    pub fn background(mut self, background: impl Into<Paint>) -> Surface {
        self.background = Some(background.into());
        self
    }

    /// This surface's border.
    pub fn border(mut self, border: Border) -> Surface {
        self.border = Some(border);
        self
    }

    /// This surface's corner radius.
    pub fn radius(mut self, radius: Radius) -> Surface {
        self.radius = Some(radius);
        self
    }

    /// This surface's opacity, 0 to 1.
    pub fn opacity(mut self, opacity: f32) -> Surface {
        self.opacity = Some(opacity);
        self
    }
}

impl Border {
    /// This border's width.
    pub fn width(mut self, width: Length) -> Border {
        self.width = Some(width);
        self
    }

    /// This border's colour.
    pub fn color(mut self, color: impl Into<Paint>) -> Border {
        self.color = Some(color.into());
        self
    }
}

/// The designed view type of a command that opens none: no value of it
/// exists, so its `render` never runs.
pub enum NoDesignedView {}

impl View for NoDesignedView {
    // No value of the type exists, so this never runs: an empty column is
    // the node of nothing.
    fn render(&mut self, _cx: &mut Cx<Self>) -> impl IntoAnswer {
        column()
    }
}

/// The state of one designed view the extension opened, as Pane's resource
/// holds it: the view, the listeners of its last two renders, and the
/// `on_pop` of the view it pushed last. The SDK makes it around the state
/// [`Command::open_designed_view`] answers with; the resource's one type.
pub struct Open<V: View> {
    state: RefCell<V>,
    /// The listeners of the last two renders, newest last: the render
    /// number and its listeners by callback id.
    tables: RefCell<Vec<(u64, Vec<Run<V>>)>>,
    /// The `on_pop` this view's last push registered, run when the view it
    /// pushed pops, with the result that pop answered.
    on_pop: RefCell<Option<Box<dyn FnOnce(&mut V, Option<&str>)>>>,
}

impl<V: View> Open<V> {
    /// The resource around `state`, its listener tables empty.
    pub(crate) fn new(state: V) -> Open<V> {
        Open {
            state: RefCell::new(state),
            tables: RefCell::new(Vec::new()),
            on_pop: RefCell::new(None),
        }
    }
}

impl<V: View> GuestView for Open<V> {
    async fn render(&self, context: String) -> Result<Rendered, String> {
        let render = render_of(&context);
        // The view's id: what its pending data asks to be drawn again
        // when it answers (#243). Named for the render's duration, so the
        // work a `Pending` inside it starts asks for this view.
        ASKING_VIEW.store(view_of(&context), Ordering::Relaxed);
        let mut listeners: Vec<Run<V>> = Vec::new();
        // This render answers the refresh the view asked for by
        // `refresh_after` — the drawing Pane waited to ask for, as the
        // context names — not one an event's answer asked for, and not
        // the drawing pending data asked for itself (see `Pending`):
        // which is where an interval's work runs.
        let refreshed = why_of(&context) == Why::Refresh;
        let selected = selected_of(&context);
        let answer = {
            let mut cx = Cx {
                listeners: &mut listeners,
                refreshed,
                selected,
            };
            self.state.borrow_mut().render(&mut cx).into_answer()
        };
        ASKING_VIEW.store(0, Ordering::Relaxed);
        // A render answering an error: shown to the user, the view keeping
        // its last good tree. Its listeners go with it — no tree of this
        // render was drawn.
        if let Some(error) = answer.error {
            return Err(error);
        }
        let mut tree = String::new();
        write_node(&mut tree, &answer.node)?;
        let document = format!("{{\"version\":\"{COMPONENT_SET}\",\"root\":{tree}}}");
        // Keep the last two renders' listeners: an event raised on the tree
        // the user saw is delivered even if the view has rendered since.
        self.tables
            .borrow_mut()
            .retain(|(number, _)| *number + 1 >= render);
        self.tables.borrow_mut().push((render, listeners));
        Ok(Rendered {
            tree: document,
            // The ask, as milliseconds: a duration Pane clamps to the
            // floor and the ceiling of refreshes.
            refresh_after_ms: answer.refresh.map(|refresh| match refresh {
                Refresh::After(after) => ms_of(after),
            }),
        })
    }

    async fn handle_event(&self, event: UiEvent) -> Result<Outcome, String> {
        // The pop event: the view above this one popped, its payload the
        // result that pop answered. The view re-renders after it, as after
        // every event.
        if event.callback == POP_CALLBACK {
            let result = pop_result_of(&event.payload);
            let on_pop = self.on_pop.borrow_mut().take();
            if let Some(run) = on_pop {
                run(&mut self.state.borrow_mut(), result.as_deref());
            }
            return Ok(nothing_next());
        }
        let UiEvent {
            render,
            callback,
            payload,
            ..
        } = event;
        // The listener the tree named, taken from the table of the render
        // the user saw: an event of an older render is stale, dropped.
        let id = usize::try_from(callback.saturating_sub(1)).ok();
        let taken = {
            let mut tables = self.tables.borrow_mut();
            match (
                tables
                    .iter_mut()
                    .find(|(number, _)| *number == render)
                    .map(|(_, listeners)| listeners),
                id,
            ) {
                (Some(listeners), Some(id)) if id < listeners.len() => {
                    Some(listeners.swap_remove(id))
                }
                _ => None,
            }
        };
        // Run what the tree named, answering the navigation it asked for.
        // A value listener is told the event's text: its payload's `value`
        // (a field's, a control's) or `key` (a key pressed).
        let text = text_of(&payload);
        let next = match taken {
            Some(Run::Listener(run)) => {
                run(&mut self.state.borrow_mut());
                None
            }
            Some(Run::Value(run)) => {
                run(&mut self.state.borrow_mut(), &text);
                None
            }
            Some(Run::Canvas(run)) => {
                // The canvas event the payload names; one that does not
                // read is dropped, never an error.
                if let Some(event) = CanvasEvent::of(&payload) {
                    run(&mut self.state.borrow_mut(), event);
                }
                None
            }
            Some(Run::Form(run)) => {
                let values = crate::form::FormValues::of(&payload);
                run(&mut self.state.borrow_mut(), &values);
                None
            }
            Some(Run::Push { open, on_pop }) => {
                let state = open(&mut self.state.borrow_mut());
                *self.on_pop.borrow_mut() = on_pop;
                Some(Next::Push(Open::new(state)))
            }
            Some(Run::Replace(open)) => {
                let state = open(&mut self.state.borrow_mut());
                Some(Next::Replace(Open::new(state)))
            }
            Some(Run::Pop(answer)) => {
                let result = answer.map(|answer| answer(&mut self.state.borrow_mut()));
                Some(Next::Pop(result.unwrap_or_default()))
            }
            None => None,
        };
        Ok(match next {
            Some(Next::Push(view)) => Outcome {
                push: Some(wit::View::new(view)),
                replace: None,
                pop: None,
            },
            Some(Next::Replace(view)) => Outcome {
                push: None,
                replace: Some(wit::View::new(view)),
                pop: None,
            },
            Some(Next::Pop(result)) => Outcome {
                push: None,
                replace: None,
                pop: Some(result),
            },
            None => nothing_next(),
        })
    }
}

/// What a listener's run answered the view does next: a view to push
/// above it or to replace it with, or a result to pop the view with.
enum Next<V: View> {
    Push(Open<V>),
    Replace(Open<V>),
    Pop(String),
}

/// The outcome with nothing next: what a listener that asked for no
/// navigation answers.
fn nothing_next() -> Outcome {
    Outcome {
        push: None,
        replace: None,
        pop: None,
    }
}

/// The text a value-carrying payload names: its `value` (a field's, a
/// control's) or its `key` (a key pressed), read without parsing the whole
/// document, so a payload that grew a field still gives its text. An empty
/// string when it names none.
fn text_of(payload: &str) -> String {
    for field in ["value", "key"] {
        let needle = format!("\"{field}\"");
        if let Some(at) = payload.find(&needle) {
            let rest = payload[at + needle.len()..]
                .trim_start()
                .strip_prefix(':')
                .unwrap_or("")
                .trim_start();
            if let Some(rest) = rest.strip_prefix('"') {
                if let Some(end) = rest.find('"') {
                    let escaped = &rest[..end];
                    let mut result = String::new();
                    let mut characters = escaped.chars();
                    while let Some(character) = characters.next() {
                        match character {
                            '\\' => match characters.next() {
                                Some('"') => result.push('"'),
                                Some('\\') => result.push('\\'),
                                Some('/') => result.push('/'),
                                Some('n') => result.push('\n'),
                                Some('r') => result.push('\r'),
                                Some('t') => result.push('\t'),
                                Some('b') => result.push('\u{8}'),
                                Some('f') => result.push('\u{c}'),
                                Some('u') => {
                                    let digits: String = characters.by_ref().take(4).collect();
                                    if let Ok(code) = u32::from_str_radix(&digits, 16) {
                                        if let Some(character) = char::from_u32(code) {
                                            result.push(character);
                                        }
                                    }
                                }
                                _ => return String::new(),
                            },
                            other => result.push(other),
                        }
                    }
                    return result;
                }
            }
        }
    }
    String::new()
}

/// The result the pop event's payload carries: `{"pop": "…"}` for a pop
/// that answered one, `{"pop": null}` for a pop that carried none (the
/// back key's, so no view's result reached the one below). Read without
/// parsing the whole document, so a payload that grew a field still gives
/// its result.
fn pop_result_of(payload: &str) -> Option<String> {
    let at = payload.find("\"pop\"")?;
    let rest = payload[at + 5..]
        .trim_start()
        .strip_prefix(':')?
        .trim_start();
    if rest.starts_with("null") {
        return None;
    }
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    let escaped = &rest[..end];
    let mut result = String::new();
    let mut characters = escaped.chars();
    while let Some(character) = characters.next() {
        match character {
            '\\' => match characters.next() {
                Some('"') => result.push('"'),
                Some('\\') => result.push('\\'),
                Some('/') => result.push('/'),
                Some('n') => result.push('\n'),
                Some('r') => result.push('\r'),
                Some('t') => result.push('\t'),
                Some('b') => result.push('\u{8}'),
                Some('f') => result.push('\u{c}'),
                Some('u') => {
                    let digits: String = characters.by_ref().take(4).collect();
                    let code = u32::from_str_radix(&digits, 16).ok()?;
                    result.push(char::from_u32(code)?);
                }
                _ => return None,
            },
            other => result.push(other),
        }
    }
    Some(result)
}

/// `after` as milliseconds, capped where Pane clamps the ceiling of
/// refreshes anyway.
fn ms_of(after: Duration) -> u32 {
    u32::try_from(after.as_millis()).unwrap_or(u32::MAX)
}

/// Why Pane asks for the view's tree, as its render's context names: the
/// render answering the view's own ask is where an interval's work runs;
/// a drawing the view's work asked for itself (#243) is not one.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Why {
    /// The view's first drawing.
    Open,
    /// The render an event's answer asks for.
    Event,
    /// The drawing the view asked Pane to wait for by
    /// [`IntoNode::refresh_after`].
    Refresh,
    /// The drawing the view's own work asked for.
    Push,
}

/// Why Pane asks for the view's tree, as `context` names: the context is
/// JSON, `{"render": N, "view": V, "why": "refresh", "ui": "1.1"}`, read
/// without parsing the whole document, so a context that grew a field
/// still gives its why.
/// The selected item's key of the view's List or Grid, as the render's
/// context names it (#240); `None` when it says none. The context is
/// JSON, `{"render": N, "view": V, "why": "open", "selected": "…", "ui":
/// "1.3"}`, read without parsing the whole document, as its other fields
/// are.
fn selected_of(context: &str) -> Option<String> {
    let at = context.find("\"selected\"")? + 10;
    let value = context[at..].trim_start().strip_prefix(':')?.trim_start();
    let value = value.strip_prefix('"')?;
    // The key runs to the quote that closes it, an escaped one skipped.
    let mut end = value.len();
    let mut chars = value.char_indices();
    while let Some((at, character)) = chars.next() {
        if character == '\\' {
            let _ = chars.next();
            continue;
        }
        if character == '"' {
            end = at;
            break;
        }
    }
    let raw = &value[..end];
    let mut selected = String::new();
    let mut chars = raw.chars();
    while let Some(character) = chars.next() {
        if character == '\\' {
            selected.push(chars.next().unwrap_or('\\'));
        } else {
            selected.push(character);
        }
    }
    Some(selected)
}

fn why_of(context: &str) -> Why {
    let Some(at) = context.find("\"why\"") else {
        return Why::Open;
    };
    let word = context[at + 5..]
        .trim_start()
        .strip_prefix(':')
        .unwrap_or("")
        .trim_start();
    for (name, why) in [
        ("\"open\"", Why::Open),
        ("\"event\"", Why::Event),
        ("\"refresh\"", Why::Refresh),
        ("\"push\"", Why::Push),
    ] {
        if word.starts_with(name) {
            return why;
        }
    }
    Why::Open
}

/// The view `context` names — its id, what [`Pending`]'s work asks to be
/// drawn again — or 0 when it says none: the context is JSON, `{"render":
/// N, "view": V, "why": "open", "ui": "1.1"}`, read without parsing the
/// whole document, so a context that grew a field still gives its id.
fn view_of(context: &str) -> u64 {
    let Some(at) = context.find("\"view\"") else {
        return 0;
    };
    let digits = context[at + 6..]
        .trim_start()
        .strip_prefix(':')
        .unwrap_or("")
        .trim_start();
    let end = digits
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(digits.len());
    digits[..end].parse().unwrap_or(0)
}

/// The render number `context` names, or 1 when it says none: the context
/// is JSON, `{"render": N, "view": V, "ui": "1.2"}`, read without parsing
/// the whole document, so a context that grew a field still gives its
/// number.
fn render_of(context: &str) -> u64 {
    let Some(at) = context.find("\"render\"") else {
        return 1;
    };
    let digits = context[at + 8..]
        .trim_start()
        .strip_prefix(':')
        .unwrap_or("")
        .trim_start();
    let end = digits
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(digits.len());
    digits[..end].parse().unwrap_or(1)
}

/// Writes `node` as the tree's JSON, naming each control's listener by the
/// id [`Cx::listener`] gave it.
fn write_node(tree: &mut String, node: &Node) -> Result<(), String> {
    tree.push_str("{\"type\":");
    string(tree, kind_of(node))?;
    if let Some(key) = &node.key {
        tree.push_str(",\"key\":");
        string(tree, key)?;
    }
    if let Some(name) = &node.name {
        tree.push_str(",\"name\":");
        string(tree, name)?;
    }
    if let Some(title) = &node.navigation_title {
        tree.push_str(",\"navigationTitle\":");
        string(tree, title)?;
    }
    if let Some(requires) = node.requires {
        let _ = write!(tree, ",\"requires\":{requires}");
    }
    if node.focus {
        tree.push_str(",\"focus\":true");
    }
    if let Some(Listener(id)) = node.on_focus {
        tree.push_str(",\"onFocus\":");
        let _ = write!(tree, "{id}");
    }
    if let Some(Listener(id)) = node.on_blur {
        tree.push_str(",\"onBlur\":");
        let _ = write!(tree, "{id}");
    }
    if let Some(ValueListener(id)) = node.on_key {
        tree.push_str(",\"onKey\":");
        let _ = write!(tree, "{id}");
    }
    write_style(tree, &node.style)?;
    if let Some(place) = node.place {
        tree.push_str(",\"place\":");
        string(tree, place_name(place))?;
    }
    if let Some((x, y)) = &node.offset {
        tree.push_str(",\"offset\":{\"x\":");
        write_length(tree, *x)?;
        tree.push_str(",\"y\":");
        write_length(tree, *y)?;
        tree.push('}');
    }
    match &node.kind {
        NodeKind::Column(layout) | NodeKind::Card(layout) | NodeKind::Row(layout) => {
            write_layout(tree, layout)?;
        }
        NodeKind::Stack(place) => {
            tree.push_str(",\"align\":");
            string(tree, place_name(*place))?;
        }
        NodeKind::Scroll(orientation) | NodeKind::Divider(orientation) => {
            tree.push_str(",\"orientation\":");
            string(
                tree,
                match *orientation {
                    Orientation::Vertical => "vertical",
                    Orientation::Horizontal => "horizontal",
                },
            )?;
        }
        NodeKind::Spacer => {}
        NodeKind::Text(text) => {
            match &text.content {
                TextContent::Plain(content) => {
                    tree.push_str(",\"text\":");
                    string(tree, content)?;
                }
                TextContent::Spans(spans) => {
                    tree.push_str(",\"spans\":[");
                    for (index, span) in spans.iter().enumerate() {
                        if index > 0 {
                            tree.push(',');
                        }
                        write_span(tree, span)?;
                    }
                    tree.push(']');
                }
            }
            write_text_props(tree, text)?;
        }
        NodeKind::Button {
            label,
            tone,
            icon,
            keys,
            on_click,
            enabled,
        } => {
            tree.push_str(",\"label\":");
            string(tree, label)?;
            if let Some(tone) = tone {
                tree.push_str(",\"tone\":");
                string(
                    tree,
                    match tone {
                        Tone::Default => "default",
                        Tone::Secondary => "secondary",
                        Tone::Ghost => "ghost",
                        Tone::Accent => "accent",
                        Tone::Destructive => "destructive",
                    },
                )?;
            }
            if let Some(icon) = icon {
                tree.push_str(",\"icon\":");
                icon::write_icon(tree, icon);
            }
            if let Some(keys) = keys {
                tree.push_str(",\"keys\":[");
                for (index, key) in keys.iter().enumerate() {
                    if index > 0 {
                        tree.push(',');
                    }
                    string(tree, key)?;
                }
                tree.push(']');
            }
            if !*enabled {
                tree.push_str(",\"enabled\":false");
            }
            if let Some(Listener(id)) = on_click {
                tree.push_str(",\"onPress\":");
                let _ = write!(tree, "{id}");
            }
        }
        NodeKind::Link {
            label,
            on_click,
            color,
        } => {
            tree.push_str(",\"label\":");
            string(tree, label)?;
            if let Some(Listener(id)) = on_click {
                tree.push_str(",\"onPress\":");
                let _ = write!(tree, "{id}");
            }
            if let Some(color) = color {
                tree.push_str(",\"color\":");
                write_paint(tree, color)?;
            }
        }
        NodeKind::Icon(icon) | NodeKind::IconTile(icon) => {
            tree.push_str(",\"icon\":");
            icon::write_icon(tree, &icon.icon);
            if let Some(size) = icon.size {
                tree.push_str(",\"size\":");
                write_size(tree, size)?;
            }
        }
        NodeKind::Image { image, size, fit } => {
            tree.push_str(",\"image\":");
            icon::write_icon(tree, image);
            if let Some(size) = size {
                tree.push_str(",\"size\":");
                write_size(tree, *size)?;
            }
            tree.push_str(",\"fit\":");
            string(
                tree,
                match fit {
                    Fit::Contain => "contain",
                    Fit::Cover => "cover",
                    Fit::Fill => "fill",
                },
            )?;
        }
        NodeKind::RichRow(row) => {
            tree.push_str(",\"title\":");
            string(tree, &row.title)?;
            if let Some(subtitle) = &row.subtitle {
                tree.push_str(",\"subtitle\":");
                string(tree, subtitle)?;
            }
            if let Some(icon) = &row.icon {
                tree.push_str(",\"icon\":");
                icon::write_icon(tree, icon);
            }
            if !row.accessories.is_empty() {
                tree.push_str(",\"accessories\":[");
                for (index, accessory) in row.accessories.iter().enumerate() {
                    if index > 0 {
                        tree.push(',');
                    }
                    tree.push_str("{\"text\":");
                    string(tree, &accessory.text)?;
                    if accessory.tag {
                        tree.push_str(",\"tag\":true");
                    }
                    if let Some(color) = &accessory.color {
                        tree.push_str(",\"color\":");
                        write_paint(tree, color)?;
                    }
                    tree.push('}');
                }
                tree.push(']');
            }
            if let Some(Listener(id)) = &row.on_click {
                tree.push_str(",\"onPress\":");
                let _ = write!(tree, "{id}");
            }
        }
        NodeKind::Keycap(key) => {
            tree.push_str(",\"key\":");
            string(tree, key)?;
        }
        NodeKind::KeySequence(keys) => {
            tree.push_str(",\"keys\":[");
            for (index, key) in keys.iter().enumerate() {
                if index > 0 {
                    tree.push(',');
                }
                string(tree, key)?;
            }
            tree.push(']');
        }
        NodeKind::Tag(tag) | NodeKind::Badge(tag) => {
            tree.push_str(",\"text\":");
            string(tree, &tag.text)?;
            if let Some(color) = &tag.color {
                tree.push_str(",\"color\":");
                write_paint(tree, color)?;
            }
        }
        NodeKind::Toggle(toggle) => {
            if toggle.on {
                tree.push_str(",\"on\":true");
            }
            if let Some(label) = &toggle.label {
                tree.push_str(",\"label\":");
                string(tree, label)?;
            }
            if let Some(Listener(id)) = &toggle.on_click {
                tree.push_str(",\"onChange\":");
                let _ = write!(tree, "{id}");
            }
            write_field(tree, &toggle.field)?;
        }
        NodeKind::Checkbox(toggle) => {
            if toggle.on {
                tree.push_str(",\"checked\":true");
            }
            if let Some(label) = &toggle.label {
                tree.push_str(",\"label\":");
                string(tree, label)?;
            }
            if let Some(Listener(id)) = &toggle.on_click {
                tree.push_str(",\"onChange\":");
                let _ = write!(tree, "{id}");
            }
            write_field(tree, &toggle.field)?;
        }
        NodeKind::Segmented(control) | NodeKind::Select(control) => {
            tree.push_str(",\"options\":[");
            for (index, option) in control.options.iter().enumerate() {
                if index > 0 {
                    tree.push(',');
                }
                tree.push_str("{\"value\":");
                string(tree, &option.value)?;
                if let Some(label) = &option.label {
                    tree.push_str(",\"label\":");
                    string(tree, label)?;
                }
                if let Some(section) = &option.section {
                    tree.push_str(",\"section\":");
                    string(tree, section)?;
                }
                tree.push('}');
            }
            tree.push(']');
            if let Some(value) = &control.value {
                tree.push_str(",\"value\":");
                string(tree, value)?;
            }
            if let Some(placeholder) = &control.placeholder {
                tree.push_str(",\"placeholder\":");
                string(tree, placeholder)?;
            }
            if let Some(label) = &control.label {
                tree.push_str(",\"label\":");
                string(tree, label)?;
            }
            if let Some(Listener(id)) = &control.on_click {
                tree.push_str(",\"onChange\":");
                let _ = write!(tree, "{id}");
            }
            if let Some(ValueListener(id)) = &control.on_input {
                tree.push_str(",\"onInput\":");
                let _ = write!(tree, "{id}");
            }
            if !control.search {
                tree.push_str(",\"search\":false");
            }
            write_field(tree, &control.field)?;
        }
        NodeKind::Slider {
            value,
            min,
            max,
            step,
            on_click,
            label,
        } => {
            let _ = write!(tree, ",\"value\":{value}");
            let _ = write!(tree, ",\"min\":{min}");
            let _ = write!(tree, ",\"max\":{max}");
            let _ = write!(tree, ",\"step\":{step}");
            if let Some(label) = label {
                tree.push_str(",\"label\":");
                string(tree, label)?;
            }
            if let Some(Listener(id)) = on_click {
                tree.push_str(",\"onChange\":");
                let _ = write!(tree, "{id}");
            }
        }
        NodeKind::Progress { value, label } => {
            let _ = write!(tree, ",\"value\":{value}");
            if let Some(label) = label {
                tree.push_str(",\"label\":");
                string(tree, label)?;
            }
        }
        NodeKind::Loading { label } => {
            if let Some(label) = label {
                tree.push_str(",\"label\":");
                string(tree, label)?;
            }
        }
        NodeKind::Markdown(markdown) => {
            tree.push_str(",\"markdown\":");
            string(tree, markdown)?;
        }
        NodeKind::SectionHeader(header) => {
            tree.push_str(",\"title\":");
            string(tree, &header.title)?;
            if let Some(note) = &header.note {
                tree.push_str(",\"note\":");
                string(tree, note)?;
            }
        }
        NodeKind::MetadataList(list) => {
            tree.push_str(",\"items\":[");
            for (index, item) in list.items.iter().enumerate() {
                if index > 0 {
                    tree.push(',');
                }
                tree.push('{');
                if item.separator {
                    tree.push_str("\"separator\":true");
                } else {
                    let mut first = true;
                    if let Some(label) = &item.label {
                        first = false;
                        string_field(tree, "label", label)?;
                    }
                    if let Some(value) = &item.value {
                        if !first {
                            tree.push(',');
                        }
                        first = false;
                        string_field(tree, "value", value)?;
                    }
                    if !item.tags.is_empty() {
                        if !first {
                            tree.push(',');
                        }
                        first = false;
                        tree.push_str("\"tags\":[");
                        for (at, tag) in item.tags.iter().enumerate() {
                            if at > 0 {
                                tree.push(',');
                            }
                            string(tree, tag)?;
                        }
                        tree.push(']');
                    }
                    if let Some(Listener(id)) = &item.on_click {
                        if !first {
                            tree.push(',');
                        }
                        tree.push_str("\"onPress\":");
                        let _ = write!(tree, "{id}");
                    }
                }
                tree.push('}');
            }
            tree.push(']');
        }
        NodeKind::EmptyState(empty) => {
            tree.push_str(",\"title\":");
            string(tree, &empty.title)?;
            if let Some(description) = &empty.description {
                tree.push_str(",\"description\":");
                string(tree, description)?;
            }
            if let Some(icon) = &empty.icon {
                tree.push_str(",\"icon\":");
                icon::write_icon(tree, icon);
            }
        }
        NodeKind::TextInput(input) | NodeKind::PasswordInput(input) | NodeKind::TextArea(input) => {
            tree.push_str(",\"value\":");
            string(tree, &input.value)?;
            if let Some(default) = &input.default {
                tree.push_str(",\"default\":");
                string(tree, default)?;
            }
            if let Some(placeholder) = &input.placeholder {
                tree.push_str(",\"placeholder\":");
                string(tree, placeholder)?;
            }
            if let Some(label) = &input.label {
                tree.push_str(",\"label\":");
                string(tree, label)?;
            }
            if let Some(ValueListener(id)) = &input.on_input {
                tree.push_str(",\"onInput\":");
                let _ = write!(tree, "{id}");
            }
            if let Some(ValueListener(id)) = &input.on_change {
                tree.push_str(",\"onChange\":");
                let _ = write!(tree, "{id}");
            }
            if let Some(throttle) = input.throttle {
                let ms = u64::try_from(throttle.as_millis()).unwrap_or(u64::MAX);
                tree.push_str(",\"throttleMs\":");
                let _ = write!(tree, "{ms}");
            }
            write_field(tree, &input.field)?;
        }
        NodeKind::Form(form) => {
            if let Some(FormListener(id)) = &form.on_submit {
                tree.push_str(",\"onSubmit\":");
                let _ = write!(tree, "{id}");
            }
            if let Some(label) = &form.submit_label {
                tree.push_str(",\"submitLabel\":");
                string(tree, label)?;
            }
        }
        NodeKind::DatePicker(date) | NodeKind::DateTimePicker(date) => {
            tree.push_str(",\"value\":");
            string(tree, &date.value)?;
            if let Some(default) = &date.default {
                tree.push_str(",\"default\":");
                string(tree, default)?;
            }
            if let Some(placeholder) = &date.placeholder {
                tree.push_str(",\"placeholder\":");
                string(tree, placeholder)?;
            }
            if let Some(ValueListener(id)) = &date.on_change {
                tree.push_str(",\"onChange\":");
                let _ = write!(tree, "{id}");
            }
            write_field(tree, &date.field)?;
        }
        NodeKind::TagPicker(picker) => {
            if !picker.tags.is_empty() {
                tree.push_str(",\"tags\":[");
                for (index, tag) in picker.tags.iter().enumerate() {
                    if index > 0 {
                        tree.push(',');
                    }
                    string(tree, tag)?;
                }
                tree.push(']');
            }
            if !picker.default.is_empty() {
                tree.push_str(",\"default\":[");
                for (index, tag) in picker.default.iter().enumerate() {
                    if index > 0 {
                        tree.push(',');
                    }
                    string(tree, tag)?;
                }
                tree.push(']');
            }
            if !picker.options.is_empty() {
                tree.push_str(",\"options\":[");
                for (index, option) in picker.options.iter().enumerate() {
                    if index > 0 {
                        tree.push(',');
                    }
                    tree.push('{');
                    tree.push_str("\"value\":");
                    string(tree, &option.value)?;
                    if let Some(label) = &option.label {
                        tree.push_str(",\"label\":");
                        string(tree, label)?;
                    }
                    if let Some(section) = &option.section {
                        tree.push_str(",\"section\":");
                        string(tree, section)?;
                    }
                    tree.push('}');
                }
                tree.push(']');
            }
            if let Some(Listener(id)) = &picker.on_change {
                tree.push_str(",\"onChange\":");
                let _ = write!(tree, "{id}");
            }
            write_field(tree, &picker.field)?;
        }
        NodeKind::FilePicker(picker) | NodeKind::FolderPicker(picker) => {
            if !picker.paths.is_empty() {
                tree.push_str(",\"value\":");
                if picker.paths.len() == 1 {
                    string(tree, &picker.paths[0])?;
                } else {
                    tree.push('[');
                    for (index, path) in picker.paths.iter().enumerate() {
                        if index > 0 {
                            tree.push(',');
                        }
                        string(tree, path)?;
                    }
                    tree.push(']');
                }
            }
            if let Some(default) = &picker.default {
                tree.push_str(",\"default\":");
                string(tree, default)?;
            }
            tree.push_str(",\"multiple\":");
            let _ = write!(tree, "{}", picker.multiple);
            write_field(tree, &picker.field)?;
        }
        NodeKind::List(list) | NodeKind::Grid(list) => {
            if let Some(placeholder) = &list.search_placeholder {
                tree.push_str(",\"searchPlaceholder\":");
                string(tree, placeholder)?;
            }
            if let Some(text) = &list.search_text {
                tree.push_str(",\"searchText\":");
                string(tree, text)?;
            }
            if let Some(key) = &list.selected_key {
                tree.push_str(",\"selectedKey\":");
                string(tree, key)?;
            }
            if list.is_loading {
                tree.push_str(",\"isLoading\":true");
            }
            if list.is_showing_detail {
                tree.push_str(",\"isShowingDetail\":true");
            }
            if list.has_more {
                tree.push_str(",\"hasMore\":true");
            }
            if let Some(size) = list.page_size {
                tree.push_str(",\"pageSize\":");
                let _ = write!(tree, "{size}");
            }
            if let Some(ValueListener(id)) = &list.on_search_text {
                tree.push_str(",\"onSearchText\":");
                let _ = write!(tree, "{id}");
            }
            if let Some(ValueListener(id)) = &list.on_selection_change {
                tree.push_str(",\"onSelectionChange\":");
                let _ = write!(tree, "{id}");
            }
            if let Some(Listener(id)) = &list.on_load_more {
                tree.push_str(",\"onLoadMore\":");
                let _ = write!(tree, "{id}");
            }
        }
        NodeKind::ListSection(section) => {
            if let Some(title) = &section.title {
                tree.push_str(",\"title\":");
                string(tree, title)?;
            }
            if let Some(subtitle) = &section.subtitle {
                tree.push_str(",\"subtitle\":");
                string(tree, subtitle)?;
            }
            if let Some(columns) = section.columns {
                tree.push_str(",\"columns\":");
                let _ = write!(tree, "{columns}");
            }
            if let Some(ratio) = section.aspect_ratio {
                tree.push_str(",\"aspectRatio\":");
                let _ = write!(tree, "{ratio}");
            }
            if section.fit != Fit::Contain {
                tree.push_str(",\"fit\":");
                string(tree, fit_name(section.fit))?;
            }
            if section.inset {
                tree.push_str(",\"inset\":true");
            }
        }
        NodeKind::ListItem(item) => {
            tree.push_str(",\"title\":");
            string(tree, &item.title)?;
            if let Some(subtitle) = &item.subtitle {
                tree.push_str(",\"subtitle\":");
                string(tree, subtitle)?;
            }
            crate::icon::write_look(tree, &item.look);
            if !item.keywords.is_empty() {
                tree.push_str(",\"keywords\":[");
                for (index, keyword) in item.keywords.iter().enumerate() {
                    if index > 0 {
                        tree.push(',');
                    }
                    string(tree, keyword)?;
                }
                tree.push(']');
            }
            if let Some(Listener(id)) = &item.on_press {
                tree.push_str(",\"onPress\":");
                let _ = write!(tree, "{id}");
            }
            if !item.actions.is_empty() {
                tree.push_str(",\"actions\":[");
                for (index, action) in item.actions.iter().enumerate() {
                    if index > 0 {
                        tree.push(',');
                    }
                    tree.push('{');
                    if let Some(title) = &action.title {
                        tree.push_str("\"title\":");
                        string(tree, title)?;
                        tree.push(',');
                    }
                    let Listener(id) = &action.on_press;
                    tree.push_str("\"onPress\":");
                    let _ = write!(tree, "{id}");
                    tree.push('}');
                }
                tree.push(']');
            }
            if let Some(detail) = &item.detail {
                tree.push_str(",\"detail\":");
                write_node(tree, detail)?;
            }
        }
        NodeKind::GridItem(item) => {
            if let Some(title) = &item.title {
                tree.push_str(",\"title\":");
                string(tree, title)?;
            }
            if let Some(subtitle) = &item.subtitle {
                tree.push_str(",\"subtitle\":");
                string(tree, subtitle)?;
            }
            if let Some(image) = &item.image {
                tree.push_str(",\"image\":");
                icon::write_icon(tree, image);
            }
            if let Some(color) = &item.color {
                tree.push_str(",\"color\":");
                write_paint(tree, color)?;
            }
            if let Some(Listener(id)) = &item.on_press {
                tree.push_str(",\"onPress\":");
                let _ = write!(tree, "{id}");
            }
        }
        NodeKind::ListDropdown(dropdown) => {
            if let Some(value) = &dropdown.value {
                tree.push_str(",\"value\":");
                string(tree, value)?;
            }
            if let Some(placeholder) = &dropdown.placeholder {
                tree.push_str(",\"placeholder\":");
                string(tree, placeholder)?;
            }
            if let Some(ValueListener(id)) = &dropdown.on_change {
                tree.push_str(",\"onChange\":");
                let _ = write!(tree, "{id}");
            }
            if !dropdown.items.is_empty() {
                tree.push_str(",\"items\":[");
                for (index, held) in dropdown.items.iter().enumerate() {
                    if index > 0 {
                        tree.push(',');
                    }
                    tree.push('{');
                    tree.push_str("\"value\":");
                    string(tree, &held.value)?;
                    if let Some(label) = &held.label {
                        tree.push_str(",\"title\":");
                        string(tree, label)?;
                    }
                    tree.push('}');
                }
                tree.push(']');
            }
        }
        NodeKind::Canvas(canvas) => {
            if !canvas.ops.is_empty() {
                tree.push_str(",\"ops\":[");
                for (index, op) in canvas.ops.iter().enumerate() {
                    if index > 0 {
                        tree.push(',');
                    }
                    write_draw(tree, op)?;
                }
                tree.push(']');
            }
            if let Some(role) = canvas.a11y.role {
                tree.push_str(",\"role\":");
                string(
                    tree,
                    match role {
                        CanvasRole::ColorWell => "color-well",
                        CanvasRole::Slider => "slider",
                        CanvasRole::Image => "image",
                        CanvasRole::Figure => "figure",
                        CanvasRole::Group => "group",
                        CanvasRole::Generic => "generic",
                    },
                )?;
            }
            if let Some(label) = &canvas.a11y.label {
                tree.push_str(",\"label\":");
                string(tree, label)?;
            }
            if let Some(value) = &canvas.a11y.value {
                tree.push_str(",\"value\":");
                string(tree, value)?;
            }
            if let Some(Listener(id)) = canvas.handlers.on_increment {
                tree.push_str(",\"onIncrement\":");
                let _ = write!(tree, "{id}");
            }
            if let Some(Listener(id)) = canvas.handlers.on_decrement {
                tree.push_str(",\"onDecrement\":");
                let _ = write!(tree, "{id}");
            }
            if let Some(Listener(id)) = canvas.handlers.on_activate {
                tree.push_str(",\"onActivate\":");
                let _ = write!(tree, "{id}");
            }
            if let Some(CanvasListener(id)) = canvas.handlers.on_pointer_down {
                tree.push_str(",\"onPointerDown\":");
                let _ = write!(tree, "{id}");
            }
            if let Some(CanvasListener(id)) = canvas.handlers.on_pointer_up {
                tree.push_str(",\"onPointerUp\":");
                let _ = write!(tree, "{id}");
            }
            if let Some(CanvasListener(id)) = canvas.handlers.on_pointer_move {
                tree.push_str(",\"onPointerMove\":");
                let _ = write!(tree, "{id}");
            }
            if let Some(CanvasListener(id)) = canvas.handlers.on_pointer_enter {
                tree.push_str(",\"onPointerEnter\":");
                let _ = write!(tree, "{id}");
            }
            if let Some(CanvasListener(id)) = canvas.handlers.on_pointer_leave {
                tree.push_str(",\"onPointerLeave\":");
                let _ = write!(tree, "{id}");
            }
            if let Some(CanvasListener(id)) = canvas.handlers.on_wheel {
                tree.push_str(",\"onWheel\":");
                let _ = write!(tree, "{id}");
            }
            if let Some(CanvasListener(id)) = canvas.handlers.on_double_click {
                tree.push_str(",\"onDoubleClick\":");
                let _ = write!(tree, "{id}");
            }
            if let Some(CanvasListener(id)) = canvas.handlers.on_secondary {
                tree.push_str(",\"onSecondary\":");
                let _ = write!(tree, "{id}");
            }
            if let Some(CanvasListener(id)) = canvas.handlers.on_resize {
                tree.push_str(",\"onResize\":");
                let _ = write!(tree, "{id}");
            }
        }
        NodeKind::Detail(_) => {}
    }
    if !node.children.is_empty() {
        tree.push_str(",\"children\":[");
        for (index, child) in node.children.iter().enumerate() {
            if index > 0 {
                tree.push(',');
            }
            write_node(tree, child)?;
        }
        tree.push(']');
    }
    if let Some(fallback) = &node.fallback {
        tree.push_str(",\"fallback\":");
        write_node(tree, fallback)?;
    }
    tree.push('}');
    Ok(())
}

/// The kind's name in the tree.
fn kind_of(node: &Node) -> &'static str {
    match &node.kind {
        NodeKind::Column(_) => "column",
        NodeKind::Row(_) => "row",
        NodeKind::Stack(_) => "stack",
        NodeKind::Scroll(_) => "scroll",
        NodeKind::Spacer => "spacer",
        NodeKind::Divider(_) => "divider",
        NodeKind::Text(_) => "text",
        NodeKind::Button { .. } => "button",
        NodeKind::Link { .. } => "link",
        NodeKind::Icon(_) => "icon",
        NodeKind::IconTile(_) => "icon-tile",
        NodeKind::Image { .. } => "image",
        NodeKind::RichRow(_) => "rich-row",
        NodeKind::Keycap(_) => "keycap",
        NodeKind::KeySequence(_) => "key-sequence",
        NodeKind::Tag(_) => "tag",
        NodeKind::Badge(_) => "badge",
        NodeKind::Toggle(_) => "toggle",
        NodeKind::Checkbox(_) => "checkbox",
        NodeKind::Segmented(_) => "segmented",
        NodeKind::Select(_) => "select",
        NodeKind::Slider { .. } => "slider",
        NodeKind::Progress { .. } => "progress",
        NodeKind::Loading { .. } => "loading",
        NodeKind::Markdown(_) => "markdown",
        NodeKind::Card(_) => "card",
        NodeKind::SectionHeader(_) => "section-header",
        NodeKind::MetadataList(_) => "metadata-list",
        NodeKind::EmptyState(_) => "empty-state",
        NodeKind::TextInput(_) => "text-input",
        NodeKind::PasswordInput(_) => "password-input",
        NodeKind::TextArea(_) => "text-area",
        NodeKind::Canvas(_) => "canvas",
        NodeKind::Form(_) => "form",
        NodeKind::DatePicker(_) => "date-picker",
        NodeKind::DateTimePicker(_) => "date-time-picker",
        NodeKind::TagPicker(_) => "tag-picker",
        NodeKind::FilePicker(_) => "file-picker",
        NodeKind::FolderPicker(_) => "folder-picker",
        NodeKind::List(_) => "list",
        NodeKind::Grid(_) => "grid",
        NodeKind::ListSection(_) => "list-section",
        NodeKind::ListItem(_) => "list-item",
        NodeKind::GridItem(_) => "grid-item",
        NodeKind::ListDropdown(_) => "list-dropdown",
        NodeKind::Detail(_) => "detail",
    }
}

/// Writes one drawing operation of a canvas as the tree's JSON.
fn write_draw(tree: &mut String, op: &Draw) -> Result<(), String> {
    let number = |tree: &mut String, name: &str, value: f32| {
        let _ = write!(tree, ",\"{name}\":{value}");
    };
    tree.push_str("{\"op\":");
    string(
        tree,
        match op {
            Draw::Rect { .. } => "rect",
            Draw::Circle { .. } => "circle",
            Draw::Text { .. } => "text",
            Draw::Image { .. } => "image",
            Draw::MoveTo { .. } => "move",
            Draw::LineTo { .. } => "line",
            Draw::QuadTo { .. } => "quad",
            Draw::CubicTo { .. } => "cubic",
            Draw::Arc { .. } => "arc",
            Draw::Close => "close",
            Draw::Fill { .. } => "fill",
            Draw::Stroke { .. } => "stroke",
            Draw::Clip { .. } => "clip",
            Draw::Translate { .. } => "translate",
            Draw::Scale { .. } => "scale",
            Draw::Rotate { .. } => "rotate",
        },
    )?;
    match op {
        Draw::Rect {
            x,
            y,
            width,
            height,
            radius,
            fill,
            stroke,
        } => {
            number(tree, "x", *x);
            number(tree, "y", *y);
            number(tree, "width", *width);
            number(tree, "height", *height);
            if *radius > 0. {
                number(tree, "radius", *radius);
            }
            if let Some(fill) = fill {
                tree.push_str(",\"fill\":");
                write_paint(tree, fill)?;
            }
            if let Some(stroke) = stroke {
                tree.push_str(",\"stroke\":");
                write_paint(tree, &stroke.color)?;
                number(tree, "strokeWidth", stroke.width);
                if let Some(cap) = stroke.cap {
                    tree.push_str(",\"cap\":");
                    string(
                        tree,
                        match cap {
                            Cap::Butt => "butt",
                            Cap::Round => "round",
                            Cap::Square => "square",
                        },
                    )?;
                }
                if let Some(join) = stroke.join {
                    tree.push_str(",\"join\":");
                    string(
                        tree,
                        match join {
                            Join::Miter => "miter",
                            Join::Round => "round",
                            Join::Bevel => "bevel",
                        },
                    )?;
                }
            }
        }
        Draw::Circle {
            x,
            y,
            radius,
            fill,
            stroke,
        } => {
            number(tree, "x", *x);
            number(tree, "y", *y);
            number(tree, "radius", *radius);
            if let Some(fill) = fill {
                tree.push_str(",\"fill\":");
                write_paint(tree, fill)?;
            }
            if let Some(stroke) = stroke {
                tree.push_str(",\"stroke\":");
                write_paint(tree, &stroke.color)?;
                number(tree, "strokeWidth", stroke.width);
                if let Some(cap) = stroke.cap {
                    tree.push_str(",\"cap\":");
                    string(
                        tree,
                        match cap {
                            Cap::Butt => "butt",
                            Cap::Round => "round",
                            Cap::Square => "square",
                        },
                    )?;
                }
                if let Some(join) = stroke.join {
                    tree.push_str(",\"join\":");
                    string(
                        tree,
                        match join {
                            Join::Miter => "miter",
                            Join::Round => "round",
                            Join::Bevel => "bevel",
                        },
                    )?;
                }
            }
        }
        Draw::Text {
            x,
            y,
            content,
            style,
            level,
            color,
            size,
            weight,
        } => {
            number(tree, "x", *x);
            number(tree, "y", *y);
            tree.push_str(",\"text\":");
            string(tree, content)?;
            if let Some(style) = style {
                tree.push_str(",\"style\":");
                string(tree, text_style_name(*style))?;
            }
            if let Some(level) = level {
                tree.push_str(",\"level\":");
                string(tree, text_level_name(*level))?;
            }
            if let Some(color) = color {
                tree.push_str(",\"color\":");
                write_paint(tree, color)?;
            }
            if let Some(size) = size {
                number(tree, "size", *size);
            }
            if let Some(weight) = weight {
                number(tree, "weight", *weight);
            }
        }
        Draw::Image {
            image,
            x,
            y,
            width,
            height,
        } => {
            tree.push_str(",\"image\":");
            crate::icon::write_icon(tree, image);
            number(tree, "x", *x);
            number(tree, "y", *y);
            number(tree, "width", *width);
            number(tree, "height", *height);
        }
        Draw::MoveTo { x, y } => {
            number(tree, "x", *x);
            number(tree, "y", *y);
        }
        Draw::LineTo { x, y } => {
            number(tree, "x", *x);
            number(tree, "y", *y);
        }
        Draw::QuadTo { cx, cy, x, y } => {
            number(tree, "cx", *cx);
            number(tree, "cy", *cy);
            number(tree, "x", *x);
            number(tree, "y", *y);
        }
        Draw::CubicTo {
            c1x,
            c1y,
            c2x,
            c2y,
            x,
            y,
        } => {
            number(tree, "c1x", *c1x);
            number(tree, "c1y", *c1y);
            number(tree, "c2x", *c2x);
            number(tree, "c2y", *c2y);
            number(tree, "x", *x);
            number(tree, "y", *y);
        }
        Draw::Arc {
            x,
            y,
            radius,
            start,
            end,
            ccw,
        } => {
            number(tree, "x", *x);
            number(tree, "y", *y);
            number(tree, "radius", *radius);
            number(tree, "start", *start);
            number(tree, "end", *end);
            if *ccw {
                tree.push_str(",\"ccw\":true");
            }
        }
        Draw::Close => {}
        Draw::Fill { color } => {
            tree.push_str(",\"color\":");
            write_paint(tree, color)?;
        }
        Draw::Stroke { stroke } => {
            tree.push_str(",\"color\":");
            write_paint(tree, &stroke.color)?;
            number(tree, "width", stroke.width);
            if let Some(cap) = stroke.cap {
                tree.push_str(",\"cap\":");
                string(
                    tree,
                    match cap {
                        Cap::Butt => "butt",
                        Cap::Round => "round",
                        Cap::Square => "square",
                    },
                )?;
            }
            if let Some(join) = stroke.join {
                tree.push_str(",\"join\":");
                string(
                    tree,
                    match join {
                        Join::Miter => "miter",
                        Join::Round => "round",
                        Join::Bevel => "bevel",
                    },
                )?;
            }
        }
        Draw::Clip {
            x,
            y,
            width,
            height,
        } => {
            number(tree, "x", *x);
            number(tree, "y", *y);
            number(tree, "width", *width);
            number(tree, "height", *height);
        }
        Draw::Translate { x, y } => {
            number(tree, "x", *x);
            number(tree, "y", *y);
        }
        Draw::Scale { x, y } => {
            number(tree, "x", *x);
            number(tree, "y", *y);
        }
        Draw::Rotate { degrees } => {
            number(tree, "degrees", *degrees);
        }
    }
    tree.push('}');
    Ok(())
}

/// Writes `span` as the tree's JSON.
/// Writes the text node `text`'s drawing properties: the style, level,
/// colour, size, weight and truncation its builder named, as a span's are
/// written (the tree's text node carries them too).
fn write_text_props(tree: &mut String, text: &Text) -> Result<(), String> {
    if let Some(style) = text.style {
        tree.push_str(",\"style\":");
        string(tree, text_style_name(style))?;
    }
    if let Some(level) = text.level {
        tree.push_str(",\"level\":");
        string(tree, text_level_name(level))?;
    }
    if let Some(color) = &text.color {
        tree.push_str(",\"color\":");
        write_paint(tree, color)?;
    }
    if let Some(size) = text.size {
        let _ = write!(tree, ",\"size\":{size}");
    }
    if let Some(weight) = text.weight {
        let _ = write!(tree, ",\"weight\":{weight}");
    }
    if text.truncate {
        tree.push_str(",\"truncate\":true");
    }
    Ok(())
}

fn write_span(tree: &mut String, span: &Span) -> Result<(), String> {
    tree.push_str("{\"text\":");
    string(tree, &span.text)?;
    if let Some(style) = span.style {
        tree.push_str(",\"style\":");
        string(tree, text_style_name(style))?;
    }
    if let Some(level) = span.level {
        tree.push_str(",\"level\":");
        string(tree, text_level_name(level))?;
    }
    if let Some(color) = &span.color {
        tree.push_str(",\"color\":");
        write_paint(tree, color)?;
    }
    if span.code {
        tree.push_str(",\"code\":true");
    }
    if let Some(Listener(id)) = &span.on_click {
        tree.push_str(",\"onPress\":");
        let _ = write!(tree, "{id}");
    }
    tree.push('}');
    Ok(())
}

/// Writes a container's layout properties: `gap`, `padding`, `align`,
/// `justify` and `wrap`.
fn write_layout(tree: &mut String, layout: &Layout) -> Result<(), String> {
    if let Some(gap) = layout.gap {
        tree.push_str(",\"gap\":");
        write_space(tree, gap)?;
    }
    let padding = &layout.padding;
    if padding.top.is_some()
        || padding.right.is_some()
        || padding.bottom.is_some()
        || padding.left.is_some()
    {
        tree.push_str(",\"padding\":{");
        let mut first = true;
        for (name, side) in [
            ("top", padding.top),
            ("right", padding.right),
            ("bottom", padding.bottom),
            ("left", padding.left),
        ] {
            if let Some(space) = side {
                if !first {
                    tree.push(',');
                }
                first = false;
                tree.push_str(name);
                tree.push(':');
                write_space(tree, space)?;
            }
        }
        tree.push('}');
    }
    if let Some(align) = layout.align {
        tree.push_str(",\"align\":");
        string(
            tree,
            match align {
                Align::Start => "start",
                Align::Center => "center",
                Align::End => "end",
                Align::Stretch => "stretch",
                Align::Baseline => "baseline",
            },
        )?;
    }
    if let Some(justify) = layout.justify {
        tree.push_str(",\"justify\":");
        string(
            tree,
            match justify {
                Justify::Start => "start",
                Justify::Center => "center",
                Justify::End => "end",
                Justify::SpaceBetween => "space-between",
                Justify::SpaceAround => "space-around",
            },
        )?;
    }
    if layout.wrap {
        tree.push_str(",\"wrap\":true");
    }
    Ok(())
}

/// Writes a node's style: its sizing, its surface, and its hover and
/// pressed variants.
fn write_style(tree: &mut String, style: &Style) -> Result<(), String> {
    let sizing = &style.sizing;
    if let Some(grow) = sizing.grow {
        let _ = write!(tree, ",\"grow\":{grow}");
    }
    if let Some(shrink) = sizing.shrink {
        let _ = write!(tree, ",\"shrink\":{shrink}");
    }
    if let Some(basis) = sizing.basis {
        tree.push_str(",\"basis\":");
        write_length(tree, basis)?;
    }
    for (name, length) in [
        ("width", sizing.width),
        ("height", sizing.height),
        ("minWidth", sizing.min_width),
        ("maxWidth", sizing.max_width),
        ("minHeight", sizing.min_height),
        ("maxHeight", sizing.max_height),
    ] {
        if let Some(length) = length {
            tree.push_str(",\"");
            tree.push_str(name);
            tree.push_str("\":");
            write_length(tree, length)?;
        }
    }
    if let Some(ratio) = sizing.aspect_ratio {
        let _ = write!(tree, ",\"aspectRatio\":{ratio}");
    }
    write_surface(tree, &style.surface)?;
    if let Some(hover) = &style.hover {
        tree.push_str(",\"hover\":");
        write_surface_object(tree, hover)?;
    }
    if let Some(pressed) = &style.pressed {
        tree.push_str(",\"pressed\":");
        write_surface_object(tree, pressed)?;
    }
    Ok(())
}

/// Writes a node's surface properties, each named.
fn write_surface(tree: &mut String, surface: &Surface) -> Result<(), String> {
    if let Some(background) = &surface.background {
        tree.push_str(",\"background\":");
        write_paint(tree, background)?;
    }
    if let Some(border) = &surface.border {
        let mut bare = String::new();
        if let Some(width) = border.width {
            bare.push_str("\"width\":");
            write_length(&mut bare, width)?;
        }
        if let Some(color) = &border.color {
            if !bare.is_empty() {
                bare.push(',');
            }
            bare.push_str("\"color\":");
            write_paint(&mut bare, color)?;
        }
        tree.push_str(",\"border\":{");
        tree.push_str(&bare);
        tree.push('}');
    }
    if let Some(radius) = surface.radius {
        tree.push_str(",\"radius\":");
        write_radius(tree, radius)?;
    }
    if let Some(opacity) = surface.opacity {
        let _ = write!(tree, ",\"opacity\":{opacity}");
    }
    Ok(())
}

/// Writes a variant's surface properties as an object of them.
fn write_surface_object(tree: &mut String, surface: &Surface) -> Result<(), String> {
    let mut bare = String::new();
    write_surface(&mut bare, surface)?;
    // The properties were written with their leading commas; drop the
    // first to make the object's own.
    let bare = bare.trim_start_matches(',');
    tree.push('{');
    tree.push_str(bare);
    tree.push('}');
    Ok(())
}

/// Writes `length` as the tree's JSON.
fn write_length(tree: &mut String, length: Length) -> Result<(), String> {
    match length {
        Length::Token(space) => write_space(tree, space),
        Length::Px(pixels) => {
            let _ = write!(tree, "{pixels}");
            Ok(())
        }
        Length::Fraction(fraction) => {
            let _ = write!(tree, "{fraction}");
            Ok(())
        }
    }
}

/// Writes `radius` as the tree's JSON.
fn write_radius(tree: &mut String, radius: Radius) -> Result<(), String> {
    match radius {
        Radius::S => string(tree, "s"),
        Radius::M => string(tree, "m"),
        Radius::L => string(tree, "l"),
        Radius::Full => string(tree, "full"),
        Radius::Px(pixels) => {
            let _ = write!(tree, "{pixels}");
            Ok(())
        }
    }
}

/// Writes `size` as the tree's JSON.
fn write_size(tree: &mut String, size: IconSize) -> Result<(), String> {
    match size {
        IconSize::S => string(tree, "s"),
        IconSize::M => string(tree, "m"),
        IconSize::L => string(tree, "l"),
        IconSize::Xl => string(tree, "xl"),
        IconSize::Px(pixels) => {
            let _ = write!(tree, "{pixels}");
            Ok(())
        }
    }
}

/// Writes `paint` as the tree's JSON: a colour, a pair, or an exact one.
fn write_paint(tree: &mut String, paint: &Paint) -> Result<(), String> {
    let color = |tree: &mut String, color: &Color| match color {
        Color::Tone(tone) => string(tree, tone.name()),
        Color::Raw(raw) => string(tree, raw),
    };
    match paint {
        Paint::Color(held) => color(tree, held),
        Paint::Pair { light, dark } => {
            tree.push_str("{\"light\":");
            color(tree, light)?;
            tree.push_str(",\"dark\":");
            color(tree, dark)?;
            tree.push('}');
            Ok(())
        }
        Paint::Exact(exact) => {
            tree.push_str("{\"raw\":");
            color(tree, exact)?;
            tree.push('}');
            Ok(())
        }
        Paint::ExactPair { light, dark } => {
            tree.push_str("{\"raw\":{\"light\":");
            color(tree, light)?;
            tree.push_str(",\"dark\":");
            color(tree, dark)?;
            tree.push_str("}}");
            Ok(())
        }
    }
}

/// Where `place` is in a stack, as the tree names it.
fn place_name(place: Place) -> &'static str {
    match place {
        Place::TopStart => "top-start",
        Place::Top => "top",
        Place::TopEnd => "top-end",
        Place::Start => "start",
        Place::Center => "center",
        Place::End => "end",
        Place::BottomStart => "bottom-start",
        Place::Bottom => "bottom",
        Place::BottomEnd => "bottom-end",
    }
}

/// Writes `space` as the tree's token name.
fn write_space(tree: &mut String, space: Space) -> Result<(), String> {
    string(
        tree,
        match space {
            Space::Xs => "xs",
            Space::S => "s",
            Space::M => "m",
            Space::L => "l",
            Space::Xl => "xl",
            Space::Xxl => "xxl",
        },
    )
}

/// The name of a text style in the tree.
fn text_style_name(style: TextStyle) -> &'static str {
    match style {
        TextStyle::Heading => "heading",
        TextStyle::Title => "title",
        TextStyle::Body => "body",
        TextStyle::Caption => "caption",
        TextStyle::Mono => "mono",
        TextStyle::SmallMono => "small-mono",
    }
}

/// The name of a text level in the tree.
fn text_level_name(level: TextLevel) -> &'static str {
    match level {
        TextLevel::Primary => "primary",
        TextLevel::Secondary => "secondary",
        TextLevel::Tertiary => "tertiary",
        TextLevel::Quaternary => "quaternary",
    }
}

/// Writes `"name":"value"` into `tree`.
fn string_field(tree: &mut String, name: &str, value: &str) -> Result<(), String> {
    tree.push('"');
    tree.push_str(name);
    tree.push_str("\":");
    string(tree, value)
}

/// Writes `text` as the tree's JSON string.
/// Writes what a field of a form says around its control: its `title`,
/// `info`, `error` and `remember` (#241).
fn write_field(tree: &mut String, field: &FieldPayload) -> Result<(), String> {
    if let Some(title) = &field.title {
        tree.push_str(",\"title\":");
        string(tree, title)?;
    }
    if let Some(info) = &field.info {
        tree.push_str(",\"info\":");
        string(tree, info)?;
    }
    if let Some(error) = &field.error {
        tree.push_str(",\"error\":");
        string(tree, error)?;
    }
    if field.remember {
        tree.push_str(",\"remember\":true");
    }
    Ok(())
}

fn string(tree: &mut String, text: &str) -> Result<(), String> {
    tree.push('"');
    for character in text.chars() {
        match character {
            '"' => tree.push_str("\\\""),
            '\\' => tree.push_str("\\\\"),
            '\n' => tree.push_str("\\n"),
            '\r' => tree.push_str("\\r"),
            '\t' => tree.push_str("\\t"),
            other if (other as u32) < 0x20 => {
                write!(tree, "\\u{:04x}", other as u32)
                    .map_err(|_| "the tree is not writable".to_owned())?;
            }
            other => tree.push(other),
        }
    }
    tree.push('"');
    Ok(())
}
