//! Markdown, drawn: the blocks pane-core's small parser read from a
//! `markdown` node, laid out in the theme's own typography and chrome —
//! headings, paragraphs, code on a card, quotes with a rule, lists with
//! their bullets, numbers and task checkboxes, rules, and GitHub's
//! tables (#237, ADR 0036). A link is drawn as a link, its address its
//! tooltip; opening what it names is the standard views' to do (#240).
//! Each link is a tab stop of its own, in reading order: Enter or Space
//! opens it, and the footer shows its address while it is focused
//! (#499).
//!
//! Inline content flows as a wrapping row of runs, as a text node's
//! spans do: prose, code, emphasis, strong text, images and links beside
//! each other, a hard break wrapping the row. An image is the icon model's
//! — its source resolved and loaded as the tree's icons are (#240), its
//! size the `=WxH` its source named — and a link is opened by Pane with
//! the system's handler, as a computed result's is.

use std::cell::Cell;

use gpui::prelude::*;
use gpui::{AnyElement, ClickEvent, Context, Div, FocusHandle, Role, TextAlign, div, px};

use pane_core::Space;
use pane_core::markdown::{Alignment, Block, Inline, Item};

use crate::app::LauncherWindow;
use crate::ui::controls::focus_ring;
use crate::ui::tokens;
use crate::ui::tooltip::{TooltipLook, text_tooltip};

use super::tree::Draw;
use super::{BUTTON_CONTEXT, Press};

/// The tab stops of one Markdown node's links, handed out in reading
/// order as its links are drawn: the order [`link_targets`] lists them in.
struct Links<'a> {
    node: &'a str,
    next: Cell<usize>,
}

impl Links<'_> {
    /// The next link's tab stop, when the node keeps one for it.
    fn next(&self, draw: &Draw) -> Option<FocusHandle> {
        let place = self.next.get();
        self.next.set(place + 1);
        draw.link_focus(self.node, place)
    }
}

/// The addresses of a Markdown node's links, in reading order — the
/// order they are drawn and tabbed through in (#499).
pub(super) fn link_targets(blocks: &[Block]) -> Vec<String> {
    let mut targets = Vec::new();
    for block in blocks {
        block_links(block, &mut targets);
    }
    targets
}

/// The addresses of one block's links, after `targets`.
fn block_links(block: &Block, targets: &mut Vec<String>) {
    match block {
        Block::Heading { inlines, .. } | Block::Paragraph(inlines) => {
            inline_links(inlines, targets)
        }
        Block::Quote(inner) => {
            for block in inner {
                block_links(block, targets);
            }
        }
        Block::List { items, .. } => {
            for block in items.iter().flat_map(|item| &item.blocks) {
                block_links(block, targets);
            }
        }
        Block::Table { head, rows, .. } => {
            for cell in head.iter().chain(rows.iter().flatten()) {
                inline_links(cell, targets);
            }
        }
        Block::Code { .. } | Block::Rule => {}
    }
}

/// The addresses of inline content's links, after `targets`.
fn inline_links(inlines: &[Inline], targets: &mut Vec<String>) {
    for inline in inlines {
        match inline {
            Inline::Emphasis(inner) | Inline::Strong(inner) => inline_links(inner, targets),
            Inline::Link { text, href } => {
                targets.push(href.clone());
                inline_links(text, targets);
            }
            Inline::Text(_) | Inline::Code(_) | Inline::Image { .. } | Inline::Break => {}
        }
    }
}

/// The blocks of one markdown node, drawn.
pub(super) fn blocks(
    blocks: &[Block],
    path: &str,
    draw: &Draw,
    cx: &mut Context<LauncherWindow>,
) -> AnyElement {
    let links = Links {
        node: path,
        next: Cell::new(0),
    };
    nested(blocks, path, &links, draw, cx)
}

/// Blocks drawn in a column: the node's own, or a quote's.
fn nested(
    blocks: &[Block],
    path: &str,
    links: &Links,
    draw: &Draw,
    cx: &mut Context<LauncherWindow>,
) -> AnyElement {
    let mut element = div()
        .id(path.to_owned())
        .flex()
        .flex_col()
        .min_w(px(0.))
        .gap(tokens::space(Space::S));
    for (index, block) in blocks.iter().enumerate() {
        element = element.child(block_element(
            block,
            &format!("{path}/{index}"),
            links,
            draw,
            cx,
        ));
    }
    element.into_any_element()
}

