//! Pane's calculator, a default extension: typing an arithmetic
//! expression, a percentage phrase, a colour or a date or time word into
//! root search lists its answer first, and invoking the answer copies it
//! (see [`expression`], [`colour`] and [`datetime`] for what it
//! understands). Its command is a root provider (`"mode": "provider"` in
//! `pane.json`, #164): it has no row and no screen of its own, only the
//! answer. Disabling the package removes it.
#![no_std]

mod colour;
mod datetime;
mod expression;
mod math;

use pane_extension::alloc::{format, string::String, vec, vec::Vec};
use pane_extension::root::{AnswerCopy, AnswerDetail, RootAction, RootResult, WallTime};
use pane_extension::{Command, NoCustomView};

use expression::Outcome;

struct Calculator;
pane_extension::export!(Calculator);
pane_extension::root::export!(Calculator);

/// A root provider: Pane never opens or runs it, so the command keeps the
/// defaults (opening it is an error).
impl Command for Calculator {
    type CustomView = NoCustomView;
}

impl pane_extension::root::Guest for Calculator {
    /// The answer to `query` if it is an expression, a percentage phrase,
    /// a colour or a date or time word with one, else nothing: ordinary
    /// words and incomplete or invalid queries are not errors. A query
    /// about the moment is answered from `at`, when Pane asked.
    async fn results_for(query: String, at: WallTime) -> Result<Vec<RootResult>, String> {
        let query = query.trim();
        if query.chars().count() > expression::MAX_LENGTH {
            return Ok(Vec::new());
        }
        // A colour answers as its hex, drawn with a swatch and offered in
        // each form.
        if let Some(colour) = colour::parse(query) {
            let hex = colour.hex();
            return Ok(vec![RootResult {
                id: "answer".into(),
                title: hex.clone(),
                subtitle: Some(format!("{query} = {hex} · Enter copies the color")),
                action: RootAction::Copy(hex.clone()),
                answer: Some(AnswerDetail {
                    section: "Color".into(),
                    swatch: Some(hex),
                    copies: vec![
                        copy("Copy as Hex", &hex),
                        copy("Copy as RGB", &colour.rgb()),
                        copy("Copy as HSL", &colour.hsl()),
                        copy("Copy as OKLCH", &colour.oklch()),
                    ],
                }),
            }]);
        }
        // A date or time word answers as the local date or time, offered
        // as ISO 8601 and as a Unix timestamp.
        if let Some(word) = datetime::word(query) {
            let answer = datetime::answer(word, &at);
            return Ok(vec![RootResult {
                id: "answer".into(),
                title: answer.shown.clone(),
                subtitle: Some(format!("{} = {} · Enter copies it", query, answer.shown)),
                action: RootAction::Copy(answer.shown.clone()),
                answer: Some(AnswerDetail {
                    section: "Date & Time".into(),
                    swatch: None,
                    copies: vec![
                        copy("Copy ISO 8601", &answer.iso),
                        copy("Copy Unix timestamp", &answer.unix),
                    ],
                }),
            }]);
        }
        // A percentage phrase or an arithmetic expression answers as its
        // value.
        let value = expression::percentage(query).or_else(|| match expression::evaluate(query) {
            Outcome::Answer(value) => Some(value),
            _ => None,
        });
        let Some(value) = value else {
            return Ok(Vec::new());
        };
        let answer = expression::format(value);
        Ok(vec![RootResult {
            id: "answer".into(),
            title: answer.clone(),
            subtitle: Some(format!("{query} = {answer} · Enter copies the answer")),
            action: RootAction::Copy(answer),
            answer: None,
        }])
    }
}

/// A copy of `text` the Actions panel offers, titled `title`.
fn copy(title: &str, text: &str) -> AnswerCopy {
    AnswerCopy {
        title: title.into(),
        text: text.into(),
    }
}
