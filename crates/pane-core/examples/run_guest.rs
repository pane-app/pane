//! Drives one component through Pane's real extension runtime, for guests that
//! are built outside `cargo xtask guests` (such as the JS/TS research guests).
//!
//! ```text
//! cargo run -p pane-core --example run_guest -- <component.wasm> view action:<item-id> ...
//! cargo run -p pane-core --example run_guest -- <component.wasm> 'submit:form:name=Ada&greeting=hello'
//! ```
//!
//! `submit:<item-id>:<field>=<value>&...` submits the item's form with those
//! values; values are taken literally (no URL decoding).
//!
//! `view` prints the list the command's tree describes; `action:<item-id>`
//! runs that item's action as choosing it would (its tree, then its
//! action's callback) and prints the text it answers.
//!
//! Each operation prints one line: `ok\t<ms>\t<value>` or `err\t<ms>\t<error>`.
//! One process is one runtime, so every run starts a fresh guest instance.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use futures::executor::block_on;
use pane_core::{FieldValue, Runtime};

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(component) = args.next().map(PathBuf::from) else {
        eprintln!(
            "usage: run_guest <component.wasm> [view | action:<item-id> | submit:<item-id>:<field>=<value>&...]..."
        );
        return ExitCode::FAILURE;
    };
    let runtime = match Runtime::start() {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let mut failed = false;
    for operation in args {
        let start = Instant::now();
        let outcome = if operation == "view" {
            block_on(runtime.render(&component)).map(|view| format!("{view:?}"))
        } else if let Some(item_id) = operation.strip_prefix("action:") {
            block_on(runtime.run_item(&component, item_id))
                .map(|answer| format!("{answer:?}"))
        } else if let Some(form) = operation.strip_prefix("submit:") {
            let (item_id, values) = form.split_once(':').unwrap_or((form, ""));
            let values = values
                .split('&')
                .filter(|pair| !pair.is_empty())
                .map(|pair| {
                    let (id, value) = pair.split_once('=').unwrap_or((pair, ""));
                    FieldValue {
                        id: id.into(),
                        value: value.into(),
                    }
                })
                .collect();
            block_on(runtime.submit_form(&component, item_id, values))
        } else {
            eprintln!("unknown operation: {operation}");
            return ExitCode::FAILURE;
        };
        let ms = start.elapsed().as_secs_f64() * 1000.0;
        match outcome {
            Ok(value) => println!("ok\t{ms:.3}\t{value}"),
            Err(error) => {
                failed = true;
                println!("err\t{ms:.3}\t{error}");
            }
        }
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
