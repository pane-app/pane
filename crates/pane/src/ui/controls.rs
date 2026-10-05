//! The Settings controls, as the reference's Settings board draws them
//! (#98): a field group — its label (`.flabel`), its control and its
//! description (`.fdesc`) — and the control families the board's
//! Appearance page shows: the segmented choice (`.segwrap`/`.seg`), the
//! accent swatches (`.sw`), the range input and the toggle.
//!
//! Presentation only, like [`crate::ui::settings_shell`]: each piece
//! returns a plain [`Div`], and identity, accessibility, focus, keys and
//! clicks stay with the caller. Nothing here imports launcher state.
//!
//! ## What production uses
//!
//! The Appearance page offers the settings Pane has (#100): the theme and
//! the material, each a field group around a segmented choice. The
//! swatches, the range input and the toggle are the board's advanced
//! Appearance controls — accent, blur, tint, pinned visibility, footer
//! tips — which #100 defers: only the visual workbench's reference fixture
//! draws them, so they are measured against the board without being
//! promised in the product. The toggle is the board's switch, which the
//! General page's own switches share (#99).
//!
//! The other Settings pages and the launcher's form are derived
//! compositions of these families and a few more (#99, below [`toggle`]):
//! the settings row, the button, the field's well, the list item, the
//! select's trigger and list rows, the group header and the host's frame
//! around an extension's view.
//!
//! ## Pointer and keyboard
//!
//! The board's controls answer the pointer at once: `.seg:hover` changes a
//! label's color and nothing fades. Every segment carries a hover style in
//! every state, the chosen one's being its own chosen look, rather than
//! attaching one only while unchosen: GPUI updates an element's remembered
//! hover state only while a hover style is attached, so a style that comes
//! and goes with the choice leaves the state stale (see `settings_shell`'s
//! module docs). The board draws no keyboard focus for these controls;
//! Pane's ring ([`segment_focus_shadows`]) is an adaptation, and so is a
//! disabled control's opacity outside the board's one case (Solid's
//! sliders, at 40%).

use gpui::prelude::*;
use gpui::{
    AnyElement, BoxShadow, Div, Hsla, Pixels, Role, SharedString, Stateful, div, px, relative,
};
use gpui_elements::editable_text::EditableTextElement;

use crate::ui::icon::{Glyph, glyph, glyph_rotated};
use crate::ui::theme::Theme;

/// A 1px ring inset along a box's edge (`box-shadow: inset 0 0 0 1px`):
/// it takes no layout space.
fn inset_ring(color: Hsla, width: Pixels) -> BoxShadow {
    BoxShadow::new(px(0.), px(0.), color)
        .spread_radius(width)
        .inset()
}

/// A page's column of field groups, 18px apart (the heading block above
/// them keeps its own 4px below it).
pub(crate) fn column(theme: &Theme) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(theme.geometry.controls.group_gap)
}

/// One field group: its label, its control and its description, 8px
/// apart. The caller adds them in that order.
pub(crate) fn field(theme: &Theme) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(theme.geometry.controls.field_gap)
}

/// A field's label (`.flabel`): 13.5/500 in the title ink, in the
/// reference's 18px line.
pub(crate) fn field_label(label: impl Into<SharedString>, theme: &Theme) -> Div {
    let line = theme.typography.settings.field_label;
    div()
        .text_size(line.size)
        .line_height(line.line_height)
        .font_weight(theme.typography.medium)
        .text_color(theme.text_title)
        .child(label.into())
}

/// A field's description (`.fdesc`): 12.5 at line height 1.45, in `color`
/// — the muted ink, or the warning's where it reports a fallback.
pub(crate) fn field_description(text: impl Into<SharedString>, color: Hsla, theme: &Theme) -> Div {
    let line = theme.typography.settings.field_description;
    div()
        .text_size(line.size)
        .line_height(line.line_height)
        .font_weight(theme.typography.regular)
        .text_color(color)
        .child(text.into())
}