/// One block, drawn.
fn block_element(
    block: &Block,
    path: &str,
    links: &Links,
    draw: &Draw,
    cx: &mut Context<LauncherWindow>,
) -> AnyElement {
    let theme = draw.theme;
    match block {
        Block::Heading { level, inlines } => {
            let (size, weight) = match level {
                1 => (px(16.), theme.typography.medium),
                2 => (theme.typography.row_title_size, theme.typography.medium),
                _ => (theme.typography.row_subtitle_size, theme.typography.medium),
            };
            let name = plain(inlines);
            div()
                .id(path.to_owned())
                .flex_none()
                .min_w(px(0.))
                .text_size(size)
                .font_weight(weight)
                .text_color(theme.text_title)
                .role(Role::Heading)
                .map(|heading| heading.aria_label(name))
                .map(|heading| heading.aria_level(*level))
                .child(inlines_element(inlines, links, draw, cx))
                .into_any_element()
        }
        Block::Paragraph(inlines) => div()
            .id(path.to_owned())
            .flex_none()
            .min_w(px(0.))
            .text_size(theme.typography.row_subtitle_size)
            .text_color(theme.text_body)
            .child(inlines_element(inlines, links, draw, cx))
            .into_any_element(),
        Block::Code { language, text } => div()
            .id(path.to_owned())
            .flex_none()
            .min_w(px(0.))
            .p(tokens::space(Space::S))
            .rounded(theme.geometry.settings.card_radius)
            .bg(theme.card_fill)
            .shadow(vec![crate::ui::controls::inset_ring(
                theme.card_edge,
                px(1.),
            )])
            .font_family(theme.typography.mono_family.clone())
            .text_size(theme.typography.keycap_size)
            .text_color(theme.text_title)
            .map(|code| {
                code.when_some(
                    language.clone().map(gpui::SharedString::from),
                    |code, language| code.aria_label(language),
                )
            })
            .child(text.clone())
            .into_any_element(),
        Block::Quote(inner) => div()
            .id(path.to_owned())
            .flex_none()
            .min_w(px(0.))
            .pl(tokens::space(Space::S))
            .border_l_2()
            .border_color(theme.hairline)
            .child(nested(inner, &format!("{path}/quote"), links, draw, cx))
            .into_any_element(),
        Block::List { ordered, items } => list(*ordered, items, path, links, draw, cx),
        Block::Rule => div()
            .id(path.to_owned())
            .flex_none()
            .w_full()
            .h(px(1.))
            .bg(theme.hairline_soft)
            .into_any_element(),
        Block::Table { aligns, head, rows } => table(aligns, head, rows, path, links, draw, cx),
    }
}

/// One list: its items, each with its bullet or number, and its task's
/// checkbox drawn as a mark.
fn list(
    ordered: bool,
    items: &[Item],
    path: &str,
    links: &Links,
    draw: &Draw,
    cx: &mut Context<LauncherWindow>,
) -> AnyElement {
    let theme = draw.theme;
    let mut element = div()
        .id(path.to_owned())
        .flex()
        .flex_col()
        .min_w(px(0.))
        .gap(tokens::space(Space::Xs));
    for (index, item) in items.iter().enumerate() {
        let mark: gpui::SharedString = match (ordered, item.checked) {
            (_, Some(checked)) => (if checked { "☑ " } else { "☐ " }).into(),
            (true, None) => format!("{}. ", index + 1).into(),
            (false, None) => "• ".into(),
        };
        element = element.child(
            div()
                .id(format!("{path}/{index}"))
                .flex()
                .min_w(px(0.))
                .gap(tokens::space(Space::Xs))
                .child(
                    div()
                        .flex_none()
                        .text_size(theme.typography.row_subtitle_size)
                        .text_color(theme.text_muted)
                        .child(mark),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .min_w(px(0.))
                        .flex_1()
                        .gap(tokens::space(Space::Xs))
                        .children(
                            item.blocks
                                .iter()
                                .enumerate()
                                .map(|(at, block)| {
                                    block_element(
                                        block,
                                        &format!("{path}/{index}/{at}"),
                                        links,
                                        draw,
                                        cx,
                                    )
                                })
                                .collect::<Vec<AnyElement>>(),
                        ),
                ),
        );
    }
    element.into_any_element()
}

