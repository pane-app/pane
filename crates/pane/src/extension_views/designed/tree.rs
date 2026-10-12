//! The tree's layout: how the walk draws each node, the layout
//! primitives (`column`, `row`, `stack`, `scroll`, `spacer`, `divider`)
//! and the [`Style`] every node carries — its sizing and its surface,
//! with the `hover` and `pressed` variants Pane applies itself. Layout is
//! computed by GPUI's flex layout; nothing is laid out by the extension.

use std::collections::{HashMap, HashSet};

use gpui::prelude::*;
use gpui::{
    AnyElement, Context, Div, Hsla, Length as GpuiLength, Pixels, Role, Stateful, div, px, relative,
};

use pane_core::{
    Align, DesignedTree, Finite, Justify, Layout as NodeLayout, Length as NodeLength, Node,
    NodeKind, Orientation, Paint, Place, RadiusLength, Sizing, Surface,
};

use crate::app::LauncherWindow;
use crate::ui::theme::Theme;
use crate::ui::tokens;

use super::canvas;
use super::components::{self, FieldKind};
use super::fields;
use super::reconcile::{FieldEvents, Held, KeyedState};

/// What one node's drawing carries with it down the tree.
#[derive(Clone, Copy)]
pub(super) struct Draw<'a> {
    pub theme: &'a Theme,
    /// The keyed state of the tree's stateful nodes, by their paths: the
    /// focus each control holds, the editing state each field keeps, the
    /// select's own state, the scroll regions' positions.
    pub state: &'a HashMap<String, KeyedState>,
    /// The render whose tree is being drawn — the events its controls
    /// raise carry it (the tree the user saw).
    pub render: u64,
    /// The surface under the node: the panel, or the background the node
    /// it sits in drew, composited over it — what a raw text or icon
    /// colour is corrected against.
    pub surface: Hsla,
}

impl Draw<'_> {
    /// The focus handle of the node at `path`, when its state holds one.
    pub(super) fn focus_of(&self, path: &str) -> Option<gpui::FocusHandle> {
        self.state.get(path).and_then(|state| state.focus_handle())
    }

    /// The keyed field at `path`: its editing state, its focus, and the
    /// bookkeeping of its input events.
    pub(super) fn field(
        &self,
        path: &str,
    ) -> Option<(
        &gpui::Entity<gpui_elements::editable_text::EditableTextState>,
        &gpui::FocusHandle,
        &FieldEvents,
    )> {
        match &self.state.get(path)?.held {
            Held::Field {
                editing,
                focus,
                events,
            } => Some((editing, focus, events)),
            _ => None,
        }
    }

    /// The keyed select at `path`: the searchable select entity holding
    /// its open state, query and highlight.
    pub(super) fn select(&self, path: &str) -> Option<&gpui::Entity<crate::ui::select::Select>> {
        match &self.state.get(path)?.held {
            Held::Select { select, .. } => Some(select),
            _ => None,
        }
    }
}

/// Appends the place of the child at `index` with `key` to `path`.
pub(super) fn push(path: &mut String, key: Option<&str>, index: usize) {
    path.push('/');
    match key {
        Some(key) => path.push_str(key),
        None => path.push_str(&index.to_string()),
    }
}

/// Appends the place of the child at `index` to `path`: its key, unless a
/// sibling shares it (Pane's positional fallback for duplicate keys,
/// which development reports).
pub(super) fn place_child(
    path: &mut String,
    child: &Node,
    index: usize,
    duplicates: &HashSet<&str>,
) {
    let held = child.key.as_deref().filter(|key| !duplicates.contains(key));
    push(path, held, index);
}

/// As [`place_child`], for a set of owned keys (the designed list's
/// presentation holds its own).
pub(super) fn place_child_owned(
    path: &mut String,
    child: &Node,
    index: usize,
    duplicates: &std::collections::HashSet<String>,
) {
    let held = child
        .key
        .as_deref()
        .filter(|key| !duplicates.contains(*key));
    push(path, held, index);
}