/// A segmented choice's track (`.segwrap`): its segments side by side in
/// equal shares, 2px apart inside its 3px padding — 36 high around 30px
/// segments — on black 24% under a white 6% inset ring, radius 10.
pub(crate) fn segment_track(theme: &Theme) -> Div {
    let controls = &theme.geometry.controls;
    div()
        .w_full()
        .flex()
        .gap(controls.segment_gap)
        .p(controls.track_padding)
        .rounded(controls.track_radius)
        .bg(theme.controls.segment_track)
        .shadow(vec![inset_ring(theme.controls.segment_edge, px(1.))])
}

/// The chosen segment's inset shadows: its 1px top inset (white 8%).
fn chosen_shadows(theme: &Theme) -> Vec<BoxShadow> {
    let highlight = BoxShadow::new(px(0.), px(1.), theme.controls.segment_on_highlight).inset();
    vec![highlight]
}

/// A segment's shadows while the keyboard focuses it (`focus_visible`):
/// the chosen one's top inset, if `chosen`, under Pane's 2px focus ring
/// (the theme's focus color) — an adaptation: the board draws none.
pub(crate) fn segment_focus_shadows(chosen: bool, theme: &Theme) -> Vec<BoxShadow> {
    let mut shadows = if chosen {
        chosen_shadows(theme)
    } else {
        Vec::new()
    };
    let ring = inset_ring(theme.focus_ring, theme.geometry.controls.focus_width);
    shadows.push(ring);
    shadows
}

/// One segment (`.seg`): an equal share of its track, 30 high, radius 7,
/// its label centered in 12.5/500 — #9A9BA0 at rest and #EDEDEF under the
/// pointer, or, `chosen`, white on the white 12% wash under its white 8%
/// top inset, which the pointer leaves as it is. Nothing fades. A segment
/// that is not `enabled` keeps its look under the pointer (its hover style
/// is its resting one, still attached). The caller attaches the segment's
/// identity, focus, keys and click.
pub(crate) fn segment(
    label: impl Into<SharedString>,
    chosen: bool,
    enabled: bool,
    theme: &Theme,
) -> Div {
    let controls = &theme.geometry.controls;
    let colors = &theme.controls;
    let line = theme.typography.settings.segment;
    let (text, hover_text) = match (chosen, enabled) {
        (true, _) => (colors.segment_on_text, colors.segment_on_text),
        (false, true) => (colors.segment_text, colors.segment_hover_text),
        (false, false) => (colors.segment_text, colors.segment_text),
    };
    div()
        .flex_1()
        .min_w(px(0.))
        .h(controls.segment_height)
        .flex()
        .items_center()
        .justify_center()
        .rounded(controls.segment_radius)
        .cursor_pointer()
        .text_size(line.size)
        .line_height(line.line_height)
        .font_weight(theme.typography.medium)
        .text_color(text)
        .when(chosen, |segment| {
            segment.bg(colors.segment_on).shadow(chosen_shadows(theme))
        })
        .hover(move |segment| segment.text_color(hover_text))
        .child(div().min_w(px(0.)).truncate().child(label.into()))
}

/// The accent swatches' row (`role=group`): 10px apart.
pub(crate) fn swatch_row(theme: &Theme) -> Div {
    div()
        .flex()
        .items_center()
        .gap(theme.geometry.controls.swatch_gap)
}

/// One accent swatch (`.sw`): a 30px disc of `fill` under a black 20%
/// inset edge or, `chosen`, ringed by a 2px gap of the page's color and a
/// 2px ring of its own color outside that.
pub(crate) fn swatch(fill: Hsla, chosen: bool, theme: &Theme) -> Div {
    let controls = &theme.geometry.controls;
    let size = controls.swatch_size;
    let disc = div().flex_none().size(size).rounded(size / 2.).bg(fill);
    if chosen {
        // `0 0 0 2px gap, 0 0 0 4px fill`: CSS paints its first shadow on
        // top and GPUI its last, so the list is the reference's reversed.
        disc.shadow(vec![
            BoxShadow::new(px(0.), px(0.), fill)
                .spread_radius(controls.swatch_ring_gap + controls.swatch_ring),
            BoxShadow::new(px(0.), px(0.), theme.controls.swatch_gap)
                .spread_radius(controls.swatch_ring_gap),
        ])
    } else {
        disc.shadow(vec![inset_ring(theme.controls.swatch_edge, px(1.))])
    }
}

