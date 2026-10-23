//! Root search's no-results notice and computed answers (#96): what the
//! launcher's presentation of root search holds, translated into the
//! shared result layouts (`crate::ui::result_layouts`), for the
//! launcher.
//!
//! Only what the launcher holds is drawn. The notice names the real query
//! and is shown when nothing but fallbacks is listed for it; the first
//! fallback under it is selected, so Enter sends it the query (ADR 0031).
//! A computed answer's card shows the query it answers, typeset as an
//! expression (`2*3` as `2 × 3`), over "Expression", and the text Enter
//! copies over "Result" (#472), with a swatch in the caption's place when
//! the command answered a colour (#196) — no history, which no command
//! supplies — and the row behind it keeps its id, its selection and its
//! copy action. The command that answered sends only its answer; the
//! query is Pane's, so Pane typesets it, changing only the signs that can
//! be nothing but operators.

use gpui::prelude::*;
use gpui::{Div, Hsla, Role, Stateful, rgb_to_hsla, rgba};
use pane_core::{ComputedAnswer, Screen};

use crate::ui::result_layouts::{self, AnswerCard, AnswerSide, NoticeCopy};
use crate::ui::theme::Theme;

/// The notice's description while fallbacks are listed under it: the
/// first is selected (ADR 0031), so Enter sends the text to it.
const WITH_FALLBACKS: &str =
    "Enter sends it to the first fallback below, or install an extension that knows about it.";

/// The notice's description while the user has no fallback for the text:
/// the extension list offers a command that takes text as a fallback.
const WITHOUT_FALLBACKS: &str =
    "Install an extension that knows about it, or offer a fallback in Settings.";

/// What the notice says for `query`: that nothing matches it — root
/// search looks at commands, applications and a granted folder's files,
/// not the whole computer, so it does not claim more — and what the user
/// can do, by whether `fallbacks` are listed under it. Pane installs
/// extensions; it has no store to suggest one from.
pub(crate) fn notice_copy(query: &str, fallbacks: bool) -> NoticeCopy {
    let description = if fallbacks {
        WITH_FALLBACKS
    } else {
        WITHOUT_FALLBACKS
    };
    NoticeCopy {
        title: format!("Nothing matches “{}”", query.trim()).into(),
        description: description.into(),
    }
}

/// The notice root search shows on `screen`, if any: on root search, for
/// a query that is not blank, while nothing but fallbacks is listed
/// (`only_fallbacks`, vacuously true while nothing is) — whichever of them
/// is selected. `fallbacks` says whether any is listed.
pub(crate) fn nothing_found(
    screen: &Screen,
    only_fallbacks: bool,
    fallbacks: bool,
) -> Option<NoticeCopy> {
    let Screen::Root { query } = screen else {
        return None;
    };
    if query.trim().is_empty() || !only_fallbacks {
        return None;
    }
    Some(notice_copy(query, fallbacks))
}

/// The notice, as the result list's first child: its identity
/// (`no-results`) and its accessible note — the title as its name, the
/// description as its description.
pub(crate) fn notice(copy: &NoticeCopy, theme: &Theme) -> Stateful<Div> {
    result_layouts::no_results_notice(copy, theme)
        .id("no-results")
        .debug_selector(|| "no-results".into())
        .role(Role::Note)
        .aria_label(copy.title.clone())
        .aria_description(copy.description.clone())
}

/// The card a computed answer is drawn as: the query it answers, written
/// as an expression (see [`expression`]), over "Expression"; its answer
/// over "Result", as Raycast labels its calculator's two sides — or, for a
/// colour, over its swatch in the caption's place; and whether it is the
/// selected result. It carries no chips: nothing the launcher holds fills
/// them.
pub(crate) fn answer_card(answer: &ComputedAnswer, selected: bool, theme: &Theme) -> Div {
    let swatch = answer.swatch.as_deref().and_then(swatch);
    result_layouts::answer_card(
        &AnswerCard {
            source: AnswerSide {
                value: expression(&answer.query).into(),
                caption: Some("Expression".into()),
            },
            answer: AnswerSide {
                value: answer.answer.clone().into(),
                caption: swatch.is_none().then(|| "Result".into()),
            },
            also: Vec::new(),
            swatch,
            selected,
        },
        theme,
    )
}

