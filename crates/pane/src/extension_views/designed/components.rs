//! The shared UI components a designed tree names, drawn from Pane's own
//! families: the button from `ui::controls`, the toggle and the segmented
//! control from the Settings board's, the keycaps from `ui::keycap`, the
//! icons and images from `ui::extension_icon`, the rows after the
//! launcher's own, the fields GPUI CE's editable text and the select the
//! searchable select of `ui::select`. An extension's screen is Pane's
//! design — the same chrome, the same keyboard behaviour, the same
//! accessibility — not a copy of it (#237, ADR 0036).
//!
//! Each component has one accessibility mapping (role, name, value,
//! state), owned here: a text is a `Label` named by its content, a button
//! a `Button` named by its label, a toggle a `Switch` with its state, and
//! so on. What a component cannot be named by, the tree's `name` says.
//!
//! A change a control reports (a toggle flipped, a slider adjusted, a
//! field committed) leaves as an event whose payload names the value; an
//! event is raised on the tree the user saw — the render the drawing
//! carries — and on the node with its key (#238). The fields edit at
//! once: their text, caret and composition live in the keyed state the
//! reconciler holds, their input events are sent as the user types, and
//! their commits go out on Enter and a blur.

use std::cell::Cell;
use std::rc::Rc;

use gpui::prelude::*;
use gpui::{
    AnimationExt as _, AnyElement, ClickEvent, ColorExt as _, Div, FontWeight, ObjectFit, Role,
    SharedString, Stateful, Toggled, div, img, px, relative, svg, transparent_black,
};
use gpui_elements::editable_text::{text_area as area_input, text_input as edit_input};

use pane_core::{
    Badge as BadgeNode, Binding, Button as ButtonNode, Checkbox as CheckboxNode, DesignedHandler,
    EmptyState as EmptyStateNode, Finite, Fit, Icon, IconExtent, IconNode, Image as ImageNode,
    KeySequence as KeySequenceNode, Keycap as KeycapNode, Link as LinkNode, Loading as LoadingNode,
    Markdown as MarkdownNode, MetadataItem as MetadataItemNode, MetadataList as MetadataListNode,
    Node, Paint, Progress as ProgressNode, RichRow as RichRowNode, SectionHeader,
    Segmented as SegmentedNode, Select as SelectNode, Slider as SliderNode, Span, Tag as TagNode,
    Text as TextNode, TextContent, TextInput as TextInputNode, Toggle as ToggleNode,
};
use pane_core::{Space, TextLevel};

use crate::app::LauncherWindow;
use crate::features::icons;
use crate::keyboard::binding_keys;
use crate::ui::controls::{self, focus_ring, inset_ring};
use crate::ui::extension_icon::{self, IconImage, IconSize};
use crate::ui::keycap::{CapStyle, Key, KeySequence, key_sequence as draw_keys};
use crate::ui::theme::{Theme, pressed};
use crate::ui::tokens;

use super::fields;
use super::tree::{Draw, duplicate_keys, place_child};
use super::{
    AREA_CONTEXT, BUTTON_CONTEXT, CHECKBOX_CONTEXT, Commit, INPUT_CONTEXT, Move, Press,
    SEGMENTED_CONTEXT, SLIDER_CONTEXT, TOGGLE_CONTEXT, Toggle, payload, plain_payload,
};

/// The event a control's change carries: `{"value": …}` for whatever the
/// control changed to.
fn change(value: &str) -> String {
    payload(value)
}

/// The event a control's change carries, a number or boolean as it is.
fn plain_change(value: impl std::fmt::Display) -> String {
    plain_payload(value)
}

/// A debug selector's text, kept short: what the tests name a node by.
pub(super) fn short(text: &str) -> String {
    let end = text.char_indices().nth(48).map_or(text.len(), |(at, _)| at);
    text[..end].replace('\n', " ")
}

// ---------------------------------------------------------------- text

/// One text node: what it says, plain or in spans, with links.
pub(super) fn text(
    text: &TextNode,
    key: Option<&str>,
    path: &str,
    draw: &Draw,
    cx: &mut gpui::Context<LauncherWindow>,
) -> Stateful<Div> {
    match &text.content {
        TextContent::Plain(content) => {
            let debug = format!("designed-text-{}", short(content));
            run(
                content,
                text.style,
                text.level,
                text.color.as_ref(),
                text.size,
                text.weight,
                false,
                text.truncate,
                path,
                draw,
                &debug,
            )
        }
        TextContent::Spans(spans) => {
            // The spans flow beside each other, wrapping onto lines as the
            // width runs out; a link span is a link — raised on the text
            // node's key, whose handler its spans name.
            let key = key.unwrap_or_default().to_owned();

            let runs = spans
                .iter()
                .enumerate()
                .map(|(index, span)| span_run(span, index, &key, path, draw, text.truncate, cx))
                .collect::<Vec<AnyElement>>();
            let name: SharedString = spans
                .iter()
                .map(|span| span.text.as_str())
                .collect::<String>()
                .into();
            div()
                .id(path.to_owned())
                .flex()
                .flex_wrap()
                .items_baseline()
                .min_w(px(0.))
                .role(Role::Label)
                .map(|text| text.aria_label(name))
                .children(runs)
        }
    }
}

/// One run of a text: what it says, in the style, level and colour its
/// node or span names. Its children (in JSX, its content) spell its
/// `text`; `debug` is the selector the tests name it by.
#[allow(clippy::too_many_arguments)]
fn run(
    content: &str,
    style: Option<pane_core::TextStyle>,
    level: Option<TextLevel>,
    color: Option<&Paint>,
    size: Option<Finite>,
    weight: Option<Finite>,
    code: bool,
    truncate: bool,
    path: &str,
    draw: &Draw,
    debug: &str,
) -> Stateful<Div> {
    let theme = draw.theme;
    let (style_size, style_weight, family) = if code {
        (
            theme.typography.row_subtitle_size,
            theme.typography.regular,
            theme.typography.mono_family.clone(),
        )
    } else {
        tokens::text_style(style, theme)
    };
    let size = size.map_or(style_size, |Finite(pixels)| px(pixels));
    let weight = weight.map_or(style_weight, |Finite(units)| FontWeight::from(units));
    let label: SharedString = content.into();
    let ink = color
        .map(|paint| tokens::foreground(paint, draw.surface, theme))
        .unwrap_or_else(|| tokens::text_level(level, theme));
    let debug = debug.to_owned();
    div()
        .id(path.to_owned())
        .flex_none()
        .min_w(px(0.))
        .debug_selector(move || debug.clone())
        .text_size(size)
        .font_weight(weight)
        .font_family(family)
        .line_height(size * theme.typography.line_height)
        .text_color(ink)
        .when(truncate, |text| text.truncate())
        .role(Role::Label)
        .map(|run| run.aria_label(label.clone()))
        .child(label)
}

/// One span of a text: a run of its content, a link when it carries
/// `onPress` — raised on the text node's `key`.
fn span_run(
    span: &Span,
    index: usize,
    key: &str,
    path: &str,
    draw: &Draw,
    truncate: bool,
    cx: &mut gpui::Context<LauncherWindow>,
) -> AnyElement {
    let path = format!("{path}/{index}");
    let debug = format!("designed-text-{}", short(&span.text));
    if span.on_press.is_none() {
        return run(
            &span.text,
            span.style,
            span.level,
            span.color.as_ref(),
            None,
            None,
            span.code,
            truncate,
            &path,
            draw,
            &debug,
        )
        .into_any_element();
    }
    link_of(
        span.on_press,
        key,
        &span.text,
        &path,
        span.color.as_ref(),
        &debug,
        draw,
        cx,
    )
    .into_any_element()
}

// ------------------------------------------------------------- buttons

/// The colors of a button's tone, resolved onto the theme: its fill, its
/// 1px inner edge (a plain button's ring), and its label's ink. The
/// mapping the public token layer holds (#235's, extracted).
fn tone(
    tone: Option<pane_core::ButtonTone>,
    theme: &Theme,
) -> (gpui::Hsla, Option<gpui::Hsla>, gpui::Hsla) {
    match tone {
        // The plain pill a Settings button draws.
        None | Some(pane_core::ButtonTone::Default) => (
            theme.results.pill_fill,
            Some(theme.results.pill_edge),
            theme.text_title,
        ),
        // The ghost pill: transparent, its label in the body ink.
        Some(pane_core::ButtonTone::Secondary) | Some(pane_core::ButtonTone::Ghost) => {
            (gpui::transparent_black(), None, theme.text_body)
        }
        // The accent: the accent's fill, its ink on it.
        Some(pane_core::ButtonTone::Accent) => (theme.accent, None, theme.accent_ink),
        // The destructive tone: the danger colour, at the fill an icon's
        // danger disc uses.
        Some(pane_core::ButtonTone::Destructive) => (
            gpui::Hsla::opacity(&theme.danger, 0.18),
            Some(theme.danger),
            theme.danger,
        ),
    }
}