/// The custom-color swatch: a 30px disc under a white 22% ring, holding
/// a 14px plus in the body ink.
pub(crate) fn swatch_add(theme: &Theme) -> Div {
    let controls = &theme.geometry.controls;
    let size = controls.swatch_size;
    div()
        .flex_none()
        .size(size)
        .rounded(size / 2.)
        .flex()
        .items_center()
        .justify_center()
        .shadow(vec![inset_ring(theme.controls.swatch_add_edge, px(1.))])
        .child(glyph(Glyph::Plus, controls.swatch_glyph, theme.text_body))
}

/// A slider's header line: its label (a field label) at the left and its
/// value (Geist Mono 12.5 in the body ink) at the right, their lines'
/// bottoms aligned, as the board's baseline puts them.
pub(crate) fn slider_header(
    label: impl Into<SharedString>,
    value: impl Into<SharedString>,
    theme: &Theme,
) -> Div {
    let line = theme.typography.settings.value;
    div()
        .flex()
        .items_end()
        .justify_between()
        .child(field_label(label, theme))
        .child(
            div()
                .flex_none()
                .font_family(theme.typography.mono_family.clone())
                .text_size(line.size)
                .line_height(line.line_height)
                .text_color(theme.text_body)
                .child(value.into()),
        )
}

/// A slider: Pane's drawing of the board's range input, whose look the
/// reference leaves to the browser — 20 high, a 4px track (the accent up
/// to `fraction` of its width, white 16% beyond) and a 16px accent thumb
/// on the value.
pub(crate) fn slider(fraction: f32, theme: &Theme) -> Div {
    let controls = &theme.geometry.controls;
    let fraction = fraction.clamp(0., 1.);
    let height = controls.slider_height;
    let track = controls.slider_track;
    let thumb = controls.slider_thumb;
    div()
        .relative()
        .w_full()
        .h(height)
        .child(
            div()
                .absolute()
                .left(px(0.))
                .right(px(0.))
                .top((height - track) / 2.)
                .h(track)
                .rounded(track / 2.)
                .bg(theme.controls.slider_track)
                .child(
                    div()
                        .h_full()
                        .w(relative(fraction))
                        .rounded(track / 2.)
                        .bg(theme.accent),
                ),
        )
        .child(
            // The thumb's travel: the track's width less the thumb's.
            div()
                .absolute()
                .top(px(0.))
                .bottom(px(0.))
                .left(px(0.))
                .right(thumb)
                .child(
                    div()
                        .absolute()
                        .top((height - thumb) / 2.)
                        .left(relative(fraction))
                        .size(thumb)
                        .rounded(thumb / 2.)
                        .bg(theme.accent),
                ),
        )
}

/// The toggles' list: rows of [`toggle_row`].
pub(crate) fn toggle_list() -> Div {
    div().flex().flex_col()
}

/// A toggle's row: its label (a field label at 400) at the left and
/// `toggle` at the right, 44 high with a 1px rule (white 6%) along its
/// top.
pub(crate) fn toggle_row(
    label: impl Into<SharedString>,
    toggle: impl IntoElement,
    theme: &Theme,
) -> Div {
    let label = field_label(label, theme).font_weight(theme.typography.regular);
    div()
        .flex()
        .items_center()
        .justify_between()
        .h(theme.geometry.controls.toggle_row_height)
        .border_t_1()
        .border_color(theme.hairline_soft)
        .child(label)
        .child(toggle)
}

