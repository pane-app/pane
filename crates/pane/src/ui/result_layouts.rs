//! Root search's result layouts beyond the row (#96), as the reference's
//! empty and calculator boards draw them: the no-results notice heading
//! the fallbacks, and the computed answer's card, with its swatch where
//! the answer is a colour (#196). The boards' other variants — a
//! calculation-history row, an extension suggestion, a section label
//! whose note ends with keys — are not drawn: no command supplies a
//! calculation history, unit conversions or extension suggestions (#100).
//!
//! Presentation only, in the result row's shape: each function returns a
//! plain [`Div`] that the caller gives its identity, accessibility and
//! behavior. The launcher draws the notice and the card from what root
//! search's presentation holds (`crate::features::root_search::layouts`).

use gpui::prelude::*;
use gpui::{BoxShadow, Div, Hsla, Pixels, Role, SharedString, Stateful, div, px};

use crate::ui::icon::{self, Glyph};
use crate::ui::shell::LAUNCHER_CLIENT;
use crate::ui::theme::{Theme, TypeLine};

/// What the no-results notice says: its title, which names the query,
/// and what the user can do about it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct NoticeCopy {
    pub(crate) title: SharedString,
    pub(crate) description: SharedString,
}

/// The reference empty board's notice: 84 high, padded 8 above, 4 below
/// and 12 either side; a 44px disc (white 6%, a 1px white 8% ring) holding
/// the 20px magnifier-with-a-minus, then 16 on, the title in 15/500 over
/// the description in 13, 4 apart. Either line ellipsizes rather than
/// wrap: the notice keeps its height, and the query's full text stays in
/// the search field.
pub(crate) fn no_results_notice(copy: &NoticeCopy, theme: &Theme) -> Div {
    let geometry = &theme.geometry.results;
    let types = &theme.typography.results;
    let colors = &theme.results;
    div()
        .flex_none()
        .flex()
        .items_center()
        .gap(geometry.notice_gap)
        .h(geometry.notice_height)
        .pt(geometry.notice_padding_top)
        .pb(geometry.notice_padding_bottom)
        .px(geometry.notice_padding_x)
        .child(
            disc(geometry.notice_disc, colors.notice_disc)
                .shadow(vec![ring(colors.notice_disc_edge)])
                .child(icon::glyph(
                    Glyph::SearchNone,
                    geometry.notice_glyph,
                    colors.notice_glyph,
                )),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .flex_col()
                .gap(geometry.notice_text_gap)
                .child(
                    text(types.notice_title)
                        .truncate()
                        .font_weight(theme.typography.medium)
                        .text_color(theme.text_title)
                        .child(copy.title.clone()),
                )
                .child(
                    text(types.notice_description)
                        .truncate()
                        .text_color(theme.text_muted)
                        .child(copy.description.clone()),
                ),
        )
}

/// One side of the answer card: a value, and the caption under it where
/// a board authors one ("Inches"); a computed answer has none.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AnswerSide {
    pub(crate) value: SharedString,
    pub(crate) caption: Option<SharedString>,
}

/// What the answer card shows: what was typed and its answer, the
/// calculator board's "Also" chips where it authors them, the swatch of
/// a colour answer, and whether the card is the selected result.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AnswerCard {
    pub(crate) source: AnswerSide,
    pub(crate) answer: AnswerSide,
    pub(crate) also: Vec<SharedString>,
    /// The answer's swatch, when it is a colour, drawn under the answer's
    /// value and named by it for assistive technology.
    pub(crate) swatch: Option<Hsla>,
    pub(crate) selected: bool,
}