/// The keys `parent`'s children share with a sibling: those children are
/// matched by position instead of by key.
pub(super) fn duplicate_keys(parent: &Node) -> HashSet<&str> {
    let mut counts: HashMap<&str, u32> = HashMap::new();
    for child in &parent.children {
        if let Some(key) = child.key.as_deref() {
            *counts.entry(key).or_default() += 1;
        }
    }
    counts
        .into_iter()
        .filter(|(_, count)| *count > 1)
        .map(|(key, _)| key)
        .collect()
}

/// The first list or grid in `tree`, with its path by the walk's grammar
/// (the root's segment first, every child's after): the view's list, whose
/// search field, selection, rows, dropdown, empty view and detail pane the
/// window draws (#240). `None` on a tree that names none.
pub(super) fn find_list(tree: &DesignedTree) -> Option<(String, &Node)> {
    fn held<'a>(node: &'a Node, path: &mut String) -> Option<(String, &'a Node)> {
        let start = path.len();
        let found = match &node.kind {
            NodeKind::List(_) | NodeKind::Grid(_) => Some(node),
            _ => None,
        };
        if let Some(found) = found {
            return Some((path.clone(), found));
        }
        let duplicates = duplicate_keys(node);
        for (index, child) in node.children.iter().enumerate() {
            place_child(path, child, index, &duplicates);
            if let Some(found) = held(child, path) {
                return Some(found);
            }
            path.truncate(start);
        }
        if let Some(fallback) = node.fallback.as_deref() {
            path.push_str("/fallback");
            if let Some(found) = held(fallback, path) {
                return Some(found);
            }
            path.truncate(start);
        }
        None
    }
    let mut path = String::new();
    push(&mut path, tree.root.key.as_deref(), 0);
    held(&tree.root, &mut path)
}

/// The paths of every item of the list or grid at `list_path` (its
/// sections' children and its own, by the walk's grammar), and the path
/// of its search-bar dropdown and its empty view: keyed by the item's
/// key, as the presentation's rows name them.
pub(super) fn list_paths(
    node: &Node,
    list_path: &str,
) -> (Vec<(String, String)>, Option<String>, Option<String>) {
    let mut items = Vec::new();
    let mut dropdown = None;
    let mut empty = None;
    let duplicates = duplicate_keys(node);
    for (index, child) in node.children.iter().enumerate() {
        let mut path = list_path.to_owned();
        place_child(&mut path, child, index, &duplicates);
        match &child.kind {
            NodeKind::ListSection(_) => {
                let inner = duplicate_keys(child);
                for (at, item) in child.children.iter().enumerate() {
                    let mut item_path = path.clone();
                    place_child(&mut item_path, item, at, &inner);
                    if let Some(key) = item.key.clone() {
                        items.push((key, item_path));
                    } else {
                        items.push((at.to_string(), item_path));
                    }
                }
            }
            NodeKind::ListItem(_) | NodeKind::GridItem(_) => {
                let key = child.key.clone().unwrap_or_else(|| index.to_string());
                items.push((key, path));
            }
            NodeKind::ListDropdown(_) => dropdown = Some(path),
            _ => empty = Some(path),
        }
    }
    (items, dropdown, empty)
}

/// The node `path` names in `tree`, reached by the tree walk's grammar:
/// each segment is a child's key, else its index — the key preferred when
/// both spell a segment, as a keyed child's place is.
pub(super) fn node_at<'a>(tree: &'a DesignedTree, path: &str) -> Option<&'a Node> {
    let mut segments = path.trim_start_matches('/').split('/');
    // The path's first segment is the root's own place — the key it
    // gave, or the index 0 the path builds a keyless root's from — not
    // one of its children's.
    let root = segments.next()?;
    let named = match tree.root.key.as_deref() {
        Some(key) => key == root,
        None => root == "0",
    };
    if !named {
        return None;
    }
    let mut node = &tree.root;
    for segment in segments {
        if segment.is_empty() {
            continue;
        }
        let child = node
            .children
            .iter()
            .find(|child| child.key.as_deref() == Some(segment))
            .or_else(|| {
                segment
                    .parse::<usize>()
                    .ok()
                    .and_then(|index| node.children.get(index))
                    .filter(|child| child.key.is_none())
            });
        node = child?;
    }
    Some(node)
}