/// One table: a head row, bold with a rule under it, and its body rows,
/// each column sharing the width.
#[allow(clippy::too_many_arguments)]
fn table(
    aligns: &[Alignment],
    head: &[Vec<Inline>],
    rows: &[Vec<Vec<Inline>>],
    path: &str,
    links: &Links,
    draw: &Draw,
    cx: &mut Context<LauncherWindow>,
) -> AnyElement {
    let theme = draw.theme;
    let columns = aligns.len().max(head.len());
    let empty: Vec<Inline> = Vec::new();
    let no_align = Alignment::None;
    let mut element = div()
        .id(path.to_owned())
        .flex()
        .flex_col()
        .min_w(px(0.))
        .gap(tokens::space(Space::Xs));
    element = element.child(
        div()
            .id(format!("{path}/head"))
            .flex()
            .min_w(px(0.))
            .pb(tokens::space(Space::Xs))
            .border_b_1()
            .border_color(theme.hairline)
            .children(
                head.iter()
                    .chain(std::iter::repeat(&empty))
                    .take(columns)
                    .zip(aligns.iter().chain(std::iter::repeat(&no_align)))
                    .enumerate()
                    .map(|(at, (cell, align))| {
                        cell_element(
                            cell,
                            *align,
                            true,
                            &format!("{path}/head/{at}"),
                            links,
                            draw,
                            cx,
                        )
                    })
                    .collect::<Vec<AnyElement>>(),
            ),
    );
    for (index, row) in rows.iter().enumerate() {
        element = element.child(
            div()
                .id(format!("{path}/{index}"))
                .flex()
                .min_w(px(0.))
                .children(
                    row.iter()
                        .chain(std::iter::repeat(&empty))
                        .take(columns)
                        .zip(aligns.iter().chain(std::iter::repeat(&no_align)))
                        .enumerate()
                        .map(|(at, (cell, align))| {
                            cell_element(
                                cell,
                                *align,
                                false,
                                &format!("{path}/{index}/{at}"),
                                links,
                                draw,
                                cx,
                            )
                        })
                        .collect::<Vec<AnyElement>>(),
                ),
        );
    }
    element.into_any_element()
}

/// One cell of a table: its inline content, aligned as its column is.
fn cell_element(
    cell: &[Inline],
    align: Alignment,
    head: bool,
    path: &str,
    links: &Links,
    draw: &Draw,
    cx: &mut Context<LauncherWindow>,
) -> AnyElement {
    let theme = draw.theme;
    let element = div()
        .id(path.to_owned())
        .flex_1()
        .min_w(px(0.))
        .map(|cell| match align {
            Alignment::Left => cell.text_align(TextAlign::Left),
            Alignment::Center => cell.text_align(TextAlign::Center),
            Alignment::Right => cell.text_align(TextAlign::Right),
            Alignment::None => cell,
        })
        .text_size(theme.typography.row_subtitle_size)
        .map(|cell| {
            if head {
                cell.font_weight(theme.typography.medium)
                    .text_color(theme.text_title)
            } else {
                cell.text_color(theme.text_body)
            }
        })
        .child(inlines_element(cell, links, draw, cx));
    element.into_any_element()
}