/// What a button of `tone` fills with while the pointer is over it.
fn hover_fill(tone: Option<pane_core::ButtonTone>, theme: &Theme) -> gpui::Hsla {
    match tone {
        Some(pane_core::ButtonTone::Secondary) | Some(pane_core::ButtonTone::Ghost) => {
            theme.hover_wash
        }
        Some(pane_core::ButtonTone::Accent) => gpui::Hsla::opacity(&theme.accent, 0.9),
        Some(pane_core::ButtonTone::Destructive) => gpui::Hsla::opacity(&theme.danger, 0.24),
        None | Some(pane_core::ButtonTone::Default) => theme.results.pill_hover,
    }
}

/// One button: its label in the tone its properties name, with its icon
/// before and its keycaps after it, focusable, Enter and Space (and a
/// click) pressing it by the callback id its tree named.
pub(super) fn button(
    node: &Node,
    button: &ButtonNode,
    path: &str,
    draw: &Draw,
    cx: &mut gpui::Context<LauncherWindow>,
) -> Stateful<Div> {
    let theme = draw.theme;
    let Some(callback) = button.on_press.filter(|_| button.enabled) else {
        // A button that cannot be pressed: its label, drawn as a text.
        let debug = format!("designed-button-{}", short(&button.label));
        return run(
            &button.label,
            None,
            Some(TextLevel::Secondary),
            None,
            None,
            None,
            false,
            false,
            path,
            draw,
            &debug,
        );
    };
    let label: SharedString = button.label.clone().into();
    let debug = format!("designed-button-{}", button.label);
    let ring = focus_ring(theme);
    let (fill, edge, ink) = tone(button.tone, theme);
    let hover = hover_fill(button.tone, theme);
    let pill = &theme.geometry.results;
    let line = theme.typography.results.pill;
    // A press of the button: the callback id its tree named, raised on
    // the node with `key` (or none when the tree gave it none) on the
    // tree the user saw.
    let key = node.key.clone().unwrap_or_default();
    let (for_press, key_for_press) = (callback, key.clone());
    let (for_click, key_for_click) = (callback, key.clone());
    let seen = draw.render;
    let (press, click) = (
        cx.listener(move |this, _: &Press, window, cx| {
            this.designed_event(
                DesignedHandler::Press,
                for_press,
                key_for_press.clone(),
                seen,
                "{}".into(),
                window,
                cx,
            );
        }),
        cx.listener(move |this, _: &ClickEvent, window, cx| {
            this.designed_event(
                DesignedHandler::Press,
                for_click,
                key_for_click.clone(),
                seen,
                "{}".into(),
                window,
                cx,
            );
        }),
    );
    let keys = button
        .keys
        .as_ref()
        .map(|keys| draw_keys(&keys_of(keys), CapStyle::Regular, theme));
    let icon = button
        .icon
        .as_ref()
        .map(|icon| icon_at(icon, 16., path, draw));
    div()
        .id(path.to_owned())
        .debug_selector(move || debug)
        .key_context(BUTTON_CONTEXT)
        .role(Role::Button)
        .map(|button| button.aria_label(label.clone()))
        .when(!button.enabled, |button| button.aria_disabled(true))
        // A Settings button's box (`.pill`), filled in the button's tone.
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .gap(theme.geometry.controls.button_gap)
        .h(pill.pill_height)
        .px(pill.pill_padding_x)
        .rounded(pill.pill_radius)
        .text_size(line.size)
        .line_height(line.line_height)
        .font_weight(theme.typography.medium)
        .whitespace_nowrap()
        .cursor_pointer()
        .text_color(ink)
        .bg(fill)
        .when_some(edge, |button, edge| {
            button.shadow(vec![inset_ring(edge, px(1.))])
        })
        .when(!button.enabled, |button| {
            button.opacity(theme.geometry.controls.disabled_opacity)
        })
        .hover(move |button| button.bg(hover))
        .active(move |button| button.bg(pressed(hover)))
        .when_some(draw.focus_of(path), |button, focus| {
            button
                .track_focus(&focus)
                .focus(move |button| button.shadow(ring))
        })
        .on_action(press)
        .on_click(click)
        .when_some(icon, |button, icon| button.child(icon))
        .map(|button| button.child(label))
        .when_some(keys, |button, keys| button.child(keys))
}

/// The keys `keys` name, as a key sequence: a binding's, when they spell
/// one, else a cap each.
fn keys_of(keys: &[String]) -> KeySequence {
    let joined = keys.join("+");
    match Binding::parse(&joined) {
        Ok(binding) => binding_keys(&binding),
        Err(_) => KeySequence {
            keys: keys
                .iter()
                .map(|key| Key::new(key.to_owned(), key.to_owned()))
                .collect(),
        },
    }
}

/// One keycap: the key its cap shows.
pub(super) fn keycap(keycap: &KeycapNode, path: &str, theme: &Theme) -> AnyElement {
    let keys = KeySequence {
        keys: vec![Key::new(keycap.key.clone(), keycap.key.clone())],
    };
    let debug = format!("designed-keycap-{}", keycap.key);
    div()
        .id(path.to_owned())
        .debug_selector(move || debug)
        .flex_none()
        .child(draw_keys(&keys, CapStyle::Regular, theme))
        .into_any_element()
}

/// A key sequence: the keys its caps show, one cap per key.
pub(super) fn key_sequence(keys: &KeySequenceNode, path: &str, theme: &Theme) -> AnyElement {
    let sequence = keys_of(&keys.keys);
    let name = sequence.name();
    div()
        .id(path.to_owned())
        .debug_selector(move || "designed-keys".into())
        .flex_none()
        .child(draw_keys(&sequence, CapStyle::Regular, theme))
        .role(Role::Image)
        .map(|keys| keys.aria_label(name))
        .into_any_element()
}

/// One link: its label in the accent, underlined, focusable, Enter and a
/// click pressing it. Its colour is its own when the tree named one.
pub(super) fn link(
    link: &LinkNode,
    key: Option<&str>,
    path: &str,
    draw: &Draw,
    cx: &mut gpui::Context<LauncherWindow>,
) -> Stateful<Div> {
    let debug = format!("designed-link-{}", short(&link.label));
    link_of(
        link.on_press,
        key.unwrap_or_default(),
        &link.label,
        path,
        link.color.as_ref(),
        &debug,
        draw,
        cx,
    )
}

/// One link, from its parts (a span's, or a metadata row's): raised on
/// the node with `key` — the text or list the link belongs to, whose
/// handler the tree names on it.
#[allow(clippy::too_many_arguments)]
fn link_of(
    on_press: Option<u32>,
    key: &str,
    label: &str,
    path: &str,
    color: Option<&Paint>,
    debug: &str,
    draw: &Draw,
    cx: &mut gpui::Context<LauncherWindow>,
) -> Stateful<Div> {
    let theme = draw.theme;
    let ink = color
        .map(|paint| tokens::foreground(paint, draw.surface, theme))
        .unwrap_or(theme.accent_text);
    let name: SharedString = label.into();
    let debug = debug.to_owned();
    let element = div()
        .id(path.to_owned())
        .debug_selector(move || debug.clone())
        .flex_none()
        .min_w(px(0.))
        .cursor_pointer()
        .text_size(theme.typography.row_subtitle_size)
        .line_height(theme.typography.row_subtitle_size * theme.typography.line_height)
        .text_color(ink)
        .underline()
        .role(Role::Link)
        .map(|link| link.aria_label(name.clone()))
        .child(name);
    let Some(callback) = on_press else {
        return element.cursor_default();
    };
    let key = key.to_owned();
    let (for_press, key_for_press) = (callback, key.clone());
    let (for_click, key_for_click) = (callback, key.clone());
    let seen = draw.render;
    let (press, click) = (
        cx.listener(move |this, _: &Press, window, cx| {
            this.designed_event(
                DesignedHandler::Press,
                for_press,
                key_for_press.clone(),
                seen,
                "{}".into(),
                window,
                cx,
            );
        }),
        cx.listener(move |this, _: &ClickEvent, window, cx| {
            this.designed_event(
                DesignedHandler::Press,
                for_click,
                key_for_click.clone(),
                seen,
                "{}".into(),
                window,
                cx,
            );
        }),
    );
    let ring = focus_ring(theme);
    element
        .key_context(BUTTON_CONTEXT)
        .when_some(draw.focus_of(path), |link, focus| {
            link.track_focus(&focus)
                .focus(move |link| link.shadow(ring))
        })
        .on_action(press)
        .on_click(click)
}

