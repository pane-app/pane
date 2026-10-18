//! What a designed view renders, as its `view.render` answers it: a JSON
//! document naming the version of the UI component set it uses, and a root
//! node (`docs/designed-tree.md`, ADR 0036's envelope for the tree #121
//! introduces). Reading it here keeps the JSON at the edge of the runtime:
//! the launcher sees only the typed [`DesignedTree`], and the window draws
//! from it without meeting JSON at all.
//!
//! Reading is strict in the document's shape and lenient exactly where
//! versioning asks for it. A property Pane does not know is ignored; a
//! node whose type Pane does not know, or whose `requires` it does not
//! meet, draws the `fallback` the author gave, else its children; a
//! document of Pane's major version and any minor is read for what Pane
//! understands of it. A document that is not JSON, lacks a field Pane
//! needs (`version`, `root`, a node's `type`, a text's `text`, a button's
//! `label`) or gives one of the wrong type is unreadable, which the runtime
//! answers as [`super::CallError::Unreadable`]: the command's failure,
//! never a crash, so it does not count towards pausing the package. A
//! document over one of the limits ([`MAX_NODES`], [`MAX_DEPTH`],
//! [`MAX_TREE_BYTES`], [`MAX_TEXT_CHARS`], [`MAX_MARKDOWN_CHARS`],
//! [`MAX_INLINE_IMAGE`]), or of another major version, is the extension's
//! error ([`ReadError::Guest`]): the view keeps its last good tree either
//! way.
//!
//! The component set of this Pane (1.1) adds the layout primitives
//! (`stack`, `scroll`, `spacer`, `divider`), the sizing and surface every
//! node may carry with their `hover` and `pressed` variants, the shared
//! UI components, the tone token's semantic names, raw colours in every
//! form and Markdown (#237); 1.2 adds the keyed state the reconciler
//! keeps — text fields that edit, the select's searchable state, scroll
//! by key — the inputs' partial control (`onInput`, `throttleMs`, the
//! `focus` ask) and the focus, blur and key events (#238). 1.3 adds the
//! standard views built on the tree (#240): the List with its sections,
//! item keywords, host filtering, controlled search text and selection,
//! the search-bar dropdown, pagination, the empty view and the detail
//! pane; the Grid; the Detail, and Markdown images; the canvas, a leaf
//! the extension draws into (#242, the custom view's successor); and the
//! form — a `form` node whose submission is an action, the field
//! components' titles, notes, errors and remembered values, and the
//! date, tag, file and folder pickers (#241).

use serde::Deserialize;
use serde_json::{Map, Value};

use crate::icons::{self, Icon, Tint};
use crate::markdown;
use crate::runtime::tree::Accessory;
use crate::tokens::{IconSize, Radius, Space, TextLevel, TextStyle};

/// The version of the UI component set this Pane renders: major 1, minor 3.
/// A document of this major and any minor is read (unknown fields and nodes
/// degrading); a document of another major is refused naming both versions.
pub const COMPONENT_SET: (u64, u64) = (1, 3);

/// Which handler of a node an event raises — the property of the node the
/// event names, which the stale-event rule checks before delivering: an
/// event raised on a node the user could see is delivered even if the
/// extension has rendered since, while the node with that key still has a
/// handler of that kind; otherwise it is dropped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DesignedHandler {
    /// `onPress` — a button, a link, a row or a span pressed.
    Press,
    /// `onChange` — a value committed (a field's, a control's).
    Change,
    /// `onInput` — a field's value as the user types it.
    Input,
    /// `onSubmit` — a form submitted (its fields' values with it).
    Submit,
    /// `onFocus` — a node taking the keyboard.
    Focus,
    /// `onBlur` — a node losing it.
    Blur,
    /// `onKey` — a key pressed while the node is focused.
    Key,
    /// `onSelectionChange` — a List's or Grid's selection moving to
    /// another of its items.
    Selection,
    /// `onLoadMore` — a List's or Grid's selection nearing the end of
    /// what it shows, while the tree says more is there.
    More,
    /// One of a canvas's pointer handlers (`onPointerDown` and its kind).
    Pointer,
    /// `onWheel` — a canvas's wheel.
    Wheel,
    /// `onDoubleClick` — a canvas's double click.
    DoubleClick,
    /// `onSecondary` — a canvas's secondary button.
    Secondary,
    /// `onResize` — a canvas changing size.
    Resize,
}

/// The most nodes one document may hold, counting `fallback` subtrees.
pub const MAX_NODES: usize = 10_000;
/// How deep one document may be, counting its root.
pub const MAX_DEPTH: usize = 64;
/// The most bytes of JSON one document may be.
pub const MAX_TREE_BYTES: usize = 4 * 1024 * 1024;
/// The most characters one text node may hold.
pub const MAX_TEXT_CHARS: usize = 64 * 1024;
/// The most characters one canvas text operation's text may hold.
pub const MAX_CANVAS_TEXT_CHARS: usize = 64 * 1024;
/// The most drawing operations one canvas may hold.
pub const MAX_CANVAS_OPS: usize = 20_000;
/// The most characters one markdown node's source may hold.
pub const MAX_MARKDOWN_CHARS: usize = 1024 * 1024;
/// The most bytes of inline image data one icon's `data:` URL may hold.
pub const MAX_INLINE_IMAGE: usize = 1024 * 1024;
/// The most pixels a raw length may name, and the least.
pub const MAX_PX: f32 = 4096.;

/**
 * A designed view's tree: its root node. The version it named is checked
 * while reading; a tree that was read is of a version Pane renders.
 */
#[derive(Clone, Debug, PartialEq)]
pub struct DesignedTree {
    pub root: Node,
}

impl DesignedTree {
    /// The view's navigation title: the name its root node carries, shown
    /// where a screen's title is (the footer names it as it names a custom
    /// view's; the search header, once the List's search field owns it,
    /// #240). `None` when the tree names none.
    pub fn navigation_title(&self) -> Option<&str> {
        self.root.navigation_title.as_deref()
    }
}

impl Node {
    /// Whether this node is one whose state Pane keeps by key: a field, a
    /// select, a scroll region, a canvas, or a control with a handler. A
    /// tree that names no key for such a node is reported in development
    /// (its state is kept by position instead).
    pub fn stateful(&self) -> bool {
        self.handles(DesignedHandler::Press)
            || self.handles(DesignedHandler::Change)
            || self.handles(DesignedHandler::Input)
            || self.handles(DesignedHandler::Submit)
            || self.handles(DesignedHandler::Focus)
            || self.handles(DesignedHandler::Blur)
            || self.handles(DesignedHandler::Key)
            || self.handles(DesignedHandler::Selection)
            || self.handles(DesignedHandler::More)
            || self.handles(DesignedHandler::Pointer)
            || self.handles(DesignedHandler::Wheel)
            || self.handles(DesignedHandler::DoubleClick)
            || self.handles(DesignedHandler::Secondary)
            || self.handles(DesignedHandler::Resize)
            || matches!(
                &self.kind,
                NodeKind::TextInput(_)
                    | NodeKind::PasswordInput(_)
                    | NodeKind::TextArea(_)
                    | NodeKind::Select(_)
                    | NodeKind::Toggle(_)
                    | NodeKind::Checkbox(_)
                    | NodeKind::Segmented(_)
                    | NodeKind::Slider(_)
                    | NodeKind::DatePicker(_)
                    | NodeKind::DateTimePicker(_)
                    | NodeKind::TagPicker(_)
                    | NodeKind::FilePicker(_)
                    | NodeKind::FolderPicker(_)
                    | NodeKind::Scroll { .. }
                    | NodeKind::List(_)
                    | NodeKind::Grid(_)
                    | NodeKind::Canvas(_)
            )
    }

    /// Whether this node names a handler of `kind` — the stale-event
    /// rule's check: an event is delivered while the node with its key
    /// still has a handler for it. A text's spans and a metadata list's
    /// rows name handlers of the node they belong to.
    pub fn handles(&self, kind: DesignedHandler) -> bool {
        let held = |id: Option<u32>| id.is_some();
        match kind {
            DesignedHandler::Press => match &self.kind {
                NodeKind::Button(button) => held(button.on_press),
                NodeKind::Link(link) => held(link.on_press),
                NodeKind::RichRow(row) => held(row.on_press),
                NodeKind::ListItem(item) => held(item.on_press) || !item.actions.is_empty(),
                NodeKind::GridItem(item) => held(item.on_press),
                NodeKind::Text(text) => match &text.content {
                    TextContent::Plain(_) => false,
                    TextContent::Spans(spans) => spans.iter().any(|span| held(span.on_press)),
                },
                NodeKind::MetadataList(list) => list.items.iter().any(|item| held(item.on_press)),
                // The canvas's semantic handlers (the up and down arrows,
                // Space) are presses, as a button's is.
                NodeKind::Canvas(canvas) => {
                    held(canvas.handlers.on_increment)
                        || held(canvas.handlers.on_decrement)
                        || held(canvas.handlers.on_activate)
                }
                _ => false,
            },
            DesignedHandler::Change => match &self.kind {
                NodeKind::Toggle(toggle) => held(toggle.on_change),
                NodeKind::Checkbox(checkbox) => held(checkbox.on_change),
                NodeKind::Segmented(control) => held(control.on_change),
                NodeKind::Select(control) => held(control.on_change),
                NodeKind::Slider(slider) => held(slider.on_change),
                NodeKind::TextInput(input) => held(input.on_change),
                NodeKind::ListDropdown(dropdown) => held(dropdown.on_change),
                NodeKind::TagPicker(picker) => held(picker.on_change),
                _ => false,
            },
            DesignedHandler::Input => match &self.kind {
                NodeKind::TextInput(input) => held(input.on_input),
                NodeKind::Select(select) => held(select.on_input),
                NodeKind::List(list) => held(list.on_search_text),
                NodeKind::Grid(list) => held(list.on_search_text),
                _ => false,
            },
            DesignedHandler::Selection => match &self.kind {
                NodeKind::List(list) => held(list.on_selection_change),
                NodeKind::Grid(list) => held(list.on_selection_change),
                _ => false,
            },
            DesignedHandler::More => match &self.kind {
                NodeKind::List(list) => held(list.on_load_more),
                NodeKind::Grid(list) => held(list.on_load_more),
                _ => false,
            },
            DesignedHandler::Submit => match &self.kind {
                NodeKind::Form(form) => held(form.on_submit),
                _ => false,
            },
            DesignedHandler::Focus => held(self.on_focus),
            DesignedHandler::Blur => held(self.on_blur),
            DesignedHandler::Key => held(self.on_key),
            DesignedHandler::Pointer => match &self.kind {
                NodeKind::Canvas(canvas) => {
                    held(canvas.handlers.on_pointer_down)
                        || held(canvas.handlers.on_pointer_up)
                        || held(canvas.handlers.on_pointer_move)
                        || held(canvas.handlers.on_pointer_enter)
                        || held(canvas.handlers.on_pointer_leave)
                }
                _ => false,
            },
            DesignedHandler::Wheel => match &self.kind {
                NodeKind::Canvas(canvas) => held(canvas.handlers.on_wheel),
                _ => false,
            },
            DesignedHandler::DoubleClick => match &self.kind {
                NodeKind::Canvas(canvas) => held(canvas.handlers.on_double_click),
                _ => false,
            },
            DesignedHandler::Secondary => match &self.kind {
                NodeKind::Canvas(canvas) => held(canvas.handlers.on_secondary),
                _ => false,
            },
            DesignedHandler::Resize => match &self.kind {
                NodeKind::Canvas(canvas) => held(canvas.handlers.on_resize),
                _ => false,
            },
        }
    }
}

/// One node of a designed view's tree: a layout primitive or UI component,
/// or a type this Pane does not know (drawn by its `fallback`, else its
/// children). Every node may carry a `key` (the stable identity Pane keeps
/// node state under, #238), a `name` assistive technology reads it by, a
/// [`Style`] every node shares; a child of a `stack` may also say where in
/// it it is placed; the root node's `navigation-title` names the view
/// itself. A focusable node may ask for the keyboard (`focus`) and name
/// handlers for the events Pane raises on it (`on-focus`, `on-blur`,
/// `on-key`).
#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub kind: NodeKind,
    /// The sizing and surface every node may carry, with their `hover` and
    /// `pressed` variants.
    pub style: Style,
    /// Where in a `stack` this node is placed; `None` for the stack's own
    /// placement. Nowhere else is it read.
    pub place: Option<Place>,
    /// How far this node sits from its place in a `stack`, in its
    /// placement's directions.
    pub offset: Option<Offset>,
    /// The node's key, unique among its siblings; `None` for a static node.
    pub key: Option<String>,
    /// The name assistive technology reads the node by, when the node's own
    /// content does not name it (a column's or row's).
    pub name: Option<String>,
    /// The view's navigation title, read from the root node only: what
    /// names the view where a screen's title is. `None` on the others,
    /// which Pane ignores.
    pub navigation_title: Option<String>,
    /// The minimum minor version of the UI component set this node needs.
    pub requires: Option<u64>,
    /// Drawn instead of this node when Pane does not know it.
    pub fallback: Option<Box<Node>>,
    pub children: Vec<Node>,
    /// Whether the node asks for the keyboard: a node whose ask is new
    /// (the tree the user saw did not name it) is focused, as an
    /// auto-focus on open and a code-driven re-focus both read. Honoured
    /// while the node is focusable.
    pub focus: bool,
    /// The callback a focus of this node runs (it taking the keyboard).
    pub on_focus: Option<u32>,
    /// The callback a blur of this node runs (it losing the keyboard).
    pub on_blur: Option<u32>,
    /// The callback a key pressed while this node is focused runs (Tab,
    /// Enter and Escape stay with Pane).
    pub on_key: Option<u32>,
}

/// What a node is.
#[derive(Clone, Debug, PartialEq)]
pub enum NodeKind {
    Column(Layout),
    Row(Layout),
    /// Children drawn over each other, each placed by its [`Node::place`].
    Stack(Place),
    /// A scrolling region, whose position Pane keeps by key.
    Scroll {
        orientation: Orientation,
    },
    /// Space: it grows to fill what it is given.
    Spacer,
    /// A hairline rule.
    Divider {
        orientation: Orientation,
    },
    Text(Text),
    Button(Button),
    Link(Link),
    /// One icon, by name, file, URL or system reference.
    Icon(IconNode),
    /// An icon on Pane's tile.
    IconTile(IconNode),
    /// An image, with its fit and its placeholder while it loads.
    Image(Image),
    /// A row the launcher's own results are: an icon, a title, a subtitle
    /// and accessories.
    RichRow(RichRow),
    Keycap(Keycap),
    /// Keycaps side by side, one per key.
    KeySequence(KeySequence),
    Tag(Tag),
    Badge(Badge),
    Toggle(Toggle),
    Checkbox(Checkbox),
    /// A choice of segments, one of them chosen.
    Segmented(Segmented),
    Slider(Slider),
    Progress(Progress),
    /// A loading indicator, indeterminate.
    Loading(Loading),
    Markdown(Markdown),
    /// A card of children, on Pane's own card surface.
    Card(Layout),
    SectionHeader(SectionHeader),
    MetadataList(MetadataList),
    EmptyState(EmptyState),
    TextInput(TextInput),
    PasswordInput(TextInput),
    TextArea(TextInput),
    Select(Select),
    /// A standard List (#240): the view's items in sections, its search
    /// field and selection Pane's, its rows the launcher's own.
    List(ListNode),
    /// A standard Grid (#240): a List's behaviour with cells of images,
    /// colours, icons or subtrees.
    Grid(ListNode),
    /// A section of a List's or Grid's items, under its title.
    ListSection(ListSection),
    /// One item of a List, carrying the action that activates it and the
    /// detail pane's content when it is selected.
    ListItem(ListItem),
    /// One cell of a Grid: an image, a colour or the author's own subtree.
    GridItem(GridItem),
    /// A List's search-bar dropdown, beside its search field.
    ListDropdown(ListDropdown),
    /// A Detail (#240): a scrolling column of what a record or an article
    /// is — Markdown, a metadata panel, a loading state, actions.
    Detail(Layout),
    /// A canvas: a leaf the extension draws into with drawing operations,
    /// taking input and reporting its size (#242, the custom view's
    /// successor). Its size comes from its style's sizing as any node's
    /// does — a fixed `width` and `height`, or the space the layout gives
    /// it — and the render context names it.
    Canvas(Canvas),
    /// A form: a column of fields whose submission is an action (#241).
    /// Its children are the author's own layout; every field component
    /// in its subtree is one of its fields, whose values a submission
    /// carries.
    Form(FormNode),
    /// One date field, typed or stepped with the arrow keys: its value
    /// "YYYY-MM-DD".
    DatePicker(DateField),
    /// One date and time field, typed or stepped with the arrow keys:
    /// its value "YYYY-MM-DD HH:MM".
    DateTimePicker(DateField),
    /// One tag picker: a multi-select of its options, its chosen tags as
    /// chips.
    TagPicker(TagPicker),
    /// One file picker: a path typed or chosen with the system's dialog.
    FilePicker(FilePicker),
    /// One folder picker: a path typed or chosen with the system's
    /// dialog.
    FolderPicker(FilePicker),
    /// A node whose type Pane does not know, or whose `requires` it does
    /// not meet: its `fallback` and children decide what is drawn.
    Unknown(String),
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

/// What a `column`, `row` or `card` says about its layout.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Layout {
    /// The gap between its children.
    pub gap: Option<Space>,
    /// The padding inside it.
    pub padding: Padding,
    /// How children are laid out along the cross axis.
    pub align: Option<Align>,
    /// How children share the main axis.
    pub justify: Option<Justify>,
    /// Whether children wrap onto further lines.
    pub wrap: bool,
}

