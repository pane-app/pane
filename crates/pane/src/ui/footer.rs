//! The launcher footer's parts, as the reference's root and Actions
//! boards draw them: the Pane mark and the hint on the left, then the
//! right-hand buttons (`.fbtn`) — the selected result's primary action
//! and, on root search, Actions — with a 1×16 rule between them.
//!
//! These are presentation only, composed by the launcher
//! ([`crate::app::LauncherWindow`]); the caller attaches clicks, focus and
//! state.
//!
//! The footer's left side is a Windows/Pane adaptation of the reference's
//! decorative mark: Pane's app menu (its Settings entry) opens from the
//! mark, since the contextual Actions panel is not the app menu (#100)
//! and Settings must stay reachable by mouse. At rest the mark is drawn
//! exactly as the reference's, with no button chrome.

use gpui::{AnyElement, Div, IntoElement, SharedString, Stateful, div, prelude::*, px};

use crate::ui::keycap::{CapStyle, KeySequence, key_sequence};
use crate::ui::theme::{Theme, pressed};

/// One part of the footer's hint line.
pub(crate) enum HintPart {
    /// Muted text: "Type to filter actions ·".
    Text(SharedString),
    /// A key sequence in the regular caps: the binding the hint teaches.
    Keys(KeySequence),
}

/// The footer's hint line: "Type to filter actions · Esc goes back" while
/// Actions is open (at rest the footer's buttons already show the keys,
/// so it shows none) — tertiary text and regular caps, 6px apart, on
/// one line.
pub(crate) fn hint_line(parts: Vec<HintPart>, theme: &Theme) -> Div {
    div()
        .debug_selector(|| "footer-hint".into())
        .flex()
        .items_center()
        .gap(theme.geometry.footer_hint_gap)
        .min_w(px(0.))
        .overflow_hidden()
        .whitespace_nowrap()
        .text_size(theme.typography.footer_size)
        .text_color(theme.text_tertiary)
        .children(parts.into_iter().enumerate().map(|(index, part)| {
            match part {
                HintPart::Text(text) => div().flex_none().child(text).into_any_element(),
                // Each sequence in its own scope: a key sequence's id is
                // fixed, so two in one hint would clash.
                HintPart::Keys(keys) => div()
                    .id(("hint-keys", index))
                    .flex_none()
                    .child(key_sequence(&keys, CapStyle::Regular, theme))
                    .into_any_element(),
            }
        }))
}

/// How a footer button answers the pointer and its panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ButtonWash {
    /// Transparent at rest, the 6% wash on hover (`.fbtn:hover`).
    Hover,
    /// Transparent, with no hover wash: the reference's Actions button,
    /// whose inline background (transparent while its panel is closed)
    /// overrides `.fbtn:hover`.
    None,
    /// The open wash and a white label: the Actions button while its
    /// panel is open.
    Pressed,
}

/// A footer button's chrome (`.fbtn`): 34 high, radius 8, 8px either
/// side, its label in 12.5px/500 and its keys 6px after it, washed as
/// `wash` says.
pub(crate) fn footer_button(
    id: &'static str,
    label: impl Into<SharedString>,
    keys: &KeySequence,
    caps: CapStyle,
    wash: ButtonWash,
    theme: &Theme,
) -> Stateful<Div> {
    let geometry = &theme.geometry;
    div()
        .id(id)
        .debug_selector(move || id.into())
        // The button shrinks under pressure: its label ellipsizes, its keys
        // never do.
        .flex_initial()
        .min_w(px(0.))
        .h(geometry.action_height)
        .flex()
        .items_center()
        .gap(geometry.action_gap)
        .px(geometry.action_padding_x)
        .rounded(geometry.action_radius)
        .text_size(theme.typography.footer_size)
        .font_weight(theme.typography.medium)
        .map(|button| match wash {
            ButtonWash::Pressed => button
                .bg(theme.footer_button_open)
                .text_color(theme.footer_button_open_text),
            ButtonWash::Hover => button
                .text_color(theme.footer_button_text)
                .hover(|button| button.bg(theme.control_hover))
                .active(|button| button.bg(pressed(theme.control_hover))),
            ButtonWash::None => button.text_color(theme.footer_button_text),
        })
        .child(
            div()
                .flex_initial()
                .min_w(px(0.))
                .truncate()
                .child(label.into()),
        )
        .child(
            div()
                .flex_none()
                .debug_selector(move || format!("{id}-keys"))
                .child(key_sequence(keys, caps, theme)),
        )
}