// ------------------------------------------------------------ images

/// The icon `icon` as the shared drawing draws it here, its tint corrected
/// for contrast against the surface under it at the component set's own
/// ratio.
pub(super) fn drawn(icon: &Icon, draw: &Draw) -> extension_icon::DrawnIcon {
    icons::drawn_on(
        icon,
        draw.theme,
        crate::ui::contrast::DESIGNED,
        draw.surface,
    )
}

/// One icon at `pixels`, drawn bare.
pub(super) fn icon_at(icon: &Icon, pixels: f32, path: &str, draw: &Draw) -> Stateful<Div> {
    let theme = draw.theme;
    let drawn = drawn(icon, draw);
    let size = IconSize::small(px(pixels));
    extension_icon::draw(&drawn, size, path.to_owned(), "designed", theme)
}

/// One image of a Grid's cell (#240), drawn bare, filling the box the
/// cell gives it as its `fit` says: the icon model's image, its tint
/// corrected as the tree's images are.
pub(super) fn grid_image(icon: &Icon, fit: Fit, path: &str, draw: &Draw) -> AnyElement {
    let drawn = drawn(icon, draw);
    let element = match &drawn.image {
        IconImage::Glyph(markup) => svg()
            .data(markup)
            .size_full()
            .text_color(drawn.color)
            .into_any_element(),
        IconImage::File {
            path: file,
            tinted: true,
        } => svg()
            .external_path(file.to_string_lossy().into_owned())
            .size_full()
            .text_color(drawn.color)
            .into_any_element(),
        IconImage::Data {
            image,
            tinted: true,
        } => svg()
            .data(&image.bytes)
            .size_full()
            .text_color(drawn.color)
            .into_any_element(),
        IconImage::File { path: file, .. } => img(file.clone())
            .size_full()
            .object_fit(object_fit(fit))
            .into_any_element(),
        IconImage::Data { image, .. } => img(image.clone())
            .size_full()
            .object_fit(object_fit(fit))
            .into_any_element(),
        IconImage::Letter { letter, background } => div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(*background)
            .text_size(px(24.))
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .text_color(drawn.color)
            .child(letter.to_string())
            .into_any_element(),
    };
    div()
        .id(path.to_owned())
        .debug_selector(move || "designed-cell-image".into())
        .size_full()
        .child(element)
        .into_any_element()
}

/// One icon, drawn at the size its node names, bare.
pub(super) fn icon(icon: &IconNode, path: &str, draw: &Draw) -> AnyElement {
    let pixels = extent_of(icon.size, 16., draw.theme);
    match &icon.icon {
        Some(held) => icon_at(held, pixels, path, draw).into_any_element(),
        None => div().into_any_element(),
    }
}

/// One icon on Pane's tile: the extension's icon inside the tile Pane's
/// own rows carry, at the size its node names.
pub(super) fn icon_tile(icon: &IconNode, path: &str, draw: &Draw) -> AnyElement {
    let theme = draw.theme;
    let tile = theme.geometry.tile;
    let scale = extent_of(icon.size, tile.size.as_f32(), theme) / tile.size.as_f32();
    let size = IconSize {
        size: tile.size * scale,
        glyph: tile.glyph * scale,
        radius: tile.radius * scale,
    };
    let drawn = icon.icon.as_ref().map(|held| drawn(held, draw));
    div()
        .id(path.to_owned())
        .debug_selector(move || "designed-icon-tile".into())
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .size(size.size)
        .rounded(size.radius)
        .bg(theme.tile_background)
        .shadow(vec![inset_ring(theme.tile_border, px(1.))])
        .map(|tile| match drawn {
            Some(drawn) => tile.child(extension_icon::draw(
                &drawn,
                size,
                path.to_owned(),
                "designed-tile",
                theme,
            )),
            None => tile,
        })
        .into_any_element()
}

/// The pixels an icon's or image's size names.
fn extent_of(size: Option<IconExtent>, default: f32, _theme: &Theme) -> f32 {
    match size {
        Some(IconExtent::Px(Finite(pixels))) => pixels,
        Some(IconExtent::Token(token)) => tokens::icon_size(token).as_f32(),
        None => default,
    }
    .clamp(0., pane_core::MAX_PX)
}

/// One image: drawn at its size, fitting its box as its `fit` names, with
/// its children standing in for it while it loads or cannot be read.
pub(super) fn image(
    node: &Node,
    image: &ImageNode,
    path: &str,
    draw: &Draw,
    cx: &mut gpui::Context<LauncherWindow>,
) -> AnyElement {
    let theme = draw.theme;
    let pixels = extent_of(image.size, theme.geometry.tile.size.as_f32(), theme);
    let Some(held) = &image.image else {
        // No image Pane can read: its placeholder.
        return placeholder(node, path, draw, cx);
    };
    let drawn = drawn(held, draw);
    let fit = image.fit;
    let element = match &drawn.image {
        IconImage::Glyph(markup) => svg()
            .data(markup)
            .size(px(pixels))
            .text_color(drawn.color)
            .into_any_element(),
        IconImage::File {
            path: file,
            tinted: true,
        } => svg()
            .external_path(file.to_string_lossy().into_owned())
            .size(px(pixels))
            .text_color(drawn.color)
            .into_any_element(),
        IconImage::Data {
            image,
            tinted: true,
        } => svg()
            .data(&image.bytes)
            .size(px(pixels))
            .text_color(drawn.color)
            .into_any_element(),
        IconImage::File { path: file, .. } => img(file.clone())
            .size(px(pixels))
            .object_fit(object_fit(fit))
            .rounded(px(pixels) / 4.)
            .into_any_element(),
        IconImage::Data { image, .. } => img(image.clone())
            .size(px(pixels))
            .object_fit(object_fit(fit))
            .rounded(px(pixels) / 4.)
            .into_any_element(),
        IconImage::Letter { letter, background } => div()
            .size(px(pixels))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(pixels) / 4.)
            .bg(*background)
            .text_size(px(pixels) / 2.)
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .text_color(drawn.color)
            .child(letter.to_string())
            .into_any_element(),
    };
    // An image that has not arrived (a web image Pane is downloading, a
    // system icon it is extracting) stands in as its fallback; with none,
    // the node's children stand in for it.
    let loading = matches!(
        &held.source,
        pane_core::IconSource::Url(url) if pane_core::icons::is_web_url(url)
    ) || matches!(
        &held.source,
        pane_core::IconSource::File(_) | pane_core::IconSource::Application(_)
    );
    let standing_in = loading && matches!(drawn.image, IconImage::Glyph(_));
    if standing_in {
        let placeholder = placeholder(node, path, draw, cx);
        return div()
            .relative()
            .size(px(pixels))
            .child(element)
            .child(placeholder)
            .into_any_element();
    }
    div().size(px(pixels)).child(element).into_any_element()
}

/// One fit, as GPUI draws it.
fn object_fit(fit: Fit) -> ObjectFit {
    match fit {
        Fit::Contain => ObjectFit::Contain,
        Fit::Cover => ObjectFit::Cover,
        Fit::Fill => ObjectFit::Fill,
    }
}

/// The placeholder an image node's children spell.
fn placeholder(
    node: &Node,
    path: &str,
    draw: &Draw,
    cx: &mut gpui::Context<LauncherWindow>,
) -> AnyElement {
    let duplicates = duplicate_keys(node);
    let children: Vec<AnyElement> = node
        .children
        .iter()
        .enumerate()
        .map(|(index, child)| {
            let mut child_path = format!("{path}/placeholder");
            place_child(&mut child_path, child, index, &duplicates);
            super::tree::draw_node(child, &mut child_path, *draw, cx)
        })
        .collect();
    div()
        .id(format!("{path}/placeholder"))
        .flex()
        .flex_col()
        .min_w(px(0.))
        .children(children)
        .into_any_element()
}

// -------------------------------------------------------------- rows