/// Whether the node at `path` sits inside a form of `tree` (#241): its
/// ancestors from the root include a `form` node.
pub(super) fn inside_form(tree: &DesignedTree, path: &str) -> bool {
    let mut node = &tree.root;
    let mut inside = false;
    for segment in path.trim_start_matches('/').split('/') {
        if segment.is_empty() {
            continue;
        }
        if matches!(node.kind, NodeKind::Form(_)) {
            inside = true;
        }
        let duplicates = duplicate_keys(node);
        let child = node
            .children
            .iter()
            .enumerate()
            .find(|(index, child)| {
                let key = child.key.as_deref().filter(|key| !duplicates.contains(key));
                key == Some(segment) || (child.key.is_none() && index.to_string() == segment)
            })
            .map(|(_, child)| child);
        let Some(found) = child else {
            return inside;
        };
        node = found;
    }
    inside || matches!(node.kind, NodeKind::Form(_))
}

/// One node of the tree, drawn: `path` is the node's place in the tree
/// (its key, else its index among its siblings), already ending with its
/// own segment, which keeps the focus of the controls Pane drew before.
pub(super) fn draw_node(
    node: &Node,
    path: &mut String,
    draw: Draw,
    cx: &mut Context<LauncherWindow>,
) -> AnyElement {
    let start = path.len();
    let name = node.name.clone();
    // The surface the node's children draw on: its own background, over
    // the one it draws on.
    let inner = with_surface(draw, node.style.surface.background.as_ref(), draw.theme);
    let drawn = match &node.kind {
        // A node Pane does not know: the `fallback` the tree gave, drawn
        // in its place, else its children, drawn as they are.
        NodeKind::Unknown(_) => match &node.fallback {
            Some(fallback) => {
                let element = draw_node(fallback, path, inner, cx);
                styled(node, path, draw, element)
            }
            None => {
                let children = children(node, path, inner, cx);
                let group = div()
                    .id(path.clone())
                    .flex()
                    .flex_col()
                    .map(|group| named(group, name.as_deref()))
                    .children(children);
                styled(node, path, draw, group.into_any_element())
            }
        },
        NodeKind::Column(layout) => {
            let own = path.clone();
            let children = children(node, path, inner, cx);
            let div = apply(container(true, layout, children).id(own), node, &draw);
            named(div, name.as_deref()).into_any_element()
        }
        NodeKind::Card(layout) => {
            let own = path.clone();
            let children = children(node, path, inner, cx);
            let div = apply(card(layout, children, draw.theme).id(own), node, &draw);
            named(div, name.as_deref()).into_any_element()
        }
        NodeKind::Row(layout) => {
            let own = path.clone();
            let children = children(node, path, inner, cx);
            let div = apply(container(false, layout, children).id(own), node, &draw);
            named(div, name.as_deref()).into_any_element()
        }
        NodeKind::Stack(align) => {
            let own = path.clone();
            let children = stack_children(node, path, inner, cx, *align);
            let div = apply(stack(children).id(own), node, &draw);
            named(div, name.as_deref()).into_any_element()
        }
        NodeKind::Scroll { orientation } => {
            let own = path.clone();
            let children = children(node, path, inner, cx);
            let tracked = draw.state.get(&own).and_then(|state| match &state.held {
                Held::Scroll(handle) => Some(handle.clone()),
                _ => None,
            });
            let div = apply(
                scroll(*orientation, children, own, tracked).map(|div| named(div, name.as_deref())),
                node,
                &draw,
            );
            div.into_any_element()
        }
        NodeKind::Spacer => {
            let own = path.clone();
            apply(div().id(own).flex_grow(1.), node, &draw).into_any_element()
        }
        NodeKind::Divider { orientation } => {
            let own = path.clone();
            apply(divider(*orientation, draw.theme).id(own), node, &draw).into_any_element()
        }
        NodeKind::Text(text) => {
            let element = components::text(text, node.key.as_deref(), path, &draw, cx);
            let element = match name.as_deref() {
                Some(name) => element.aria_label(name),
                None => element,
            };
            styled(node, path, draw, element.into_any_element())
        }
        NodeKind::Button(button) => styled(
            node,
            path,
            draw,
            components::button(node, button, path, &draw, cx).into_any_element(),
        ),
        NodeKind::Link(link) => styled(
            node,
            path,
            draw,
            components::link(link, node.key.as_deref(), path, &draw, cx).into_any_element(),
        ),
        NodeKind::Icon(icon) => styled(
            node,
            path,
            draw,
            components::icon(icon, path, &draw).into_any_element(),
        ),
        NodeKind::IconTile(icon) => styled(
            node,
            path,
            draw,
            components::icon_tile(icon, path, &draw).into_any_element(),
        ),
        NodeKind::Image(image) => styled(
            node,
            path,
            draw,
            components::image(node, image, path, &draw, cx).into_any_element(),
        ),
        NodeKind::RichRow(row) => styled(
            node,
            path,
            draw,
            components::rich_row(node, row, path, &draw, cx).into_any_element(),
        ),
        NodeKind::Keycap(keycap) => styled(
            node,
            path,
            draw,
            components::keycap(keycap, path, draw.theme).into_any_element(),
        ),
        NodeKind::KeySequence(keys) => styled(
            node,
            path,
            draw,
            components::key_sequence(keys, path, draw.theme).into_any_element(),
        ),
        NodeKind::Tag(tag) => styled(
            node,
            path,
            draw,
            components::tag(tag, path, &draw).into_any_element(),
        ),
        NodeKind::Badge(badge) => styled(
            node,
            path,
            draw,
            components::badge(badge, path, &draw).into_any_element(),
        ),
        NodeKind::Toggle(toggle) => styled(
            node,
            path,
            draw,
            components::toggle(toggle, node.key.as_deref(), path, &draw, cx).into_any_element(),
        ),
        NodeKind::Checkbox(checkbox) => styled(
            node,
            path,
            draw,
            components::checkbox(checkbox, node.key.as_deref(), path, &draw, cx).into_any_element(),
        ),
        NodeKind::Segmented(segmented) => styled(
            node,
            path,
            draw,
            components::segmented(segmented, node.key.as_deref(), path, &draw, cx)
                .into_any_element(),
        ),
        NodeKind::Slider(slider) => styled(
            node,
            path,
            draw,
            components::slider(slider, node.key.as_deref(), path, &draw, cx).into_any_element(),
        ),
        NodeKind::Progress(progress) => styled(
            node,
            path,
            draw,
            components::progress(progress, path, draw.theme).into_any_element(),
        ),
        NodeKind::Loading(loading) => styled(
            node,
            path,
            draw,
            components::loading(loading, path, draw.theme).into_any_element(),
        ),
        NodeKind::Markdown(markdown) => styled(
            node,
            path,
            draw,
            components::markdown(markdown, path, &draw, cx).into_any_element(),
        ),
        NodeKind::SectionHeader(header) => styled(
            node,
            path,
            draw,
            components::section_header(header, path, draw.theme).into_any_element(),
        ),
        NodeKind::MetadataList(list) => styled(
            node,
            path,
            draw,
            components::metadata_list(list, node.key.as_deref(), path, &draw, cx)
                .into_any_element(),
        ),
        NodeKind::EmptyState(empty) => styled(
            node,
            path,
            draw,
            components::empty_state(node, empty, path, &draw, cx).into_any_element(),
        ),
        NodeKind::TextInput(input) => styled(
            node,
            path,
            draw,
            components::text_input(input, path, &draw, FieldKind::Text, cx).into_any_element(),
        ),
        NodeKind::PasswordInput(input) => styled(
            node,
            path,
            draw,
            components::text_input(input, path, &draw, FieldKind::Password, cx).into_any_element(),
        ),
        NodeKind::TextArea(input) => styled(
            node,
            path,
            draw,
            components::text_input(input, path, &draw, FieldKind::Area, cx).into_any_element(),
        ),
        NodeKind::Select(select) => styled(
            node,
            path,
            draw,
            components::select(select, path, &draw, cx).into_any_element(),
        ),
        NodeKind::Form(form) => {
            let children = children(node, path, inner, cx);
            styled(
                node,
                path,
                draw,
                fields::form(node, form, path, &draw, children, cx),
            )
        }
        NodeKind::DatePicker(date) => styled(
            node,
            path,
            draw,
            fields::date_field(node, date, path, &draw, false, cx),
        ),
        NodeKind::DateTimePicker(date) => styled(
            node,
            path,
            draw,
            fields::date_field(node, date, path, &draw, true, cx),
        ),
        NodeKind::TagPicker(picker) => styled(
            node,
            path,
            draw,
            fields::tag_field(node, picker, path, &draw, cx),
        ),
        NodeKind::FilePicker(picker) => styled(
            node,
            path,
            draw,
            fields::path_field(node, picker, path, &draw, false, cx),
        ),
        NodeKind::FolderPicker(picker) => styled(
            node,
            path,
            draw,
            fields::path_field(node, picker, path, &draw, true, cx),
        ),
        // A List or Grid the launcher does not present — a second one in
        // the tree, under the first, which the screen presents (#240) —
        // draws its rows as a plain column, the launcher's own rows list
        // carrying the presented one.
        NodeKind::List(_) | NodeKind::Grid(_) => {
            let own = path.clone();
            let drawn = children(node, path, inner, cx);
            let div =
                apply(div().id(own).flex().flex_col().min_w(px(0.)), node, &draw).children(drawn);
            named(div, name.as_deref()).into_any_element()
        }
        NodeKind::ListSection(_) => {
            let own = path.clone();
            let drawn = children(node, path, inner, cx);
            let div = apply(
                div()
                    .id(own)
                    .flex()
                    .flex_col()
                    .min_w(px(0.))
                    .gap(tokens::space(pane_core::Space::Xs)),
                node,
                &draw,
            )
            .children(drawn);
            named(div, name.as_deref()).into_any_element()
        }
        NodeKind::ListItem(item) => styled(
            node,
            path,
            draw,
            components::list_item(node, item, path, &draw, cx).into_any_element(),
        ),
        NodeKind::GridItem(item) => styled(
            node,
            path,
            draw,
            components::grid_item(item, path, &draw, cx).into_any_element(),
        ),
        NodeKind::ListDropdown(_) => {
            // The presented list's dropdown draws in its header; one
            // nested elsewhere draws its choices as a plain column.
            let own = path.clone();
            let drawn = children(node, path, inner, cx);
            let div =
                apply(div().id(own).flex().flex_col().min_w(px(0.)), node, &draw).children(drawn);
            named(div, name.as_deref()).into_any_element()
        }
        NodeKind::Detail(_) => {
            let own = path.clone();
            let children = children(node, path, inner, cx);
            let div = apply(
                scroll(Orientation::Vertical, children, own, None),
                node,
                &draw,
            );
            div.into_any_element()
        }
        NodeKind::Canvas(held) => styled(
            node,
            path,
            draw,
            canvas::canvas(node, held, path, &draw, cx).into_any_element(),
        ),
    };
    path.truncate(start);
    drawn
}