/// The hint's parts while the Actions panel is open: "Type to filter
/// actions · `escape` goes back".
pub(crate) fn actions_hint(escape: KeySequence) -> Vec<HintPart> {
    vec![
        HintPart::Text("Type to filter actions ·".into()),
        HintPart::Keys(escape),
        HintPart::Text("goes back".into()),
    ]
}

/// The Actions button: "Actions" and the Open actions binding's keys, in
/// the open wash while its panel is `open` and with no hover wash
/// otherwise, as the reference's (see [`ButtonWash::None`]). The caller
/// attaches the click.
pub(crate) fn actions_button(keys: &KeySequence, open: bool, theme: &Theme) -> Stateful<Div> {
    let wash = if open {
        ButtonWash::Pressed
    } else {
        ButtonWash::None
    };
    footer_button(
        "actions-button",
        "Actions",
        keys,
        CapStyle::Regular,
        wash,
        theme,
    )
    // In a window too narrow for both buttons, this one gives way —
    // clipped, shrinking a hundredfold faster — before the primary
    // action's label, let alone its keys.
    .overflow_hidden()
    .map(|mut button| {
        button.style().flex_shrink = Some(100.);
        button
    })
    .role(gpui::Role::Button)
    .aria_label("Actions")
    .aria_expanded(open)
    .aria_keyshortcuts(keys.name())
    .cursor_pointer()
}

/// The footer mark's button chrome: the Pane mark, 18px, centered in a
/// 28px box for the hover wash. [`footer_row`] gives it the mark's own
/// 18px slot, which the box bleeds 5px beyond on either side, so at rest
/// the mark sits exactly where the reference's does. The caller attaches
/// the button's identity, focus and click (the app menu's, see
/// `crate::features::footer_menu`).
pub(crate) fn mark_button(theme: &Theme) -> Div {
    let geometry = &theme.geometry;
    div()
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .size(geometry.footer_mark_size + geometry.footer_mark_bleed * 2.)
        .rounded(geometry.action_radius)
        .child(crate::ui::icon::pane_mark(
            geometry.footer_mark_size,
            theme.text_muted,
            theme.footer_mark,
        ))
}

/// The footer's right-hand buttons: `primary` (the selected result's
/// action), the rule, and `actions` (root search's Actions button), each
/// when shown; the rule only between two.
pub(crate) fn buttons(
    primary: Option<AnyElement>,
    actions: Option<AnyElement>,
    theme: &Theme,
) -> Vec<AnyElement> {
    let rule = (primary.is_some() && actions.is_some()).then(|| divider(theme).into_any_element());
    primary.into_iter().chain(rule).chain(actions).collect()
}

/// The strip's first line, inside its 1px top rule: where the lead, the
/// hint and the buttons center.
fn line(theme: &Theme) -> gpui::Pixels {
    theme.geometry.footer_height - px(1.)
}

/// The 1×16 rule between the footer's buttons, at the separator level.
pub(crate) fn divider(theme: &Theme) -> Div {
    div()
        .flex_none()
        .w(px(1.))
        .h(theme.geometry.footer_divider_height)
        .bg(theme.separator)
}