/// A rich row: the launcher's own result row as a component — an icon, a
/// title, a subtitle and accessories, pressable when it names a callback.
pub(super) fn rich_row(
    node: &Node,
    row: &RichRowNode,
    path: &str,
    draw: &Draw,
    cx: &mut gpui::Context<LauncherWindow>,
) -> AnyElement {
    let theme = draw.theme;
    let geometry = &theme.geometry;
    let pressable = row.on_press.is_some();
    let title: SharedString = row.title.clone().into();
    let subtitle: SharedString = row
        .subtitle
        .clone()
        .map(SharedString::from)
        .unwrap_or_default();
    let debug = format!("designed-row-{}", row.title);
    let icon = row
        .icon
        .as_ref()
        .map(|icon| icon_at(icon, geometry.tile.size.as_f32(), path, draw).into_any_element());
    let accessories: Vec<AnyElement> = row
        .accessories
        .iter()
        .enumerate()
        .map(|(index, accessory)| accessory_run(accessory, index, path, draw))
        .collect();
    let hover = theme.hover_wash;
    let row_element = div()
        .id(path.to_owned())
        .debug_selector(move || debug)
        .flex()
        .items_center()
        .min_h(geometry.row_min_height)
        .px(geometry.row_padding_x)
        .rounded(geometry.row_radius)
        .gap(geometry.row_gap)
        .role(Role::ListItem)
        .map(|row| row.aria_label(title.clone()))
        .when(!subtitle.is_empty(), |row| {
            row.aria_description(subtitle.clone())
        })
        .when(pressable, |row| {
            row.cursor_pointer().hover(move |row| row.bg(hover))
        })
        .when_some(icon, |row, icon| row.child(icon))
        .child(
            div()
                .flex()
                .flex_col()
                .min_w(px(0.))
                .flex_1()
                .gap_1()
                .child(
                    div()
                        .min_w(px(0.))
                        .truncate()
                        .text_size(theme.typography.row_title_size)
                        .font_weight(theme.typography.medium)
                        .text_color(theme.text_title)
                        .child(title),
                )
                .when(!subtitle.is_empty(), |column| {
                    column.child(
                        div()
                            .min_w(px(0.))
                            .truncate()
                            .text_size(theme.typography.row_subtitle_size)
                            .text_color(theme.text_body)
                            .child(subtitle),
                    )
                }),
        )
        .when(!accessories.is_empty(), |row| {
            row.child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(geometry.row_gap)
                    .children(accessories),
            )
        });
    let Some(callback) = row.on_press else {
        return row_element.into_any_element();
    };
    // A press of the row: the callback id its tree named, raised on the
    // node with `key` on the tree the user saw.
    let key = node.key.clone().unwrap_or_default();
    let (for_press, key_for_press) = (callback, key.clone());
    let (for_click, key_for_click) = (callback, key.clone());
    let seen = draw.render;
    let (press, click) = (
        cx.listener(move |this, _: &Press, window, cx| {
            this.designed_event(
                DesignedHandler::Press,
                for_press,
                key_for_press.clone(),
                seen,
                "{}".into(),
                window,
                cx,
            );
        }),
        cx.listener(move |this, _: &ClickEvent, window, cx| {
            this.designed_event(
                DesignedHandler::Press,
                for_click,
                key_for_click.clone(),
                seen,
                "{}".into(),
                window,
                cx,
            );
        }),
    );
    let ring = focus_ring(theme);
    row_element
        .key_context(BUTTON_CONTEXT)
        .when_some(draw.focus_of(path), |row, focus| {
            row.track_focus(&focus).focus(move |row| row.shadow(ring))
        })
        .on_action(press)
        .on_click(click)
        .into_any_element()
}

/// One accessory at a rich row's end: text, or a tag.
fn accessory_run(
    accessory: &pane_core::RowAccessory,
    index: usize,
    path: &str,
    draw: &Draw,
) -> AnyElement {
    let theme = draw.theme;
    let color = accessory
        .color
        .as_ref()
        .map(|paint| tokens::foreground(paint, draw.surface, theme))
        .unwrap_or(if accessory.tag {
            theme.text_body
        } else {
            theme.text_muted
        });
    if accessory.tag {
        let path = format!("{path}/accessory/{index}");
        return tag_chip(&accessory.text, color, theme)
            .id(path)
            .debug_selector(move || format!("designed-tag-{}", accessory.text))
            .role(Role::Label)
            .map(|tag| tag.aria_label(accessory.text.clone()))
            .into_any_element();
    }
    div()
        .flex_none()
        .text_size(theme.typography.row_kind_size)
        .text_color(color)
        .child(accessory.text.clone())
        .into_any_element()
}

/// One tag: a short label in a chip, the row's alias chip's shape.
pub(super) fn tag(tag: &TagNode, path: &str, draw: &Draw) -> AnyElement {
    let theme = draw.theme;
    let color = tag
        .color
        .as_ref()
        .map(|paint| tokens::foreground(paint, draw.surface, theme))
        .unwrap_or(theme.text_body);
    let debug = format!("designed-tag-{}", tag.text);
    tag_chip(&tag.text, color, theme)
        .id(path.to_owned())
        .debug_selector(move || debug)
        .role(Role::Label)
        .map(|chip| chip.aria_label(tag.text.clone()))
        .into_any_element()
}

/// A tag chip's shape: the alias chip's, in `color`. The caller gives it
/// its identity.
fn tag_chip(text: &str, color: gpui::Hsla, theme: &Theme) -> Div {
    let geometry = &theme.geometry;
    let text = text.to_owned();
    div()
        .flex_none()
        .flex()
        .items_center()
        .py(geometry.alias_padding_y)
        .px(geometry.alias_padding_x)
        .rounded(geometry.alias_radius)
        .shadow(vec![inset_ring(theme.alias_edge, px(1.))])
        .font_family(theme.typography.mono_family.clone())
        .text_size(theme.typography.alias_size)
        .text_color(color)
        .child(text)
}

/// One badge: a short count in a filled chip.
pub(super) fn badge(badge: &BadgeNode, path: &str, draw: &Draw) -> AnyElement {
    let theme = draw.theme;
    let (fill, ink) = badge
        .color
        .as_ref()
        .map(|paint| {
            let fill = tokens::paint_color(paint, theme);
            (fill, tokens::foreground(paint, fill, theme))
        })
        .unwrap_or((theme.accent, theme.accent_ink));
    let debug = format!("designed-badge-{}", badge.text);
    div()
        .id(path.to_owned())
        .debug_selector(move || debug)
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .min_w(theme.typography.alias_size * 2.)
        .px(theme.typography.alias_size / 2.)
        .text_size(theme.typography.alias_size)
        .font_weight(theme.typography.medium)
        .text_color(ink)
        .bg(fill)
        .role(Role::Label)
        .map(|chip| chip.aria_label(badge.text.clone()))
        .child(badge.text.clone())
        .into_any_element()
}

// ---------------------------------------------------------- controls

/// One toggle: the Settings board's switch with its label beside it,
/// focusable, Enter and Space (and a click) telling the extension what it
/// now is.
pub(super) fn toggle(
    toggle: &ToggleNode,
    key: Option<&str>,
    path: &str,
    draw: &Draw,
    cx: &mut gpui::Context<LauncherWindow>,
) -> AnyElement {
    let theme = draw.theme;
    let Some(callback) = toggle.on_change else {
        return controls::toggle(toggle.on, theme).into_any_element();
    };
    let key = key.unwrap_or_default().to_owned();

    let (for_press, key_for_press) = (callback, key.clone());
    let (for_click, key_for_click) = (callback, key.clone());
    let next = !toggle.on;
    let seen = draw.render;
    let (press, click) = (
        cx.listener(move |this, _: &Toggle, window, cx| {
            this.designed_event(
                DesignedHandler::Change,
                for_press,
                key_for_press.clone(),
                seen,
                plain_change(next),
                window,
                cx,
            );
        }),
        cx.listener(move |this, _: &ClickEvent, window, cx| {
            this.designed_event(
                DesignedHandler::Change,
                for_click,
                key_for_click.clone(),
                seen,
                plain_change(next),
                window,
                cx,
            );
        }),
    );
    let ring = focus_ring(theme);
    let label = toggle
        .label
        .clone()
        .or_else(|| toggle.field.title.clone())
        .unwrap_or_else(|| "toggle".into());

    let switch = div()
        .id(path.to_owned())
        .debug_selector(move || "designed-toggle".into())
        .flex()
        .items_center()
        .gap(tokens::space(Space::S))
        .cursor_pointer()
        .key_context(TOGGLE_CONTEXT)
        .role(Role::Switch)
        .map(|switch| switch.aria_label(label.clone()))
        .aria_toggled(if toggle.on {
            Toggled::True
        } else {
            Toggled::False
        })
        .when_some(toggle.label.clone(), |switch, label| {
            switch.child(
                div()
                    .min_w(px(0.))
                    .text_size(theme.typography.settings_text_size)
                    .text_color(theme.text_title)
                    .child(label),
            )
        })
        .child(controls::toggle(toggle.on, theme))
        .when_some(draw.focus_of(path), |switch, focus| {
            switch
                .track_focus(&focus)
                .focus(move |switch| switch.shadow(ring))
        })
        .on_action(press)
        .on_click(click)
        .into_any_element();
    // The form field's chrome around it: its title over, its note and
    // error under (#241).
    fields::field_group(&key.clone(), &toggle.field, None, switch, theme).into_any_element()
}