/// `query` written as an expression is typeset: `*`, `·` and a `x`
/// between two numbers as ×, `/` before a number as ÷, a minus as −, and
/// one space either side of each operator between two operands (`2*3` as
/// `2 × 3`, `-5+2` as `−5 + 2`). The command that answered owns the
/// query's language, so only those signs change, where they can only be
/// operators: `0xff`, `km/h`, `1e-3`, `2026-12-25` and words stay as they
/// were typed.
pub(crate) fn expression(query: &str) -> String {
    let chars: Vec<char> = query.trim().chars().collect();
    let operand_end =
        |c: char| c.is_alphanumeric() || matches!(c, ')' | '%' | '.' | '²' | '³' | '!');
    let operand_start =
        |c: char| c.is_alphanumeric() || matches!(c, '(' | '.' | '√' | '∛' | '-' | '−' | '–');
    let number_end = |c: char| c.is_ascii_digit() || matches!(c, ')' | '%' | '²' | '³' | '!');
    let number_start =
        |c: char| c.is_ascii_digit() || matches!(c, '(' | '.' | '√' | '∛' | '-' | '−' | '–');
    let mut out = String::new();
    for (at, &c) in chars.iter().enumerate() {
        let before = chars[..at]
            .iter()
            .rev()
            .find(|c| !c.is_whitespace())
            .copied();
        let after = chars[at + 1..].iter().find(|c| !c.is_whitespace()).copied();
        let tight_before = at > 0 && !chars[at - 1].is_whitespace();
        let tight_after = chars.get(at + 1).is_some_and(|c| !c.is_whitespace());
        let spaced = !tight_before && !tight_after;
        let between = before.is_some_and(operand_end) && after.is_some_and(operand_start);
        let binary = match c {
            '*' => {
                let double = chars.get(at + 1) == Some(&'*') || (at > 0 && chars[at - 1] == '*');
                (!double && between).then_some('×')
            }
            '×' | '·' | '÷' => between.then_some(if c == '÷' { '÷' } else { '×' }),
            'x' | 'X' => {
                // A word of its own between numbers, not the x of 0x1F.
                let alone = !at
                    .checked_sub(1)
                    .is_some_and(|previous| chars[previous].is_alphabetic())
                    && !chars.get(at + 1).is_some_and(|c| c.is_alphabetic());
                let hex = at > 0
                    && chars[at - 1] == '0'
                    && !(at > 1 && (chars[at - 2].is_alphanumeric() || chars[at - 2] == '.'));
                (alone && !hex && before.is_some_and(number_end) && after.is_some_and(number_start))
                    .then_some('×')
            }
            '/' => {
                let divides = before.is_some_and(operand_end) && after.is_some_and(number_start);
                (divides || (spaced && between)).then_some('÷')
            }
            '+' => between.then_some('+'),
            '-' | '−' | '–' => {
                // 2026-12-25 and new-york keep their hyphens; 5-3 does not.
                let binary = if spaced {
                    between
                } else {
                    !dated(&chars, at)
                        && before.is_some_and(number_end)
                        && after.is_some_and(number_start)
                };
                if binary {
                    Some('−')
                } else {
                    // A sign: at the start, or after an operator or an
                    // opening parenthesis, before what it negates.
                    let sign = before.is_none_or(|b| "+-−–*×·/÷^(x".contains(b))
                        && tight_after
                        && after.is_some_and(operand_start);
                    if sign {
                        out.push('−');
                        continue;
                    }
                    None
                }
            }
            _ => None,
        };
        match binary {
            Some(operator) => {
                out.truncate(out.trim_end().len());
                out.push(' ');
                out.push(operator);
                out.push(' ');
            }
            None if c.is_whitespace() && out.ends_with(' ') => {}
            None => out.push(c),
        }
    }
    out
}