/// `draw`, carrying the surface `background` draws over the one it draws
/// on.
pub(super) fn with_surface<'a>(
    draw: Draw<'a>,
    background: Option<&Paint>,
    theme: &Theme,
) -> Draw<'a> {
    let surface = match background {
        Some(paint) => {
            let background = tokens::paint_color(paint, theme);
            use gpui::ColorExt as _;
            background.blend(&draw.surface)
        }
        None => draw.surface,
    };
    Draw { surface, ..draw }
}

/// Gives `node` the group role and name assistive technology reads it by:
/// every node is reported, with its own name when the tree gave one.
fn named(node: Stateful<Div>, name: Option<&str>) -> Stateful<Div> {
    node.role(Role::Group)
        .when_some(name, |node, name| node.aria_label(name))
}

/// The children of a container, each drawn with its place in `path`.
fn children(
    parent: &Node,
    path: &mut String,
    draw: Draw,
    cx: &mut Context<LauncherWindow>,
) -> Vec<AnyElement> {
    let duplicates = duplicate_keys(parent);
    parent
        .children
        .iter()
        .enumerate()
        .map(|(index, child)| {
            let start = path.len();
            place_child(path, child, index, &duplicates);
            let drawn = draw_node(child, path, draw, cx);
            path.truncate(start);
            drawn
        })
        .collect()
}