/// One checkbox: a box with a check, its label beside it, focusable,
/// Enter and Space (and a click) telling the extension what it now is.
pub(super) fn checkbox(
    checkbox: &CheckboxNode,
    key: Option<&str>,
    path: &str,
    draw: &Draw,
    cx: &mut gpui::Context<LauncherWindow>,
) -> AnyElement {
    let theme = draw.theme;
    let Some(callback) = checkbox.on_change else {
        return checkbox_box(checkbox.checked, theme).into_any_element();
    };
    let key = key.unwrap_or_default().to_owned();

    let (for_press, key_for_press) = (callback, key.clone());
    let (for_click, key_for_click) = (callback, key.clone());
    let next = !checkbox.checked;
    let seen = draw.render;
    let (press, click) = (
        cx.listener(move |this, _: &Toggle, window, cx| {
            this.designed_event(
                DesignedHandler::Change,
                for_press,
                key_for_press.clone(),
                seen,
                plain_change(next),
                window,
                cx,
            );
        }),
        cx.listener(move |this, _: &ClickEvent, window, cx| {
            this.designed_event(
                DesignedHandler::Change,
                for_click,
                key_for_click.clone(),
                seen,
                plain_change(next),
                window,
                cx,
            );
        }),
    );
    let ring = focus_ring(theme);
    let label = checkbox
        .label
        .clone()
        .or_else(|| checkbox.field.title.clone())
        .unwrap_or_else(|| "checkbox".into());

    let control = div()
        .id(path.to_owned())
        .debug_selector(move || "designed-checkbox".into())
        .flex()
        .items_center()
        .gap(tokens::space(Space::S))
        .cursor_pointer()
        .key_context(CHECKBOX_CONTEXT)
        .role(Role::CheckBox)
        .map(|checkbox| checkbox.aria_label(label.clone()))
        .aria_toggled(if checkbox.checked {
            Toggled::True
        } else {
            Toggled::False
        })
        .child(checkbox_box(checkbox.checked, theme))
        .when_some(checkbox.label.clone(), |checkbox, label| {
            checkbox.child(
                div()
                    .min_w(px(0.))
                    .text_size(theme.typography.settings_text_size)
                    .text_color(theme.text_title)
                    .child(label),
            )
        })
        .when_some(draw.focus_of(path), |checkbox, focus| {
            checkbox
                .track_focus(&focus)
                .focus(move |checkbox| checkbox.shadow(ring))
        })
        .on_action(press)
        .on_click(click)
        .into_any_element();
    // The form field's chrome around it: its title over, its note and
    // error under (#241).
    fields::field_group(&key.clone(), &checkbox.field, None, control, theme).into_any_element()
}

/// A checkbox's box, with its check.
fn checkbox_box(checked: bool, theme: &Theme) -> Div {
    let mark = px(16.);
    div()
        .flex_none()
        .size(mark)
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(4.))
        .border_1()
        .border_color(if checked {
            theme.accent
        } else {
            theme.text_muted
        })
        .when(checked, |mark| {
            mark.bg(theme.accent)
                .child(div().text_color(theme.accent_ink).child("✓"))
        })
}

/// A segmented control: the Settings board's track, one segment per
/// option, the chosen one filled. The group holds the keyboard: its
/// arrows move the choice to the next option, and a click steps it there
/// too.
pub(super) fn segmented(
    segmented: &SegmentedNode,
    key: Option<&str>,
    path: &str,
    draw: &Draw,
    cx: &mut gpui::Context<LauncherWindow>,
) -> AnyElement {
    let theme = draw.theme;
    let chosen = position_of(&segmented.options, segmented.value.as_deref());
    let ring = focus_ring(theme);
    let mut track = controls::segment_track(theme)
        .id(path.to_owned())
        .debug_selector(move || "designed-segmented".into())
        .key_context(SEGMENTED_CONTEXT)
        .role(Role::RadioGroup)
        .map(|group| {
            group.when_some(segmented.label.clone(), |group, label| {
                group.aria_label(label)
            })
        });
    for (index, option) in segmented.options.iter().enumerate() {
        let label: SharedString = option
            .label
            .clone()
            .unwrap_or_else(|| option.value.clone())
            .into();
        let is_chosen = Some(index) == chosen;
        track = track.child(
            controls::segment(label.clone(), is_chosen, true, theme)
                .id(format!("{path}/{index}"))
                .role(Role::RadioButton)
                .map(|segment| segment.aria_label(label))
                .aria_toggled(if is_chosen {
                    Toggled::True
                } else {
                    Toggled::False
                })
                .aria_position_in_set(index + 1)
                .aria_size_of_set(segmented.options.len())
                .when(is_chosen, |segment| segment.aria_active_descendant()),
        );
    }
    let Some(callback) = segmented.on_change else {
        return track.into_any_element();
    };
    // The arrows move the choice by one, wrapping; a click steps it too.
    let values = Rc::new(
        segmented
            .options
            .iter()
            .map(|option| option.value.clone())
            .collect::<Vec<String>>(),
    );
    let current = Rc::new(chosen);
    let key = key.unwrap_or_default().to_owned();

    let (for_move, key_for_move) = (callback, key.clone());
    let (for_click, key_for_click) = (callback, key);
    let (moved_values, moved_current) = (values.clone(), current.clone());
    let seen = draw.render;
    let (moved, click) = (
        cx.listener(move |this, _: &Move, window, cx| {
            if let Some(value) = step(&moved_values, *moved_current, 1) {
                this.designed_event(
                    DesignedHandler::Change,
                    for_move,
                    key_for_move.clone(),
                    seen,
                    change(&value),
                    window,
                    cx,
                );
            }
        }),
        cx.listener(move |this, _: &ClickEvent, window, cx| {
            if let Some(value) = step(&values, *current, 1) {
                this.designed_event(
                    DesignedHandler::Change,
                    for_click,
                    key_for_click.clone(),
                    seen,
                    change(&value),
                    window,
                    cx,
                );
            }
        }),
    );
    track
        .when_some(draw.focus_of(path), |track, focus| {
            track
                .track_focus(&focus)
                .focus(move |track| track.shadow(ring))
        })
        .on_action(moved)
        .on_click(click)
        .into_any_element()
}

/// Where the chosen option sits among `options`.
fn position_of(options: &[pane_core::Segment], value: Option<&str>) -> Option<usize> {
    value.and_then(|value| options.iter().position(|option| option.value == value))
}

/// The option `step` places from `current` in `options`, wrapping.
fn step(options: &[String], current: Option<usize>, step: isize) -> Option<String> {
    if options.is_empty() {
        return None;
    }
    let at = match current {
        Some(at) => (at as isize + step).rem_euclid(options.len() as isize) as usize,
        None => 0,
    };
    options.get(at).cloned()
}

