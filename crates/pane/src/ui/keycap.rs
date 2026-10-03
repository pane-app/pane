//! The shared keycap: a presentation-only keystroke component.
//!
//! A keycap shows one key's glyph in a small cap of the reference's control
//! chrome — the neutral tile's background, inset edge and top highlight at
//! the keycap's own scale — and names the key to assistive technology, so
//! the cap a user sees and the key a screen reader reads cannot diverge.
//! Both come from the same [`Key`] value.
//!
//! Presentation only: the component renders the key it is given and knows
//! nothing of actions or bindings. The caller that knows the effective
//! binding chooses the key (the launcher's footer passes [`Key::Enter`],
//! the key its Confirm binding uses today; when bindings become
//! configurable, the same call site feeds the key that is in effect).
//!
//! The cap is an accessibility node — an image named for the key — so its
//! name is exposed wherever a keycap is placed. Its id is fixed, so a
//! parent that shows several keycaps in one scope wraps each in its own
//! container.

use gpui::prelude::*;
use gpui::{BoxShadow, Div, Role, Stateful, div, px, svg};

use crate::ui::theme::Theme;

/// A key the launcher shows a keycap for: the key's accessible name and its
/// glyph come from the same value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Key {
    /// The primary action's key, the launcher's Confirm binding.
    Enter,
}

impl Key {
    /// The key's name: what assistive technology reads and the keycap's
    /// accessible label.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Key::Enter => "Enter",
        }
    }

    /// The embedded SVG bytes for this key's glyph, authored in the
    /// reference icon set's style (24×24 viewBox, 1.6 stroke, round caps
    /// and joins).
    fn svg_bytes(self) -> &'static [u8] {
        match self {
            Key::Enter => include_bytes!("../../assets/icons/enter.svg"),
        }
    }
}

/// A keycap showing `key`'s glyph with its accessible key name. See the
/// module docs for what the component owns and what the caller owns.
pub(crate) fn keycap(key: Key, theme: &Theme) -> Stateful<Div> {
    let geometry = &theme.geometry;
    div()
        .id("keycap")
        .debug_selector(|| "keycap".into())
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .h(geometry.keycap_height)
        .px(geometry.keycap_padding_x)
        .rounded(geometry.keycap_radius)
        .bg(theme.tile_background)
        // The neutral tile's chrome at the keycap's scale: a 1px inset
        // edge and a top inset highlight.
        .shadow(vec![
            BoxShadow::new(px(0.), px(0.), theme.tile_border)
                .spread_radius(px(1.))
                .inset(),
            BoxShadow::new(px(0.), px(1.), theme.tile_highlight).inset(),
        ])
        .role(Role::Image)
        .aria_label(key.name())
        .child(
            svg()
                .data(key.svg_bytes())
                .size(geometry.keycap_glyph_size)
                .text_color(theme.tile_foreground),
        )
}