/// The reference calculator board's card: the list's full width, 2 above
/// it and 4 below, padded 20 above and either side and 16 below, radius
/// 14, white 6%, and while selected a 1px inset ring in the accent. Its
/// values sit in two equal columns either side of a 40px disc holding the
/// arrow, 16 apart, each centered over its caption 4 below it, in Geist
/// Mono 500 with −.03em of tracking — what was typed in #D9DADD, the
/// answer in white (see [`answer_value_type`] for their size). A colour
/// answer's swatch is under the answer's value, the caption's place, 28
/// high and rounded as a chip, named for assistive technology by the
/// value. Where the board authors chips, a line 14 below the values holds
/// "Also" and the chips, centered, under a white 7% rule with 14 above
/// them.
pub(crate) fn answer_card(card: &AnswerCard, theme: &Theme) -> Div {
    let geometry = &theme.geometry.results;
    let types = &theme.typography.results;
    let colors = &theme.results;
    let value = answer_value_type(card, theme);
    let side = |part: &AnswerSide, color: Hsla, debug: &'static str| {
        div()
            .flex_1()
            .min_w(px(0.))
            .flex()
            .flex_col()
            .items_center()
            .gap(geometry.card_value_gap)
            .child(
                text(value)
                    .debug_selector(move || debug.into())
                    .w_full()
                    .text_center()
                    .font_family(theme.typography.mono_family.clone())
                    .font_weight(theme.typography.medium)
                    .letter_spacing(value.size * types.answer_tracking)
                    .text_color(color)
                    .child(part.value.clone()),
            )
            .when_some(part.caption.clone(), |column, caption| {
                column.child(
                    text(types.answer_caption)
                        .text_color(theme.text_muted)
                        .child(caption),
                )
            })
    };
    let values = div()
        .w_full()
        .flex()
        .items_center()
        .gap(geometry.card_column_gap)
        .child(side(&card.source, colors.card_source, "answer-source"))
        .child(arrow(theme))
        .child(
            // The answer's swatch, when it is a colour: under the answer's
            // value, where a board authors a caption, named by it for
            // assistive technology.
            side(&card.answer, colors.card_answer, "answer-value")
                .when_some(card.swatch, |column, colour| {
                    column.child(swatch(colour, &card.answer.value, theme))
                }),
        );
    let also = (!card.also.is_empty()).then(|| {
        div()
            .w_full()
            .flex()
            .items_center()
            .justify_center()
            .gap(geometry.also_gap)
            .pt(geometry.also_padding_top)
            .border_t_1()
            .border_color(colors.card_rule)
            .child(
                text(types.answer_also)
                    .text_color(theme.text_muted)
                    .child("Also"),
            )
            .children(card.also.iter().map(|label| chip(label.clone(), theme)))
    });
    div()
        .flex_none()
        .w_full()
        .flex()
        .flex_col()
        .gap(geometry.card_gap)
        .mt(geometry.card_margin_top)
        .mb(geometry.card_margin_bottom)
        .pt(geometry.card_padding_top)
        .px(geometry.card_padding_x)
        .pb(geometry.card_padding_bottom)
        .rounded(geometry.card_radius)
        .bg(colors.card_fill)
        .cursor_pointer()
        .font_family(theme.typography.family.clone())
        .when(card.selected, |surface| {
            surface.shadow(vec![ring(theme.accent_text)])
        })
        .child(values)
        .children(also)
}

/// The type the card's values are set in: the authored Geist Mono 34
/// while the longer value fits its column at the launcher's own width,
/// else the first smaller step that does (24); a value longer still takes
/// the smallest (18) and wraps in its column. Every Geist Mono glyph
/// advances the same, so a value's width is its length in characters.
/// Both values share the type, as the reference sets them alike.
pub(crate) fn answer_value_type(card: &AnswerCard, theme: &Theme) -> TypeLine {
    let types = &theme.typography.results;
    let length = |side: &AnswerSide| side.value.chars().count();
    let longest = length(&card.source).max(length(&card.answer)) as f32;
    let column = answer_column_width(theme);
    let advance = types.mono_advance + types.answer_tracking;
    [types.answer_value, types.answer_value_compact]
        .into_iter()
        .find(|step| longest * f32::from(step.size) * advance <= column)
        .unwrap_or(types.answer_value_small)
}

/// A colour answer's swatch: the answer column's width, `card_swatch`
/// high, rounded as a chip with its ring, filled with `colour` — a colour
/// well, named by the answer's `label` (the colour's value).
fn swatch(colour: Hsla, label: &SharedString, theme: &Theme) -> Stateful<Div> {
    let geometry = &theme.geometry.results;
    let colors = &theme.results;
    div()
        .id("answer-swatch")
        .debug_selector(|| "answer-swatch".into())
        .role(Role::ColorWell)
        .aria_label(label.clone())
        .w_full()
        .h(geometry.card_swatch)
        .rounded(geometry.chip_radius)
        .bg(colour)
        .shadow(vec![ring(colors.chip_edge)])
}