/// The children of a `stack`, each wrapped in the full-size layer its
/// place puts it in.
fn stack_children(
    parent: &Node,
    path: &mut String,
    draw: Draw,
    cx: &mut Context<LauncherWindow>,
    default: Place,
) -> Vec<AnyElement> {
    let duplicates = duplicate_keys(parent);
    parent
        .children
        .iter()
        .enumerate()
        .map(|(index, child)| {
            let start = path.len();
            place_child(path, child, index, &duplicates);
            let drawn = draw_node(child, path, draw, cx);
            path.truncate(start);
            layer(child, drawn, default, draw.theme)
        })
        .collect()
}

/// `element` in the layer a `stack`'s child is placed in: full-size and
/// absolutely positioned, aligning the child by its `place` (the stack's
/// own, `align`, when the child says none) and offset from it in its
/// placement's directions.
fn layer(child: &Node, element: AnyElement, default: Place, theme: &Theme) -> AnyElement {
    let place = child.place.unwrap_or(default);
    let layer = div()
        .absolute()
        .size_full()
        .flex()
        .map(|layer| match place {
            Place::TopStart | Place::Start | Place::BottomStart => layer.items_start(),
            Place::Top | Place::Center | Place::Bottom => layer.items_center(),
            Place::TopEnd | Place::End | Place::BottomEnd => layer.items_end(),
        })
        .map(|layer| match place {
            Place::TopStart | Place::Top | Place::TopEnd => layer.justify_start(),
            Place::Start | Place::Center | Place::End => layer.justify_center(),
            Place::BottomStart | Place::Bottom | Place::BottomEnd => layer.justify_end(),
        })
        .child(element);
    // An offset moves the child away from its place, in the direction
    // that place reads: down and right from a top-start place, up and
    // left from a bottom-end one.
    let layer = match (place, child.offset) {
        (
            Place::TopStart
            | Place::Top
            | Place::TopEnd
            | Place::Start
            | Place::Center
            | Place::End,
            Some(offset),
        ) => layer
            .mt(length_pixels(offset.y, Some(theme)))
            .ml(length_pixels(offset.x, Some(theme))),
        (Place::BottomStart | Place::Bottom | Place::BottomEnd, Some(offset)) => layer
            .mb(length_pixels(offset.y, Some(theme)))
            .mr(length_pixels(offset.x, Some(theme))),
        (_, None) => layer,
    };
    layer.into_any_element()
}