/// One slider: a rail with its filled part and its knob, its value
/// between its bounds. Its arrows adjust it by its step, and a click
/// moves it to where the rail was clicked.
pub(super) fn slider(
    slider: &SliderNode,
    key: Option<&str>,
    path: &str,
    draw: &Draw,
    cx: &mut gpui::Context<LauncherWindow>,
) -> AnyElement {
    let theme = draw.theme;
    let SliderNode {
        value,
        min,
        max,
        step: by,
        on_change,
        label,
    } = slider;
    let (min, max, value, by) = (min.0, max.0, value.0, by.0.abs().max(f32::EPSILON));
    let span = (max - min).abs().max(f32::EPSILON);
    let fraction = ((value - min) / span).clamp(0., 1.);
    let Some(callback) = *on_change else {
        return div()
            .id(path.to_owned())
            .debug_selector(move || "designed-slider".into())
            .flex()
            .items_center()
            .flex_none()
            .w(theme.geometry.settings.choice_width)
            .child(rail_of(fraction, theme))
            .child(knob_of(theme))
            .into_any_element();
    };
    // The rail's bounds, recorded as it paints, for a click's place on it.
    let bounds = Rc::new(Cell::new(gpui::Bounds::default()));
    let rail = {
        let bounds = bounds.clone();
        rail_of(fraction, theme).child(gpui::canvas(
            move |area, _, _| bounds.set(area),
            |_, _, _, _| {},
        ))
    };
    let key = key.unwrap_or_default().to_owned();

    let (for_adjust, key_for_adjust) = (callback, key.clone());
    let (for_click, key_for_click) = (callback, key);
    let seen = draw.render;
    let clicked = cx.listener(move |this, event: &ClickEvent, window, cx| {
        let position = match event {
            ClickEvent::Mouse(click) => click.up.position,
            _ => bounds.get().center(),
        };
        let bounds = bounds.get();
        let width = bounds.size.width.as_f32().max(f32::EPSILON);
        let at = ((position.x.as_f32() - bounds.origin.x.as_f32()) / width).clamp(0., 1.);
        let next = min + at * span;
        this.designed_event(
            DesignedHandler::Change,
            for_click,
            key_for_click.clone(),
            seen,
            plain_change(next),
            window,
            cx,
        );
    });
    let ring = focus_ring(theme);
    div()
        .id(path.to_owned())
        .debug_selector(move || "designed-slider".into())
        .flex()
        .items_center()
        .flex_none()
        .w(theme.geometry.settings.choice_width)
        .cursor_pointer()
        .key_context(SLIDER_CONTEXT)
        .role(Role::Slider)
        .map(|slider| slider.when_some(label.clone(), |slider, label| slider.aria_label(label)))
        .aria_numeric_value(value as f64)
        .aria_min_numeric_value(min as f64)
        .aria_max_numeric_value(max as f64)
        .aria_numeric_value_step(by as f64)
        .child(rail)
        .child(knob_of(theme))
        .when_some(draw.focus_of(path), |slider, focus| {
            slider
                .track_focus(&focus)
                .focus(move |slider| slider.shadow(ring))
        })
        .on_action(cx.listener(move |this, _: &super::Adjust, window, cx| {
            let next = (value + by).clamp(min, max);
            this.designed_event(
                DesignedHandler::Change,
                for_adjust,
                key_for_adjust.clone(),
                seen,
                plain_change(next),
                window,
                cx,
            );
        }))
        .on_click(clicked)
        .into_any_element()
}

/// A slider's rail: the track with its filled part.
fn rail_of(fraction: f32, theme: &Theme) -> Div {
    div()
        .flex_1()
        .h(px(4.))
        .rounded(px(2.))
        .bg(theme.controls.toggle_off)
        .child(
            div()
                .w(relative(fraction))
                .h(px(4.))
                .rounded(px(2.))
                .bg(theme.accent),
        )
}

/// A slider's knob.
fn knob_of(theme: &Theme) -> Div {
    div()
        .flex_none()
        .size(px(12.))
        .rounded_full()
        .bg(theme.controls.toggle_knob)
        .shadow(vec![
            gpui::BoxShadow::new(px(0.), px(1.), theme.controls.toggle_knob_shadow)
                .blur_radius(px(2.)),
        ])
}

/// One progress bar: how far along it is, 0 to 1.
pub(super) fn progress(progress: &ProgressNode, path: &str, theme: &Theme) -> AnyElement {
    let value = progress.value.0.clamp(0., 1.);
    let width = theme.geometry.settings.choice_width;
    let label: SharedString = progress
        .label
        .clone()
        .unwrap_or_else(|| "progress".into())
        .into();
    div()
        .id(path.to_owned())
        .debug_selector(move || "designed-progress".into())
        .flex_none()
        .w(width)
        .h(px(4.))
        .rounded(px(2.))
        .bg(theme.controls.toggle_off)
        .child(
            div()
                .w(relative(value))
                .h(px(4.))
                .rounded(px(2.))
                .bg(theme.accent),
        )
        .role(Role::ProgressIndicator)
        .map(|bar| bar.aria_label(label))
        .aria_numeric_value(value as f64)
        .into_any_element()
}

/// One loading indicator: an indeterminate bar, its highlight sweeping
/// across it, Pane's own motion policy animating it (and reduced motion
/// stopping it).
pub(super) fn loading(loading: &LoadingNode, path: &str, theme: &Theme) -> AnyElement {
    let width = theme.geometry.settings.choice_width;
    let label: SharedString = loading
        .label
        .clone()
        .unwrap_or_else(|| "loading".into())
        .into();
    let highlight = div()
        .id(format!("{path}/highlight"))
        .h(px(4.))
        .w(width / 3.)
        .rounded(px(2.))
        .bg(theme.accent)
        .with_animation(
            format!("{path}/sweep"),
            gpui::Animation::new(std::time::Duration::from_millis(1200))
                .repeat()
                .with_max_fps(60.),
            move |highlight, phase| {
                let at = phase * (width.as_f32() + width.as_f32() / 3.) - width.as_f32() / 3.;
                highlight.ml(px(at))
            },
        );
    div()
        .id(path.to_owned())
        .debug_selector(move || "designed-loading".into())
        .flex_none()
        .w(width)
        .h(px(4.))
        .rounded(px(2.))
        .bg(theme.controls.toggle_off)
        .overflow_hidden()
        .child(highlight)
        .role(Role::ProgressIndicator)
        .map(|bar| bar.aria_label(label))
        .into_any_element()
}

// ------------------------------------------------------------ content

/// One markdown node: its blocks, drawn with the theme.
pub(super) fn markdown(
    markdown: &MarkdownNode,
    path: &str,
    draw: &Draw,
    cx: &mut gpui::Context<LauncherWindow>,
) -> AnyElement {
    super::markdown::blocks(&markdown.blocks, path, draw, cx)
}

/// A section header: a title over a group, with its note beside it.
pub(super) fn section_header(header: &SectionHeader, path: &str, theme: &Theme) -> AnyElement {
    let title: SharedString = header.title.clone().into();
    let debug = format!("designed-section-{}", header.title);
    div()
        .id(path.to_owned())
        .debug_selector(move || debug)
        .flex()
        .items_baseline()
        .gap(theme.geometry.section_gap)
        .child(
            div()
                .min_w(px(0.))
                .truncate()
                .text_size(theme.typography.section_size)
                .font_weight(theme.typography.medium)
                .text_color(theme.text_muted)
                .child(title.clone()),
        )
        .when_some(header.note.clone(), |header, note| {
            header.child(
                div()
                    .min_w(px(0.))
                    .flex_1()
                    .truncate()
                    .text_size(theme.typography.row_kind_size)
                    .text_color(theme.text_placeholder)
                    .child(note),
            )
        })
        .role(Role::Heading)
        .map(|header| header.aria_label(title))
        .into_any_element()
}

/// A metadata list: rows of a label and its value, link or tags. A row's
/// link is raised on the list node's `key`, whose handler the tree names
/// on it.
pub(super) fn metadata_list(
    list: &MetadataListNode,
    key: Option<&str>,
    path: &str,
    draw: &Draw,
    cx: &mut gpui::Context<LauncherWindow>,
) -> AnyElement {
    let key = key.unwrap_or_default().to_owned();

    let rows = list
        .items
        .iter()
        .enumerate()
        .map(|(index, item)| metadata_row(item, index, &key, path, draw, cx))
        .collect::<Vec<AnyElement>>();
    div()
        .id(path.to_owned())
        .debug_selector(move || "designed-metadata".into())
        .flex()
        .flex_col()
        .min_w(px(0.))
        .gap(tokens::space(Space::Xs))
        .children(rows)
        .into_any_element()
}