/// The padding inside a node, per side: `{"x": "m", "y": "s"}` names the
/// sides, `{"top": ..., "right": ..., "bottom": ..., "left": ...}` each
/// one, and a bare token all four.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Padding {
    pub top: Option<Space>,
    pub right: Option<Space>,
    pub bottom: Option<Space>,
    pub left: Option<Space>,
}

/// One of the nine places a `stack`'s child may be put.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Place {
    #[default]
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

/// How far a `stack`'s child sits from its place.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Offset {
    pub x: Length,
    pub y: Length,
}

/// Which way a `scroll` scrolls, or a `divider` runs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Orientation {
    #[default]
    Vertical,
    Horizontal,
}

/// One text node: what it says, plain or in spans, and how it is drawn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Text {
    pub content: TextContent,
    pub style: Option<TextStyle>,
    pub level: Option<TextLevel>,
    /// The colour it is drawn in, a tone or a raw colour.
    pub color: Option<Paint>,
    /// Its size in pixels, when the styles do not say what it needs.
    pub size: Option<Finite>,
    /// Its weight, 100 to 900.
    pub weight: Option<Finite>,
    /// Whether it is drawn on one line with an ellipsis.
    pub truncate: bool,
}

/// What a text says: one string, or runs of styled and linked spans.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextContent {
    Plain(String),
    Spans(Vec<Span>),
}

/// One span of a text: a run of its content, styled, and a link when it
/// carries `onPress`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    pub style: Option<TextStyle>,
    pub level: Option<TextLevel>,
    pub color: Option<Paint>,
    /// Whether it is drawn in the mono family, as code is.
    pub code: bool,
    /// The callback id a press of the link runs.
    pub on_press: Option<u32>,
}

/// One button: its label, its tone, the icon and keycaps it carries, and
/// the callback a `press` of it runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Button {
    pub label: String,
    pub tone: Option<Tone>,
    /// The icon drawn before its label.
    pub icon: Option<Icon>,
    /// The keys drawn after its label, one keycap each.
    pub keys: Option<Vec<String>>,
    /// The callback id the tree named; `None` for a button that cannot be
    /// pressed.
    pub on_press: Option<u32>,
    /// Whether it can be pressed.
    pub enabled: bool,
}

impl Default for Button {
    fn default() -> Button {
        Button {
            label: String::new(),
            tone: None,
            icon: None,
            keys: None,
            on_press: None,
            enabled: true,
        }
    }
}

/// One link: its label and the callback a `press` of it runs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Link {
    pub label: String,
    pub on_press: Option<u32>,
    pub color: Option<Paint>,
}

/// One icon or image node: the icon it draws, and how big.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IconNode {
    /// The icon, read leniently: one Pane cannot read is not drawn.
    pub icon: Option<Icon>,
    pub size: Option<IconExtent>,
}

/// One image node: the image it draws, how it fits its box, and its
/// placeholder (its children) while it loads or cannot be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    pub image: Option<Icon>,
    pub size: Option<IconExtent>,
    pub fit: Fit,
}

/// How an image fits the box it is given.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Fit {
    #[default]
    Contain,
    Cover,
    Fill,
}

/// How big an icon or an image is: a token, or pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconExtent {
    Token(IconSize),
    Px(Finite),
}

/// A rich row: the launcher's own result row as a component.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RichRow {
    pub title: String,
    pub subtitle: Option<String>,
    pub icon: Option<Icon>,
    pub accessories: Vec<RowAccessory>,
    pub on_press: Option<u32>,
}

/// One accessory at a rich row's end.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RowAccessory {
    pub text: String,
    /// Whether it is drawn as a tag.
    pub tag: bool,
    pub color: Option<Paint>,
}

/// One keycap: the key its cap shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Keycap {
    pub key: String,
}

/// A key sequence: the keys its caps show, in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeySequence {
    pub keys: Vec<String>,
}

/// One tag: a short label in a chip.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tag {
    pub text: String,
    pub color: Option<Paint>,
}

/// One badge: a short count or state in a filled chip.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Badge {
    pub text: String,
    pub color: Option<Paint>,
}

/// One toggle: on or off, and the callback a change of it runs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Toggle {
    pub on: bool,
    pub on_change: Option<u32>,
    /// Its label, drawn beside it and naming it to assistive technology.
    pub label: Option<String>,
    /// What it says around the control as a form field: its title, note,
    /// error and remembered value (#241).
    pub field: FieldProps,
}

/// One checkbox: checked or not, and the callback a change of it runs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Checkbox {
    pub checked: bool,
    pub on_change: Option<u32>,
    /// Its label, drawn beside it and naming it to assistive technology.
    pub label: Option<String>,
    /// What it says around the control as a form field: its title, note,
    /// error and remembered value (#241).
    pub field: FieldProps,
}

/// A segmented control: its segments and the one chosen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Segmented {
    pub options: Vec<Segment>,
    pub value: Option<String>,
    pub on_change: Option<u32>,
    /// Its label, naming it to assistive technology.
    pub label: Option<String>,
}

/// A select: its options and the one chosen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Select {
    pub options: Vec<Segment>,
    pub value: Option<String>,
    pub on_change: Option<u32>,
    /// The callback the popup's query runs as the user types it, when the
    /// tree asks for it — a dropdown whose search the extension handles,
    /// answering options that match (#241).
    pub on_input: Option<u32>,
    /// Whether the popup filters its choices as the user types: `true`,
    /// the default, or `false` for a dropdown whose search the extension
    /// handles.
    pub search: bool,
    /// Shown while no choice is committed.
    pub placeholder: Option<String>,
    /// Its label, naming it to assistive technology.
    pub label: Option<String>,
    /// What it says around the control as a form field: its title, note,
    /// error and remembered value (#241).
    pub field: FieldProps,
}

/// One option of a segmented control or a select.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Segment {
    pub value: String,
    /// What is drawn for it; its `value` when the tree gives none.
    pub label: Option<String>,
    /// The section the option belongs to, drawn as a group's header; the
    /// options of one section are drawn together under it (#241).
    pub section: Option<String>,
}

impl Default for Select {
    /// The select a tree gives no properties: it offers nothing, and its
    /// popup searches as one always does.
    fn default() -> Select {
        Select {
            options: Vec::new(),
            value: None,
            on_change: None,
            on_input: None,
            search: true,
            placeholder: None,
            label: None,
            field: FieldProps::default(),
        }
    }
}

/// One slider: its value between its bounds, and the callback a change of
/// it runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Slider {
    pub value: Finite,
    pub min: Finite,
    pub max: Finite,
    pub step: Finite,
    pub on_change: Option<u32>,
    /// Its label, naming it to assistive technology.
    pub label: Option<String>,
}

/// One progress bar: how far along it is, 0 to 1.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Progress {
    pub value: Finite,
    /// Its label, naming it to assistive technology.
    pub label: Option<String>,
}

/// One loading indicator, indeterminate.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Loading {
    pub label: Option<String>,
}

/// One markdown node: its source, parsed into blocks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Markdown {
    pub blocks: Vec<markdown::Block>,
}

/// A section header: a title over a group, with an optional note.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SectionHeader {
    pub title: String,
    pub note: Option<String>,
}

/// A metadata list: rows of a label and its value, link or tags.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MetadataList {
    pub items: Vec<MetadataItem>,
}

/// One row of a metadata list.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MetadataItem {
    pub label: Option<String>,
    pub value: Option<String>,
    /// The callback a `press` of the value runs, making it a link.
    pub on_press: Option<u32>,
    /// Tag chips drawn as the row's value.
    pub tags: Vec<String>,
    /// Whether the row is a separator alone.
    pub separator: bool,
}

/// An empty state: an icon, a title and a description.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmptyState {
    pub title: String,
    pub description: Option<String>,
    pub icon: Option<Icon>,
}

/// One text input, password field or text area, as the tree draws it: its
/// value and placeholder, and the events it asks for. The field edits at
/// once (#238): the value the tree names is the value the field starts
/// from and the one an echo of it never fights, a commit (`on-change`)
/// tells the extension what it holds, and `on-input` — with an optional
/// `throttle-ms` — hears it as the user types. A field of a form also
/// carries what every field does (#241): its `title`, the note under it,
/// the error the extension's last answer set, the value it starts from
/// when the tree names none (`default`), and whether its last submitted
/// value is remembered.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TextInput {
    pub value: String,
    pub placeholder: Option<String>,
    /// The callback a commit of the field runs (Enter, a blur).
    pub on_change: Option<u32>,
    /// The callback the field's value runs as the user types it, only when
    /// the tree asks for it.
    pub on_input: Option<u32>,
    /// The least time between the field's input events, when it asks for
    /// one: the events are coalesced to the latest while one is in flight
    /// however.
    pub throttle_ms: Option<u64>,
    /// Its label, naming it to assistive technology.
    pub label: Option<String>,
    /// What it says around the control as a form field (#241).
    pub field: FieldProps,
}

/// What every field component says around its control (#241): its title,
/// the note under it, the error the extension's last answer set, and
/// whether its last submitted value is kept as the package's settings and
/// prefilled the next time the field appears.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FieldProps {
    /// The field's title, drawn over its control; also names it to
    /// assistive technology when the field's own label does not.
    pub title: Option<String>,
    /// The note drawn under the field.
    pub info: Option<String>,
    /// The error drawn under the field, in the danger tone: what the
    /// extension's last answer set, as its next render does.
    pub error: Option<String>,
    /// Whether the field's last submitted value is kept as the package's
    /// settings and prefilled the next time the field's key appears.
    pub remember: bool,
}

/// One field's value in a form's submission (#241), as the window
/// collects it from the field's own state: the text a field edits, the
/// state a checkbox or toggle is in, the tags or paths a picker chose.
#[derive(Clone, Debug, PartialEq)]
pub enum FormValue {
    /// A text field's, password field's, text area's, date field's,
    /// dropdown's or single-path picker's value.
    Text(String),
    /// A checkbox's or toggle's state.
    On(bool),
    /// A tag picker's chosen tags, or a picker-of-many's chosen paths.
    List(Vec<String>),
}

impl FormValue {
    /// The value as a form Pane itself asks reads it: its text, a
    /// checkbox's or toggle's state as `true` or `false`, a list joined
    /// by commas.
    pub fn as_text(&self) -> String {
        match self {
            FormValue::Text(text) => text.clone(),
            FormValue::On(on) => on.to_string(),
            FormValue::List(values) => values.join(","),
        }
    }
}

/// One form: a column of fields whose submission is an action (#241). Its
/// children are the author's own layout, and every field component in its
/// subtree is one of its fields: a submission collects their values and
/// runs `on-submit` with them, and Enter in a single-line field of the
/// form submits it, as Ctrl+Enter does in a text area (whose Enter
/// inserts a newline). A form with no `on-submit` is one Pane itself
/// answers: the window collects its values and hands them to the
/// launcher (Pane's own argument, Setup, alias, npm and Git forms).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FormNode {
    /// The callback a submission runs, with every field's value.
    pub on_submit: Option<u32>,
    /// The submit button's label; "Submit" when the tree names none.
    pub submit_label: Option<String>,
}

/// One date field (`date-picker`) or date and time field
/// (`date-time-picker`): its value typed or stepped with the arrow keys,
/// the part the caret is in — "YYYY-MM-DD" or "YYYY-MM-DD HH:MM".
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DateField {
    /// The field's text, which parses as a date or date and time or does
    /// not; the extension validates it, as a text field's value.
    pub value: String,
    pub field: FieldProps,
}

/// One tag picker: a multi-select of its options, the chosen tags drawn
/// as chips. Enter adds the highlighted option; Backspace over an empty
/// query removes the last chip.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TagPicker {
    /// The tags chosen, by their options' values.
    pub tags: Vec<String>,
    /// The tags offered.
    pub options: Vec<Segment>,
    /// The callback a change of the chosen tags runs, told them all.
    pub on_change: Option<u32>,
    pub field: FieldProps,
}

/// One file or folder picker: a path typed or chosen with the system's
/// dialog (its "Choose…" opens it), one path or several.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FilePicker {
    /// The paths chosen, one unless the picker allows many.
    pub paths: Vec<String>,
    /// Whether the picker chooses several paths; `false` names one.
    pub multiple: bool,
    pub field: FieldProps,
}

/// The most columns one grid section draws, and the least; the default
/// when a section names none is [`GRID_COLUMNS`].
pub const MIN_GRID_COLUMNS: u64 = 1;
pub const MAX_GRID_COLUMNS: u64 = 8;
pub const GRID_COLUMNS: u64 = 5;

/// The least and most items one page of a list holds; the default when it
/// names none is Pane's own.
pub const MAX_PAGE_SIZE: u64 = 100;

/// A standard List or Grid node (#240, `docs/list-tree.md`'s item
/// vocabulary on the designed tree): its items in sections, its search
/// field and selection Pane's. The children are its `list-section` and
/// `list-item` (or `grid-item`) nodes; a `list-dropdown` child is its
/// search-bar dropdown; any other child is the empty view drawn when no
/// item is shown. An item's `detail` is the detail pane's content when it
/// is selected and `is-showing-detail` names the pane.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ListNode {
    /// The search field's placeholder.
    pub search_placeholder: Option<String>,
    /// The search text the tree sets: the field's value, which wins while
    /// the user has not typed a newer one (the field's own partial
    /// control).
    pub search_text: Option<String>,
    /// The item the tree selects, by its key.
    pub selected_key: Option<String>,
    /// `is-loading`: the loading bar, drawn once loading has run past its
    /// threshold (300 ms).
    pub is_loading: bool,
    /// `is-showing-detail`: the detail pane beside the items.
    pub is_showing_detail: bool,
    /// Whether more items follow the ones shown, whose loading the list
    /// asks for as the selection nears the end.
    pub has_more: bool,
    /// How many items a page holds; clamped to 1–100.
    pub page_size: Option<u64>,
    /// The callback the search text runs on its every change, when the view
    /// handles the search itself — the list is then not filtered by Pane
    /// and the events are throttled (250 ms by default).
    pub on_search_text: Option<u32>,
    /// The callback the selection's change runs, told the selected item's
    /// key.
    pub on_selection_change: Option<u32>,
    /// The callback the load of the next page runs, as the selection nears
    /// the end of what is shown.
    pub on_load_more: Option<u32>,
}

/// One section of a List's or Grid's items, under its title and subtitle.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ListSection {
    pub title: Option<String>,
    pub subtitle: Option<String>,
    /// How many columns the section's cells sit in (a Grid's), 1–8;
    /// 5 when the section names none.
    pub columns: Option<u64>,
    /// The cells' width over their height (a Grid's); a non-positive one
    /// is left out.
    pub aspect_ratio: Option<Finite>,
    /// How the section's images fit their cells (a Grid's).
    pub fit: Fit,
    /// Whether the cells sit inset from the grid's edges (a Grid's).
    pub inset: bool,
}

/// One item of a List, as the tree gives it: the List document's
/// vocabulary on the designed tree. Its children are its own row subtree,
/// drawn in the place of the standard row while Pane still selects and
/// activates it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ListItem {
    pub title: String,
    pub subtitle: Option<String>,
    /// Shown while the pointer rests on the item's title.
    pub title_tooltip: Option<String>,
    /// Shown while the pointer rests on the item's subtitle.
    pub subtitle_tooltip: Option<String>,
    /// Words the search matches as the subtitle is.
    pub keywords: Vec<String>,
    pub icon: Option<Icon>,
    pub accessories: Vec<Accessory>,
    /// The callback that activates the item (its primary action).
    pub on_press: Option<u32>,
    /// Its further actions in order: the second is the item's secondary
    /// action, Ctrl+Enter. (The Actions panel that lists them all is the
    /// action model's, #120's; this slice binds no shortcuts.)
    pub actions: Vec<ListAction>,
    /// The detail pane's content when this item is selected and the list
    /// shows the pane.
    pub detail: Option<Box<Node>>,
}

/// One of a List item's actions: what runs it (its callback id), and what
/// the footer calls it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListAction {
    pub title: Option<String>,
    pub on_press: u32,
}