/// A toggle (the board's switch): 40×24, radius 12, the accent while `on`
/// and white 16% while off, its 18px white knob 3px in from its left
/// (off) or its right (on) under a short shadow. The board slides the
/// knob over .2s; that motion is the caller's to add, and reduced
/// motion's to drop.
pub(crate) fn toggle(on: bool, theme: &Theme) -> Div {
    let controls = &theme.geometry.controls;
    let knob_shadow =
        BoxShadow::new(px(0.), px(1.), theme.controls.toggle_knob_shadow).blur_radius(px(3.));
    let knob_left = if on {
        controls.toggle_width - controls.toggle_inset - controls.toggle_knob
    } else {
        controls.toggle_inset
    };
    div()
        .relative()
        .flex_none()
        .w(controls.toggle_width)
        .h(controls.toggle_height)
        .rounded(controls.toggle_height / 2.)
        .bg(if on {
            theme.accent
        } else {
            theme.controls.toggle_off
        })
        .child(
            div()
                .absolute()
                .top(controls.toggle_inset)
                .left(knob_left)
                .size(controls.toggle_knob)
                .rounded(controls.toggle_knob / 2.)
                .bg(theme.controls.toggle_knob)
                .shadow(vec![knob_shadow]),
        )
}

// ------------------------------------------- the other pages' families (#99)
//
// Only the Appearance page has an authored layout. The other Settings pages
// (General, Launcher, Shortcuts, Keyboard, Extensions, About) and the
// launcher's form are composed from the board's families, as derived
// compositions — reference-consistent, never claimed to match a board the
// reference does not have:
//
// - a page is a column of field groups ([`column`], [`field`]): a label
//   over its controls, then the notes that explain them;
// - a setting with a control is a settings row ([`setting_row`]), the
//   board's toggle row generalized to a description and any control: the
//   toggle, a button, a recorder's well;
// - an action is a button ([`button`], the empty board's `.pill`), or a
//   secondary one ([`ghost_button`], the store board's ghost pill);
// - text, a binding, a choice that opens a list, a filter is a field's well
//   ([`well`]), the sidebar search's family;
// - a pressable entry of a list (an extension, a group of commands) is a
//   list item ([`list_item`]), with the sidebar item's hover;
// - a note, a refusal or a status is a field description in its tone.
//
// Every pointer style here is attached in every state and nothing fades:
// see the module docs.

/// The focus ring Pane draws on a control the keyboard is on: 2px of the
/// focus color, inset (an adaptation: the board draws no focus).
pub(crate) fn focus_ring(theme: &Theme) -> Vec<BoxShadow> {
    vec![inset_ring(
        theme.focus_ring,
        theme.geometry.controls.focus_width,
    )]
}

/// A settings row: `label` (a field label at 400, as the board's toggle
/// rows set theirs) over `lines` (its description, a reason, an error) at
/// the left, at least 44 high under a 1px rule (white 6%) — and 10px above
/// and below the text when it has lines. The caller adds the row's control
/// at its right end, 16px from the text, and the row's identity.
pub(crate) fn setting_row(
    label: impl Into<SharedString>,
    lines: Vec<AnyElement>,
    theme: &Theme,
) -> Div {
    let controls = &theme.geometry.controls;
    let padded = !lines.is_empty();
    let label = field_label(label, theme).font_weight(theme.typography.regular);
    let text = div()
        .flex_1()
        .min_w(px(0.))
        .flex()
        .flex_col()
        .child(label)
        .children(lines);
    div()
        .w_full()
        .flex()
        .items_center()
        .gap(controls.row_gap)
        .min_h(controls.toggle_row_height)
        .when(padded, |row| row.py(controls.row_padding_y))
        .border_t_1()
        .border_color(theme.hairline_soft)
        .child(text)
}

/// A list of settings rows: one under the other, each under its own rule.
pub(crate) fn setting_list() -> Div {
    div().w_full().flex().flex_col()
}