/// The card's arrow: the 18px glyph in its 40px disc (white 7%).
fn arrow(theme: &Theme) -> Div {
    let geometry = &theme.geometry.results;
    let colors = &theme.results;
    let glyph = icon::glyph(Glyph::ArrowRight, geometry.card_arrow, colors.card_arrow);
    disc(geometry.card_arrow_disc, colors.card_arrow_disc).child(glyph)
}

/// One of the card's two value columns' width at the launcher's own
/// width: the list's less its side paddings, the card's paddings, the
/// arrow's disc and the two gaps beside it, halved (the reference's 314).
pub(crate) fn answer_column_width(theme: &Theme) -> f32 {
    let geometry = &theme.geometry;
    let results = &geometry.results;
    let inner = LAUNCHER_CLIENT.0
        - 2. * f32::from(geometry.list_padding_x)
        - 2. * f32::from(results.card_padding_x);
    (inner - f32::from(results.card_arrow_disc) - 2. * f32::from(results.card_column_gap)) / 2.
}

/// A chip on the card's "Also" line (`.chip.mono`): 30 high, 10 either
/// side, radius 8, white 6% under a white 7% ring (white 10% under the
/// pointer), its label in Geist Mono 12.5.
fn chip(label: SharedString, theme: &Theme) -> Div {
    let geometry = &theme.geometry.results;
    let colors = &theme.results;
    text(theme.typography.results.chip)
        .flex_none()
        .flex()
        .items_center()
        .h(geometry.chip_height)
        .px(geometry.chip_padding_x)
        .rounded(geometry.chip_radius)
        .bg(colors.chip_fill)
        .shadow(vec![ring(colors.chip_edge)])
        .hover(|chip| chip.bg(colors.chip_hover))
        .font_family(theme.typography.mono_family.clone())
        .text_color(colors.chip_text)
        .child(label)
}

/// A box for text in `line`'s size and line box.
fn text(line: TypeLine) -> Div {
    div().text_size(line.size).line_height(line.line_height)
}

/// A 1px inset ring in `color`.
fn ring(color: Hsla) -> BoxShadow {
    BoxShadow::new(px(0.), px(0.), color)
        .spread_radius(px(1.))
        .inset()
}

/// A disc `size` across in `fill`, centering what it holds.
fn disc(size: Pixels, fill: Hsla) -> Div {
    div()
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .size(size)
        .rounded(size * 0.5)
        .bg(fill)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(source: &str, answer: &str) -> AnswerCard {
        let side = |value: &str| AnswerSide {
            value: value.to_owned().into(),
            caption: None,
        };
        AnswerCard {
            source: side(source),
            answer: side(answer),
            also: Vec::new(),
            swatch: None,
            selected: true,
        }
    }

    /// A value column is the reference's 314px at the launcher's width.
    #[test]
    fn the_value_columns_are_the_references_width() {
        assert_eq!(answer_column_width(&Theme::dark()), 314.);
    }

    /// The values keep the authored 34px while the longer one fits its
    /// column (16 characters of Geist Mono at −.03em), then step down to
    /// 24 (22 characters) and to 18, where a longer value wraps.
    #[test]
    fn the_values_keep_the_authored_size_while_they_fit_and_step_down_after() {
        let theme = Theme::dark();
        let types = theme.typography.results;
        let size = |source: &str, answer: &str| answer_value_type(&card(source, answer), &theme);
        assert_eq!(size("72 in", "182.88 cm"), types.answer_value);
        assert_eq!(size("6*7", "42"), types.answer_value);
        assert_eq!(size("2 + 3 × 4 − 6 /", "8"), types.answer_value);
        // The longer value decides, whichever side it is on.
        assert_eq!(size("2 + 3 × 4 − 6 / 2", "11"), types.answer_value_compact);
        assert_eq!(
            size("1 / 3", "0.33333333333333333"),
            types.answer_value_compact
        );
        assert_eq!(
            size("999999 * 999999 + 123456 / 7", "1"),
            types.answer_value_small
        );
        assert_eq!(size(&"1 + ".repeat(60), "60"), types.answer_value_small);
    }
}
