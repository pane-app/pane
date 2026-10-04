//! The shared keycap: a presentation-only keystroke component.
//!
//! A keycap shows one key's glyph in a small cap of the reference's control
//! chrome — the neutral tile's background, inset edge and top highlight at
//! the keycap's own scale — and names the key to assistive technology, so
//! the cap a user sees and the key a screen reader reads cannot diverge.
//! Both come from the same value: a [`Key`] with its own glyph, or a
//! [`pane_core::Binding`] named in text by [`binding_keycap`].
//!
//! Presentation only: the component renders the key it is given and knows
//! nothing of actions or bindings. The caller that knows the effective
//! binding chooses the key (the launcher's footer passes the invoke
//! action's binding, which is the Enter key until the Keyboard page rebinds
//! it — and the Enter key keeps its glyph, whatever else takes its place).
//!
//! The cap is an accessibility node — an image named for the key — so its
//! name is exposed wherever a keycap is placed. Its id is fixed, so a
//! parent that shows several keycaps in one scope wraps each in its own
//! container.

use gpui::prelude::*;
use gpui::{BoxShadow, Div, Role, Stateful, div, px, svg};
use pane_core::Binding;

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
    cap(
        key.name().to_owned(),
        theme,
        div().child(
            svg()
                .data(key.svg_bytes())
                .size(geometry.keycap_glyph_size)
                .text_color(theme.tile_foreground),
        ),
    )
}

/// A keycap for an effective binding: the Enter glyph for the Enter key
/// itself, the binding's name in the same chrome for any other key — so
/// the hint a keycap teaches follows the binding in force, and its
/// accessible name is the name the binding's key shows. Used where the
/// caller knows the binding (the launcher's footer, the menu's Settings
/// entry); [`keycap`] is used where only the key is known.
pub(crate) fn binding_keycap(binding: &Binding, theme: &Theme) -> Stateful<Div> {
    // A binding of the plain Enter key keeps the Enter glyph; anything
    // else — another key, or a modifier with it — is named in text.
    let (control, alt, _shift, platform, function) = binding.modifiers();
    let plain_enter = !control && !alt && !platform && !function && binding.key() == "enter";
    if plain_enter {
        return keycap(Key::Enter, theme);
    }
    cap(binding.to_string(), theme, div().child(binding.to_string()))
}

/// One cap of the keycap chrome: `name` for assistive technology, the
/// `content` inside — a glyph or the binding's text.
fn cap(name: String, theme: &Theme, content: Div) -> Stateful<Div> {
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
        .aria_label(name)
        .text_size(theme.typography.row_title_size)
        .font_weight(theme.typography.medium)
        .text_color(theme.tile_foreground)
        .child(content)
}