/// The footer's content row over the strip's 50px floor: `lead` (the
/// mark) and `middle` (the hint, or a status message, which may wrap and
/// grow the strip) on the left, `buttons` on the right, the lead and the
/// buttons centred in the first 50px however tall the message grows it.
pub(crate) fn footer_row(
    lead: AnyElement,
    middle: AnyElement,
    buttons: Vec<AnyElement>,
    theme: &Theme,
) -> Div {
    let geometry = &theme.geometry;
    let line = line(theme);
    // Stretched, not top-aligned: a status message's scroll viewport takes
    // the strip's capped height (and scrolls past it), while the lead and
    // the buttons keep their one line.
    div()
        // An imperceptible fill (black at 1/255, a fraction of a level) that
        // keeps the footer's content above a popup's drop shadow, as the
        // reference draws it. GPUI orders each primitive by the primitives
        // its box overlaps, and a shadow's box leaves out its blur: content
        // that only the blur reaches would sort below the shadow and be
        // darkened by it. This box overlaps the shadow's, so everything
        // painted inside it sorts above. (A fully transparent fill paints
        // nothing, and so orders nothing.)
        .bg(theme.footer_order_fill)
        .flex()
        .w_full()
        .min_w(px(0.))
        .flex_1()
        .min_h(px(0.))
        .gap(geometry.footer_lead_gap)
        // The lead's slot is the mark's 18px: its button's box bleeds
        // past it, centered.
        .child(
            div()
                .flex_none()
                .w(geometry.footer_mark_size)
                .h(line)
                .flex()
                .items_center()
                .justify_center()
                .child(lead),
        )
        .child(middle)
        .child(
            // The buttons shrink under pressure — their labels ellipsize,
            // their keys do not — so a narrow window keeps them inside
            // the strip.
            div()
                .flex_initial()
                .min_w(px(0.))
                .h(line)
                .flex()
                .items_center()
                .gap(geometry.footer_buttons_gap)
                .children(buttons),
        )
}

/// A status message in [`footer_row`]'s middle, in place of the hint: its
/// own scroll viewport, filling the room the buttons leave. The message
/// wraps there — a long error is several readable lines, never one
/// clipped — and the strip grows with it; past the strip's cap the message
/// scrolls here, inside the strip, so the strip never scrolls and the
/// buttons and any popup above them stay put. One line of it centers in
/// the strip's first line. The viewport is `status-scroll`, the message
/// `status-message` (tests see its wrapping and scroll by it).
pub(crate) fn status_message(text: impl Into<SharedString>, theme: &Theme) -> Stateful<Div> {
    div()
        .id("status-scroll")
        .flex_1()
        .min_w(px(0.))
        .overflow_y_scroll()
        .child(
            div()
                .w_full()
                .min_w(px(0.))
                .flex_none()
                .py(theme.geometry.footer_status_padding_y)
                .debug_selector(|| "status-message".into())
                .child(text.into()),
        )
}

/// The open command in the footer's left, as Raycast's footer names it
/// ("Search Files", "Clipboard History"): `icon` (the command's own, drawn
/// as the shared icon drawing draws it, at the Actions panel header's
/// tile size) and `title` in the row title's ink, truncating. An
/// extension's view has no heading line above it (#162); this is where
/// the user still sees where they are. It is also the screen's drag
/// region, as the heading line was: with the native title bar hidden, it
/// is a place outside an editable field to grab the window by.
pub(crate) fn command_lead(
    icon: &crate::ui::extension_icon::RowIcon,
    title: impl Into<SharedString>,
    theme: &Theme,
) -> Div {
    div()
        .debug_selector(|| "footer-command".into())
        .flex()
        .items_center()
        .gap(theme.geometry.footer_hint_gap)
        .min_w(px(0.))
        .overflow_hidden()
        .whitespace_nowrap()
        .text_size(theme.typography.footer_size)
        .font_weight(theme.typography.medium)
        .text_color(theme.text_title)
        .window_control_area(gpui::WindowControlArea::Drag)
        .child(crate::ui::extension_icon::row_icon_at(
            icon,
            crate::ui::icon::TileSize::Mini,
            "footer-command-icon",
            "footer-command",
            theme,
        ))
        .child(
            div()
                .debug_selector(|| "footer-command-title".into())
                .min_w(px(0.))
                .truncate()
                .child(title.into()),
        )
}

/// The hint's slot in [`footer_row`]: one line, taking the room the
/// buttons leave, centred in the strip's first 50px.
pub(crate) fn hint_slot(hint: Option<Div>, theme: &Theme) -> Div {
    div()
        .flex_1()
        .min_w(px(0.))
        .h(line(theme))
        .flex()
        .items_center()
        .when_some(hint, |slot, hint| slot.child(hint))
}