/// The colour of a swatch the answer holds, "#RRGGBB" or "#RRGGBBAA",
/// if it parses as one (a command that answers none, or an unreadable
/// one, shows no swatch).
fn swatch(hex: &str) -> Option<Hsla> {
    let digits = hex.strip_prefix('#')?;
    let value = u32::from_str_radix(digits, 16).ok()?;
    Some(rgb_to_hsla(match digits.len() {
        6 => rgba(value << 8 | 0xff),
        8 => rgba(value),
        _ => return None,
    }))
}

/// Whether the hyphen at `at` is inside a date written with them: a word
/// of digits and two hyphens with a year of four digits at either end
/// (`2026-12-25`, `25-12-2026`).
fn dated(chars: &[char], at: usize) -> bool {
    let start = chars[..at]
        .iter()
        .rposition(|c| c.is_whitespace())
        .map_or(0, |space| space + 1);
    let end = chars[at..]
        .iter()
        .position(|c| c.is_whitespace())
        .map_or(chars.len(), |space| at + space);
    let word: String = chars[start..end].iter().collect();
    let parts: Vec<&str> = word.split('-').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
        && (parts[0].len() == 4 || parts[2].len() == 4)
}

/// The card's accessible name: "6 × 7 = 42", what was typed, written as
/// the card writes it, and its answer.
pub(crate) fn answer_label(answer: &ComputedAnswer) -> String {
    format!("{} = {}", expression(&answer.query), answer.answer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operators_between_operands_are_typeset() {
        assert_eq!(expression("2*3"), "2 × 3");
        assert_eq!(expression("2 * 3"), "2 × 3");
        assert_eq!(expression("6x7"), "6 × 7");
        assert_eq!(expression("6 x 7"), "6 × 7");
        assert_eq!(expression("10x7"), "10 × 7");
        assert_eq!(expression("2·3"), "2 × 3");
        assert_eq!(expression("84/2"), "84 ÷ 2");
        assert_eq!(expression("(1+2)/3"), "(1 + 2) ÷ 3");
        assert_eq!(expression("pi/2"), "pi ÷ 2");
        assert_eq!(expression("10-4-3"), "10 − 4 − 3");
        assert_eq!(expression("5 km - 300 m"), "5 km − 300 m");
        assert_eq!(expression("2+2"), "2 + 2");
        assert_eq!(expression("2  +   2"), "2 + 2");
        assert_eq!(expression("  2+2  "), "2 + 2");
        assert_eq!(expression("now - 3 days"), "now − 3 days");
        assert_eq!(expression("50 + 10%"), "50 + 10%");
        assert_eq!(expression("2×3"), "2 × 3");
        assert_eq!(expression("8÷2"), "8 ÷ 2");
    }

    #[test]
    fn a_sign_is_a_minus_without_spaces() {
        assert_eq!(expression("-5+2"), "−5 + 2");
        assert_eq!(expression("2*-3"), "2 × −3");
        assert_eq!(expression("2^-1"), "2^−1");
        assert_eq!(expression("-(1+1)"), "−(1 + 1)");
    }

    #[test]
    fn signs_that_are_not_operators_stay_as_typed() {
        assert_eq!(expression("0xff in decimal"), "0xff in decimal");
        assert_eq!(expression("0x10"), "0x10");
        assert_eq!(expression("90 km/h in mph"), "90 km/h in mph");
        assert_eq!(expression("1e-3 * 2"), "1e-3 × 2");
        assert_eq!(expression("days until 2026-12-25"), "days until 2026-12-25");
        assert_eq!(expression("2 ** 5"), "2 ** 5");
        assert_eq!(expression("time in new-york"), "time in new-york");
        assert_eq!(expression("max 3"), "max 3");
        assert_eq!(expression("reverse 42"), "reverse 42");
        assert_eq!(expression("5 km in miles"), "5 km in miles");
        assert_eq!(expression("20% of 50"), "20% of 50");
    }
}