/// A button (`.pill`, the empty board's Install, whose tokens the result
/// layouts' pill already holds): 30 high, 12px either side, radius 8, its
/// 12.5/500 `label` in the title ink on white 8% under a white 8% inset
/// ring; white 13% under the pointer, at once. A button that is not
/// `enabled` is drawn at the disabled opacity, its hover its resting fill
/// (still attached). The caller attaches its identity, accessibility,
/// focus and click.
pub(crate) fn button(label: impl Into<SharedString>, enabled: bool, theme: &Theme) -> Div {
    let colors = &theme.results;
    let hover = if enabled {
        colors.pill_hover
    } else {
        colors.pill_fill
    };
    button_frame(enabled, theme)
        .bg(colors.pill_fill)
        .shadow(vec![inset_ring(colors.pill_edge, px(1.))])
        .text_color(theme.text_title)
        .hover(move |button| button.bg(hover))
        .child(label.into())
}

/// A secondary button (the store board's ghost pill, `.pill.ghost`): a
/// button's box, transparent, its label in the body ink. The store authors
/// no hover for it; Pane's is a footer button's white 6% (`.fbtn:hover`).
pub(crate) fn ghost_button(label: impl Into<SharedString>, enabled: bool, theme: &Theme) -> Div {
    let hover = if enabled {
        theme.control_hover
    } else {
        gpui::transparent_black()
    };
    button_frame(enabled, theme)
        .text_color(theme.text_body)
        .hover(move |button| button.bg(hover))
        .child(label.into())
}

/// A button's box and type, without its fill.
fn button_frame(enabled: bool, theme: &Theme) -> Div {
    let pill = &theme.geometry.results;
    let line = theme.typography.results.pill;
    div()
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
        .when(enabled, |button| button.cursor_pointer())
        .when(!enabled, |button| {
            button
                .opacity(theme.geometry.controls.disabled_opacity)
                .cursor_default()
        })
}

/// A field's well: the sidebar search's black 24% under a white 6% inset
/// ring, radius 8, 10px either side, its parts 8px apart and its text
/// 13px — 34 high on its own line, or 30 (`inline`) inside a settings row.
/// The caller fills it (an editable field, a binding's caps, a choice)
/// and, where it takes the keyboard, rings it with [`well_shadows`].
pub(crate) fn well(inline: bool, theme: &Theme) -> Div {
    let controls = &theme.geometry.controls;
    div()
        .flex()
        .items_center()
        .gap(controls.well_gap)
        .h(if inline {
            controls.inline_well_height
        } else {
            controls.well_height
        })
        .px(controls.well_padding_x)
        .rounded(controls.well_radius)
        .bg(theme.field_fill)
        .shadow(well_shadows(false, theme))
        .text_size(theme.typography.settings_text_size)
        .line_height(theme.typography.settings_text_size * theme.typography.line_height)
}

/// A well's ring: its white 6% at rest, the focus color while `focused`
/// (an adaptation, as the sidebar search's own).
pub(crate) fn well_shadows(focused: bool, theme: &Theme) -> Vec<BoxShadow> {
    let color = if focused {
        theme.focus_ring
    } else {
        theme.field_edge
    };
    vec![inset_ring(color, px(1.))]
}

/// The 14px glyph at a well's start (a filter's magnifier).
pub(crate) fn well_glyph(mark: Glyph, theme: &Theme) -> gpui::Svg {
    glyph(mark, theme.geometry.controls.well_glyph, theme.nav_icon).flex_none()
}

/// `input` styled as a well's text: 13px in the title ink, the muted
/// placeholder, the accent caret, filling the well and scrolling sideways.
pub(crate) fn well_input(
    input: EditableTextElement,
    placeholder: impl Into<SharedString>,
    theme: &Theme,
) -> EditableTextElement {
    let typography = &theme.typography;
    input
        .placeholder(placeholder)
        .placeholder_color(theme.text_placeholder)
        .caret_color(theme.accent_text)
        .selection_color(theme.row_selected)
        .marked_color(theme.accent_text)
        .text_size(typography.settings_text_size)
        .text_color(theme.text_title)
        .font_family(typography.family.clone())
        .font_features(typography.features.clone())
        .w_full()
        .min_w(px(0.))
        .whitespace_nowrap()
        .overflow_x_scroll()
}

