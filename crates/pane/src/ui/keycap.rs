//! Keycaps: a key sequence drawn the reference's way — one cap per key,
//! side by side.
//!
//! The reference's `.kbd` is a 20px cap at least 20px wide, 5px of padding
//! either side of its label, radius 5, in Geist Mono 11/500 at line height
//! 1: a white 7% fill with a white 8% inset ring and a black 35% line
//! inset along its *bottom*. A chord is a `.keys` group: one cap per key,
//! 3px apart. Two variants change the cap, not the arrangement:
//!
//! - **compact** (`.slot-k .kbd`): 17px high, at least 17 wide, 4px
//!   padding, 10px type — the pinned slots' corner hints;
//! - **accent**: the primary action's key — the lime accent filled under
//!   near-black ink, with neither ring nor bottom line.
//!
//! Presentation only. The component draws the [`KeySequence`] it is
//! given: which keys a binding consists of, in which order and under which
//! names, is decided by the caller that knows the binding (the launcher's
//! adapter, [`crate::keyboard::binding_keys`]). So the cap a user sees and
//! the name assistive technology reads come from the same value: the
//! group is one image node named for the whole sequence ("Ctrl+Shift+V"),
//! and its caps are its drawing. Its id is fixed, so a parent that shows
//! several sequences in one scope wraps each in its own container.

use gpui::prelude::*;
use gpui::{BoxShadow, Div, Pixels, Role, SharedString, Stateful, div, px};

use crate::ui::theme::Theme;

/// One key of a sequence: what its cap shows, and the key's name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Key {
    /// The cap's label: a word ("Ctrl"), a character ("V") or a symbol
    /// ("↵", "←").
    pub(crate) cap: SharedString,
    /// The key's name as assistive technology reads it ("Enter", "Left").
    pub(crate) name: SharedString,
}

impl Key {
    pub(crate) fn new(cap: impl Into<SharedString>, name: impl Into<SharedString>) -> Key {
        Key {
            cap: cap.into(),
            name: name.into(),
        }
    }
}

/// A key sequence: the keys pressed together, in the order their caps
/// read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct KeySequence {
    pub(crate) keys: Vec<Key>,
}

impl KeySequence {
    /// The sequence's name, as it is announced and as an element's
    /// `aria-keyshortcuts` carries it: the keys' names joined by `+`.
    pub(crate) fn name(&self) -> String {
        self.keys
            .iter()
            .map(|key| key.name.as_ref())
            .collect::<Vec<_>>()
            .join("+")
    }
}

/// Which cap the sequence is drawn in (see the module docs).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CapStyle {
    /// The reference's `.kbd`: a hint beside a row or a button.
    Regular,
    /// `.slot-k .kbd`: a pinned slot's corner hint.
    Compact,
    /// The primary action's key: the lime accent.
    Accent,
}

/// A cap style's dimensions: its height (and least width), its side
/// padding and its label's size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CapMetrics {
    pub(crate) height: Pixels,
    pub(crate) padding_x: Pixels,
    pub(crate) text_size: Pixels,
}

impl CapStyle {
    /// This style's dimensions, from the theme's tokens.
    pub(crate) fn metrics(self, theme: &Theme) -> CapMetrics {
        let geometry = &theme.geometry;
        let typography = &theme.typography;
        match self {
            CapStyle::Compact => CapMetrics {
                height: geometry.keycap_compact_height,
                padding_x: geometry.keycap_compact_padding_x,
                text_size: typography.keycap_compact_size,
            },
            CapStyle::Regular | CapStyle::Accent => CapMetrics {
                height: geometry.keycap_height,
                padding_x: geometry.keycap_padding_x,
                text_size: typography.keycap_size,
            },
        }
    }
}

/// `keys` as a group of caps in `style`, named for the whole sequence.
pub(crate) fn key_sequence(keys: &KeySequence, style: CapStyle, theme: &Theme) -> Stateful<Div> {
    div()
        .id("keycap")
        .debug_selector(|| "keycap".into())
        .flex_none()
        .flex()
        .items_center()
        .gap(theme.geometry.key_gap)
        .role(Role::Image)
        .aria_label(keys.name())
        .children(keys.keys.iter().map(|key| cap(key, style, theme)))
}

/// One cap of `style` showing `key`'s label.
fn cap(key: &Key, style: CapStyle, theme: &Theme) -> Div {
    let typography = &theme.typography;
    let metrics = style.metrics(theme);
    let cap = div()
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .h(metrics.height)
        // A cap is at least as wide as it is high: a one-character label
        // still makes a square key.
        .min_w(metrics.height)
        .px(metrics.padding_x)
        .rounded(theme.geometry.keycap_radius)
        .font_family(typography.mono_family.clone())
        .text_size(metrics.text_size)
        .font_weight(typography.medium)
        .line_height(metrics.text_size)
        .child(key.cap.clone());
    match style {
        CapStyle::Accent => cap.bg(theme.accent).text_color(theme.accent_ink),
        CapStyle::Regular | CapStyle::Compact => cap
            .bg(theme.keycap_background)
            .text_color(theme.keycap_text)
            // inset 0 0 0 1px edge, inset 0 -1px 0 bottom. CSS paints its
            // first shadow on top and GPUI its last, so the list is the
            // reference's reversed: the ring lies over the bottom line, as
            // it does in the reference.
            .shadow(vec![
                BoxShadow::new(px(0.), px(-1.), theme.keycap_bottom).inset(),
                BoxShadow::new(px(0.), px(0.), theme.keycap_edge)
                    .spread_radius(px(1.))
                    .inset(),
            ]),
    }
}
