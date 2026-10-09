//! Pane's calculator, a default extension: typing an arithmetic expression
//! into root search lists its answer first, and invoking the answer copies
//! it (see [`expression`] for what it understands). Its command is a root
//! provider (`"mode": "provider"` in `pane.json`, #164): it has no row and
//! no screen of its own, only the answer. Disabling the package removes it.
#![no_std]

mod expression;

use pane_extension::alloc::{format, string::String, vec, vec::Vec};
use pane_extension::root::{RootAction, RootResult};
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
    /// The answer to `query` if it is an expression with one, else nothing:
    /// ordinary words and incomplete or invalid expressions are not errors.
    async fn results_for(query: String) -> Result<Vec<RootResult>, String> {
        let Outcome::Answer(value) = expression::evaluate(&query) else {
            return Ok(Vec::new());
        };
        let answer = expression::format(value);
        Ok(vec![RootResult {
            id: "answer".into(),
            title: answer.clone(),
            subtitle: Some(format!(
                "{} = {answer} · Enter copies the answer",
                query.trim()
            )),
            action: RootAction::Copy(answer),
        }])
    }
}