/// A recorder's well: an inline well at least 96 wide holding `content`
/// (a binding's caps, or [`listening_mark`]) at its end, ringed in the
/// focus color while it has the keyboard — drawn so when `focused` says
/// it is (a recorder listening holds the keyboard), and by the element's
/// own focus style once the caller tracks its focus.
pub(crate) fn recorder_well(content: impl IntoElement, focused: bool, theme: &Theme) -> Div {
    let ring = well_shadows(true, theme);
    well(true, theme)
        .flex_none()
        .min_w(theme.geometry.controls.recorder_min_width)
        .justify_end()
        .cursor_pointer()
        .shadow(well_shadows(focused, theme))
        .focus(move |well| well.shadow(ring))
        .child(content)
}

/// What a recorder's row says while it listens.
pub(crate) const RECORDING_HINT: &str = "The keys are captured here: they do not act";

/// What a recorder's well shows while it listens: "Press the keys…" in
/// the accent's text tone, 12.5/500.
pub(crate) fn listening_mark(theme: &Theme) -> Div {
    let line = theme.typography.settings.segment;
    div()
        .flex_none()
        .text_size(line.size)
        .line_height(line.line_height)
        .font_weight(theme.typography.medium)
        .text_color(theme.accent_text)
        .child("Press the keys…")
}

/// A pressable entry of a Settings list (an extension, a command to open,
/// an install source): radius 8 and 10px either side like a sidebar item,
/// at least 44 high (10px above and below text with `lines`), the
/// optional 28px `tile`, `label` as a field label over its `lines`, and a
/// 14px chevron at its right end; white 5% under the pointer (`.nav:hover`),
/// at once. The caller attaches its identity, accessibility and click.
pub(crate) fn list_item(
    tile: Option<Div>,
    label: impl Into<SharedString>,
    lines: Vec<AnyElement>,
    theme: &Theme,
) -> Div {
    let controls = &theme.geometry.controls;
    let settings = &theme.geometry.settings;
    let padded = !lines.is_empty();
    let hover = theme.nav_hover;
    let text = div()
        .flex_1()
        .min_w(px(0.))
        .flex()
        .flex_col()
        .child(field_label(label, theme))
        .children(lines);
    div()
        .w_full()
        .flex()
        .items_center()
        .gap(controls.row_gap)
        .min_h(controls.toggle_row_height)
        .px(settings.item_padding_x)
        .when(padded, |row| row.py(controls.row_padding_y))
        .rounded(controls.list_radius)
        .cursor_pointer()
        .hover(move |row| row.bg(hover))
        .children(tile)
        .child(text)
        .child(glyph(Glyph::ChevronRight, controls.chevron, theme.nav_icon).flex_none())
}

/// A select's trigger (#99): a 34px well showing the committed choice's
/// `label` in the title ink, with the chevron that says a list opens at
/// its end. The well is the trigger; its field group's label and
/// description are the caller's.
pub(crate) fn select_trigger(label: impl Into<SharedString>, theme: &Theme) -> Div {
    let controls = &theme.geometry.controls;
    well(false, theme)
        .w_full()
        .justify_between()
        .cursor_pointer()
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .truncate()
                .text_color(theme.text_title)
                .child(label.into()),
        )
        .child(glyph_rotated(
            Glyph::ChevronRight,
            controls.chevron,
            theme.nav_icon,
            gpui::radians(std::f32::consts::FRAC_PI_2),
        ))
}