/// One row of a metadata list: a label, and its value, link or tags.
fn metadata_row(
    item: &MetadataItemNode,
    index: usize,
    key: &str,
    path: &str,
    draw: &Draw,
    cx: &mut gpui::Context<LauncherWindow>,
) -> AnyElement {
    let theme = draw.theme;
    let path = format!("{path}/{index}");
    if item.separator {
        return div()
            .id(path)
            .h(px(1.))
            .w_full()
            .bg(theme.hairline_soft)
            .into_any_element();
    }
    let value = match (&item.value, item.on_press) {
        // A value with a callback is a link.
        (Some(value), Some(callback)) => link_of(
            Some(callback),
            key,
            value,
            &format!("{path}/value"),
            None,
            &format!("designed-link-{}", short(value)),
            draw,
            cx,
        )
        .into_any_element(),
        (Some(value), None) => div()
            .id(format!("{path}/value"))
            .min_w(px(0.))
            .flex_1()
            .truncate()
            .text_size(theme.typography.row_subtitle_size)
            .text_color(theme.text_body)
            .role(Role::Label)
            .map(|run| run.aria_label(value.clone()))
            .child(value.clone())
            .into_any_element(),
        (None, _) => div().flex_1().into_any_element(),
    };
    let tags = if item.tags.is_empty() {
        None
    } else {
        Some(
            item.tags
                .iter()
                .map(|tag| tag_chip(tag, theme.text_body, theme).into_any_element())
                .collect::<Vec<AnyElement>>(),
        )
    };
    div()
        .id(path.clone())
        .flex()
        .items_baseline()
        .gap(tokens::space(Space::S))
        .min_w(px(0.))
        .when_some(item.label.clone(), |row, label| {
            row.child(
                div()
                    .id(format!("{path}/label"))
                    .flex_none()
                    .text_size(theme.typography.row_subtitle_size)
                    .text_color(theme.text_muted)
                    .role(Role::Label)
                    .map(|run| run.aria_label(label.clone()))
                    .child(label),
            )
        })
        .child(value)
        .when_some(tags, |row, tags| {
            row.child(div().flex().flex_wrap().gap_1().children(tags))
        })
        .into_any_element()
}

/// The row of an empty state's actions, when it has any.
fn actions_row(actions: Vec<AnyElement>) -> Option<Stateful<Div>> {
    (!actions.is_empty()).then(move || {
        div()
            .id("designed-empty-actions")
            .flex()
            .flex_wrap()
            .gap(tokens::space(Space::S))
            .children(actions)
    })
}

/// An empty state: an icon, a title and a description — the notice the
/// launcher's own empty board draws — with its children as its actions.
pub(super) fn empty_state(
    node: &Node,
    empty: &EmptyStateNode,
    path: &str,
    draw: &Draw,
    cx: &mut gpui::Context<LauncherWindow>,
) -> AnyElement {
    let theme = draw.theme;
    let results = &theme.results;
    let notice = &theme.geometry.results;
    let disc = notice.notice_disc;
    let duplicates = duplicate_keys(node);
    let title: SharedString = empty.title.clone().into();
    let debug = format!("designed-empty-{}", empty.title);
    let glyph = empty
        .icon
        .as_ref()
        .map(|icon| icon_at(icon, notice.notice_glyph.as_f32(), path, draw).into_any_element());
    let actions: Vec<AnyElement> = node
        .children
        .iter()
        .enumerate()
        .map(|(index, child)| {
            let mut child_path = format!("{path}/action");
            place_child(&mut child_path, child, index, &duplicates);
            super::tree::draw_node(child, &mut child_path, *draw, cx)
        })
        .collect();
    div()
        .id(path.to_owned())
        .debug_selector(move || debug)
        .flex()
        .items_start()
        .gap(notice.notice_gap)
        .p(tokens::space(Space::M))
        .rounded(notice.card_radius)
        .bg(results.card_fill)
        .child(
            div()
                .flex_none()
                .size(disc)
                .flex()
                .items_center()
                .justify_center()
                .rounded(disc / 2.)
                .bg(results.notice_disc)
                .shadow(vec![inset_ring(results.notice_disc_edge, px(1.))])
                .when_some(glyph, |disc, glyph| disc.child(glyph)),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .min_w(px(0.))
                .gap(tokens::space(Space::Xs))
                .child(
                    div()
                        .id(format!("{path}/title"))
                        .min_w(px(0.))
                        .text_size(theme.typography.results.notice_title.size)
                        .font_weight(theme.typography.medium)
                        .text_color(theme.text_title)
                        .role(Role::Heading)
                        .map(|heading| heading.aria_label(title.clone()))
                        .child(title),
                )
                .when_some(empty.description.clone(), |empty, description| {
                    empty.child(
                        div()
                            .min_w(px(0.))
                            .text_size(theme.typography.results.notice_description.size)
                            .text_color(theme.text_body)
                            .child(description),
                    )
                })
                .when_some(actions_row(actions), |empty, actions| empty.child(actions)),
        )
        .into_any_element()
}

// ------------------------------------------------------------ inputs

/// Which kind of field a text node is: a text input, a password field or
/// a text area.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum FieldKind {
    Text,
    Password,
    Area,
}

/// The character a password field shows for each character typed.
const CONCEALED: char = '\u{2022}';

/// One text input, password field or text area: a focusable well holding
/// GPUI CE's editable text — the text, the caret, the selection and the
/// input method's composition, with undo and the clipboard — keyed by its
/// path, so a re-render that still draws the field keeps what the user
/// was doing with it. Enter commits (a text area's Enter inserts a
/// newline) and a blur commits too; the value the tree names is the value
/// the field started from and the one an echo of it never fights, and a
/// value the extension set replaces the text.
pub(super) fn text_input(
    input: &TextInputNode,
    path: &str,
    draw: &Draw,
    kind: FieldKind,
    cx: &mut gpui::Context<LauncherWindow>,
) -> AnyElement {
    let theme = draw.theme;
    let Some((editing, focus, _)) = draw.field(path) else {
        return div().into_any_element();
    };
    let well = field_well(input, editing, focus, path, kind, theme, cx);
    // The field's commit: Enter runs it (a text area's Enter inserts a
    // newline), as a blur does — and Enter in a field of a form submits
    // the form, which its own commit handler decides (#241). Ctrl+Enter
    // submits from any field.
    let field_path = path.to_owned();
    let commit = cx.listener(move |this, _: &Commit, window, cx| {
        this.designed_field_committed(&field_path, window, cx);
    });
    let submit = cx.listener(move |this, _: &super::SubmitForm, window, cx| {
        this.submit_designed_form(window, cx);
    });
    div()
        .id(path.to_owned())
        .flex()
        .flex_col()
        .gap_1()
        .flex_none()
        .min_w(px(0.))
        .key_context(if kind == FieldKind::Area {
            AREA_CONTEXT
        } else {
            INPUT_CONTEXT
        })
        .child(fields::field_group(
            input
                .field
                .title
                .as_deref()
                .unwrap_or(path.rsplit('/').next().unwrap_or("field")),
            &input.field,
            input.label.as_deref(),
            well,
            theme,
        ))
        .on_action(commit)
        .on_action(submit)
        .into_any_element()
}

/// One field's well: GPUI CE's editable text in the field's chrome, the
/// shape every text-like field draws — a text input, a password field, a
/// text area, a date field, a picker of one path. The field's group
/// (its title, note and error) wraps it.
pub(super) fn field_well(
    input: &TextInputNode,
    editing: &gpui::Entity<gpui_elements::editable_text::EditableTextState>,
    focus: &gpui::FocusHandle,
    path: &str,
    kind: FieldKind,
    theme: &Theme,
    cx: &gpui::App,
) -> gpui::Stateful<gpui::Div> {
    let ring = controls::well_shadows(true, theme);
    let live = editing.read(cx).as_str().to_owned();
    let label: SharedString = input
        .label
        .clone()
        .or_else(|| input.field.title.clone())
        .or_else(|| input.placeholder.clone())
        .unwrap_or_else(|| "field".into())
        .into();
    let debug = format!("designed-input-{}", short(&label));
    let area = kind == FieldKind::Area;
    let password = kind == FieldKind::Password;
    // The password's text is drawn transparent, a dot for each character
    // over it, and its value reads as those dots (GPUI CE's editable text
    // has no masking of its own).
    let dots = password.then(|| live.chars().map(|_| CONCEALED).collect::<String>());
    let element = if area {
        area_input(format!("{path}/input")).state(editing.downgrade())
    } else {
        edit_input(format!("{path}/input")).state(editing.downgrade())
    };
    let element = field_input(
        element,
        &input.placeholder.clone().unwrap_or_default(),
        theme,
        area,
    );
    let value: SharedString = dots
        .clone()
        .map(SharedString::from)
        .unwrap_or_else(|| live.clone().into());
    controls::well(false, theme)
        .id(format!("{path}/well"))
        .debug_selector(move || debug.clone())
        .track_focus(focus)
        .when(area, |well| {
            // A text area is as tall as three of its lines.
            well.h(theme.typography.settings_text_size * theme.typography.line_height * 3.)
        })
        .role(match kind {
            FieldKind::Text => Role::TextInput,
            FieldKind::Password => Role::PasswordInput,
            FieldKind::Area => Role::MultilineTextInput,
        })
        .map(|field| field.aria_label(label.clone()))
        .map(|field| field.aria_value(value.clone()))
        .map(|field| {
            field.when_some(input.placeholder.clone(), |field, placeholder| {
                field.aria_placeholder(placeholder)
            })
        })
        .map(|field| {
            field.when_some(input.field.error.clone(), |field, error| {
                field.aria_description(error)
            })
        })
        .focus(move |field| field.shadow(ring))
        .child(
            div()
                .relative()
                .flex_1()
                .min_w(px(0.))
                .when_some(dots, |held, dots| {
                    held.map(|element| element.text_color(transparent_black()))
                        .child(
                            div()
                                .id(format!("{path}/concealed"))
                                .absolute()
                                .top_0()
                                .left_0()
                                .whitespace_nowrap()
                                .overflow_hidden()
                                .text_color(theme.text_title)
                                .aria_hidden()
                                .child(dots),
                        )
                })
                .child(element),
        )
}