/// One cell of a Grid: an image, a colour, or the author's own subtree in
/// its children, with a title and a subtitle under it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GridItem {
    pub title: Option<String>,
    pub subtitle: Option<String>,
    pub image: Option<Icon>,
    /// A colour the cell fills with.
    pub color: Option<Paint>,
    /// The callback that activates the cell.
    pub on_press: Option<u32>,
}

/// A List's search-bar dropdown, beside its search field: its items and
/// the one chosen, changed by the user's choice.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ListDropdown {
    /// The id of the item chosen.
    pub value: Option<String>,
    /// The dropdown's placeholder, shown while no choice is made.
    pub placeholder: Option<String>,
    /// The callback the choice's change runs, told the chosen item's id.
    pub on_change: Option<u32>,
    pub items: Vec<DropdownItem>,
}

/// One item of a List's search-bar dropdown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DropdownItem {
    pub value: String,
    pub label: Option<String>,
}

/// The tone of a button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Default,
    Secondary,
    Ghost,
    Accent,
    Destructive,
}

/// One canvas: the drawing operations it paints in order, later ones over
/// earlier ones, clipped to its size; what it is to assistive technology
/// (one node, with a role, a label and a value); and the handlers its
/// input names. Its size is its style's sizing, as any node's is — fixed,
/// or filling the space the layout gives it, which the view's render
/// context names with its key (#242).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Canvas {
    /// The operations it paints, in order.
    pub ops: Vec<CanvasOp>,
    /// What the one node the canvas is to assistive technology says.
    pub a11y: CanvasA11y,
    /// The handlers the canvas's input names, each by the callback id its
    /// tree gave it.
    pub handlers: CanvasHandlers,
}

/// What the one node a canvas is to assistive technology says: its role,
/// its label and its value, as a control's are.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CanvasA11y {
    /// What kind of control the canvas is. `None` when the tree names none
    /// the set holds, which reads as a generic one.
    pub role: Option<CanvasRole>,
    /// What names the canvas to assistive technology.
    pub label: Option<String>,
    /// What the canvas currently holds, as a color well names its chosen
    /// color or a slider its value.
    pub value: Option<String>,
}

/// What kind of control a canvas is to assistive technology: a set wide
/// enough for the controls the custom view could not name, one of which
/// the old contract's only role (`color-well`) maps to.
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

/// The handlers a canvas's input names, each by the callback id its tree
/// gave it. Keys ride the node's own `onKey` as every focusable node's do;
/// `onIncrement` and `onDecrement` are the semantic handlers the up and
/// down arrows run, `onActivate` the one Space runs, so a control-like
/// canvas needs no key parsing — a canvas naming them takes those keys
/// itself, and they reach no `onKey`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CanvasHandlers {
    pub on_increment: Option<u32>,
    pub on_decrement: Option<u32>,
    pub on_activate: Option<u32>,
    pub on_pointer_down: Option<u32>,
    pub on_pointer_up: Option<u32>,
    /// A pointer move while the button pressed over the canvas is held —
    /// a drag — coalesced to the latest while one is in flight, as the
    /// custom view's were.
    pub on_pointer_move: Option<u32>,
    /// The pointer entering the canvas, hover that sends no moves.
    pub on_pointer_enter: Option<u32>,
    /// The pointer leaving it.
    pub on_pointer_leave: Option<u32>,
    pub on_wheel: Option<u32>,
    /// The primary button pressed twice over the canvas.
    pub on_double_click: Option<u32>,
    /// The secondary button pressed over the canvas.
    pub on_secondary: Option<u32>,
    /// The canvas's size changing, its payload naming the new one.
    pub on_resize: Option<u32>,
}

impl CanvasHandlers {
    /// Whether the canvas names any handler of its input (its keys ride
    /// the node's own `onKey`, which this does not count): whether it is
    /// focusable and stateful.
    pub fn any(&self) -> bool {
        self.on_increment.is_some()
            || self.on_decrement.is_some()
            || self.on_activate.is_some()
            || self.on_pointer_down.is_some()
            || self.on_pointer_up.is_some()
            || self.on_pointer_move.is_some()
            || self.on_pointer_enter.is_some()
            || self.on_pointer_leave.is_some()
            || self.on_wheel.is_some()
            || self.on_double_click.is_some()
            || self.on_secondary.is_some()
            || self.on_resize.is_some()
    }
}

/// One drawing operation of a canvas: what it paints, or how it moves the
/// state the painting that follows paints in. Coordinates are logical
/// pixels in the canvas's own space, its origin its top-left corner; a
/// position outside its size is clipped away when it is drawn.
#[derive(Clone, Debug, PartialEq)]
pub enum CanvasOp {
    /// A rectangle, filled and/or stroked, `radius` rounding its corners.
    Rect {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        radius: Option<Finite>,
        fill: Option<Paint>,
        stroke: Option<CanvasStroke>,
    },
    /// A circle, `x` and `y` its center.
    Circle {
        x: f32,
        y: f32,
        radius: Finite,
        fill: Option<Paint>,
        stroke: Option<CanvasStroke>,
    },
    /// Move the current path's point, starting a new sub-path.
    Move { x: f32, y: f32 },
    /// A straight line to a point.
    Line { x: f32, y: f32 },
    /// A quadratic curve to a point, through a control point.
    Quad { cx: f32, cy: f32, x: f32, y: f32 },
    /// A cubic curve to a point, through two control points.
    Cubic {
        c1x: f32,
        c1y: f32,
        c2x: f32,
        c2y: f32,
        x: f32,
        y: f32,
    },
    /// An arc of a circle, `x` and `y` its center, from `start` to `end`
    /// radians, drawn the short way from `start` to `end` unless `ccw`.
    Arc {
        x: f32,
        y: f32,
        radius: Finite,
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
    /// One line of text, its top-left corner at `x`, `y`, in a token style
    /// or a raw size, its color corrected as a text's is.
    Text(CanvasText),
    /// An image of the icon model, at a position and size. It is drawn
    /// while it loads as its fallback or nothing.
    Image {
        image: Option<Icon>,
        x: f32,
        y: f32,
        width: Finite,
        height: Finite,
    },
    /// Clip what follows to this rectangle, intersected with the clips
    /// before it.
    Clip {
        x: f32,
        y: f32,
        width: Finite,
        height: Finite,
    },
    /// Move the origin the operations that follow draw at.
    Translate { x: f32, y: f32 },
    /// Scale the space the operations that follow draw in, from the
    /// origin.
    Scale { x: f32, y: f32 },
    /// Rotate the space the operations that follow draw in, around the
    /// origin, clockwise by this many degrees.
    Rotate { degrees: f32 },
}

/// One stroke: its colour, its width, and the shapes of its ends and
/// corners.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CanvasStroke {
    pub color: Paint,
    /// One pixel when the tree gives none.
    pub width: Option<Finite>,
    pub cap: Option<StrokeCap>,
    pub join: Option<StrokeJoin>,
}

/// How a stroke's ends are drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StrokeCap {
    Butt,
    Round,
    Square,
}

/// How a stroke's corners are drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StrokeJoin {
    Miter,
    Round,
    Bevel,
}

/// One line of text a canvas draws: its position, its content, and how it
/// is drawn — a token style and level, or a raw size, a weight, and its
/// colour, corrected as a text's is.
#[derive(Clone, Debug, PartialEq)]
pub struct CanvasText {
    pub x: f32,
    pub y: f32,
    pub content: String,
    pub style: Option<TextStyle>,
    pub level: Option<TextLevel>,
    pub color: Option<Paint>,
    pub size: Option<Finite>,
    pub weight: Option<Finite>,
}

/// A finite number as the tree gives one: a pixel length, a fraction, a
/// weight, a value. [`Eq`] holds because reading keeps every number
/// finite.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Finite(pub f32);

impl Eq for Finite {}

/// The sizing every node may set: how it takes space and how big it is.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Sizing {
    pub grow: Option<Finite>,
    pub shrink: Option<Finite>,
    pub basis: Option<Length>,
    pub width: Option<Length>,
    pub height: Option<Length>,
    pub min_width: Option<Length>,
    pub max_width: Option<Length>,
    pub min_height: Option<Length>,
    pub max_height: Option<Length>,
    /// Its width over its height; a non-positive one is left out.
    pub aspect_ratio: Option<Finite>,
}

/// The surface every node may draw: its background, border, corner radius
/// and opacity, with the `hover` and `pressed` variants Pane applies
/// itself, without calling the extension.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Surface {
    pub background: Option<Paint>,
    pub border: Option<Border>,
    pub radius: Option<RadiusLength>,
    /// 0 to 1.
    pub opacity: Option<Finite>,
}

/// The border every node may draw: how wide, and in which colour.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Border {
    /// One pixel when the tree gives none.
    pub width: Option<Length>,
    /// The theme's own edge colour when the tree gives none.
    pub color: Option<Paint>,
}

/// A corner radius: a token, or pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RadiusLength {
    Token(Radius),
    Px(Finite),
}

/// The style every node may carry: its [`Sizing`] and [`Surface`], and the
/// variants of the surface Pane applies itself while the pointer is over
/// the node or it is pressed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Style {
    pub sizing: Sizing,
    pub surface: Surface,
    pub hover: Option<Surface>,
    pub pressed: Option<Surface>,
}

impl Style {
    /// Whether nothing is set: a node whose style is this is drawn with no
    /// wrapper.
    pub fn is_empty(&self) -> bool {
        *self == Style::default()
    }
}

/// One length: a space token, pixels, or a fraction of the parent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Length {
    Space(Space),
    Px(Finite),
    Fraction(Finite),
}

/// One colour property's value: the colour, and whether Pane corrects its
/// contrast against the surface it is drawn on. A colour given as
/// `{"raw": ...}` is drawn exactly as it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Paint {
    pub tint: Tint,
    /// Whether contrast correction is off for this colour.
    pub exact: bool,
}

impl DesignedTree {
    /// The tree `document` describes, or why Pane cannot read it: an
    /// over-limit tree, or a document of another major version, is the
    /// extension's error ([`ReadError::Guest`]); a document Pane cannot
    /// read says why ([`ReadError::Unreadable`]).
    pub fn read(document: &str) -> Result<DesignedTree, ReadError> {
        if document.len() > MAX_TREE_BYTES {
            return Err(ReadError::Guest(format!(
                "the view's tree is {} bytes; at most {MAX_TREE_BYTES} are read",
                document.len()
            )));
        }
        // Serde's own recursion bound (128) sits below a tree as deep as
        // the limit allows: each level is an object and an array. The
        // document's bracket nesting is counted over its text first — no
        // recursion, whatever the document holds — and a document nesting
        // deeper than a tree at the limit can is over the depth limit, the
        // extension's error, never parsed. What is left is bounded, so the
        // parse runs with serde's recursion bound lifted: it cannot
        // overflow, and the tree's own depth is checked as it is read.
        if nesting_of(document) > 2 * MAX_DEPTH + 8 {
            return Err(ReadError::Guest(format!(
                "the view's tree is deeper than the {MAX_DEPTH} levels that are drawn"
            )));
        }
        let mut parser = serde_json::Deserializer::from_str(document);
        parser.disable_recursion_limit();
        let wire: WireDocument = WireDocument::deserialize(&mut parser)
            .map_err(|error| ReadError::Unreadable(format!("its tree: {error}")))?;
        let (major, minor) = version(&wire.version).ok_or_else(|| {
            ReadError::Unreadable(format!(
                "its tree names version {:?}, which is not \"<major>.<minor>\"",
                wire.version
            ))
        })?;
        if major != COMPONENT_SET.0 {
            return Err(ReadError::Guest(format!(
                "its tree uses UI component set {major}.{minor}; this Pane renders {}.{}",
                COMPONENT_SET.0, COMPONENT_SET.1
            )));
        }
        let mut nodes = 0;
        let root = node(wire.root, 1, &mut nodes)?;
        Ok(DesignedTree { root })
    }
}

/// Why a tree's keys do not identify its stateful nodes: a key shared by
/// siblings, and a stateful node without one (development reports these;
/// Pane matches both by position instead).
pub fn key_problems(tree: &DesignedTree) -> Vec<String> {
    let mut problems = Vec::new();
    problems_of(&tree.root, &mut problems);
    problems
}

/// The key problems of `node`'s subtree, appended to `problems`.
fn problems_of(node: &Node, problems: &mut Vec<String>) {
    let mut seen = Vec::new();
    for child in &node.children {
        if let Some(key) = &child.key {
            if seen.iter().any(|held: &String| held == key) {
                problems.push(format!(
                    "the key \"{key}\" is shared by two siblings; \
                     Pane matches those nodes by position"
                ));
            } else {
                seen.push(key.clone());
            }
        } else if child.stateful() {
            problems.push(format!(
                "a stateful {} has no key; Pane matches it by position",
                kind_name(&child.kind)
            ));
        }
        problems_of(child, problems);
    }
    if let Some(fallback) = &node.fallback {
        problems_of(fallback, problems);
    }
}

/// The wire name of a node kind, as a development report says it.
fn kind_name(kind: &NodeKind) -> String {
    match kind {
        NodeKind::Column(_) => "column".to_owned(),
        NodeKind::Row(_) => "row".to_owned(),
        NodeKind::Stack(_) => "stack".to_owned(),
        NodeKind::Scroll { .. } => "scroll".to_owned(),
        NodeKind::Spacer => "spacer".to_owned(),
        NodeKind::Divider { .. } => "divider".to_owned(),
        NodeKind::Text(_) => "text".to_owned(),
        NodeKind::Button(_) => "button".to_owned(),
        NodeKind::Link(_) => "link".to_owned(),
        NodeKind::Icon(_) => "icon".to_owned(),
        NodeKind::IconTile(_) => "icon-tile".to_owned(),
        NodeKind::Image(_) => "image".to_owned(),
        NodeKind::RichRow(_) => "rich-row".to_owned(),
        NodeKind::Keycap(_) => "keycap".to_owned(),
        NodeKind::KeySequence(_) => "key-sequence".to_owned(),
        NodeKind::Tag(_) => "tag".to_owned(),
        NodeKind::Badge(_) => "badge".to_owned(),
        NodeKind::Toggle(_) => "toggle".to_owned(),
        NodeKind::Checkbox(_) => "checkbox".to_owned(),
        NodeKind::Segmented(_) => "segmented".to_owned(),
        NodeKind::Slider(_) => "slider".to_owned(),
        NodeKind::Progress(_) => "progress".to_owned(),
        NodeKind::Loading(_) => "loading".to_owned(),
        NodeKind::Markdown(_) => "markdown".to_owned(),
        NodeKind::Card(_) => "card".to_owned(),
        NodeKind::SectionHeader(_) => "section-header".to_owned(),
        NodeKind::MetadataList(_) => "metadata-list".to_owned(),
        NodeKind::EmptyState(_) => "empty-state".to_owned(),
        NodeKind::TextInput(_) => "text-input".to_owned(),
        NodeKind::PasswordInput(_) => "password-input".to_owned(),
        NodeKind::TextArea(_) => "text-area".to_owned(),
        NodeKind::Select(_) => "select".to_owned(),
        NodeKind::List(_) => "list".to_owned(),
        NodeKind::Grid(_) => "grid".to_owned(),
        NodeKind::ListSection(_) => "list-section".to_owned(),
        NodeKind::ListItem(_) => "list-item".to_owned(),
        NodeKind::GridItem(_) => "grid-item".to_owned(),
        NodeKind::ListDropdown(_) => "list-dropdown".to_owned(),
        NodeKind::Detail(_) => "detail".to_owned(),
        NodeKind::Canvas(_) => "canvas".to_owned(),
        NodeKind::Form(_) => "form".to_owned(),
        NodeKind::DatePicker(_) => "date-picker".to_owned(),
        NodeKind::DateTimePicker(_) => "date-time-picker".to_owned(),
        NodeKind::TagPicker(_) => "tag-picker".to_owned(),
        NodeKind::FilePicker(_) => "file-picker".to_owned(),
        NodeKind::FolderPicker(_) => "folder-picker".to_owned(),
        NodeKind::Unknown(kind) => kind.clone(),
    }
}

/// Why a tree could not be read: [`ReadError::Guest`] is the extension's
/// error (the view keeps its last good tree), [`ReadError::Unreadable`]
/// the failure Pane reports as its own reading of what the extension
/// answered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReadError {
    Guest(String),
    Unreadable(String),
}

impl ReadError {
    /// The error's message.
    pub fn message(&self) -> &str {
        match self {
            ReadError::Guest(message) | ReadError::Unreadable(message) => message,
        }
    }
}

/// The deepest nesting of brackets in `document`, counted over its text:
/// a bound on the depth its parse reaches, without parsing it. Text in
/// strings does not count.
fn nesting_of(document: &str) -> usize {
    let mut nesting: usize = 0;
    let mut deepest: usize = 0;
    let mut in_string = false;
    let mut escaped = false;
    for character in document.chars() {
        if in_string {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                in_string = false;
            }
            continue;
        }
        match character {
            '"' => in_string = true,
            '{' | '[' => {
                nesting += 1;
                deepest = deepest.max(nesting);
            }
            '}' | ']' => nesting = nesting.saturating_sub(1),
            _ => {}
        }
    }
    deepest
}