/// One choice in a select's list: the Actions panel's entry family
/// (`.arow`: 36 high at least, radius 8, 8px either side, 13px/450 in its
/// ink), its `description` under its label in the muted 12.5, the 6px
/// accent mark at its end while `committed`; the white 11% wash while
/// `highlighted`, white 6% under the pointer — at once, and the hover
/// attached in every state (a highlighted or unavailable one's being its
/// resting look). A choice that is not `offered` is drawn at the disabled
/// opacity.
pub(crate) fn menu_row(
    label: impl Into<SharedString>,
    description: Option<SharedString>,
    (highlighted, committed, offered): (bool, bool, bool),
    theme: &Theme,
) -> Div {
    let actions = &theme.geometry.actions;
    let controls = &theme.geometry.controls;
    let typography = &theme.typography;
    let rest = if highlighted {
        theme.action_selected
    } else {
        gpui::transparent_black()
    };
    let hover = if highlighted || !offered {
        rest
    } else {
        theme.control_hover
    };
    let line = typography.settings.field_description;
    div()
        .flex()
        .items_center()
        .gap(actions.row_gap)
        .min_h(actions.row_height)
        .px(actions.row_padding_x)
        .py(controls.menu_padding_y)
        .rounded(actions.row_radius)
        .bg(rest)
        .hover(move |row| row.bg(hover))
        .text_size(typography.action_size)
        .font_weight(typography.action_weight)
        .text_color(theme.action_text)
        .when(offered, |row| row.cursor_pointer())
        .when(!offered, |row| {
            row.opacity(controls.disabled_opacity).cursor_default()
        })
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .flex_col()
                .child(div().truncate().child(label.into()))
                .when_some(description, |text, description| {
                    text.child(
                        div()
                            .text_size(line.size)
                            .line_height(line.line_height)
                            .font_weight(typography.regular)
                            .text_color(theme.text_muted)
                            .child(description),
                    )
                }),
        )
        .when(committed, |row| {
            row.child(
                div()
                    .flex_none()
                    .size(controls.mark_size)
                    .rounded(controls.mark_size / 2.)
                    .bg(theme.accent),
            )
        })
}

/// A list's group header (the Shortcuts page's extensions): a pressable
/// row like [`list_item`] — at least 44 high, white 5% under the pointer —
/// leading with `chevron` (the caller's disclosure glyph, turned as the
/// group is open) and its `label` over its `lines`.
pub(crate) fn group_header(
    chevron: impl IntoElement,
    label: impl Into<SharedString>,
    lines: Vec<AnyElement>,
    theme: &Theme,
) -> Div {
    let controls = &theme.geometry.controls;
    let settings = &theme.geometry.settings;
    let padded = !lines.is_empty();
    let hover = theme.nav_hover;
    div()
        .w_full()
        .flex()
        .items_center()
        .gap(settings.item_gap)
        .min_h(controls.toggle_row_height)
        .px(settings.item_padding_x)
        .when(padded, |row| row.py(controls.row_padding_y))
        .rounded(controls.list_radius)
        .cursor_pointer()
        .hover(move |row| row.bg(hover))
        .child(chevron)
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .flex_col()
                .child(field_label(label, theme))
                .children(lines),
        )
}

/// One explanatory line of a page — a note, a refusal, a status — as a
/// field's description in `color`, named `selector`, read by assistive
/// technology as a status.
pub(crate) fn status_note(
    selector: impl Into<SharedString>,
    text: impl Into<SharedString>,
    color: Hsla,
    theme: &Theme,
) -> Stateful<Div> {
    let selector = selector.into();
    let text = text.into();
    let debug = selector.to_string();
    field_description(text.clone(), color, theme)
        .id(selector)
        .debug_selector(move || debug)
        .role(Role::Status)
        .aria_label(text)
}

/// A column caption over a list (a table's column names): the aside
/// caption's 12/500 muted type.
pub(crate) fn caption(text: impl Into<SharedString>, theme: &Theme) -> Div {
    let line = theme.typography.settings.caption;
    div()
        .text_size(line.size)
        .line_height(line.line_height)
        .font_weight(theme.typography.medium)
        .text_color(theme.text_muted)
        .child(text.into())
}

/// The host's frame around an extension's own view (#99): 4px of padding
/// (a press there focuses the view without reaching it), radius 10, the
/// field's white 6% inset ring — the focus color while the view has the
/// keyboard. Layout-free: the ring is a shadow, not a border. The view's
/// own drawing keeps the colors its extension gave it.
pub(crate) fn frame(theme: &Theme) -> Div {
    let controls = &theme.geometry.controls;
    div()
        .p(controls.frame_padding)
        .rounded(controls.frame_radius)
        .shadow(well_shadows(false, theme))
}
