//! A text tooltip (#139): what an extension says about a row's title,
//! subtitle, accessory or icon, shown while the pointer rests on it — so
//! that text cut off by the row's width can still be read.
//!
//! The tooltip is a small popover with the text in the title ink. Its
//! element carries the debug selector `tooltip-<text>` for the tests.

use gpui::{
    AnyView, App, Context, Hsla, IntoElement, Pixels, Render, SharedString, Window, div,
    prelude::*, px,
};

use crate::ui::theme::Theme;

/// How a tooltip looks, read from the theme when the element that has one
/// is drawn.
#[derive(Clone)]
pub(crate) struct TooltipLook {
    background: Hsla,
    edge: Hsla,
    text: Hsla,
    size: Pixels,
    family: SharedString,
}

impl TooltipLook {
    pub(crate) fn of(theme: &Theme) -> TooltipLook {
        TooltipLook {
            background: theme.popover_solid,
            edge: theme.popover_edge,
            text: theme.text_title,
            size: theme.typography.row_kind_size,
            family: theme.typography.family.clone(),
        }
    }
}

/// The tooltip view: `text` in `look`.
struct TextTooltip {
    text: SharedString,
    look: TooltipLook,
}

impl Render for TextTooltip {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let look = &self.look;
        let text = self.text.clone();
        div()
            .debug_selector(move || format!("tooltip-{text}"))
            .max_w(px(360.))
            .px(px(8.))
            .py(px(4.))
            .rounded(px(6.))
            .bg(look.background)
            .border_1()
            .border_color(look.edge)
            .font_family(look.family.clone())
            .text_size(look.size)
            .text_color(look.text)
            .child(self.text.clone())
    }
}

/// What builds the tooltip saying `text` in `look`, for an element's
/// `.tooltip(…)`.
pub(crate) fn text_tooltip(
    text: SharedString,
    look: TooltipLook,
) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
    move |_, cx| {
        let text = text.clone();
        let look = look.clone();
        cx.new(|_| TextTooltip { text, look }).into()
    }
}