/// `text`, a version like `"1.0"`, as (major, minor).
fn version(text: &str) -> Option<(u64, u64)> {
    let (major, minor) = text.split_once('.')?;
    Some((major.parse().ok()?, minor.parse().ok()?))
}

/// The document's root: its version and its root node.
#[derive(Deserialize)]
struct WireDocument {
    version: String,
    root: WireNode,
}

/// A node as the tree gives it: its type, its common fields, and whatever
/// else it says, for its own kind to read.
#[derive(Deserialize)]
struct WireNode {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    key: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default, rename = "navigationTitle")]
    navigation_title: Option<String>,
    #[serde(default)]
    requires: Option<u64>,
    #[serde(default)]
    fallback: Option<Box<WireNode>>,
    #[serde(default)]
    children: Option<Vec<Value>>,
    #[serde(default)]
    focus: Option<bool>,
    #[serde(default, rename = "onFocus")]
    on_focus: Option<u32>,
    #[serde(default, rename = "onBlur")]
    on_blur: Option<u32>,
    #[serde(default, rename = "onKey")]
    on_key: Option<u32>,
    #[serde(flatten)]
    rest: Map<String, Value>,
}

/// The node `wire` describes at `depth`, counting it in `nodes`, or why it
/// cannot be read.
fn node(wire: WireNode, depth: usize, nodes: &mut usize) -> Result<Node, ReadError> {
    *nodes += 1;
    if *nodes > MAX_NODES {
        return Err(ReadError::Guest(format!(
            "the view has {nodes} nodes; at most {MAX_NODES} are drawn"
        )));
    }
    if depth > MAX_DEPTH {
        return Err(ReadError::Guest(format!(
            "the view's tree is {depth} levels deep; at most {MAX_DEPTH} are drawn"
        )));
    }
    // A node whose `requires` this Pane does not meet is one it does not
    // know: it asks for a later minor version than the one Pane renders,
    // and degrades as an unknown node does.
    let meets = wire
        .requires
        .is_none_or(|required| required <= COMPONENT_SET.1);
    let kind = if !meets {
        NodeKind::Unknown(wire.kind.clone())
    } else {
        match wire.kind.as_str() {
            "column" => NodeKind::Column(layout(&wire)?),
            "row" => NodeKind::Row(layout(&wire)?),
            "stack" => NodeKind::Stack(place_of(&wire, "align")?.unwrap_or_default()),
            "scroll" => NodeKind::Scroll {
                orientation: orientation_of(&wire, "orientation")?.unwrap_or_default(),
            },
            "spacer" => NodeKind::Spacer,
            "divider" => NodeKind::Divider {
                orientation: orientation_of(&wire, "orientation")?.unwrap_or_default(),
            },
            "text" => NodeKind::Text(text(&wire)?),
            "button" => NodeKind::Button(button(&wire)?),
            "link" => NodeKind::Link(link(&wire)?),
            "icon" => NodeKind::Icon(icon_node(&wire, "icon")?),
            "icon-tile" => NodeKind::IconTile(icon_node(&wire, "icon")?),
            "image" => NodeKind::Image(image(&wire)?),
            "rich-row" => NodeKind::RichRow(rich_row(&wire)?),
            "keycap" => NodeKind::Keycap(keycap(&wire)?),
            "key-sequence" => NodeKind::KeySequence(key_sequence(&wire)?),
            "tag" => NodeKind::Tag(texted(&wire, "text")?),
            "badge" => NodeKind::Badge(badge(&wire)?),
            "toggle" => NodeKind::Toggle(Toggle {
                on: boolean(&wire, "on")?
                    .or(boolean(&wire, "default")?)
                    .unwrap_or(false),
                on_change: callback(&wire, "onChange")?,
                label: string(&wire, "label")?,
                field: field_props(&wire)?,
            }),
            "checkbox" => NodeKind::Checkbox(Checkbox {
                checked: boolean(&wire, "checked")?
                    .or(boolean(&wire, "default")?)
                    .unwrap_or(false),
                on_change: callback(&wire, "onChange")?,
                label: string(&wire, "label")?,
                field: field_props(&wire)?,
            }),
            "segmented" => NodeKind::Segmented(segmented(&wire)?),
            "select" => NodeKind::Select(select(&wire)?),
            "slider" => NodeKind::Slider(slider(&wire)?),
            "progress" => NodeKind::Progress(Progress {
                value: number(&wire, "value")?.ok_or(unreadable("a progress node has no value"))?,
                label: string(&wire, "label")?,
            }),
            "loading" => NodeKind::Loading(Loading {
                label: string(&wire, "label")?,
            }),
            "markdown" => NodeKind::Markdown(markdown_node(&wire)?),
            "card" => NodeKind::Card(layout(&wire)?),
            "section-header" => NodeKind::SectionHeader(section_header(&wire)?),
            "metadata-list" => NodeKind::MetadataList(metadata_list(&wire)?),
            "empty-state" => NodeKind::EmptyState(empty_state(&wire)?),
            "text-input" => NodeKind::TextInput(text_input(&wire)?),
            "password-input" => NodeKind::PasswordInput(text_input(&wire)?),
            "text-area" => NodeKind::TextArea(text_input(&wire)?),
            "list" => NodeKind::List(list_node(&wire)?),
            "grid" => NodeKind::Grid(list_node(&wire)?),
            "list-section" => NodeKind::ListSection(list_section(&wire)?),
            "list-item" => NodeKind::ListItem(list_item(&wire, depth, nodes)?),
            "grid-item" => NodeKind::GridItem(grid_item(&wire)?),
            "list-dropdown" => NodeKind::ListDropdown(list_dropdown(&wire)?),
            "detail" => NodeKind::Detail(layout(&wire)?),
            "canvas" => NodeKind::Canvas(canvas(&wire)?),
            "form" => NodeKind::Form(FormNode {
                on_submit: callback(&wire, "onSubmit")?,
                submit_label: string(&wire, "submitLabel")?,
            }),
            "date-picker" => NodeKind::DatePicker(date_field(&wire)?),
            "date-time-picker" => NodeKind::DateTimePicker(date_field(&wire)?),
            "tag-picker" => NodeKind::TagPicker(TagPicker {
                tags: texts(&wire, "tags")?
                    .or_else(|| texts(&wire, "default").ok().flatten())
                    .unwrap_or_default(),
                options: options(&wire, "options")?,
                on_change: callback(&wire, "onChange")?,
                field: field_props(&wire)?,
            }),
            "file-picker" => NodeKind::FilePicker(file_picker(&wire)?),
            "folder-picker" => NodeKind::FolderPicker(file_picker(&wire)?),
            _ => NodeKind::Unknown(wire.kind.clone()),
        }
    };
    let style = style(&wire)?;
    let place = place_of(&wire, "place")?;
    let offset = offset_of(&wire)?;
    let children = wire
        .children
        .unwrap_or_default()
        .into_iter()
        .map(|child| {
            serde_json::from_value::<WireNode>(child)
                .map_err(|error| ReadError::Unreadable(format!("its tree: {error}")))
                .and_then(|child| node(child, depth + 1, nodes))
        })
        .collect::<Result<Vec<Node>, ReadError>>()?;
    let fallback = wire
        .fallback
        .map(|fallback| node(*fallback, depth + 1, nodes).map(Box::new))
        .transpose()?;
    Ok(Node {
        kind,
        style,
        place,
        offset,
        key: wire.key,
        name: wire.name,
        navigation_title: wire.navigation_title,
        requires: wire.requires,
        fallback,
        children,
        focus: wire.focus.unwrap_or(false),
        on_focus: wire.on_focus,
        on_blur: wire.on_blur,
        on_key: wire.on_key,
    })
}

/// The layout a column, row or card's other properties give it: `gap`,
/// `padding`, `align`, `justify` and `wrap`. A property of the wrong type
/// is unreadable; a token this version does not know is left out, as a
/// newer minor version may add one.
fn layout(wire: &WireNode) -> Result<Layout, ReadError> {
    let over = |message: String| ReadError::Unreadable(message);
    let rest = &wire.rest;
    Ok(Layout {
        gap: token(rest.get("gap"), "gap", Space::named)?,
        padding: padding(rest.get("padding")).map_err(over)?,
        align: token(rest.get("align"), "align", align)?,
        justify: token(rest.get("justify"), "justify", justify)?,
        wrap: match rest.get("wrap") {
            None | Some(Value::Null) => false,
            Some(Value::Bool(wrap)) => *wrap,
            Some(_) => return Err(over("its wrap is not a boolean".into())),
        },
    })
}

/// The text a text node's other properties give it: its `text` or its
/// `spans`, and its style, level, colour, size, weight and truncation.
fn text(wire: &WireNode) -> Result<Text, ReadError> {
    let rest = &wire.rest;
    let content = match (rest.get("text"), rest.get("spans")) {
        (Some(Value::String(content)), None) => TextContent::Plain(content.clone()),
        (None, Some(Value::Array(spans))) => TextContent::Spans(
            spans
                .iter()
                .map(span_of)
                .collect::<Result<Vec<Span>, ReadError>>()?,
        ),
        (None, None) => return Err(unreadable("a text node has no text")),
        (Some(Value::String(_)), Some(_)) => {
            return Err(unreadable("a text node names both text and spans"));
        }
        (Some(_), _) => return Err(unreadable("its text is not a string")),
        (None, Some(_)) => return Err(unreadable("its spans are not a list")),
    };
    let length = match &content {
        TextContent::Plain(content) => content.chars().count(),
        TextContent::Spans(spans) => spans.iter().map(|span| span.text.chars().count()).sum(),
    };
    if length > MAX_TEXT_CHARS {
        return Err(ReadError::Guest(format!(
            "a text of the view has {length} characters; at most {MAX_TEXT_CHARS} are drawn"
        )));
    }
    Ok(Text {
        content,
        style: token(rest.get("style"), "style", text_style)?,
        level: token(rest.get("level"), "level", text_level)?,
        color: paint(rest.get("color"), "color")?,
        size: sized(rest.get("size"), "size")?,
        weight: weighted(rest.get("weight"), "weight")?,
        truncate: boolean(wire, "truncate")?.unwrap_or(false),
    })
}

/// One span of a text: its `text`, its style, level and colour, whether it
/// is code, and its `onPress` when it is a link.
fn span_of(value: &Value) -> Result<Span, ReadError> {
    let Value::Object(fields) = value else {
        return Err(unreadable("a span of its text is not an object"));
    };
    let text = match fields.get("text") {
        Some(Value::String(text)) => text.clone(),
        _ => return Err(unreadable("a span of its text has no text")),
    };
    let wire = WireNode {
        kind: "text".into(),
        key: None,
        name: None,
        navigation_title: None,
        requires: None,
        fallback: None,
        children: None,
        focus: None,
        on_focus: None,
        on_blur: None,
        on_key: None,
        rest: fields.clone(),
    };
    Ok(Span {
        text,
        style: token(fields.get("style"), "style", text_style)?,
        level: token(fields.get("level"), "level", text_level)?,
        color: paint(fields.get("color"), "color")?,
        code: boolean(&wire, "code")?.unwrap_or(false),
        on_press: callback(&wire, "onPress")?,
    })
}

/// The button a button node's other properties give it: its `label`,
/// `tone`, `icon`, `keys`, `onPress` and `enabled`.
fn button(wire: &WireNode) -> Result<Button, ReadError> {
    let rest = &wire.rest;
    let label = match rest.get("label") {
        Some(Value::String(label)) => label.clone(),
        Some(_) => return Err(unreadable("its label is not a string")),
        None => return Err(unreadable("a button node has no label")),
    };
    let keys = match rest.get("keys") {
        None | Some(Value::Null) => None,
        Some(Value::Array(keys)) => {
            let keys = keys
                .iter()
                .map(|key| match key {
                    Value::String(key) => Ok(key.clone()),
                    _ => Err(unreadable("a key of its keys is not a string")),
                })
                .collect::<Result<Vec<String>, ReadError>>()?;
            (!keys.is_empty()).then_some(keys)
        }
        Some(_) => return Err(unreadable("its keys are not a list")),
    };
    Ok(Button {
        label,
        tone: token(rest.get("tone"), "tone", tone)?,
        icon: icon_of(rest.get("icon"), "icon")?,
        keys,
        on_press: callback(wire, "onPress")?,
        enabled: boolean(wire, "enabled")?.unwrap_or(true),
    })
}

/// The link a link node's other properties give it.
fn link(wire: &WireNode) -> Result<Link, ReadError> {
    let rest = &wire.rest;
    let label = match rest.get("label") {
        Some(Value::String(label)) => label.clone(),
        Some(_) => return Err(unreadable("its label is not a string")),
        None => return Err(unreadable("a link node has no label")),
    };
    Ok(Link {
        label,
        on_press: callback(wire, "onPress")?,
        color: paint(rest.get("color"), "color")?,
    })
}

/// The icon an icon or icon-tile node draws: its `icon` and `size`. The
/// icon is read leniently, as the list tree's are: one Pane cannot read is
/// not drawn, its node drawing nothing (or, for an image, its
/// placeholder).
fn icon_node(wire: &WireNode, property: &str) -> Result<IconNode, ReadError> {
    let rest = &wire.rest;
    Ok(IconNode {
        icon: icon_of(rest.get(property), property)?,
        size: extent(rest.get("size"), "size")?,
    })
}

/// The image an image node draws: its `image`, `size` and `fit`, with its
/// children drawn as its placeholder.
fn image(wire: &WireNode) -> Result<Image, ReadError> {
    let rest = &wire.rest;
    let fit = match rest.get("fit") {
        None | Some(Value::Null) => Fit::Contain,
        Some(Value::String(fit)) => match fit.as_str() {
            "contain" => Fit::Contain,
            "cover" => Fit::Cover,
            "fill" => Fit::Fill,
            other => {
                return Err(unreadable(&format!(
                    "its fit is {other}; a fit is \"contain\", \"cover\" or \"fill\""
                )));
            }
        },
        Some(_) => return Err(unreadable("its fit is not a fit")),
    };
    Ok(Image {
        image: icon_of(rest.get("image"), "image")?,
        size: extent(rest.get("size"), "size")?,
        fit,
    })
}

/// The rich row a rich-row node's other properties give it.
fn rich_row(wire: &WireNode) -> Result<RichRow, ReadError> {
    let rest = &wire.rest;
    let title = match rest.get("title") {
        Some(Value::String(title)) => title.clone(),
        Some(_) => return Err(unreadable("its title is not a string")),
        None => return Err(unreadable("a rich-row node has no title")),
    };
    let accessories = match rest.get("accessories") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(accessories)) => accessories
            .iter()
            .map(|accessory| {
                let Value::Object(fields) = accessory else {
                    return Err(unreadable("an accessory of its row is not an object"));
                };
                let text = match fields.get("text") {
                    Some(Value::String(text)) => text.clone(),
                    _ => return Err(unreadable("an accessory of its row has no text")),
                };
                Ok(RowAccessory {
                    text,
                    tag: match fields.get("tag") {
                        None | Some(Value::Null) => false,
                        Some(Value::Bool(tag)) => *tag,
                        Some(_) => return Err(unreadable("an accessory's tag is not a boolean")),
                    },
                    color: paint(fields.get("color"), "color")?,
                })
            })
            .collect::<Result<Vec<RowAccessory>, ReadError>>()?,
        Some(_) => return Err(unreadable("its accessories are not a list")),
    };
    Ok(RichRow {
        title,
        subtitle: string(wire, "subtitle")?,
        icon: icon_of(rest.get("icon"), "icon")?,
        accessories,
        on_press: callback(wire, "onPress")?,
    })
}

/// The keycap a keycap node shows: the `key` the document names — the
/// one field every node's identity is read from too (a keycap's shown
/// key IS its node's `key` in the document, which gives it no other), so
/// the key is the node's own, not a property the kind reads among the
/// rest.
fn keycap(wire: &WireNode) -> Result<Keycap, ReadError> {
    let key = wire
        .key
        .clone()
        .ok_or_else(|| unreadable("a keycap node has no key"))?;
    Ok(Keycap { key })
}

/// The keys a key-sequence node shows.
fn key_sequence(wire: &WireNode) -> Result<KeySequence, ReadError> {
    let rest = &wire.rest;
    let keys = match rest.get("keys") {
        Some(Value::Array(keys)) => keys
            .iter()
            .map(|key| match key {
                Value::String(key) => Ok(key.clone()),
                _ => Err(unreadable("a key of its keys is not a string")),
            })
            .collect::<Result<Vec<String>, ReadError>>()?,
        Some(_) => return Err(unreadable("its keys are not a list")),
        None => return Err(unreadable("a key-sequence node has no keys")),
    };
    Ok(KeySequence { keys })
}