/// A container of children: a `column` when `column`, else a `row`, with
/// the layout its properties give it.
fn container(column: bool, layout: &NodeLayout, children: Vec<AnyElement>) -> Div {
    let div = div()
        .flex()
        .when(column, |div| div.flex_col())
        .min_w(px(0.))
        .when_some(layout.gap, |div, gap| div.gap(tokens::space(gap)));
    let padding = layout.padding;
    let div = div
        .when_some(padding.top, |div, top| div.pt(tokens::space(top)))
        .when_some(padding.right, |div, right| div.pr(tokens::space(right)))
        .when_some(padding.bottom, |div, bottom| div.pb(tokens::space(bottom)))
        .when_some(padding.left, |div, left| div.pl(tokens::space(left)))
        .map(|div| match layout.align {
            Some(Align::Start) | None => div.items_start(),
            Some(Align::Center) => div.items_center(),
            Some(Align::End) => div.items_end(),
            Some(Align::Stretch) => div.items_stretch(),
            Some(Align::Baseline) => div.items_baseline(),
        })
        .map(|div| match layout.justify {
            Some(Justify::Start) | None => div.justify_start(),
            Some(Justify::Center) => div.justify_center(),
            Some(Justify::End) => div.justify_end(),
            Some(Justify::SpaceBetween) => div.justify_between(),
            Some(Justify::SpaceAround) => div.justify_around(),
        })
        .when(layout.wrap, |div| div.flex_wrap());
    div.children(children)
}

