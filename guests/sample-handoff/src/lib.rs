//! Pane's handoff sample: a command that keeps a counter and a draft in
//! memory and opts in to handing them to its new code (ADR 0041, #159), so
//! a Reload, an Update or a development-mode reload finds them where it
//! left them — and its screen, which Pane opens again either way, with the
//! launch record it was opened with. "Add one" counts; "Edit the draft"
//! opens the draft's form, whose text it saves. Nothing is written to
//! Pane's extension data: without the handoff, a replacement loses both,
//! exactly as `docs/generations.md` says.
//!
//! The state is a value this module versions itself, which the SDK's
//! `pane_extension::state` serialises. Items, titles and answers match the
//! JavaScript and TypeScript handoff samples.
#![no_std]

use core::cell::RefCell;

use pane_extension::alloc::{format, string::String, vec, vec::Vec};
use pane_extension::feedback::{Toast, show_toast};
use pane_extension::{
    Command, CustomView, Field, FieldKind, FieldValue, Form, FormError, Item, List, NoCustomView,
    TextField, lifecycle, state,
};

/// The state this instance keeps in memory: the counter and the draft,
/// handed to the new code on a replacement.
#[derive(Default, serde::Serialize, serde::Deserialize)]
struct Kept {
    /// How many times "Add one" ran, this Pane.
    counter: u64,
    /// The draft, as its form last saved it.
    draft: String,
}

/// The state, kept in this instance's memory: lost with it, except what
/// the handoff carries.
static KEPT: KeptCell = KeptCell(RefCell::new(Kept {
    counter: 0,
    draft: String::new(),
}));

/// [`RefCell`] is not `Sync`; the wrapper is.
struct KeptCell(RefCell<Kept>);

// SAFETY: a component's code runs on one thread, and no borrow of the
// state is held across an `await`.
unsafe impl Sync for KeptCell {}

struct Handoff;
pane_extension::export!(Handoff);
pane_extension::lifecycle::export!(Handoff);

impl lifecycle::Guest for Handoff {
    /// This sample declares no activation entry point, so Pane never calls
    /// this: exporting the lifecycle interface opts in to the state
    /// handoff, and `activate` is here because the interface has it.
    async fn activate() {}

    /// The state handed to the new code: the counter and the draft, as the
    /// SDK serialises them. `None` would hand nothing over.
    async fn snapshot() -> Option<Vec<u8>> {
        state::save(&*KEPT.0.borrow())
    }

    /// Restores what a replaced instance handed over. An error discards the
    /// state and starts fresh, which is not a failure: this answers one
    /// when the bytes are not this version's state.
    async fn restore(bytes: Vec<u8>) -> Result<(), String> {
        *KEPT.0.borrow_mut() = state::load(&bytes)?;
        Ok(())
    }
}

impl Command for Handoff {
    type CustomView = NoCustomView;

    /// The command's list: the counter and the draft as they stand, with
    /// the actions that change them. Stable item ids, so a reopened screen
    /// keeps its selection.
    async fn render() -> Result<List, String> {
        let kept = KEPT.0.borrow();
        Ok(List::new(format!(
            "Handoff: {} counted, draft {}",
            kept.counter,
            quoted(&kept.draft)
        ))
        .item(
            Item::new("add", format!("Add one ({} so far)", kept.counter)).on_action(|| async {
                KEPT.0.borrow_mut().counter += 1;
                show_toast(Toast::success("Counted one more"));
                Ok(())
            }),
        )
        .item(
            Item::new(
                "draft",
                format!(
                    "Edit the draft ({} words)",
                    kept.draft.split_whitespace().count()
                ),
            )
            .subtitle(format!("The draft is {}", quoted(&kept.draft)))
            .form(draft_form()),
        ))
    }

    /// The draft form's answer: the draft becomes the text typed, and the
    /// screen says so.
    async fn submit_form(item: String, values: Vec<FieldValue>) -> Result<String, FormError> {
        if item != "draft" {
            return Err(FormError {
                field: None,
                message: format!("unknown form: {item}"),
            });
        }
        let text = values
            .iter()
            .find(|field| field.id == "draft")
            .map(|field| field.value.clone())
            .unwrap_or_default();
        KEPT.0.borrow_mut().draft = text.clone();
        Ok(format!("Saved the draft {}", quoted(&text)))
    }
}

/// The draft's form: one text field.
fn draft_form() -> Form {
    Form {
        title: "Edit the draft".into(),
        fields: vec![Field {
            id: "draft".into(),
            label: "Draft".into(),
            kind: FieldKind::Text(TextField {
                placeholder: Some("What a replacement must keep".into()),
            }),
        }],
        submit_label: "Save".into(),
    }
}

/// `text` in quotes, or "nothing" for empty.
fn quoted(text: &str) -> String {
    if text.is_empty() {
        "nothing".into()
    } else {
        format!("“{text}”")
    }
}