/// A tag or badge node: its `text` and `color`.
fn texted(wire: &WireNode, property: &str) -> Result<Tag, ReadError> {
    let rest = &wire.rest;
    let text = match rest.get(property) {
        Some(Value::String(text)) => text.clone(),
        Some(_) => return Err(unreadable("its text is not a string")),
        None => return Err(unreadable("a tag node has no text")),
    };
    Ok(Tag {
        text,
        color: paint(rest.get("color"), "color")?,
    })
}

/// A badge, drawn from a tag's reading.
fn badge(wire: &WireNode) -> Result<Badge, ReadError> {
    let tag = texted(wire, "text")?;
    Ok(Badge {
        text: tag.text,
        color: tag.color,
    })
}

/// A segmented control's options, value and callback.
fn segmented(wire: &WireNode) -> Result<Segmented, ReadError> {
    Ok(Segmented {
        options: options(wire, "options")?,
        value: string(wire, "value")?,
        on_change: callback(wire, "onChange")?,
        label: string(wire, "label")?,
    })
}

/// A select's options, value and callback.
fn select(wire: &WireNode) -> Result<Select, ReadError> {
    Ok(Select {
        options: options(wire, "options")?,
        value: string(wire, "value")?.or(string(wire, "default")?),
        on_change: callback(wire, "onChange")?,
        on_input: callback(wire, "onInput")?,
        search: boolean(wire, "search")?.unwrap_or(true),
        placeholder: string(wire, "placeholder")?,
        label: string(wire, "label")?,
        field: field_props(wire)?,
    })
}

/// The options a segmented control, select or tag picker names.
fn options(wire: &WireNode, property: &str) -> Result<Vec<Segment>, ReadError> {
    let rest = &wire.rest;
    let options = match rest.get(property) {
        Some(Value::Array(options)) => options
            .iter()
            .map(|option| {
                let Value::Object(fields) = option else {
                    return Err(unreadable("an option of it is not an object"));
                };
                let value = match fields.get("value") {
                    Some(Value::String(value)) => value.clone(),
                    _ => return Err(unreadable("an option of it has no value")),
                };
                Ok(Segment {
                    value,
                    label: match fields.get("label") {
                        Some(Value::String(label)) => Some(label.clone()),
                        None | Some(Value::Null) => None,
                        Some(_) => return Err(unreadable("an option's label is not a string")),
                    },
                    section: match fields.get("section") {
                        Some(Value::String(section)) => Some(section.clone()),
                        None | Some(Value::Null) => None,
                        Some(_) => return Err(unreadable("an option's section is not a string")),
                    },
                })
            })
            .collect::<Result<Vec<Segment>, ReadError>>()?,
        Some(_) => return Err(unreadable(&format!("its {property} are not a list"))),
        None => return Err(unreadable(&format!("it has no {property}"))),
    };
    Ok(options)
}

/// The slider a slider node's other properties give it.
fn slider(wire: &WireNode) -> Result<Slider, ReadError> {
    let value = number(wire, "value")?.ok_or_else(|| unreadable("a slider node has no value"))?;
    let min = number(wire, "min")?.unwrap_or(Finite(0.));
    let max = number(wire, "max")?.unwrap_or(Finite(1.));
    let step = number(wire, "step")?.unwrap_or(Finite(0.1));
    Ok(Slider {
        value,
        min,
        max,
        step,
        on_change: callback(wire, "onChange")?,
        label: string(wire, "label")?,
    })
}

/// The markdown a markdown node holds, parsed into blocks.
fn markdown_node(wire: &WireNode) -> Result<Markdown, ReadError> {
    let rest = &wire.rest;
    let source = match rest.get("markdown") {
        Some(Value::String(source)) => source.clone(),
        Some(_) => return Err(unreadable("its markdown is not a string")),
        None => return Err(unreadable("a markdown node has no markdown")),
    };
    if source.chars().count() > MAX_MARKDOWN_CHARS {
        return Err(ReadError::Guest(format!(
            "the view's markdown has {} characters; at most {MAX_MARKDOWN_CHARS} are read",
            source.chars().count()
        )));
    }
    Ok(Markdown {
        blocks: markdown::parse(&source),
    })
}

/// The section header a section-header node's properties give it.
fn section_header(wire: &WireNode) -> Result<SectionHeader, ReadError> {
    let rest = &wire.rest;
    let title = match rest.get("title") {
        Some(Value::String(title)) => title.clone(),
        Some(_) => return Err(unreadable("its title is not a string")),
        None => return Err(unreadable("a section-header node has no title")),
    };
    Ok(SectionHeader {
        title,
        note: string(wire, "note")?,
    })
}

/// The metadata list a metadata-list node's items give it.
fn metadata_list(wire: &WireNode) -> Result<MetadataList, ReadError> {
    let rest = &wire.rest;
    let items = match rest.get("items") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| {
                let Value::Object(fields) = item else {
                    return Err(unreadable("an item of its list is not an object"));
                };
                let separator = match fields.get("separator") {
                    None | Some(Value::Null) => false,
                    Some(Value::Bool(separator)) => *separator,
                    Some(_) => return Err(unreadable("an item's separator is not a boolean")),
                };
                let tags = match fields.get("tags") {
                    None | Some(Value::Null) => Vec::new(),
                    Some(Value::Array(tags)) => tags
                        .iter()
                        .map(|tag| match tag {
                            Value::String(tag) => Ok(tag.clone()),
                            _ => Err(unreadable("a tag of its item is not a string")),
                        })
                        .collect::<Result<Vec<String>, ReadError>>()?,
                    Some(_) => return Err(unreadable("its item's tags are not a list")),
                };
                let wire = WireNode {
                    kind: "item".into(),
                    key: None,
                    name: None,
                    navigation_title: None,
                    requires: None,
                    focus: None,
                    on_focus: None,
                    on_blur: None,
                    on_key: None,
                    fallback: None,
                    children: None,
                    rest: fields.clone(),
                };
                Ok(MetadataItem {
                    label: string(&wire, "label")?,
                    value: string(&wire, "value")?,
                    on_press: callback(&wire, "onPress")?,
                    tags,
                    separator,
                })
            })
            .collect::<Result<Vec<MetadataItem>, ReadError>>()?,
        Some(_) => return Err(unreadable("its items are not a list")),
    };
    Ok(MetadataList { items })
}

/// The empty state an empty-state node's properties give it.
fn empty_state(wire: &WireNode) -> Result<EmptyState, ReadError> {
    let rest = &wire.rest;
    let title = match rest.get("title") {
        Some(Value::String(title)) => title.clone(),
        Some(_) => return Err(unreadable("its title is not a string")),
        None => return Err(unreadable("an empty-state node has no title")),
    };
    Ok(EmptyState {
        title,
        description: string(wire, "description")?,
        icon: icon_of(rest.get("icon"), "icon")?,
    })
}

/// The text input a text-input, password-input or text-area node's other
/// properties give it: its value (else its `default`), placeholder, the
/// events it asks for, and what it says around its control as a form
/// field.
fn text_input(wire: &WireNode) -> Result<TextInput, ReadError> {
    Ok(TextInput {
        value: string(wire, "value")?
            .or(string(wire, "default")?)
            .unwrap_or_default(),
        placeholder: string(wire, "placeholder")?,
        on_change: callback(wire, "onChange")?,
        on_input: callback(wire, "onInput")?,
        throttle_ms: number(wire, "throttleMs")?.map(|Finite(ms)| ms.max(0.) as u64),
        label: string(wire, "label")?,
        field: field_props(wire)?,
    })
}

/// A list or grid node's own properties: its search field and selection,
/// the search-bar dropdown and pagination. Its items, sections, dropdown
/// and empty view are its children.
fn list_node(wire: &WireNode) -> Result<ListNode, ReadError> {
    Ok(ListNode {
        search_placeholder: string(wire, "searchPlaceholder")?,
        search_text: string(wire, "searchText")?,
        selected_key: string(wire, "selectedKey")?,
        is_loading: boolean(wire, "isLoading")?.unwrap_or(false),
        is_showing_detail: boolean(wire, "isShowingDetail")?.unwrap_or(false),
        has_more: boolean(wire, "hasMore")?.unwrap_or(false),
        page_size: number(wire, "pageSize")?
            .map(|Finite(size)| size.clamp(1., MAX_PAGE_SIZE as f32) as u64),
        on_search_text: callback(wire, "onSearchText")?,
        on_selection_change: callback(wire, "onSelectionChange")?,
        on_load_more: callback(wire, "onLoadMore")?,
    })
}

/// A list section's properties: its title and subtitle, and a grid
/// section's columns, aspect ratio, fit and inset.
fn list_section(wire: &WireNode) -> Result<ListSection, ReadError> {
    Ok(ListSection {
        title: string(wire, "title")?,
        subtitle: string(wire, "subtitle")?,
        columns: number(wire, "columns")?.map(|Finite(columns)| {
            columns
                .clamp(MIN_GRID_COLUMNS as f32, MAX_GRID_COLUMNS as f32)
                .round() as u64
        }),
        aspect_ratio: number(wire, "aspectRatio")?.filter(|Finite(ratio)| *ratio > 0.),
        fit: fit_of(wire.rest.get("fit"))?,
        inset: boolean(wire, "inset")?.unwrap_or(false),
    })
}

/// A list item's properties: the List document's vocabulary — a title,
/// a subtitle, keywords, an icon, accessories and actions — and the
/// detail pane's content when it is selected.
fn list_item(wire: &WireNode, depth: usize, nodes: &mut usize) -> Result<ListItem, ReadError> {
    let rest = &wire.rest;
    let title = match rest.get("title") {
        Some(Value::String(title)) => title.clone(),
        Some(_) => return Err(unreadable("its title is not a string")),
        None => return Err(unreadable("a list-item node has no title")),
    };
    let keywords = match rest.get("keywords") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(keywords)) => keywords
            .iter()
            .map(|keyword| match keyword {
                Value::String(keyword) => Ok(keyword.clone()),
                _ => Err(unreadable("a keyword of its item is not a string")),
            })
            .collect::<Result<Vec<String>, ReadError>>()?,
        Some(_) => return Err(unreadable("its keywords are not a list")),
    };
    let accessories = match rest.get("accessories") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(accessories)) => accessories
            .iter()
            .filter_map(crate::runtime::tree::read_accessory)
            .collect(),
        Some(_) => return Err(unreadable("its accessories are not a list")),
    };
    let actions = match rest.get("actions") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(actions)) => actions
            .iter()
            .map(|action| {
                let Value::Object(fields) = action else {
                    return Err(unreadable("an action of its item is not an object"));
                };
                let action_wire = WireNode {
                    kind: "action".into(),
                    key: None,
                    name: None,
                    navigation_title: None,
                    requires: None,
                    fallback: None,
                    children: None,
                    focus: None,
                    on_focus: None,
                    on_blur: None,
                    on_key: None,
                    rest: fields.clone(),
                };
                Ok(ListAction {
                    title: string(&action_wire, "title")?,
                    on_press: callback(&action_wire, "onPress")?
                        .ok_or_else(|| unreadable("an action of its item has no onPress"))?,
                })
            })
            .collect::<Result<Vec<ListAction>, ReadError>>()?,
        Some(_) => return Err(unreadable("its actions are not a list")),
    };
    let detail = wire
        .rest
        .get("detail")
        .filter(|value| !matches!(value, Value::Null))
        .map(|detail| child_node(detail, depth, nodes))
        .transpose()?;
    Ok(ListItem {
        title,
        subtitle: string(wire, "subtitle")?,
        title_tooltip: string(wire, "titleTooltip")?,
        subtitle_tooltip: string(wire, "subtitleTooltip")?,
        keywords,
        icon: icon_of(rest.get("icon"), "icon")?,
        accessories,
        on_press: callback(wire, "onPress")?,
        actions,
        detail,
    })
}

/// A grid cell's properties: its title and subtitle, and the image,
/// colour or subtree it shows.
fn grid_item(wire: &WireNode) -> Result<GridItem, ReadError> {
    let rest = &wire.rest;
    Ok(GridItem {
        title: string(wire, "title")?,
        subtitle: string(wire, "subtitle")?,
        image: icon_of(rest.get("image"), "image")?,
        color: paint(rest.get("color"), "color")?,
        on_press: callback(wire, "onPress")?,
    })
}

/// A search-bar dropdown's properties: its items and the one chosen.
fn list_dropdown(wire: &WireNode) -> Result<ListDropdown, ReadError> {
    let rest = &wire.rest;
    let items = match rest.get("items") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| {
                let Value::Object(fields) = item else {
                    return Err(unreadable("an item of its dropdown is not an object"));
                };
                let item_wire = WireNode {
                    kind: "item".into(),
                    key: None,
                    name: None,
                    navigation_title: None,
                    requires: None,
                    fallback: None,
                    children: None,
                    focus: None,
                    on_focus: None,
                    on_blur: None,
                    on_key: None,
                    rest: fields.clone(),
                };
                Ok(DropdownItem {
                    value: string(&item_wire, "value")?
                        .ok_or_else(|| unreadable("an item of its dropdown has no value"))?,
                    label: fields
                        .get("title")
                        .or_else(|| fields.get("label"))
                        .map(|label| match label {
                            Value::String(label) => Ok(Some(label.clone())),
                            _ => Err(unreadable("an item's title is not a string")),
                        })
                        .transpose()?
                        .flatten(),
                })
            })
            .collect::<Result<Vec<DropdownItem>, ReadError>>()?,
        Some(_) => return Err(unreadable("its items are not a list")),
    };
    Ok(ListDropdown {
        value: string(wire, "value")?,
        placeholder: string(wire, "placeholder")?,
        on_change: callback(wire, "onChange")?,
        items,
    })
}

/// One node held as another's property (a list item's `detail`), read at
/// `depth` and counting toward the document's nodes.
fn child_node(value: &Value, depth: usize, nodes: &mut usize) -> Result<Box<Node>, ReadError> {
    let wire: WireNode = serde_json::from_value(value.clone())
        .map_err(|error| ReadError::Unreadable(format!("its tree: {error}")))?;
    Ok(Box::new(node(wire, depth + 1, nodes)?))
}

/// A fit property, as an image's is; `contain` when the tree gives none.
fn fit_of(value: Option<&Value>) -> Result<Fit, ReadError> {
    match value {
        None | Some(Value::Null) => Ok(Fit::Contain),
        Some(Value::String(fit)) => match fit.as_str() {
            "contain" => Ok(Fit::Contain),
            "cover" => Ok(Fit::Cover),
            "fill" => Ok(Fit::Fill),
            other => Err(unreadable(&format!(
                "its fit is {other}; a fit is \"contain\", \"cover\" or \"fill\""
            ))),
        },
        Some(_) => Err(unreadable("its fit is not a fit")),
    }
}

/// The canvas a canvas node's properties give it: its operations, what it
/// is to assistive technology, and the handlers its input names. Its size
/// comes from its style's sizing, as any node's does.
fn canvas(wire: &WireNode) -> Result<Canvas, ReadError> {
    let rest = &wire.rest;
    let ops = match rest.get("ops") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(ops)) => {
            if ops.len() > MAX_CANVAS_OPS {
                return Err(ReadError::Guest(format!(
                    "a canvas of the view has {} operations; at most {MAX_CANVAS_OPS} are drawn",
                    ops.len()
                )));
            }
            ops.iter()
                .map(canvas_op)
                .collect::<Result<Vec<Option<CanvasOp>>, ReadError>>()?
                .into_iter()
                .flatten()
                .collect()
        }
        Some(_) => return Err(unreadable("a canvas node's ops are not a list")),
    };
    Ok(Canvas {
        ops,
        a11y: CanvasA11y {
            role: token(rest.get("role"), "role", canvas_role)?,
            label: string(wire, "label")?,
            value: string(wire, "value")?,
        },
        handlers: CanvasHandlers {
            on_increment: callback(wire, "onIncrement")?,
            on_decrement: callback(wire, "onDecrement")?,
            on_activate: callback(wire, "onActivate")?,
            on_pointer_down: callback(wire, "onPointerDown")?,
            on_pointer_up: callback(wire, "onPointerUp")?,
            on_pointer_move: callback(wire, "onPointerMove")?,
            on_pointer_enter: callback(wire, "onPointerEnter")?,
            on_pointer_leave: callback(wire, "onPointerLeave")?,
            on_wheel: callback(wire, "onWheel")?,
            on_double_click: callback(wire, "onDoubleClick")?,
            on_secondary: callback(wire, "onSecondary")?,
            on_resize: callback(wire, "onResize")?,
        },
    })
}