/// A card of children, on Pane's own card surface: a column on the
/// Settings card's fill, with its ring, its radius and its padding.
fn card(layout: &NodeLayout, children: Vec<AnyElement>, theme: &Theme) -> Div {
    let padding = layout.padding;
    let div = div()
        .flex()
        .flex_col()
        .min_w(px(0.))
        .p(theme.geometry.settings.card_padding_x)
        .when_some(layout.gap, |div, gap| div.gap(tokens::space(gap)))
        .when_some(padding.top, |div, top| div.pt(tokens::space(top)))
        .when_some(padding.right, |div, right| div.pr(tokens::space(right)))
        .when_some(padding.bottom, |div, bottom| div.pb(tokens::space(bottom)))
        .when_some(padding.left, |div, left| div.pl(tokens::space(left)))
        .map(|div| match layout.align {
            Some(Align::Start) | None => div.items_start(),
            Some(Align::Center) => div.items_center(),
            Some(Align::End) => div.items_end(),
            Some(Align::Stretch) => div.items_stretch(),
            Some(Align::Baseline) => div.items_baseline(),
        })
        .map(|div| match layout.justify {
            Some(Justify::Start) | None => div.justify_start(),
            Some(Justify::Center) => div.justify_center(),
            Some(Justify::End) => div.justify_end(),
            Some(Justify::SpaceBetween) => div.justify_between(),
            Some(Justify::SpaceAround) => div.justify_around(),
        })
        .when(layout.wrap, |div| div.flex_wrap());
    container_chrome(div, children, theme)
}

/// The card's own chrome: the fill, ring and radius of the Settings card.
fn container_chrome(div: Div, children: Vec<AnyElement>, theme: &Theme) -> Div {
    let ring = crate::ui::controls::inset_ring(theme.card_edge, px(1.));
    div.bg(theme.card_fill)
        .rounded(theme.geometry.settings.card_radius)
        .shadow(vec![ring])
        .children(children)
}

/// A stack of children drawn over each other, each in the layer its
/// place puts it in.
fn stack(children: Vec<AnyElement>) -> Div {
    div().relative().flex().children(children)
}

/// A scrolling region, whose position Pane keeps by key: the scroll's
/// handle is the node's keyed state, so a re-render that still draws the
/// region keeps where it was scrolled to.
fn scroll(
    orientation: Orientation,
    children: Vec<AnyElement>,
    id: String,
    tracked: Option<gpui::ScrollHandle>,
) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .min_w(px(0.))
        .min_h(px(0.))
        .when(orientation == Orientation::Vertical, |div| {
            div.flex_col().overflow_y_scroll()
        })
        .when(orientation == Orientation::Horizontal, |div| {
            div.flex_row().overflow_x_scroll()
        })
        .when_some(tracked, |div, handle| div.track_scroll(&handle))
        .children(children)
}

/// A divider: a hairline rule, horizontal in a column and vertical in a
/// row.
fn divider(orientation: Orientation, theme: &Theme) -> Div {
    div()
        .flex_none()
        .when(orientation == Orientation::Horizontal, |div| {
            div.w_full().h(px(1.))
        })
        .when(orientation == Orientation::Vertical, |div| {
            div.h_full().w(px(1.))
        })
        .bg(theme.hairline_soft)
}

/// `element` wrapped in the node's own [`Style`]: its sizing, its
/// surface, and the `hover` and `pressed` variants of that surface Pane
/// applies without calling the extension. A node whose style says nothing
/// is drawn as it is; a container wears its style directly instead.
fn styled(node: &Node, path: &mut String, draw: Draw, element: AnyElement) -> AnyElement {
    if node.style.is_empty() {
        return element;
    }
    // The wrapper holds the node in its parent's flex flow (the sizing is
    // the wrapper's) and draws its surface around whatever the node is,
    // its child filling it.
    let id = format!("{path}/surface");
    let wrapper = apply(div().id(id).flex().flex_col().min_w(px(0.)), node, &draw);
    wrapper
        .child(div().flex_1().min_w(px(0.)).min_h(px(0.)).child(element))
        .into_any_element()
}

/// `div` wearing the node's own [`Style`]: its sizing, its surface, and
/// the `hover` and `pressed` variants of that surface Pane applies
/// without calling the extension.
fn apply(div: Stateful<Div>, node: &Node, draw: &Draw) -> Stateful<Div> {
    div.map(|div| sized(div, &node.style.sizing))
        .map(|div| surface(div, &node.style.surface, draw.theme))
        .map(|div| {
            let variant = node.style.hover.clone();
            let theme = draw.theme;
            div.when_some(variant, move |div, variant| {
                div.hover(move |style| restyle(style, &variant, theme))
            })
        })
        .map(|div| {
            let variant = node.style.pressed.clone();
            let theme = draw.theme;
            div.when_some(variant, move |div, variant| {
                div.active(move |style| restyle(style, &variant, theme))
            })
        })
}

