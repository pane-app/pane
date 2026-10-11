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
//! `pane_extension::state` serialises. The draft's form is a designed view
//! the item's action opens (#241), as the typed form it was is retired;
//! items, titles and answers match the JavaScript and TypeScript handoff
//! samples.
#![no_std]

use core::cell::RefCell;

use pane_extension::alloc::{borrow::ToOwned, format, string::String, vec::Vec};
use pane_extension::feedback::{Toast, show_toast};
use pane_extension::view::{Cx, IntoAnswer, View, button, column, item, list, text_input};
use pane_extension::{Command, Item, LaunchRecord, List, lifecycle, state};

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
    /// The draft's form, which the "Edit the draft" item opens: a
    /// designed view (the package's pane.json gives it its own command,
    /// `"mode": "designed"`).
    type DesignedView = DraftView;

    async fn open_designed_view(
        command: String,
        _launch: LaunchRecord,
    ) -> Result<DraftView, String> {
        if command != "draft" {
            return Err(format!("unknown command: {command}"));
        }
        Ok(DraftView {
            text: RefCell::new(KEPT.0.borrow().draft.clone()),
        })
    }

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
            .on_action(|| async {
                pane_extension::commands::launch(
                    &pane_extension::commands::CommandRef {
                        source: None,
                        command: "draft".into(),
                    },
                    pane_extension::LaunchType::UserInitiated,
                    &[],
                    None,
                )
                .map_err(|problem| format!("could not open the draft: {problem}"))
            }),
        ))
    }
}

/// The draft's form as a designed view (#241): a text field and a Save
/// button. Saving pops the view back to the list, which re-renders with
/// the draft it kept.
struct DraftView {
    /// The field's value, as the user typed it.
    text: RefCell<String>,
}

impl View for DraftView {
    fn render(&mut self, cx: &mut Cx<Self>) -> impl IntoAnswer {
        let text = self.text.borrow().clone();
        column()
            .navigation_title("Edit the draft")
            .gap(pane_extension::view::Space::M)
            .child(
                text_input(text.clone())
                    .label("Draft")
                    .placeholder("What a replacement must keep")
                    .on_input(cx.value_listener(|this: &mut Self, value: &str| {
                        *this.text.borrow_mut() = value.to_owned();
                    })),
            )
            .child(button("Save").on_click(cx.pop_with(|this| {
                let text = this.text.borrow().clone();
                KEPT.0.borrow_mut().draft = text.clone();
                format!("Saved the draft {}", quoted(&text))
            })))
            .into_answer()
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