/// One drawing operation, as the tree gives it: an object naming its `op`
/// and the properties that op reads. An op this version does not know is
/// `None` — a newer minor version may add one — while one whose properties
/// are of the wrong type is unreadable, as a node's are.
fn canvas_op(value: &Value) -> Result<Option<CanvasOp>, ReadError> {
    let Value::Object(fields) = value else {
        return Err(unreadable("an operation of a canvas is not an object"));
    };
    let op = match fields.get("op") {
        Some(Value::String(op)) => op.as_str(),
        _ => return Err(unreadable("an operation of a canvas names no op")),
    };
    let at = |name: &str| coordinate(fields, name);
    let size = |name: &str| measure(fields, name);
    let paint = |name: &str| paint(fields.get(name), name);
    let stroked = || -> Result<CanvasStroke, ReadError> {
        Ok(CanvasStroke {
            color: paint("stroke")?.ok_or(unreadable("its stroke names no colour"))?,
            width: optional_measure(fields, "strokeWidth")?,
            cap: token(fields.get("cap"), "cap", stroke_cap)?,
            join: token(fields.get("join"), "join", stroke_join)?,
        })
    };
    let known = match op {
        "rect" => CanvasOp::Rect {
            x: at("x")?,
            y: at("y")?,
            width: at("width")?,
            height: at("height")?,
            radius: optional_measure(fields, "radius")?,
            fill: paint("fill")?,
            stroke: paint("stroke")?.is_some().then(stroked).transpose()?,
        },
        "circle" => CanvasOp::Circle {
            x: at("x")?,
            y: at("y")?,
            radius: size("radius")?,
            fill: paint("fill")?,
            stroke: paint("stroke")?.is_some().then(stroked).transpose()?,
        },
        "move" => CanvasOp::Move {
            x: at("x")?,
            y: at("y")?,
        },
        "line" => CanvasOp::Line {
            x: at("x")?,
            y: at("y")?,
        },
        "quad" => CanvasOp::Quad {
            cx: at("cx")?,
            cy: at("cy")?,
            x: at("x")?,
            y: at("y")?,
        },
        "cubic" => CanvasOp::Cubic {
            c1x: at("c1x")?,
            c1y: at("c1y")?,
            c2x: at("c2x")?,
            c2y: at("c2y")?,
            x: at("x")?,
            y: at("y")?,
        },
        "arc" => CanvasOp::Arc {
            x: at("x")?,
            y: at("y")?,
            radius: size("radius")?,
            start: angle(fields, "start")?,
            end: angle(fields, "end")?,
            ccw: match fields.get("ccw") {
                None | Some(Value::Null) => false,
                Some(Value::Bool(ccw)) => *ccw,
                Some(_) => return Err(unreadable("its arc's ccw is not a boolean")),
            },
        },
        "close" => CanvasOp::Close,
        "fill" => CanvasOp::Fill {
            color: paint("color")?.ok_or(unreadable("a fill names no colour"))?,
        },
        "stroke" => CanvasOp::Stroke {
            stroke: CanvasStroke {
                color: paint("color")?.ok_or(unreadable("a stroke names no colour"))?,
                width: optional_measure(fields, "width")?,
                cap: token(fields.get("cap"), "cap", stroke_cap)?,
                join: token(fields.get("join"), "join", stroke_join)?,
            },
        },
        "text" => {
            let content = match fields.get("text") {
                Some(Value::String(content)) => content.clone(),
                _ => return Err(unreadable("a text operation has no text")),
            };
            if content.chars().count() > MAX_CANVAS_TEXT_CHARS {
                return Err(ReadError::Guest(format!(
                    "a text of the canvas has {} characters; at most \
                     {MAX_CANVAS_TEXT_CHARS} are drawn",
                    content.chars().count()
                )));
            }
            CanvasOp::Text(CanvasText {
                x: at("x")?,
                y: at("y")?,
                content,
                style: token(fields.get("style"), "style", text_style)?,
                level: token(fields.get("level"), "level", text_level)?,
                color: paint("color")?,
                size: sized(fields.get("size"), "size")?,
                weight: weighted(fields.get("weight"), "weight")?,
            })
        }
        "image" => CanvasOp::Image {
            image: icon_of(fields.get("image"), "image")?,
            x: at("x")?,
            y: at("y")?,
            width: size("width")?,
            height: size("height")?,
        },
        "clip" => CanvasOp::Clip {
            x: at("x")?,
            y: at("y")?,
            width: size("width")?,
            height: size("height")?,
        },
        "translate" => CanvasOp::Translate {
            x: at("x")?,
            y: at("y")?,
        },
        "scale" => CanvasOp::Scale {
            x: at("x")?,
            y: at("y")?,
        },
        "rotate" => CanvasOp::Rotate {
            degrees: angle(fields, "degrees")?,
        },
        // An operation a newer minor version added: skipped, as a
        // property Pane does not know is.
        _ => return Ok(None),
    };
    Ok(Some(known))
}

/// One coordinate an operation names: any finite number, in the canvas's
/// own space, so a drawing may begin outside what its clip shows.
fn coordinate(fields: &Map<String, Value>, name: &str) -> Result<f32, ReadError> {
    match fields.get(name) {
        Some(Value::Number(number)) => number
            .as_f64()
            .filter(|number| number.is_finite())
            .map(|number| number as f32)
            .ok_or_else(|| unreadable(&format!("its {name} is not a coordinate"))),
        _ => Err(unreadable(&format!(
            "an operation of a canvas has no {name}"
        ))),
    }
}

/// One size an operation names, clamped to what a canvas draws.
fn measure(fields: &Map<String, Value>, name: &str) -> Result<Finite, ReadError> {
    optional_measure(fields, name)?
        .ok_or_else(|| unreadable(&format!("an operation of a canvas has no {name}")))
}

/// One size an operation may name, clamped to what a canvas draws.
fn optional_measure(fields: &Map<String, Value>, name: &str) -> Result<Option<Finite>, ReadError> {
    match fields.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(number)) => Ok(number
            .as_f64()
            .filter(|number| number.is_finite())
            .map(|number| Finite((number as f32).clamp(0., MAX_PX)))),
        Some(_) => Err(unreadable(&format!("its {name} is not a size"))),
    }
}

/// One angle an operation names: radians for an arc, degrees for a
/// rotation, any finite number.
fn angle(fields: &Map<String, Value>, name: &str) -> Result<f32, ReadError> {
    match fields.get(name) {
        Some(Value::Number(number)) => number
            .as_f64()
            .filter(|number| number.is_finite())
            .map(|number| number as f32)
            .ok_or_else(|| unreadable(&format!("its {name} is not an angle"))),
        _ => Err(unreadable(&format!(
            "an operation of a canvas has no {name}"
        ))),
    }
}

/// A canvas's role, as the tree names it.
fn canvas_role(name: &str) -> Option<CanvasRole> {
    match name {
        "color-well" => Some(CanvasRole::ColorWell),
        "slider" => Some(CanvasRole::Slider),
        "image" => Some(CanvasRole::Image),
        "figure" => Some(CanvasRole::Figure),
        "group" => Some(CanvasRole::Group),
        "generic" => Some(CanvasRole::Generic),
        _ => None,
    }
}

/// A stroke's cap, as the tree names it.
fn stroke_cap(name: &str) -> Option<StrokeCap> {
    match name {
        "butt" => Some(StrokeCap::Butt),
        "round" => Some(StrokeCap::Round),
        "square" => Some(StrokeCap::Square),
        _ => None,
    }
}

/// A stroke's join, as the tree names it.
fn stroke_join(name: &str) -> Option<StrokeJoin> {
    match name {
        "miter" => Some(StrokeJoin::Miter),
        "round" => Some(StrokeJoin::Round),
        "bevel" => Some(StrokeJoin::Bevel),
        _ => None,
    }
}

/// What a field node says around its control: its `title`, `info`,
/// `error` and `remember`.
fn field_props(wire: &WireNode) -> Result<FieldProps, ReadError> {
    Ok(FieldProps {
        title: string(wire, "title")?,
        info: string(wire, "info")?,
        // An empty error is none: the SDKs write a field's `error` with
        // the empty string while it is clean (its view's error closure
        // answers "" for every field), and nothing draws an empty
        // refusal under a field.
        error: string(wire, "error")?.filter(|error| !error.is_empty()),
        remember: boolean(wire, "remember")?.unwrap_or(false),
    })
}

/// The date field a date-picker or date-time-picker node's other
/// properties give it: its value, else its `default`.
fn date_field(wire: &WireNode) -> Result<DateField, ReadError> {
    Ok(DateField {
        value: string(wire, "value")?
            .or(string(wire, "default")?)
            .unwrap_or_default(),
        field: field_props(wire)?,
    })
}

/// The paths a file-picker or folder-picker node names, its `value` or
/// `default` — one path, or a list of them when it allows many.
fn file_picker(wire: &WireNode) -> Result<FilePicker, ReadError> {
    let paths = |name: &str| -> Result<Option<Vec<String>>, ReadError> {
        match wire.rest.get(name) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(path)) => Ok(Some(vec![path.clone()])),
            Some(Value::Array(paths)) => {
                let mut read = Vec::with_capacity(paths.len());
                for path in paths {
                    match path {
                        Value::String(path) => read.push(path.clone()),
                        _ => return Err(unreadable("a path of it is not a string")),
                    }
                }
                Ok(Some(read))
            }
            Some(_) => Err(unreadable("its paths are not a path or a list of them")),
        }
    };
    Ok(FilePicker {
        paths: paths("value")?.or(paths("default")?).unwrap_or_default(),
        multiple: boolean(wire, "multiple")?.unwrap_or(false),
        field: field_props(wire)?,
    })
}

/// A list of strings property, `Err` when it is not one.
fn texts(wire: &WireNode, name: &str) -> Result<Option<Vec<String>>, ReadError> {
    match wire.rest.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Array(values)) => Ok(Some(
            values
                .iter()
                .map(|value| match value {
                    Value::String(value) => Ok(value.clone()),
                    _ => Err(unreadable(&format!("a {name} entry is not a string"))),
                })
                .collect::<Result<Vec<String>, ReadError>>()?,
        )),
        Some(_) => Err(unreadable(&format!("its {name} is not a list"))),
    }
}

/// The style every node carries: its sizing, its surface, and its hover
/// and pressed variants.
fn style(wire: &WireNode) -> Result<Style, ReadError> {
    let rest = &wire.rest;
    let mut sizing = Sizing {
        grow: number(wire, "grow")?.map(|Finite(grow)| Finite(grow.max(0.))),
        shrink: number(wire, "shrink")?.map(|Finite(shrink)| Finite(shrink.max(0.))),
        basis: length(rest.get("basis"), "basis")?,
        width: length(rest.get("width"), "width")?,
        height: length(rest.get("height"), "height")?,
        min_width: length(rest.get("minWidth"), "minWidth")?,
        max_width: length(rest.get("maxWidth"), "maxWidth")?,
        min_height: length(rest.get("minHeight"), "minHeight")?,
        max_height: length(rest.get("maxHeight"), "maxHeight")?,
        aspect_ratio: None,
    };
    sizing.aspect_ratio = number(wire, "aspectRatio")?.filter(|Finite(ratio)| *ratio > 0.);
    Ok(Style {
        sizing,
        surface: surface_of(wire)?,
        hover: variant(rest.get("hover"))?,
        pressed: variant(rest.get("pressed"))?,
    })
}

/// The surface a node draws: `background`, `border`, `radius` and
/// `opacity`. `within` names the properties' owner for the messages, as
/// the variants name themselves.
fn surface_of(wire: &WireNode) -> Result<Surface, ReadError> {
    let rest = &wire.rest;
    let border = match rest.get("border") {
        None | Some(Value::Null) => None,
        Some(Value::Object(fields)) => Some(Border {
            width: length(fields.get("width"), "width")?,
            color: paint(fields.get("color"), "color")?,
        })
        .filter(|border| border.width.is_some() || border.color.is_some()),
        Some(_) => return Err(unreadable("its border is not a width and a colour")),
    };
    Ok(Surface {
        background: paint(rest.get("background"), "background")?,
        border,
        radius: radius(rest.get("radius"), "radius")?,
        opacity: number(wire, "opacity")?.map(|Finite(opacity)| Finite(opacity.clamp(0., 1.))),
    })
}

/// One variant of a node's surface, for `hover` or `pressed`: the surface
/// properties it restates, applied by Pane itself.
fn variant(value: Option<&Value>) -> Result<Option<Surface>, ReadError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let Value::Object(fields) = value else {
        return Err(unreadable(
            "its variant is not an object of surface properties",
        ));
    };
    let wire = WireNode {
        kind: "node".into(),
        key: None,
        name: None,
        navigation_title: None,
        requires: None,
        fallback: None,
        focus: None,
        on_focus: None,
        on_blur: None,
        on_key: None,
        children: None,
        rest: fields.clone(),
    };
    let surface = surface_of(&wire)?;
    Ok((!surface.background.is_none()
        || surface.border.is_some()
        || surface.radius.is_some()
        || surface.opacity.is_some())
    .then_some(surface))
}

/// A token property, as the tree gives it: `Err` when the value is not of
/// the property's type, `None` when it names a token this version does not
/// know. `read` maps the token's name to it.
fn token<T>(
    value: Option<&Value>,
    name: &str,
    read: fn(&str) -> Option<T>,
) -> Result<Option<T>, ReadError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let Value::String(token) = value else {
        return Err(ReadError::Unreadable(format!(
            "its {name} is not one of its names"
        )));
    };
    Ok(read(token))
}

/// A string property: `Err` when it is not a string.
fn string(wire: &WireNode, name: &str) -> Result<Option<String>, ReadError> {
    match wire.rest.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        Some(_) => Err(unreadable(&format!("its {name} is not a string"))),
    }
}

/// A boolean property: `Err` when it is not a boolean.
fn boolean(wire: &WireNode, name: &str) -> Result<Option<bool>, ReadError> {
    match wire.rest.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(value)) => Ok(Some(*value)),
        Some(_) => Err(unreadable(&format!("its {name} is not a boolean"))),
    }
}

/// A number property: `Err` when it is not a number.
fn number(wire: &WireNode, name: &str) -> Result<Option<Finite>, ReadError> {
    match wire.rest.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(value)) => Ok(value
            .as_f64()
            .filter(|value| value.is_finite())
            .map(|value| Finite(value as f32))),
        Some(_) => Err(unreadable(&format!("its {name} is not a number"))),
    }
}

/// A callback id property: `Err` when it is not one.
fn callback(wire: &WireNode, name: &str) -> Result<Option<u32>, ReadError> {
    match wire.rest.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(id)) => Ok(Some(
            id.as_u64()
                .filter(|id| *id <= u32::MAX as u64)
                .ok_or_else(|| unreadable(&format!("its {name} is not a callback id")))?
                as u32,
        )),
        Some(_) => Err(unreadable(&format!("its {name} is not a callback id"))),
    }
}

/// A length property: a space token, pixels, or a fraction of the parent
/// (`"1/2"`, `"50%"`), as a number or a string. Raw pixels are clamped to
/// 0–4096.
fn length(value: Option<&Value>, name: &str) -> Result<Option<Length>, ReadError> {
    let Some(value) = value else {
        return Ok(None);
    };
    length_of(value, name).map(Some)
}

/// One length value.
fn length_of(value: &Value, name: &str) -> Result<Length, ReadError> {
    let over = |why: String| unreadable(&format!("its {name} {why}"));
    match value {
        Value::Null => Err(over("is not a length".into())),
        Value::Number(number) => Ok(Length::Px(px_of(
            number
                .as_f64()
                .ok_or_else(|| over("is not a length".into()))? as f32,
        ))),
        Value::String(text) => {
            let text = text.trim();
            if let Some(space) = Space::named(text) {
                return Ok(Length::Space(space));
            }
            if let Some(fraction) = text
                .strip_suffix('%')
                .and_then(|digits| digits.trim().parse::<f32>().ok().map(|value| value / 100.))
            {
                return Ok(Length::Fraction(Finite(fraction.clamp(0., 1.))));
            }
            if let Some((top, bottom)) = text.split_once('/') {
                if let (Ok(top), Ok(bottom)) =
                    (top.trim().parse::<f32>(), bottom.trim().parse::<f32>())
                    && top >= 0.
                    && bottom > 0.
                {
                    return Ok(Length::Fraction(Finite((top / bottom).clamp(0., 1.))));
                }
                return Err(over("is not a fraction of the parent".into()));
            }
            let pixels = text
                .strip_suffix("px")
                .unwrap_or(text)
                .trim()
                .parse::<f32>()
                .map_err(|_| over("is not a length".into()))?;
            Ok(Length::Px(px_of(pixels)))
        }
        _ => Err(over("is not a length".into())),
    }
}

/// `value` as pixels, clamped to Pane's bounds.
fn px_of(value: f32) -> Finite {
    Finite(value.clamp(0., MAX_PX))
}

