//! Pane's calculator, a default extension: typing an arithmetic expression
//! into root search lists its answer first, and invoking the answer copies
//! it. Its command lists the expressions it understands (see [`expression`]).
//! Disabling the package removes both.
#![no_std]

mod expression;

use pane_guest::alloc::{format, string::String, vec, vec::Vec};
use pane_guest::feedback::{Toast, show_toast};
use pane_guest::root::{RootAction, RootResult};
use pane_guest::{Command, CustomView, FieldValue, FormError, Item, List, NoCustomView};

use expression::Outcome;

struct Calculator;
pane_guest::export!(Calculator);
pane_guest::root::export!(Calculator);

/// The command's items: (id, title, example expression).
const EXAMPLES: [(&str, &str, &str); 3] = [
    (
        "arithmetic",
        "Add, subtract, multiply and divide",
        "2 + 3 × 4 − 6 / 2",
    ),
    ("powers", "Powers with a whole-number exponent", "2^10"),
    (
        "parentheses",
        "Parentheses and negative numbers",
        "-(1.5 + 2.5) * 3",
    ),
];

/// Runs the action of the item `item_id`: a toast with the answer to its
/// example.
async fn act(item_id: &str) -> Result<(), String> {
    let (_, _, example) = EXAMPLES
        .iter()
        .find(|(id, _, _)| *id == item_id)
        .ok_or_else(|| format!("unknown item: {item_id}"))?;
    match expression::evaluate(example) {
        Outcome::Answer(value) => {
            show_toast(Toast::success(format!(
                "{example} = {}",
                expression::format(value)
            )));
            Ok(())
        }
        other => Err(format!("{example} has no answer: {other:?}")),
    }
}

impl Command for Calculator {
    type CustomView = NoCustomView;

    async fn render() -> Result<List, String> {
        let items = EXAMPLES.into_iter().map(|(id, title, example)| {
            Item::new(id, title)
                .subtitle(format!("For example {example}"))
                .on_action(move || act(id))
        });
        Ok(List::new("Calculator: type an expression in root search").items(items))
    }

    async fn submit_form(_item_id: String, _values: Vec<FieldValue>) -> Result<String, FormError> {
        Err(FormError {
            field: None,
            message: "the calculator has no forms".into(),
        })
    }

    async fn open_view(_item_id: String) -> Result<CustomView, String> {
        Err("the calculator has no custom views".into())
    }
}

impl pane_guest::root::Guest for Calculator {
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