/// One field's editable text, in the shape the form's fields draw theirs.
fn field_input(
    input: gpui_elements::editable_text::EditableTextElement,
    placeholder: &str,
    theme: &Theme,
    area: bool,
) -> gpui_elements::editable_text::EditableTextElement {
    let typography = &theme.typography;
    input
        .placeholder(placeholder)
        .placeholder_color(theme.text_placeholder)
        .caret_color(theme.accent_text)
        .selection_color(theme.selection_wash)
        .marked_color(theme.accent_text)
        .text_size(typography.settings_text_size)
        .text_color(theme.text_title)
        .font_family(typography.family.clone())
        .font_features(typography.features.clone())
        .w_full()
        .min_w(px(0.))
        .when(area, |input| input.whitespace_normal())
        .when(!area, |input| input.whitespace_nowrap().overflow_x_scroll())
}

/// One item of a list the launcher does not present (a second list in
/// the tree, #240): a row of its icon, title and subtitle, drawn as a
/// rich row is, pressable by the action it carries.
pub(super) fn list_item(
    node: &Node,
    item: &pane_core::ListItem,
    path: &str,
    draw: &Draw,
    cx: &mut gpui::Context<LauncherWindow>,
) -> AnyElement {
    let theme = draw.theme;
    let debug = format!("designed-row-{}", item.title);
    let title: SharedString = item.title.clone().into();
    let subtitle: SharedString = item
        .subtitle
        .clone()
        .map(SharedString::from)
        .unwrap_or_default();
    let icon = item
        .icon
        .as_ref()
        .map(|icon| icon_at(icon, 16., path, draw).into_any_element());
    let row = div()
        .id(path.to_owned())
        .debug_selector(move || debug)
        .flex()
        .items_center()
        .min_h(theme.geometry.row_min_height)
        .px(theme.geometry.row_padding_x)
        .rounded(theme.geometry.row_radius)
        .gap(theme.geometry.row_gap)
        .role(Role::ListItem)
        .map(|row| row.aria_label(title.clone()))
        .when(!subtitle.is_empty(), |row| {
            row.aria_description(subtitle.clone())
        })
        .when_some(icon, |row, icon| row.child(icon))
        .child(
            div()
                .flex()
                .flex_col()
                .min_w(px(0.))
                .flex_1()
                .gap_1()
                .child(
                    div()
                        .min_w(px(0.))
                        .truncate()
                        .text_size(theme.typography.row_title_size)
                        .font_weight(theme.typography.medium)
                        .text_color(theme.text_title)
                        .child(title),
                )
                .when(!subtitle.is_empty(), |column| {
                    column.child(
                        div()
                            .min_w(px(0.))
                            .truncate()
                            .text_size(theme.typography.row_subtitle_size)
                            .text_color(theme.text_body)
                            .child(subtitle),
                    )
                }),
        );
    let Some(callback) = item.on_press else {
        return row.into_any_element();
    };
    let key = node.key.clone().unwrap_or_default();
    let (for_press, key_for_press) = (callback, key.clone());
    let (for_click, key_for_click) = (callback, key);
    let seen = draw.render;
    let (press, click) = (
        cx.listener(move |this, _: &Press, window, cx| {
            this.designed_event(
                DesignedHandler::Press,
                for_press,
                key_for_press.clone(),
                seen,
                "{}".into(),
                window,
                cx,
            );
        }),
        cx.listener(move |this, _: &ClickEvent, window, cx| {
            this.designed_event(
                DesignedHandler::Press,
                for_click,
                key_for_click.clone(),
                seen,
                "{}".into(),
                window,
                cx,
            );
        }),
    );
    let ring = focus_ring(theme);
    row.key_context(BUTTON_CONTEXT)
        .when_some(draw.focus_of(path), |row, focus| {
            row.track_focus(&focus).focus(move |row| row.shadow(ring))
        })
        .on_action(press)
        .on_click(click)
        .into_any_element()
}

/// One cell of a grid the launcher does not present: its image or colour,
/// with its title and subtitle under it.
pub(super) fn grid_item(
    item: &pane_core::GridItem,
    path: &str,
    draw: &Draw,
    cx: &mut gpui::Context<LauncherWindow>,
) -> AnyElement {
    let theme = draw.theme;
    let _ = cx;
    let image = item
        .image
        .as_ref()
        .map(|image| grid_image(image, pane_core::Fit::Contain, path, draw));
    let color = item.color.as_ref().map(|paint| {
        let fill = tokens::paint_color(paint, theme);
        div()
            .size(px(48.))
            .rounded(theme.geometry.row_radius)
            .bg(fill)
            .into_any_element()
    });
    div()
        .id(path.to_owned())
        .flex()
        .flex_col()
        .min_w(px(0.))
        .gap(tokens::space(pane_core::Space::Xs))
        .when_some(image.or(color), |cell, held| cell.child(held))
        .when_some(item.title.clone(), |cell, title| {
            cell.child(
                div()
                    .min_w(px(0.))
                    .truncate()
                    .text_size(theme.typography.row_subtitle_size)
                    .text_color(theme.text_title)
                    .child(title),
            )
        })
        .when_some(item.subtitle.clone(), |cell, subtitle| {
            cell.child(
                div()
                    .min_w(px(0.))
                    .truncate()
                    .text_size(theme.typography.row_kind_size)
                    .text_color(theme.text_muted)
                    .child(subtitle),
            )
        })
        .into_any_element()
}

/// One select: the searchable select of `ui::select`, keyed by its path
/// so its open state, query and highlight survive a re-render that still
/// draws it. Its trigger shows the choice the tree names, read live each
/// frame; a choice the user commits is told to the extension through the
/// change handler the tree names, and a select whose search the extension
/// handles tells it the query as the user types it (#241), its popup
/// showing the choices the extension answers with.
pub(super) fn select(
    select: &SelectNode,
    path: &str,
    draw: &Draw,
    _cx: &mut gpui::Context<LauncherWindow>,
) -> AnyElement {
    let theme = draw.theme;
    if select.options.is_empty() {
        // A select with no options draws its trigger alone, offering
        // nothing.
        let label: SharedString = select
            .value
            .as_deref()
            .and_then(|value| {
                select
                    .options
                    .iter()
                    .find(|option| option.value == value)
                    .map(|option| option.label.clone().unwrap_or_else(|| option.value.clone()))
            })
            .unwrap_or_else(|| " ".into())
            .into();
        return controls::select_trigger(label, theme).into_any_element();
    }
    let Some(select_entity) = draw.select(path) else {
        return div().into_any_element();
    };
    // The select's trigger carries its own focus and keys; the field's
    // group carries its title, note and error.
    let trigger = div().child(select_entity.clone());
    fields::field_group(
        select
            .field
            .title
            .as_deref()
            .or(select.label.as_deref())
            .unwrap_or(path.rsplit('/').next().unwrap_or("field")),
        &select.field,
        select.label.as_deref(),
        trigger,
        theme,
    )
    .into_any_element()
}

/// One inline query field, as a tag picker's query draws: the shared
/// editable text element in the field's own type.
pub(super) fn query_input(
    input: gpui::Entity<gpui_elements::editable_text::EditableTextState>,
    theme: &Theme,
) -> gpui_elements::editable_text::EditableTextElement {
    field_input(
        edit_input("query").state(input.downgrade()),
        "",
        theme,
        false,
    )
}

/// One chosen chip, as a tag picker's chosen tag or a picker-of-many's
/// chosen path draws: the tag chip's own shape, in the body text's
/// colour. The caller attaches the click that removes it.
pub(super) fn chosen_chip(text: &str, theme: &Theme) -> Stateful<Div> {
    tag_chip(text, theme.text_body, theme).id(SharedString::from(text.to_owned()))
}