/// An icon size property: a token, or pixels.
fn extent(value: Option<&Value>, name: &str) -> Result<Option<IconExtent>, ReadError> {
    let Some(value) = value else {
        return Ok(None);
    };
    match value {
        Value::String(text) => {
            let text = text.trim();
            if let Some(size) = IconSize::named(text) {
                return Ok(Some(IconExtent::Token(size)));
            }
            let pixels = text
                .strip_suffix("px")
                .unwrap_or(text)
                .trim()
                .parse::<f32>()
                .map_err(|_| unreadable(&format!("its {name} is not a size")))?;
            Ok(Some(IconExtent::Px(px_of(pixels))))
        }
        Value::Number(number) => Ok(Some(IconExtent::Px(px_of(
            number
                .as_f64()
                .ok_or_else(|| unreadable(&format!("its {name} is not a size")))?
                as f32,
        )))),
        _ => Err(unreadable(&format!("its {name} is not a size"))),
    }
}

/// A radius property: a token, or pixels.
fn radius(value: Option<&Value>, name: &str) -> Result<Option<RadiusLength>, ReadError> {
    let Some(value) = value else {
        return Ok(None);
    };
    match value {
        Value::String(text) => {
            let text = text.trim();
            if let Some(radius) = Radius::named(text) {
                return Ok(Some(RadiusLength::Token(radius)));
            }
            let pixels = text
                .strip_suffix("px")
                .unwrap_or(text)
                .trim()
                .parse::<f32>()
                .map_err(|_| unreadable(&format!("its {name} is not a radius")))?;
            Ok(Some(RadiusLength::Px(px_of(pixels))))
        }
        Value::Number(number) => Ok(Some(RadiusLength::Px(px_of(
            number
                .as_f64()
                .ok_or_else(|| unreadable(&format!("its {name} is not a radius")))?
                as f32,
        )))),
        _ => Err(unreadable(&format!("its {name} is not a radius"))),
    }
}

/// A text's `size`: pixels, clamped to what a screen draws.
fn sized(value: Option<&Value>, name: &str) -> Result<Option<Finite>, ReadError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(number)) => Ok(number
            .as_f64()
            .map(|value| Finite((value as f32).clamp(1., 128.)))),
        Some(_) => Err(unreadable(&format!("its {name} is not a number"))),
    }
}

/// A text's `weight`: 100 to 900, clamped.
fn weighted(value: Option<&Value>, name: &str) -> Result<Option<Finite>, ReadError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(number)) => Ok(number
            .as_f64()
            .map(|value| Finite((value as f32).clamp(100., 900.)))),
        Some(_) => Err(unreadable(&format!("its {name} is not a number"))),
    }
}

/// One colour property: a tone or a raw colour (corrected), a
/// `{"light": ..., "dark": ...}` pair (corrected per appearance), or
/// `{"raw": ...}` — a colour, or a pair, drawn exactly as it is, with
/// contrast correction off. A string naming neither a tone nor a colour
/// is left out, as a token a later version may add.
fn paint(value: Option<&Value>, name: &str) -> Result<Option<Paint>, ReadError> {
    let Some(value) = value else {
        return Ok(None);
    };
    match value {
        // A name this version does not know: a token a later version may
        // add, left out.
        Value::String(text) => Ok(icons::color_of(text).map(|color| Paint {
            tint: Tint::Same(color),
            exact: false,
        })),
        _ => paint_of(value, name, false).map(Some),
    }
}

/// One colour value, `exact` when correction is off for it.
fn paint_of(value: &Value, name: &str, exact: bool) -> Result<Paint, ReadError> {
    let over = |why: String| unreadable(&format!("its {name} {why}"));
    match value {
        Value::String(text) => icons::color_of(text)
            .map(|color| Paint {
                tint: Tint::Same(color),
                exact,
            })
            .ok_or_else(|| over("is not a colour this Pane draws".into())),
        Value::Object(fields) => {
            if let Some(raw) = fields.get("raw") {
                return paint_of(raw, name, true);
            }
            match (fields.get("light"), fields.get("dark")) {
                (Some(Value::String(light)), Some(Value::String(dark))) => {
                    let read = |text: &str| {
                        icons::color_of(text)
                            .ok_or_else(|| over("names a colour this Pane does not draw".into()))
                    };
                    Ok(Paint {
                        tint: Tint::Pair {
                            light: read(light)?,
                            dark: read(dark)?,
                        },
                        exact,
                    })
                }
                (None, None) => Err(over("is not a colour".into())),
                _ => Err(over("names only one of a light and a dark colour".into())),
            }
        }
        _ => Err(over("is not a colour".into())),
    }
}

/// An icon property, read leniently as the list tree reads one: an icon
/// Pane cannot read is `None` and is not drawn. An inline `data:` URL over
/// the bound is the extension's error.
fn icon_of(value: Option<&Value>, _name: &str) -> Result<Option<Icon>, ReadError> {
    let Some(value) = value else {
        return Ok(None);
    };
    bounded_data(value)?;
    Ok(icons::read(value))
}