/// The surface a variant restates, applied to the style refinement a
/// hover or press callback is given.
fn restyle(
    style: gpui::StyleRefinement,
    surface: &Surface,
    theme: &Theme,
) -> gpui::StyleRefinement {
    let mut style = style;
    if let Some(background) = &surface.background {
        style = style.bg(tokens::paint_color(background, theme));
    }
    if let Some(border) = &surface.border {
        let width = border
            .width
            .map_or(px(1.), |width| length_pixels(width, Some(theme)));
        let color = border
            .color
            .map_or(theme.hairline, |color| tokens::paint_color(&color, theme));
        style = style.border(width).border_color(color);
    }
    if let Some(radius) = surface.radius {
        style = style.rounded(radius_pixels(radius, theme));
    }
    if let Some(Finite(opacity)) = surface.opacity {
        style = style.opacity(opacity);
    }
    style
}

/// The sizing a node asks for: how it takes space in its parent, and how
/// big it is.
fn sized<D: Styled + IntoElement>(div: D, sizing: &Sizing) -> D {
    div.when_some(sizing.grow, |div, Finite(grow)| div.flex_grow(grow))
        .when_some(sizing.shrink, |div, Finite(shrink)| div.flex_shrink(shrink))
        .when_some(sizing.basis, |div, basis| div.flex_basis(length(basis)))
        .when_some(sizing.width, |div, width| div.w(length(width)))
        .when_some(sizing.height, |div, height| div.h(length(height)))
        .when_some(sizing.min_width, |div, width| div.min_w(length(width)))
        .when_some(sizing.max_width, |div, width| div.max_w(length(width)))
        .when_some(sizing.min_height, |div, height| div.min_h(length(height)))
        .when_some(sizing.max_height, |div, height| div.max_h(length(height)))
        .when_some(sizing.aspect_ratio, |div, Finite(ratio)| {
            div.aspect_ratio(ratio)
        })
}

/// One length, as GPUI takes it.
fn length(value: NodeLength) -> GpuiLength {
    match value {
        // A space token in a length: the pixels it names.
        NodeLength::Space(token) => tokens::space(token).into(),
        NodeLength::Px(Finite(pixels)) => px(pixels).into(),
        NodeLength::Fraction(Finite(fraction)) => relative(fraction).into(),
    }
}

/// One length, as the pixels it names (a fraction of no parent is its
/// parent's whole size).
fn length_pixels(value: NodeLength, theme: Option<&Theme>) -> Pixels {
    match value {
        NodeLength::Space(token) => theme.map_or(px(0.), |_| tokens::space(token)),
        NodeLength::Px(Finite(pixels)) => px(pixels),
        NodeLength::Fraction(Finite(fraction)) => px(fraction * 4096.),
    }
}

/// The surface a node draws: its background, border, corner radius and
/// opacity.
fn surface<D: Styled + IntoElement>(div: D, surface: &Surface, theme: &Theme) -> D {
    let border = surface.border.as_ref();
    div.when_some(surface.background, |div, background| {
        div.bg(tokens::paint_color(&background, theme))
    })
    .when_some(border, |div, border| {
        let width = border
            .width
            .map_or(px(1.), |width| length_pixels(width, Some(theme)));
        let color = border
            .color
            .map_or(theme.hairline, |color| tokens::paint_color(&color, theme));
        div.border(width).border_color(color)
    })
    .when_some(surface.radius, |div, radius| {
        div.rounded(radius_pixels(radius, theme))
    })
    .when_some(surface.opacity, |div, Finite(opacity)| div.opacity(opacity))
}

/// A corner radius, as GPUI takes it.
fn radius_pixels(radius: RadiusLength, _theme: &Theme) -> Pixels {
    match radius {
        RadiusLength::Token(token) => tokens::radius(token),
        RadiusLength::Px(Finite(pixels)) => px(pixels),
    }
}