/// One run of inline content: prose, code, emphasis, strong text and
/// links, flowing beside each other and wrapping onto lines as the width
/// runs out.
fn inlines_element(
    inlines: &[Inline],
    links: &Links,
    draw: &Draw,
    cx: &mut Context<LauncherWindow>,
) -> Div {
    let theme = draw.theme;
    let mut element = div()
        .flex()
        .flex_wrap()
        .items_baseline()
        .min_w(px(0.))
        .text_size(theme.typography.row_subtitle_size)
        .text_color(theme.text_body);
    for inline in inlines {
        element = match inline {
            Inline::Text(text) => element.child(
                div()
                    .flex_none()
                    .min_w(px(0.))
                    .child(gpui::SharedString::from(text.clone())),
            ),
            Inline::Code(text) => element.child(
                div()
                    .flex_none()
                    .font_family(theme.typography.mono_family.clone())
                    .text_size(theme.typography.keycap_size)
                    .text_color(theme.text_title)
                    .child(gpui::SharedString::from(text.clone())),
            ),
            // An image: its icon as the tree resolved it, drawn as the
            // grid's cells draw theirs, at the size the source named.
            Inline::Image {
                alt,
                source,
                title,
                width,
                height,
                icon,
            } => {
                let pixels = width.or(*height).map_or(px(96.), |named| px(named as f32));
                let named: gpui::SharedString = source.clone().into();
                let tooltip = title
                    .clone()
                    .or_else(|| (!alt.is_empty()).then(|| alt.clone()));
                let image = match icon {
                    Some(icon) => super::components::grid_image(
                        icon,
                        pane_core::Fit::Contain,
                        &format!("markdown-image-{source}"),
                        draw,
                    ),
                    // Not resolved yet, or one Pane cannot read: its alt
                    // names the place it holds.
                    None => div()
                        .id(format!("markdown-image-missing-{source}"))
                        .size(pixels)
                        .rounded(theme.geometry.row_radius)
                        .bg(theme.card_fill)
                        .into_any_element(),
                };
                element.child(
                    div()
                        .id(format!("markdown-image-{source}"))
                        .flex_none()
                        .size(pixels)
                        .overflow_hidden()
                        .rounded(theme.geometry.row_radius)
                        .role(Role::Image)
                        .map(|image_node| {
                            image_node.aria_label(named.clone()).when_some(
                                tooltip,
                                |image_node, tooltip| {
                                    image_node.tooltip(text_tooltip(
                                        gpui::SharedString::from(tooltip),
                                        TooltipLook::of(theme),
                                    ))
                                },
                            )
                        })
                        .child(image),
                )
            }
            Inline::Emphasis(inner) => element.child(
                div()
                    .flex_none()
                    .min_w(px(0.))
                    .italic()
                    .child(inlines_element(inner, links, draw, cx)),
            ),
            Inline::Strong(inner) => element.child(
                div()
                    .flex_none()
                    .min_w(px(0.))
                    .font_weight(theme.typography.medium)
                    .text_color(theme.text_title)
                    .child(inlines_element(inner, links, draw, cx)),
            ),
            Inline::Link { text, href } => {
                let label = plain(text);
                let url = href.clone();
                let pressed = href.clone();
                // Its tab stop, taken before its text's, as the links are
                // listed (#499).
                let focus = links.next(draw);
                let ring = focus_ring(theme);
                let debug = format!("markdown-link-{label}");
                element.child(
                    div()
                        .id(format!("markdown-link-{href}"))
                        .debug_selector(move || debug.clone())
                        .flex_none()
                        .min_w(px(0.))
                        .cursor_pointer()
                        .underline()
                        .text_color(theme.accent_text)
                        .role(Role::Link)
                        .map(|link| link.aria_label(label))
                        .map(|link| {
                            link.tooltip(text_tooltip(
                                gpui::SharedString::from(href.clone()),
                                TooltipLook::of(theme),
                            ))
                        })
                        // The standard views open what a Markdown link
                        // names (#240), as a computed result's row does.
                        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                            this.designed_open_link(&url, window, cx);
                        }))
                        // Enter and Space open it while it is focused, as
                        // they press a link node (#499).
                        .key_context(BUTTON_CONTEXT)
                        .when_some(focus, |link, focus| {
                            link.track_focus(&focus)
                                .focus(move |link| link.shadow(ring))
                        })
                        .on_action(cx.listener(move |this, _: &Press, window, cx| {
                            this.designed_open_link(&pressed, window, cx);
                        }))
                        .child(inlines_element(text, links, draw, cx)),
                )
            }
            // A hard break wraps the row: a full-width, no-height run.
            Inline::Break => element.child(div().w_full().h(px(0.))),
        };
    }
    element
}

/// The plain text of inline content.
fn plain(inlines: &[Inline]) -> gpui::SharedString {
    let mut text = String::new();
    for inline in inlines {
        match inline {
            Inline::Text(part) | Inline::Code(part) => text.push_str(part),
            Inline::Emphasis(inner) | Inline::Strong(inner) => text.push_str(&plain(inner)),
            Inline::Link { text: inner, .. } => text.push_str(&plain(inner)),
            Inline::Image { alt, .. } => text.push_str(alt),
            Inline::Break => text.push(' '),
        }
    }
    text.into()
}