/// Whether `value`'s `data:` URLs stay within the bound of inline image
/// data, checking its fallbacks.
fn bounded_data(value: &Value) -> Result<(), ReadError> {
    match value {
        Value::Object(fields) => {
            if let Some(Value::String(url)) = fields.get("url")
                && icons::is_data_url(url)
                && url.len() > MAX_INLINE_IMAGE
            {
                return Err(ReadError::Guest(format!(
                    "an image of the view holds {} bytes of inline data; at most \
                     {MAX_INLINE_IMAGE} are read",
                    url.len()
                )));
            }
            if let Some(fallback) = fields.get("fallback") {
                bounded_data(fallback)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// A padding: a bare token for all four sides, or `{x, y}` or
/// `{top, right, bottom, left}` naming some of them. `Err` when the tree
/// gives something that is not one of these.
fn padding(value: Option<&Value>) -> Result<Padding, String> {
    let side = |fields: &Map<String, Value>, name: &str| match fields.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => space_of(value)
            .map(Some)
            .ok_or_else(|| format!("its padding's {name} is not a space token")),
    };
    match value {
        None | Some(Value::Null) => Ok(Padding::default()),
        Some(Value::Object(fields)) => {
            let mut padding = Padding {
                top: side(fields, "top")?,
                right: side(fields, "right")?,
                bottom: side(fields, "bottom")?,
                left: side(fields, "left")?,
            };
            let x = side(fields, "x")?;
            let y = side(fields, "y")?;
            padding.left = padding.left.or(x);
            padding.right = padding.right.or(x);
            padding.top = padding.top.or(y);
            padding.bottom = padding.bottom.or(y);
            Ok(padding)
        }
        Some(value) => space_of(value)
            .map(|all| Padding {
                top: Some(all),
                right: Some(all),
                bottom: Some(all),
                left: Some(all),
            })
            .ok_or_else(|| "its padding is not a space token or an object".into()),
    }
}

/// A place a stack's child is put at.
fn place_of(wire: &WireNode, name: &str) -> Result<Option<Place>, ReadError> {
    token(wire.rest.get(name), name, place)
}

/// An offset from a stack child's place: a length for both axes, or
/// `{x, y}`.
fn offset_of(wire: &WireNode) -> Result<Option<Offset>, ReadError> {
    match wire.rest.get("offset") {
        None | Some(Value::Null) => Ok(None),
        Some(value) => {
            let offset = match value {
                Value::Object(fields) => {
                    let x = match fields.get("x") {
                        None | Some(Value::Null) => Length::Px(Finite(0.)),
                        Some(value) => length_of(value, "offset")?,
                    };
                    let y = match fields.get("y") {
                        None | Some(Value::Null) => Length::Px(Finite(0.)),
                        Some(value) => length_of(value, "offset")?,
                    };
                    Offset { x, y }
                }
                value => {
                    let both = length_of(value, "offset")?;
                    Offset { x: both, y: both }
                }
            };
            Ok(Some(offset))
        }
    }
}

/// An orientation a scroll or divider takes.
fn orientation_of(wire: &WireNode, name: &str) -> Result<Option<Orientation>, ReadError> {
    token(wire.rest.get(name), name, orientation)
}

/// A space token, as the tree gives it: a string naming it, else nothing.
fn space_of(value: &Value) -> Option<Space> {
    let Value::String(token) = value else {
        return None;
    };
    Space::named(token)
}

fn place(name: &str) -> Option<Place> {
    match name {
        "top-start" | "top-left" => Some(Place::TopStart),
        "top" | "top-center" => Some(Place::Top),
        "top-end" | "top-right" => Some(Place::TopEnd),
        "start" | "left" | "middle-start" | "middle-left" => Some(Place::Start),
        "center" | "middle" | "middle-center" => Some(Place::Center),
        "end" | "right" | "middle-end" | "middle-right" => Some(Place::End),
        "bottom-start" | "bottom-left" => Some(Place::BottomStart),
        "bottom" | "bottom-center" => Some(Place::Bottom),
        "bottom-end" | "bottom-right" => Some(Place::BottomEnd),
        _ => None,
    }
}

fn orientation(name: &str) -> Option<Orientation> {
    match name {
        "vertical" => Some(Orientation::Vertical),
        "horizontal" => Some(Orientation::Horizontal),
        _ => None,
    }
}

fn align(name: &str) -> Option<Align> {
    match name {
        "start" => Some(Align::Start),
        "center" => Some(Align::Center),
        "end" => Some(Align::End),
        "stretch" => Some(Align::Stretch),
        "baseline" => Some(Align::Baseline),
        _ => None,
    }
}

fn justify(name: &str) -> Option<Justify> {
    match name {
        "start" => Some(Justify::Start),
        "center" => Some(Justify::Center),
        "end" => Some(Justify::End),
        "space-between" => Some(Justify::SpaceBetween),
        "space-around" => Some(Justify::SpaceAround),
        _ => None,
    }
}

fn text_style(name: &str) -> Option<TextStyle> {
    TextStyle::named(name)
}

fn text_level(name: &str) -> Option<TextLevel> {
    TextLevel::named(name)
}

fn tone(name: &str) -> Option<Tone> {
    match name {
        "default" => Some(Tone::Default),
        "secondary" => Some(Tone::Secondary),
        "ghost" => Some(Tone::Ghost),
        "accent" => Some(Tone::Accent),
        "destructive" => Some(Tone::Destructive),
        _ => None,
    }
}

/// A reading error, as the messages say it.
fn unreadable(message: &str) -> ReadError {
    ReadError::Unreadable(message.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::icons::Color;

    /// A minimal document with the given JSON for its root.
    fn tree(root: &str) -> Result<DesignedTree, ReadError> {
        DesignedTree::read(&format!(r#"{{"version":"1.0","root":{root}}}"#))
    }

    #[test]
    fn a_column_reads_with_its_layout_and_children() {
        let tree = tree(
            r#"{"type":"column","key":"main","gap":"m","padding":{"x":"l","y":"s"},
                "align":"center","justify":"space-between",
                "children":[
                    {"type":"text","text":"Count: 3","style":"title","level":"secondary"},
                    {"type":"row","gap":"xs","wrap":true,"children":[
                        {"type":"button","label":"Up","onPress":7},
                        {"type":"button","label":"Down","tone":"destructive","onPress":8}
                    ]}
                ]}"#,
        )
        .unwrap();

        let NodeKind::Column(layout) = &tree.root.kind else {
            panic!("a column: {:?}", tree.root);
        };
        assert_eq!(tree.root.key.as_deref(), Some("main"));
        assert_eq!(layout.gap, Some(Space::M));
        assert_eq!(layout.padding.left, Some(Space::L));
        assert_eq!(layout.padding.top, Some(Space::S));
        assert_eq!(layout.align, Some(Align::Center));
        assert_eq!(layout.justify, Some(Justify::SpaceBetween));
        assert!(!layout.wrap);
        let NodeKind::Text(text) = &tree.root.children[0].kind else {
            panic!("a text: {:?}", tree.root.children[0]);
        };
        assert_eq!(text.content, TextContent::Plain("Count: 3".into()));
        assert_eq!(text.style, Some(TextStyle::Title));
        assert_eq!(text.level, Some(TextLevel::Secondary));
        let row = &tree.root.children[1];
        let NodeKind::Row(row_layout) = &row.kind else {
            panic!("a row: {row:?}");
        };
        assert!(row_layout.wrap);
        let NodeKind::Button(up) = &row.children[0].kind else {
            panic!("a button: {:?}", row.children[0]);
        };
        assert_eq!(up.label, "Up");
        assert_eq!(up.on_press, Some(7));
        assert_eq!(up.tone, None);
        let NodeKind::Button(down) = &row.children[1].kind else {
            panic!("a button: {:?}", row.children[1]);
        };
        assert_eq!(down.tone, Some(Tone::Destructive));
    }

    #[test]
    fn padding_reads_a_bare_token_and_each_side() {
        let read = |root: &str| {
            tree(root)
                .map(|tree| match tree.root.kind {
                    NodeKind::Column(layout) => layout.padding,
                    _ => panic!("a column"),
                })
                .map_err(|error| error.message().to_owned())
        };
        assert_eq!(
            read(r#"{"type":"column","padding":"m"}"#).unwrap(),
            Padding {
                top: Some(Space::M),
                right: Some(Space::M),
                bottom: Some(Space::M),
                left: Some(Space::M),
            }
        );
        assert_eq!(
            read(r#"{"type":"column","padding":{"top":"xs","bottom":"l"}}"#).unwrap(),
            Padding {
                top: Some(Space::Xs),
                bottom: Some(Space::L),
                ..Padding::default()
            }
        );
        assert_eq!(read(r#"{"type":"column"}"#).unwrap(), Padding::default());
        assert_eq!(
            read(r#"{"type":"column","padding":5}"#).unwrap_err(),
            "its padding is not a space token or an object"
        );
        assert_eq!(
            read(r#"{"type":"column","padding":{"x":"huge"}}"#).unwrap_err(),
            "its padding's x is not a space token"
        );
    }

    #[test]
    fn the_layout_primitives_read_their_properties() {
        let tree = tree(
            r#"{"type":"stack","align":"center","children":[
                {"type":"image","image":{"builtin":"star"},"size":"l",
                 "place":"top-right","offset":{"x":8,"y":"4px"},"fit":"cover"},
                {"type":"scroll","orientation":"horizontal","children":[
                    {"type":"spacer","grow":2},
                    {"type":"divider"}
                ]},
                {"type":"divider","orientation":"vertical"}
            ]}"#,
        )
        .unwrap();
        assert!(matches!(tree.root.kind, NodeKind::Stack(Place::Center)));
        let image = &tree.root.children[0];
        let NodeKind::Image(image) = &image.kind else {
            panic!("an image: {image:?}");
        };
        assert_eq!(image.fit, Fit::Cover);
        assert_eq!(image.size, Some(IconExtent::Token(IconSize::L)));
        assert_eq!(tree.root.children[0].place, Some(Place::TopEnd));
        assert_eq!(
            tree.root.children[0].offset,
            Some(Offset {
                x: Length::Px(Finite(8.)),
                y: Length::Px(Finite(4.)),
            })
        );
        let scroll = &tree.root.children[1];
        assert!(matches!(
            scroll.kind,
            NodeKind::Scroll {
                orientation: Orientation::Horizontal
            }
        ));
        assert_eq!(scroll.children[0].style.sizing.grow, Some(Finite(2.)));
        assert!(matches!(scroll.children[0].kind, NodeKind::Spacer));
        assert!(matches!(scroll.children[1].kind, NodeKind::Divider { .. }));
        assert!(matches!(
            tree.root.children[2].kind,
            NodeKind::Divider {
                orientation: Orientation::Vertical
            }
        ));
    }

    #[test]
    fn every_node_carries_its_sizing_and_surface_with_variants() {
        let read = tree(
            r##"{"type":"text","text":"Hi","grow":1,"shrink":0,"basis":"40px",
                "width":"1/2","height":64,"minWidth":"s","maxWidth":4096,
                "aspectRatio":1.5,
                "background":"danger","border":{"width":"2px","color":"#0f0"},
                "radius":"full","opacity":0.8,
                "hover":{"background":{"raw":"#ff0000"}},"pressed":{"opacity":1}}"##,
        )
        .unwrap();
        let style = &read.root.style;
        assert_eq!(style.sizing.grow, Some(Finite(1.)));
        assert_eq!(style.sizing.shrink, Some(Finite(0.)));
        assert_eq!(style.sizing.basis, Some(Length::Px(Finite(40.))));
        assert_eq!(style.sizing.width, Some(Length::Fraction(Finite(0.5))));
        assert_eq!(style.sizing.height, Some(Length::Px(Finite(64.))));
        assert_eq!(style.sizing.min_width, Some(Length::Space(Space::S)));
        assert_eq!(style.sizing.max_width, Some(Length::Px(Finite(4096.))));
        assert_eq!(style.sizing.aspect_ratio, Some(Finite(1.5)));
        assert_eq!(
            style.surface.background,
            Some(Paint {
                tint: Tint::Same(Color::Tone(icons::Tone::Danger)),
                exact: false,
            })
        );
        assert_eq!(
            style.surface.border,
            Some(Border {
                width: Some(Length::Px(Finite(2.))),
                color: Some(Paint {
                    tint: Tint::Same(Color::Rgba(0x00FF00FF)),
                    exact: false,
                }),
            })
        );
        assert_eq!(
            style.surface.radius,
            Some(RadiusLength::Token(Radius::Full))
        );
        assert_eq!(style.surface.opacity, Some(Finite(0.8)));
        assert_eq!(
            style.hover.as_ref().unwrap().background,
            Some(Paint {
                tint: Tint::Same(Color::Rgba(0xFF0000FF)),
                exact: true,
            })
        );
        assert_eq!(style.pressed.as_ref().unwrap().opacity, Some(Finite(1.)));

        // Raw lengths clamp, opacity clamps, and a non-positive aspect
        // ratio is left out.
        let clamped = tree(
            r#"{"type":"text","text":"Hi","width":99999,"minWidth":-5,"opacity":9,
                "aspectRatio":0}"#,
        )
        .unwrap();
        let style = &clamped.root.style;
        assert_eq!(style.sizing.width, Some(Length::Px(Finite(4096.))));
        assert_eq!(style.sizing.min_width, Some(Length::Px(Finite(0.))));
        assert_eq!(style.surface.opacity, Some(Finite(1.)));
        assert_eq!(style.sizing.aspect_ratio, None);
    }

    #[test]
    fn colours_read_as_tones_raws_pairs_and_exact_raws() {
        let read = |root: &str| {
            let field = |tree: &DesignedTree| match &tree.root.kind {
                NodeKind::Text(text) => text.color,
                _ => panic!("a text"),
            };
            tree(root)
                .map(|tree| field(&tree))
                .map_err(|e| e.message().to_owned())
        };
        assert_eq!(
            read(r#"{"type":"text","text":"a","color":"success"}"#).unwrap(),
            Some(Paint {
                tint: Tint::Same(Color::Tone(icons::Tone::Success)),
                exact: false,
            })
        );
        assert_eq!(
            read(r#"{"type":"text","text":"a","color":"hsl(0, 100%, 50%)"}"#).unwrap(),
            Some(Paint {
                tint: Tint::Same(Color::Rgba(0xFF0000FF)),
                exact: false,
            })
        );
        assert_eq!(
            read(r##"{"type":"text","text":"a","color":{"light":"#111","dark":"secondary"}}"##)
                .unwrap(),
            Some(Paint {
                tint: Tint::Pair {
                    light: Color::Rgba(0x111111FF),
                    dark: Color::Tone(icons::Tone::Secondary),
                },
                exact: false,
            })
        );
        assert_eq!(
            read(r##"{"type":"text","text":"a","color":{"raw":{"light":"#111","dark":"#eee"}}}"##)
                .unwrap(),
            Some(Paint {
                tint: Tint::Pair {
                    light: Color::Rgba(0x111111FF),
                    dark: Color::Rgba(0xEEEEEEFF),
                },
                exact: true,
            })
        );
        // A name this version does not know is left out; a broken pair or
        // object says why.
        assert_eq!(
            read(r#"{"type":"text","text":"a","color":"sparkle"}"#).unwrap(),
            None
        );
        assert!(
            read(r##"{"type":"text","text":"a","color":{"light":"#111"}}"##)
                .unwrap_err()
                .contains("only one of a light and a dark")
        );
        assert!(
            read(r#"{"type":"text","text":"a","color":5}"#)
                .unwrap_err()
                .contains("is not a colour")
        );
    }

    #[test]
    fn the_shared_components_read_their_properties() {
        let tree = tree(
            r#"{"type":"column","children":[
                {"type":"text","spans":[
                    {"text":"Accept the "},
                    {"text":"terms","color":"accent","onPress":3},
                    {"text":" now","code":true}
                ]},
                {"type":"button","label":"Save","icon":{"builtin":"check"},
                 "keys":["ctrl","s"],"enabled":false,"onPress":1},
                {"type":"link","label":"Docs","onPress":2},
                {"type":"icon","icon":"star","size":18},
                {"type":"icon-tile","icon":{"builtin":"bell"}},
                {"type":"rich-row","title":"Pane","subtitle":"A launcher",
                 "icon":{"builtin":"blocks"},
                 "accessories":[{"text":"beta","tag":true,"color":"blue"}],"onPress":4},
                {"type":"keycap","key":"ctrl"},
                {"type":"key-sequence","keys":["ctrl","shift","p"]},
                {"type":"tag","text":"new","color":"magenta"},
                {"type":"badge","text":"3"},
                {"type":"toggle","on":true,"label":"Dark mode","onChange":5},
                {"type":"checkbox","checked":true,"label":"Remember","onChange":6},
                {"type":"segmented","options":[{"value":"a","label":"A"},{"value":"b"}],
                 "value":"b","onChange":7},
                {"type":"slider","value":0.4,"min":0,"max":2,"step":0.2,"onChange":8},
                {"type":"progress","value":0.6},
                {"type":"loading","label":"Loading"},
                {"type":"card","gap":"m","children":[{"type":"text","text":"in"}]},
                {"type":"section-header","title":"General","note":"How Pane starts"},
                {"type":"metadata-list","items":[
                    {"label":"Author","value":"Vu","onPress":9},
                    {"label":"Tags","tags":["one","two"]},
                    {"separator":true}
                ]},
                {"type":"empty-state","title":"Nothing","description":"Empty",
                 "icon":{"builtin":"search-none"}},
                {"type":"text-input","value":"typed","placeholder":"Type","onChange":10},
                {"type":"password-input","placeholder":"Password"},
                {"type":"text-area","value":"two\nlines"},
                {"type":"select","options":[{"value":"x","label":"X"}],"value":"x",
                 "onChange":11}
            ]}"#,
        )
        .unwrap();
        let nodes = &tree.root.children;
        let text = match &nodes[0].kind {
            NodeKind::Text(text) => text,
            _ => panic!("a text"),
        };
        let TextContent::Spans(spans) = &text.content else {
            panic!("spans: {:?}", text.content);
        };
        assert_eq!(spans.len(), 3);
        assert_eq!(spans[1].on_press, Some(3));
        assert!(spans[2].code);
        let NodeKind::Button(save) = &nodes[1].kind else {
            panic!("a button");
        };
        assert!(save.icon.is_some());
        assert_eq!(
            save.keys.as_deref(),
            Some(&["ctrl".to_owned(), "s".to_owned()][..])
        );
        assert!(!save.enabled);
        let NodeKind::Link(link) = &nodes[2].kind else {
            panic!("a link");
        };
        assert_eq!(link.on_press, Some(2));
        let NodeKind::Icon(icon) = &nodes[3].kind else {
            panic!("an icon");
        };
        assert_eq!(icon.size, Some(IconExtent::Px(Finite(18.))));
        assert!(matches!(&nodes[4].kind, NodeKind::IconTile(_)));
        let NodeKind::RichRow(row) = &nodes[5].kind else {
            panic!("a rich row");
        };
        assert_eq!(row.title, "Pane");
        assert_eq!(row.accessories.len(), 1);
        assert!(row.accessories[0].tag);
        assert!(matches!(&nodes[6].kind, NodeKind::Keycap(key) if key.key == "ctrl"));
        assert!(matches!(
            &nodes[7].kind,
            NodeKind::KeySequence(keys) if keys.keys.len() == 3
        ));
        let NodeKind::Tag(tag) = &nodes[8].kind else {
            panic!("a tag");
        };
        assert_eq!(tag.text, "new");
        assert!(matches!(&nodes[9].kind, NodeKind::Badge(badge) if badge.text == "3"));
        assert!(matches!(&nodes[10].kind, NodeKind::Toggle(t) if t.on));
        assert!(matches!(&nodes[11].kind, NodeKind::Checkbox(c) if c.checked));
        let NodeKind::Segmented(segmented) = &nodes[12].kind else {
            panic!("a segmented control");
        };
        assert_eq!(segmented.value.as_deref(), Some("b"));
        assert_eq!(segmented.options[1].label, None);
        let NodeKind::Slider(slider) = &nodes[13].kind else {
            panic!("a slider");
        };
        assert_eq!(slider.max, Finite(2.));
        assert!(matches!(&nodes[14].kind, NodeKind::Progress(p) if p.value == Finite(0.6)));
        assert!(matches!(&nodes[15].kind, NodeKind::Loading(l) if l.label.is_some()));
        assert!(matches!(&nodes[16].kind, NodeKind::Card(_)));
        assert!(matches!(&nodes[17].kind, NodeKind::SectionHeader(h) if h.note.is_some()));
        let NodeKind::MetadataList(list) = &nodes[18].kind else {
            panic!("a metadata list");
        };
        assert_eq!(list.items.len(), 3);
        assert_eq!(list.items[1].tags, vec!["one", "two"]);
        assert!(list.items[2].separator);
        assert!(matches!(&nodes[19].kind, NodeKind::EmptyState(e) if e.description.is_some()));
        assert!(matches!(&nodes[20].kind, NodeKind::TextInput(i) if i.value == "typed"));
        assert!(matches!(&nodes[21].kind, NodeKind::PasswordInput(p) if p.placeholder.is_some()));
        assert!(matches!(&nodes[22].kind, NodeKind::TextArea(a) if a.value == "two\nlines"));
        assert!(matches!(&nodes[23].kind, NodeKind::Select(s) if s.options.len() == 1));
    }

    #[test]
    fn markdown_reads_and_is_bounded() {
        let read =
            tree(r##"{"type":"markdown","markdown":"# Hi\n\nSome *prose*.\n\n- [x] done"}"##)
                .unwrap();
        let NodeKind::Markdown(markdown) = &read.root.kind else {
            panic!("markdown: {:?}", read.root.kind);
        };
        assert!(matches!(
            markdown.blocks[0],
            markdown::Block::Heading { level: 1, .. }
        ));
        assert!(matches!(markdown.blocks[2], markdown::Block::List { .. }));
        let long = format!(
            r#"{{"type":"markdown","markdown":"{}"}}"#,
            "a".repeat(MAX_MARKDOWN_CHARS + 1)
        );
        assert!(
            tree(&long).unwrap_err().message().contains("markdown has"),
            "{}",
            tree(&long).unwrap_err().message()
        );
    }

    #[test]
    fn unknown_nodes_fall_back_or_draw_their_children() {
        let tree = tree(
            r#"{"type":"marquee","children":[
                {"type":"marquee","fallback":{"type":"text","text":"Buy now"}},
                {"type":"marquee","children":[{"type":"text","text":"Hi"}]},
                {"type":"video","requires":9}
            ]}"#,
        )
        .unwrap();

        // An unknown node with a fallback draws it; without one its
        // children; one whose `requires` Pane does not meet is unknown too,
        // and with no fallback and no children draws nothing.
        assert!(matches!(&tree.root.kind, NodeKind::Unknown(name) if name == "marquee"));
        assert!(matches!(
            tree.root.children[0]
                .fallback
                .as_ref()
                .map(|fallback| fallback.kind.clone()),
            Some(NodeKind::Text(_))
        ));
        assert!(matches!(
            &tree.root.children[1].children[0].kind,
            NodeKind::Text(_)
        ));
        let video = &tree.root.children[2];
        assert!(matches!(video.kind, NodeKind::Unknown(_)));
        assert!(video.fallback.is_none());
        assert!(video.children.is_empty());
    }

    #[test]
    fn a_newer_minor_reads_and_another_major_is_refused() {
        // A newer minor: read for what Pane understands.
        let newer = DesignedTree::read(
            r#"{"version":"1.9","extra":true,"root":{"type":"column","novelty":{},
                "children":[{"type":"text","text":"Hi","sparkle":true}]}}"#,
        )
        .unwrap();
        assert!(matches!(newer.root.kind, NodeKind::Column(_)));

        // Another major: refused, naming both versions.
        let error = DesignedTree::read(r#"{"version":"2.0","root":{"type":"column"}}"#)
            .unwrap_err()
            .message()
            .to_owned();
        assert!(error.contains("UI component set 2.0"), "{error}");

        // A version Pane cannot read.
        let error = DesignedTree::read(r#"{"version":"one","root":{"type":"column"}}"#)
            .unwrap_err()
            .message()
            .to_owned();
        assert!(error.contains("version"), "{error}");
    }

    #[test]
    fn trees_over_a_limit_say_what_the_limit_is() {
        let over = |root: &str| tree(root).unwrap_err().message().to_owned();
        // Depth: 64 levels are drawn, 65 are not.
        let nest = |levels: usize| {
            let inner = r#"{"type":"text","text":"a"}"#;
            (0..levels).fold(inner.to_owned(), |nested, _| {
                format!(r#"{{"type":"column","children":[{nested}]}}"#)
            })
        };
        assert!(
            tree(&nest(MAX_DEPTH - 1)).is_ok(),
            "a tree 64 levels deep is drawn: {:?}",
            tree(&nest(MAX_DEPTH - 1))
        );
        let error = over(&nest(MAX_DEPTH));
        assert!(
            error.contains("65 levels deep; at most 64 are drawn"),
            "{error}"
        );
        // Text: one node with too much in it.
        let long = format!(
            r#"{{"type":"text","text":"{}"}}"#,
            "a".repeat(MAX_TEXT_CHARS + 1)
        );
        assert!(
            over(&long).contains(&format!("characters; at most {MAX_TEXT_CHARS} are drawn")),
            "{}",
            over(&long)
        );
        // Nodes: one more than the ceiling.
        let many = vec![r#"{"type":"text","text":"a"}"#; MAX_NODES + 1].join(",");
        let many = format!(r#"{{"type":"column","children":[{many}]}}"#);
        assert!(
            over(&many).contains("10001 nodes; at most 10000 are drawn"),
            "{}",
            over(&many)
        );
        // Inline image data: a data URL over the bound.
        let big = format!(
            r#"{{"type":"icon","icon":{{"url":"data:image/png;base64,{}"}}}}"#,
            "a".repeat(MAX_INLINE_IMAGE + 1)
        );
        assert!(
            over(&big).contains("bytes of inline data"),
            "{}",
            over(&big)
        );
    }

    #[test]
    fn a_tree_pane_cannot_read_says_why() {
        let error = |document: &str| {
            DesignedTree::read(document)
                .unwrap_err()
                .message()
                .to_owned()
        };
        for (document, why) in [
            ("not json", "its tree:"),
            (r#"{"root":{}}"#, "missing field"),
            (r#"{"version":"1.0"}"#, "missing field `root`"),
            (r#"{"version":"1.0","root":{}}"#, "missing field `type`"),
            (
                r#"{"version":"1.0","root":{"type":"text"}}"#,
                "a text node has no text",
            ),
            (
                r#"{"version":"1.0","root":{"type":"text","text":5}}"#,
                "its text is not a string",
            ),
            (
                r#"{"version":"1.0","root":{"type":"button"}}"#,
                "a button node has no label",
            ),
            (
                r#"{"version":"1.0","root":{"type":"button","label":"Up","onPress":"up"}}"#,
                "its onPress is not a callback id",
            ),
            (
                r#"{"version":"1.0","root":{"type":"column","gap":12}}"#,
                "its gap is not one of its names",
            ),
            (
                r#"{"version":"1.0","root":{"type":"column","wrap":"yes"}}"#,
                "its wrap is not a boolean",
            ),
            (
                r#"{"version":"1.0","root":{"type":"column","children":"none"}}"#,
                "invalid type",
            ),
            (
                r#"{"version":"1.0","root":{"type":"text","text":"a","width":"lots"}}"#,
                "its width is not a length",
            ),
            (
                r#"{"version":"1.0","root":{"type":"text","text":"a","opacity":"high"}}"#,
                "its opacity is not a number",
            ),
            (
                r#"{"version":"1.0","root":{"type":"image","fit":"stretch"}}"#,
                "a fit is",
            ),
            (
                r#"{"version":"1.0","root":{"type":"markdown"}}"#,
                "a markdown node has no markdown",
            ),
            (
                r#"{"version":"1.0","root":{"type":"select"}}"#,
                "it has no options",
            ),
            (
                r#"{"version":"1.0","root":{"type":"progress"}}"#,
                "a progress node has no value",
            ),
        ] {
            let message = error(document);
            assert!(message.contains(why), "{document}: {message}");
        }
    }

    #[test]
    fn an_unknown_token_or_property_is_left_out() {
        // A token a newer minor added, or a property Pane does not know: the
        // field is treated as absent, never the tree unreadable.
        let tree = tree(
            r#"{"type":"column","gap":"huge","align":"diag","tooltip":"Later",
                "children":[{"type":"text","text":"Hi","style":"fancy","level":"dim"},
                    {"type":"button","label":"Go","tone":"neon","onPress":1}]}"#,
        )
        .unwrap();
        let NodeKind::Column(layout) = tree.root.kind else {
            panic!("a column");
        };
        assert_eq!(layout.gap, None);
        assert_eq!(layout.align, None);
        let NodeKind::Text(text) = &tree.root.children[0].kind else {
            panic!("a text");
        };
        assert_eq!(text.style, None);
        assert_eq!(text.level, None);
        let NodeKind::Button(button) = &tree.root.children[1].kind else {
            panic!("a button");
        };
        assert_eq!(button.tone, None);
    }
}
