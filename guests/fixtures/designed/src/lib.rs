//! Test fixture: a designed view whose tree and answers are JSON written
//! by hand against Pane's current contract (`wit/extension.wit`), not by
//! pane-extension, so that Pane's reading of them is checked on its own
//! (see `crates/pane-core/tests/designed_views.rs` and
//! `crates/pane-core/tests/navigation_stack.rs`).
//!
//! Its screen is a counter — a column of a text ("Count: N"), a text
//! naming how many times the view was drawn ("Renders: N", which a
//! refresh raises) and a row of the buttons below, each naming its own
//! callback id — whose presses answer what the next drawing is, once,
//! then the counter again:
//!
//! - "Increment" counts one up, and only when the event's `key` names the
//!   button, so the test checks that Pane sends the node's key;
//! - "Answer an error" makes the next `render` answer an error;
//! - "Answer an over-limit tree" makes it answer a tree of 10,001 nodes;
//! - "Draw an unknown node with a fallback" makes it answer a tree whose
//!   root is a type Pane does not know, with a `fallback` text;
//! - "Draw an unknown node without a fallback" the same without one, with
//!   a text child;
//! - "Draw a newer minor tree" a tree naming version 1.9;
//! - "Draw another major's tree" a tree naming version 2.0;
//! - "Answer an unreadable tree" makes it answer text that is not JSON;
//! - "Answer an unknown callback" answers the event itself with an error.
//!
//! The navigation stack (#239) is answered by hand too: "Push a view"
//! answers a pushed view (a text "Pushed" with the same navigation
//! buttons), "Pop with a result" pops answering "the result", and
//! "Replace this view" replaces it with a pushed view. The pop event —
//! the event with callback id 0 — is recorded and drawn ("Popped: …",
//! or "Popped" when it carried no result), so a test can see it arrive.
//!
//! The keyed state (#238) is answered by hand as well: "Draw the fields"
//! answers a tree of keyed fields — a text input with `onInput` and
//! `onChange`, a text area — and every event the view receives is
//! recorded and drawn in it ("Sent: …"), so a test can see the payloads
//! Pane sent; "Draw a tree with key problems" answers one whose keys two
//! siblings share and whose text input has none, which a developed
//! package's log reports. And the view may ask Pane to draw it again
//! itself (#243, the push channel of `pane:extension/view`): "Push a
//! render" has the next render call `ask-to-render` with the id its
//! context names — by hand, as no SDK writes it — and "Push a render,
//! answering slowly" the same with the drawing it asks for held back a
//! moment, so a test can leave the view before the drawing answers.
//!
//! The `form` command's view (#241) is answered by hand as well: a form
//! of every field kind, whose submission's values are drawn — as the
//! fields' tree draws the events it received — so a test can see what
//! Pane sent, and whose empty name is refused with the field's error in
//! the next drawing.
//!
//! Every render also answers `refresh-after-ms` as the state's
//! `refresh_ms` says, so the tests of #236 drive Pane's refreshing of a
//! view through it — each button below sets it, its presses' answers
//! asking from then on, and the hand-written values ride outside any SDK:
//!
//! - "Answer refresh 10ms" asks for 10 ms, below the 100 ms floor;
//! - "Answer refresh 25h" asks for 25 hours, above the 24 h ceiling;
//! - "Answer refresh 1s" asks for a second;
//! - "Stop refreshing" asks for none.
//!
//! It cannot use `pane-extension`, which writes the tree itself, so it
//! supplies the allocator, panic handler, byte comparisons and
//! `cabi_realloc`.
#![no_std]

extern crate alloc;

use alloc::borrow::ToOwned;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::{Cell, RefCell};
use core::ffi::c_void;

wit_bindgen::generate!({ path: "../../../wit", world: "extension" });

use exports::pane::extension::command::{
    Guest, GuestView, LaunchRecord, Outcome, Rendered, UiEvent, View,
};

/// The version of the UI component set the fixture writes.
const COMPONENT_SET: &str = "1.0";

/// The callback id of the pop event Pane sends the view below a pop.
const POP_CALLBACK: u32 = 0;

/// What the next drawing answers.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Next {
    /// The counter, as a version 1.0 tree.
    Counter,
    /// An error, once.
    Error,
    /// A tree of more nodes than the limit, once.
    OverLimit,
    /// A tree whose root is unknown, with a fallback, once.
    UnknownWithFallback,
    /// A tree whose root is unknown, without a fallback, once.
    UnknownWithoutFallback,
    /// A tree naming a newer minor version, once.
    NewerMinor,
    /// A tree naming another major version, once.
    OtherMajor,
    /// Text that is not JSON, once.
    Unreadable,
    /// The component set's vocabulary, once, then again with the toggle
    /// flipped.
    Components,
    /// The fields, whose events the view records and draws.
    Fields,
    /// A tree whose keys two siblings share and whose text input has
    /// none, which a developed package's log reports.
    KeyProblems,
    /// The form (#241): every field kind, whose submission's values are
    /// drawn, as the fields' tree draws the events it receives.
    Form,
}

/// The root view's state, kept in the resource.
struct State {
    count: Cell<u32>,
    next: Cell<Next>,
    /// Whether the components tree's toggle is on, flipped by the change
    /// event's payload.
    flipped: Cell<bool>,
    /// How many times the view was drawn, refreshes included.
    renders: Cell<u32>,
    /// What every render answers `refresh-after-ms` as, which the refresh
    /// buttons below set: None until one is pressed.
    refresh_ms: Cell<Option<u32>>,
    /// The events the view has received, drawn in the fields' tree so a
    /// test can see what Pane sent (#238): (callback, key, payload).
    received: RefCell<Vec<(u32, String, String)>>,
    /// Whether the next render asks Pane to draw the view again (#243),
    /// which the push buttons below set: the render after the press calls
    /// `ask-to-render` by hand.
    push: Cell<bool>,
    /// How many renders away answers slowly: the slow push button sets 2,
    /// so the render that asks answers promptly and the drawing Pane
    /// sends for the ask is the one held back.
    slow_in: Cell<u32>,
    /// Whether the form's name field is refused, as an empty submission
    /// refuses it: the field's error in the next drawing (#241).
    form_error: Cell<bool>,
}

// SAFETY: a component's code runs on one thread.
unsafe impl Sync for State {}

/// The fixture's open view: the root (the counter, whose buttons name
/// every case above), one a push opened above it, or the `canvas`
/// command's filler. One type serves every view the component opens, as
/// the resource's one representation; the pop event a screen view last
/// received is drawn, so a test can see it arrive.
enum Designed {
    /// The counter's, or a pushed view's: the pop event it last received
    /// is drawn, so a test can see it arrive.
    Screen {
        /// Whether this is the root view (the counter): a pushed view
        /// draws its own tree, with the navigation buttons only.
        root: bool,
        /// The pop event this view last received, if any: the result it
        /// carried (`None` for one that carried none).
        popped: RefCell<Option<Option<String>>>,
    },
    /// The `canvas` command's view (#242): a canvas that fills the space
    /// the layout gives it, drawing the size the render context names for
    /// it and every event it receives, so the tests can see both arrive.
    Filler {
        /// The size the last render's context named for the canvas, as
        /// "WxH", when it named one.
        size: RefCell<String>,
        /// What every event the view received says, drawn in the canvas's
        /// value, so a test can see what Pane sent.
        received: RefCell<Vec<String>>,
    },
}

impl GuestView for Designed {
    async fn render(&self, context: String) -> Result<Rendered, String> {
        // The filler draws its own tree, whose canvas answers the size
        // the context names and the events it received.
        if let Designed::Filler { size, received } = self {
            return filler_rendered(size, received, &context);
        }
        let Designed::Screen { root, popped } = self else {
            unreachable!("a screen or filler view")
        };
        let renders = STATE.renders.get();
        STATE.renders.set(renders + 1);
        // The push a button asked for (#243): the render after its press
        // asks Pane to draw the view again, by hand, with the id this
        // context names.
        if STATE.push.replace(false) {
            pane::extension::view::ask_to_render(view_of(&context));
        }
        // The slow answer a push asked for: this render is the drawing
        // the ask sent for, held back so a test can leave before it
        // answers.
        let slow_in = STATE.slow_in.get();
        if slow_in > 0 {
            STATE.slow_in.set(slow_in - 1);
            if slow_in == 1 {
                wasip3::clocks::monotonic_clock::wait_for(300_000_000).await;
            }
        }
        let popped = popped_text(&popped.borrow());
        // A pushed view draws its own tree, with the navigation buttons
        // only; the root draws the counter, with every case's button.
        if !root {
            return Ok(Rendered {
                tree: pushed(popped),
                refresh_after_ms: STATE.refresh_ms.get(),
            });
        }
        let next = STATE.next.replace(Next::Counter);
        // The component set stays on screen until the counter's buttons
        // are pressed again; its toggle flips within it, as the fields'
        // and the key problems' trees answer their own events.
        if matches!(
            next,
            Next::Components | Next::Fields | Next::KeyProblems | Next::Form
        ) {
            STATE.next.set(next);
        }
        let tree = match next {
            Next::Counter => counter(popped),
            Next::Error => return Err("the view failed on purpose".into()),
            Next::OverLimit => over_limit(),
            Next::UnknownWithFallback => format!(
                "{{\"version\":\"{COMPONENT_SET}\",\"root\":{{\"type\":\"marquee\",\
                 \"fallback\":{{\"type\":\"text\",\"text\":\"The fallback\"}}}}}}"
            ),
            Next::UnknownWithoutFallback => format!(
                "{{\"version\":\"{COMPONENT_SET}\",\"root\":{{\"type\":\"video\",\
                 \"children\":[{{\"type\":\"text\",\"text\":\"Inside\"}}]}}}}"
            ),
            Next::NewerMinor => format!(
                "{{\"version\":\"1.9\",\"root\":{{\"type\":\"column\",\
                 \"children\":[{{\"type\":\"text\",\"text\":\"Newer\"}}]}}}}"
            ),
            Next::OtherMajor => format!(
                "{{\"version\":\"2.0\",\"root\":{{\"type\":\"column\",\
                 \"children\":[{{\"type\":\"text\",\"text\":\"Future\"}}]}}}}"
            ),
            Next::Unreadable => {
                return Ok(Rendered {
                    tree: "{\"version\":\"1.0\",\"root\":".into(),
                    refresh_after_ms: None,
                });
            }
            Next::Components => components(),
            Next::Fields => fields(&received_texts()),
            Next::KeyProblems => key_problems(),
            Next::Form => form_tree(&received_texts()),
        };
        Ok(Rendered {
            tree,
            refresh_after_ms: STATE.refresh_ms.get(),
        })
    }

    async fn handle_event(&self, event: UiEvent) -> Result<Outcome, String> {
        // The filler's events: every one but the pop and the resize is
        // recorded, drawn in the next render's value.
        if let Designed::Filler { received, .. } = self {
            return filler_event(received, &event);
        }
        let Designed::Screen { popped, .. } = self else {
            unreachable!("a screen or filler view")
        };
        // The pop event: the view above this one popped, its payload the
        // result that pop answered. It is drawn by the next render.
        if event.callback == POP_CALLBACK {
            *popped.borrow_mut() = Some(pop_result_of(&event.payload));
            return Ok(outcome());
        }
        // Every event is recorded, drawn in the fields' tree, so a test
        // can see what Pane sent: the callback, the key, the payload.
        {
            let mut received = STATE.received.borrow_mut();
            received.push((event.callback, event.key.clone(), event.payload.clone()));
            let excess = received.len().saturating_sub(8);
            received.drain(0..excess);
        }
        match event.callback {
            1 if event.key == "increment" => {
                STATE.count.set(STATE.count.get() + 1);
            }
            2 => STATE.next.set(Next::Error),
            3 => STATE.next.set(Next::OverLimit),
            4 => STATE.next.set(Next::UnknownWithFallback),
            5 => STATE.next.set(Next::UnknownWithoutFallback),
            6 => STATE.next.set(Next::NewerMinor),
            7 => STATE.next.set(Next::OtherMajor),
            8 => STATE.next.set(Next::Unreadable),
            9 => return Err(format!("unknown callback: {}", event.callback)),
            // The navigation stack: a push, a pop with a result, a replace.
            10 => {
                return Ok(Outcome {
                    push: Some(View::new(Designed::pushed_view())),
                    replace: None,
                    pop: None,
                });
            }
            11 => {
                return Ok(Outcome {
                    push: None,
                    replace: None,
                    pop: Some("the result".into()),
                });
            }
            12 => {
                return Ok(Outcome {
                    push: None,
                    replace: Some(View::new(Designed::pushed_view())),
                    pop: None,
                });
            }
            13 => STATE.next.set(Next::Components),
            18 => STATE.next.set(Next::Fields),
            19 => STATE.next.set(Next::KeyProblems),
            20 => STATE.next.set(Next::Counter),
            // The form's submission (#241): the event is recorded above,
            // drawn in the form's tree, so a test can see the values Pane
            // sent. An empty name is refused: the field's error in the
            // next drawing, the form staying open.
            31 => {
                STATE.form_error.set(
                    submitted_text(&event.payload, "name").is_some_and(|name| name.is_empty()),
                );
                STATE.next.set(Next::Form);
            }
            // The fields' input and change handlers: the event is
            // recorded above, drawn in the fields' tree; nothing next.
            29 => {}
            30 => {}
            22 => {
                // The toggle's change: the payload names the value the
                // user chose, so the fixture flips with it.
                if event.payload.contains("true") {
                    STATE.flipped.set(true);
                } else {
                    STATE.flipped.set(false);
                }
            }
            // The refresh buttons: each makes every render from now on ask
            // Pane to draw the view again as it names (10 ms is below the
            // floor, 25 hours above the ceiling), or not again.
            14 => STATE.refresh_ms.set(Some(10)),
            15 => STATE.refresh_ms.set(Some(25 * 60 * 60 * 1000)),
            16 => STATE.refresh_ms.set(Some(1000)),
            17 => STATE.refresh_ms.set(None),
            // The push buttons (#243): the next render asks Pane to draw
            // the view again; the slow one has the drawing it asks for
            // held back, two renders away.
            23 => STATE.push.set(true),
            24 => {
                STATE.push.set(true);
                STATE.slow_in.set(2);
            }
            _ => return Err(format!("unknown callback: {}", event.callback)),
        }
        Ok(outcome())
    }
}

impl Designed {
    /// A view to push above the root or replace it with.
    fn pushed_view() -> Designed {
        Designed::Screen {
            root: false,
            popped: RefCell::new(None),
        }
    }
}

/// The outcome with nothing next.
fn outcome() -> Outcome {
    Outcome {
        push: None,
        replace: None,
        pop: None,
    }
}

/// The text naming the pop event the view last received, when it received
/// one: `Popped: …`, or `Popped` when it carried no result.
fn popped_text(popped: &Option<Option<String>>) -> Option<String> {
    match popped {
        None => None,
        Some(None) => Some("Popped".into()),
        Some(Some(result)) => Some(format!("Popped: {result}")),
    }
}

/// The view `context` names — its id, what `ask-to-render` asks for — or 0
/// when it says none: the context is JSON, `{"render": N, "view": V, "why":
/// "open", "ui": "1.1"}`, read without parsing the whole document, so a
/// context that grew a field still gives its id.
fn view_of(context: &str) -> u64 {
    let Some(at) = context.find("\"view\"") else {
        return 0;
    };
    let digits = context[at + 6..]
        .trim_start()
        .strip_prefix(':')
        .unwrap_or("")
        .trim_start();
    let end = digits
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(digits.len());
    digits[..end].parse().unwrap_or(0)
}

/// The result the pop event's payload carries: `{"pop": "…"}` for a pop
/// that answered one, `{"pop": null}` for one that carried none. Read
/// without parsing the whole document; the plain-ASCII strings the
/// fixture is asked to answer need no unescaping.
fn pop_result_of(payload: &str) -> Option<String> {
    let at = payload.find("\"pop\"")?;
    let rest = payload[at + 5..]
        .trim_start()
        .strip_prefix(':')?
        .trim_start();
    if rest.starts_with("null") {
        return None;
    }
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(rest[..end].into())
}

static STATE: State = State {
    count: Cell::new(0),
    next: Cell::new(Next::Counter),
    flipped: Cell::new(false),
    renders: Cell::new(0),
    refresh_ms: Cell::new(None),
    received: RefCell::new(Vec::new()),
    push: Cell::new(false),
    slow_in: Cell::new(0),
    form_error: Cell::new(false),
};

/// The buttons the counter's tree names: (label, key, callback id). The
/// navigation ones answer the stack (#239), the component set's draws
/// the component tree (#237), and the next four set what every render
/// asks `refresh-after-ms` (#236); the fields' and the key problems'
/// trees answer the keyed state (#238); the last two ask Pane to draw
/// the view again (#243); a pushed view's tree names the navigation ones
/// alone.
const BUTTONS: [(&str, &str, u32); 21] = [
    ("Increment", "increment", 1),
    ("Answer an error", "error", 2),
    ("Answer an over-limit tree", "over-limit", 3),
    ("Draw an unknown node with a fallback", "with-fallback", 4),
    (
        "Draw an unknown node without a fallback",
        "without-fallback",
        5,
    ),
    ("Draw a newer minor tree", "newer-minor", 6),
    ("Draw another major's tree", "other-major", 7),
    ("Answer an unreadable tree", "unreadable", 8),
    ("Answer an unknown callback", "unknown-callback", 9),
    ("Push a view", "push", 10),
    ("Pop with a result", "pop", 11),
    ("Replace this view", "replace", 12),
    ("Draw the component set", "components", 13),
    ("Draw the fields", "fields", 18),
    ("Draw a tree with key problems", "key-problems", 19),
    ("Answer refresh 10ms", "refresh-10", 14),
    ("Answer refresh 25h", "refresh-25h", 15),
    ("Answer refresh 1s", "refresh-1s", 16),
    ("Stop refreshing", "stop-refresh", 17),
    ("Push a render", "push-a-render", 23),
    (
        "Push a render, answering slowly",
        "push-a-render-slowly",
        24,
    ),
];

/// The buttons a pushed view's tree names: the navigation ones.
const PUSHED_BUTTONS: [(&str, &str, u32); 3] = [
    ("Push a view", "push", 10),
    ("Pop with a result", "pop", 11),
    ("Replace this view", "replace", 12),
];

/// The counter as its tree, with one button per case above, naming the
/// pop event the view last received.
fn counter(popped: Option<String>) -> String {
    let count = format!("Count: {}", STATE.count.get());
    view(&BUTTONS, &count, popped)
}

/// A pushed view as its tree, with the navigation buttons.
fn pushed(popped: Option<String>) -> String {
    view(&PUSHED_BUTTONS, "Pushed", popped)
}

/// The view's tree: its `title` text, how many times the view was drawn
/// (refreshes raise it), the pop event it last received (when it
/// received one), and one button per case.
fn view(buttons: &[(&str, &str, u32)], title: &str, popped: Option<String>) -> String {
    let drawn: Vec<String> = buttons
        .iter()
        .map(|&(label, key, callback)| {
            format!(
                "{{\"type\":\"button\",\"key\":\"{key}\",\"label\":\"{label}\",\
                 \"onPress\":{callback}}}"
            )
        })
        .collect();
    format!(
        "{{\"version\":\"{COMPONENT_SET}\",\"root\":{{\"type\":\"column\",\"gap\":\"m\",\
         \"children\":[{{\"type\":\"text\",\"text\":\"{title}\",\"style\":\"title\"}},\
         {{\"type\":\"text\",\"text\":\"Renders: {}\"}}{},\
         {{\"type\":\"row\",\"gap\":\"s\",\"children\":[{}]}}]}}}}",
        STATE.renders.get(),
        popped
            .map(|text| format!(",{{\"type\":\"text\",\"text\":\"{text}\"}}"))
            .unwrap_or_default(),
        drawn.join(","),
    )
}

/// The component set's vocabulary, as a tree: the layout primitives, the
/// style every node carries, the shared components, the tokens and the
/// raw values — every kind of node, once, with the toggle's state drawn
/// from what the change events flipped.
fn components() -> String {
    let on = STATE.flipped.get();
    format!(
        "{{\"version\":\"{COMPONENT_SET}\",\"root\":{{\"type\":\"scroll\",         \"children\":[{{\"type\":\"column\",\"gap\":\"l\",\"children\":[\
         {{\"type\":\"text\",\"text\":\"The component set\",\"style\":\"heading\"}},\
         {{\"type\":\"stack\",\"align\":\"top-end\",\"children\":[\
         {{\"type\":\"icon\",\"icon\":{{\"builtin\":\"layers\"}},\"size\":\"l\"}},\
         {{\"type\":\"badge\",\"text\":\"4\",\"place\":\"bottom-end\",         \"offset\":{{\"x\":4,\"y\":\"4px\"}}}}]}},\
         {{\"type\":\"text\",\"spans\":[\
         {{\"text\":\"Accept the \"}},\
         {{\"text\":\"terms\",\"onPress\":12,\"color\":\"accent\"}},\
         {{\"text\":\" now\",\"code\":true}}],\"level\":\"body\"}},\
         {{\"type\":\"row\",\"gap\":\"s\",\"name\":\"Marks\",\"children\":[\
         {{\"type\":\"keycap\",\"key\":\"ctrl\"}},\
         {{\"type\":\"key-sequence\",\"keys\":[\"ctrl\",\"shift\",\"p\"]}},\
         {{\"type\":\"tag\",\"text\":\"beta\",\"color\":\"blue\"}},\
         {{\"type\":\"badge\",\"text\":\"3\"}}]}},\
         {{\"type\":\"card\",\"gap\":\"s\",\"children\":[\
         {{\"type\":\"rich-row\",\"title\":\"Pane\",\"subtitle\":\"A tree\",\
         \"icon\":{{\"builtin\":\"layers\"}},\
         \"accessories\":[{{\"text\":\"new\",\"tag\":true}}],\"onPress\":21}}]}},\
         {{\"type\":\"toggle\",\"key\":\"toggle\",\"on\":{on},\"label\":\"Dark mode\",         \"onChange\":22}},\
         {{\"type\":\"checkbox\",\"checked\":{on},\"label\":\"Remember\",\"onChange\":22}},\
         {{\"type\":\"segmented\",\"options\":[{{\"value\":\"a\",\"label\":\"A\"}},\
         {{\"value\":\"b\"}}],\"value\":\"a\",\"onChange\":23}},\
         {{\"type\":\"slider\",\"value\":0.4,\"min\":0,\"max\":2,\"step\":0.2,\"onChange\":24}},\
         {{\"type\":\"progress\",\"value\":0.7}},\
         {{\"type\":\"loading\",\"label\":\"Checking\"}},\
         {{\"type\":\"markdown\",\"markdown\":\"# Title\\n\\nSome *prose*.\",\"grow\":1}},\
         {{\"type\":\"metadata-list\",\"items\":[\
         {{\"label\":\"Author\",\"value\":\"Vu\",\"onPress\":25}},\
         {{\"label\":\"Tags\",\"tags\":[\"one\",\"two\"]}},\
         {{\"separator\":true}},\
         {{\"label\":\"Kind\",\"value\":\"sample\"}}]}},\
         {{\"type\":\"empty-state\",\"title\":\"Nothing\",\"description\":\"Over\",\
         \"icon\":{{\"builtin\":\"search-minus\"}},\
         \"children\":[{{\"type\":\"button\",\"label\":\"Start over\",\"onPress\":26}}]}},\
         {{\"type\":\"text-input\",\"key\":\"name\",\"value\":\"typed\",         \"placeholder\":\"Type here\",\"label\":\"Name\",\"onChange\":27}},\
         {{\"type\":\"password-input\",\"label\":\"Secret\"}},\
         {{\"type\":\"text-area\",\"value\":\"two lines\",\"label\":\"Notes\"}},\
         {{\"type\":\"select\",\"options\":[{{\"value\":\"x\",\"label\":\"X\"}}],\
         \"value\":\"x\",\"label\":\"Pick\",\"onChange\":28}},\
         {{\"type\":\"divider\"}},{{\"type\":\"spacer\"}},\
         {{\"type\":\"text\",\"text\":\"Surface\",\"level\":\"secondary\",         \"background\":\"danger\",\"radius\":\"m\",\"hover\":{{\"background\":\"accent\"}}}},\
         {{\"type\":\"text\",\"text\":\"Corrected\",\"color\":\"#88ccff\"}},\
         {{\"type\":\"text\",\"text\":\"Exact\",\"color\":{{\"raw\":\"#ff6363\"}}}}\
         ]}}]}}}}"
    )
}

/// The fields as their tree: a text input keyed `name`, asking for its
/// value as the user types it (callback 29) and on its commits (30), a
/// text area keyed `notes`, and the events the view last received drawn
/// as texts, so a test can see what Pane sent.
fn fields(received: &[String]) -> String {
    let mut children = Vec::new();
    children.push(String::from(
        "{\"type\":\"text\",\"text\":\"The fields\",\"style\":\"title\"}",
    ));
    children.push(format!(
        "{{\"type\":\"text\",\"text\":\"Renders: {}\"}}",
        STATE.renders.get()
    ));
    children.push(String::from(
        "{\"type\":\"text-input\",\"key\":\"name\",\"value\":\"typed\",         \"placeholder\":\"Type here\",\"label\":\"Name\",\"onInput\":29,\"onChange\":30}",
    ));
    children.push(String::from(
        "{\"type\":\"text-area\",\"key\":\"notes\",\"value\":\"two lines\",\"label\":\"Notes\"}",
    ));
    for text in received {
        children.push(format!("{{\"type\":\"text\",\"text\":\"{text}\"}}"));
    }
    children.push(String::from(
        "{\"type\":\"row\",\"gap\":\"s\",\"children\":[{\"type\":\"button\",         \"key\":\"again\",\"label\":\"Answer the counter again\",\"onPress\":20}]}",
    ));
    format!(
        "{{\"version\":\"{COMPONENT_SET}\",\"root\":{{\"type\":\"column\",\"gap\":\"m\",         \"children\":[{}]}}}}",
        children.join(",")
    )
}

/// The form's tree (#241): a `form` node whose children are every field
/// kind, each keyed and titled, the name's carrying the error an empty
/// submission set and asking to be remembered — and the events the view
/// received drawn under them, the submission's payload among them, so a
/// test can see the values Pane sent. Written by hand, as the counter's
/// is.
fn form_tree(received: &[String]) -> String {
    let error = if STATE.form_error.get() {
        ",\"error\":\"Enter a name\""
    } else {
        ""
    };
    let mut children = Vec::from([
        format!(
            "{{\"type\":\"text-input\",\"key\":\"name\",\"title\":\"Name\",\
             \"placeholder\":\"Ada Lovelace\",\"focus\":true,\"remember\":true{error}}}"
        ),
        String::from("{\"type\":\"password-input\",\"key\":\"secret\",\"title\":\"Secret\"}"),
        String::from("{\"type\":\"text-area\",\"key\":\"notes\",\"title\":\"Notes\"}"),
        String::from("{\"type\":\"date-picker\",\"key\":\"day\",\"title\":\"Day\"}"),
        String::from("{\"type\":\"date-time-picker\",\"key\":\"at\",\"title\":\"At\"}"),
        String::from(
            "{\"type\":\"select\",\"key\":\"greeting\",\"title\":\"Greeting\",\
             \"options\":[{\"value\":\"hello\",\"label\":\"Hello\",\"section\":\"Plain\"},\
             {\"value\":\"morning\",\"label\":\"Good morning\",\"section\":\"Warm\"}],\
             \"value\":\"hello\"}",
        ),
        String::from(
            "{\"type\":\"tag-picker\",\"key\":\"tags\",\"title\":\"Tags\",\
             \"options\":[{\"value\":\"friend\",\"label\":\"Friend\"},\
             {\"value\":\"colleague\",\"label\":\"Colleague\"}]}",
        ),
        String::from("{\"type\":\"file-picker\",\"key\":\"file\",\"title\":\"File\"}"),
        String::from(
            "{\"type\":\"folder-picker\",\"key\":\"folder\",\"title\":\"Folder\",\"multiple\":true}",
        ),
        String::from(
            "{\"type\":\"checkbox\",\"key\":\"updates\",\"title\":\"Send updates\",\"checked\":true}",
        ),
        String::from(
            "{\"type\":\"toggle\",\"key\":\"quiet\",\"title\":\"Quiet mode\",\"on\":false}",
        ),
    ]);
    for text in received {
        children.push(format!("{{\"type\":\"text\",\"text\":\"{text}\"}}"));
    }
    children.push(String::from(
        "{\"type\":\"button\",\"key\":\"again\",\"label\":\"Answer the counter again\",\"onPress\":20}",
    ));
    format!(
        "{{\"version\":\"{COMPONENT_SET}\",\"root\":{{\"type\":\"form\",\"key\":\"form\",\
         \"onSubmit\":31,\"submitLabel\":\"Send\",\"children\":[{}]}}}}",
        children.join(",")
    )
}

/// The text the field `name` of a form's submission payload holds, when
/// it names one: read without parsing the whole document, as the pop
/// result is.
fn submitted_text(payload: &str, name: &str) -> Option<String> {
    let needle = format!("\"{name}\":\"");
    let at = payload.find(&needle)? + needle.len();
    let rest = &payload[at..];
    let end = rest.find('"')?;
    Some(rest[..end].into())
}

/// A tree whose keys two siblings share and whose text input has none:
/// what a developed package's log reports, and what Pane matches by
/// position instead.
fn key_problems() -> String {
    let children = Vec::from([
        String::from("{\"type\":\"text\",\"text\":\"Key problems\"}"),
        String::from(
            "{\"type\":\"row\",\"gap\":\"s\",\"children\":[\
             {\"type\":\"text\",\"key\":\"shared\",\"text\":\"One\"},\
             {\"type\":\"text\",\"key\":\"shared\",\"text\":\"Two\"}]}",
        ),
        String::from("{\"type\":\"text-input\",\"value\":\"keyless\"}"),
        String::from(
            "{\"type\":\"button\",\"key\":\"again\",\"label\":\
             \"Answer the counter again\",\"onPress\":20}",
        ),
    ]);
    format!(
        "{{\"version\":\"{COMPONENT_SET}\",\"root\":{{\"type\":\"column\",\"gap\":\"m\",\
         \"children\":[{}]}}}}",
        children.join(",")
    )
}

/// The events the view last received, as the texts the fields' tree
/// draws: "Sent: {callback} on {key}: {payload}".
fn received_texts() -> Vec<String> {
    STATE
        .received
        .borrow()
        .iter()
        .map(|(callback, key, payload)| {
            format!(
                "Sent: {callback} on {}: {}",
                if key.is_empty() { "(no key)" } else { key },
                escaped(payload)
            )
        })
        .collect()
}

/// `text` with the characters escaped that a JSON string cannot hold
/// bare, so the fixture can draw a payload it received.
fn escaped(text: &str) -> String {
    let mut escaped = String::new();
    for character in text.chars() {
        match character {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            other if (other as u32) < 0x20 => {
                escaped.push_str(&format!("\\u{:04x}", other as u32));
            }
            other => escaped.push(other),
        }
    }
    escaped
}

/// A tree of one node more than the limit, with a count of 10001.
fn over_limit() -> String {
    let mut tree =
        format!("{{\"version\":\"{COMPONENT_SET}\",\"root\":{{\"type\":\"column\",\"children\":[");
    for _ in 0..10_001 {
        tree.push_str("{\"type\":\"text\",\"text\":\"a\"},");
    }
    tree.pop();
    tree.push_str("]}}");
    tree
}

struct Fixture;
export!(Fixture);

/// The filler's tree, written by hand as the counter's is: a canvas that
/// fills the space the layout gives it, drawing the size the render
/// context named for it and every event the view received (#242).
fn filler_rendered(
    size: &RefCell<String>,
    received: &RefCell<Vec<String>>,
    context: &str,
) -> Result<Rendered, String> {
    // The context names the size each canvas was laid out at:
    // {\"canvases\":{\"fill\":{\"width\":W,\"height\":H}}}. Read without a
    // parser, as the fields' payloads are.
    let mut drawn = String::from("0x0");
    if let Some(at) = context.find("\"canvases\"") {
        let rest = &context[at..];
        if let Some(width) = number_after(rest, "\"width\":") {
            if let Some(height) = number_after(rest, "\"height\":") {
                drawn = format!("{width}x{height}");
            }
        }
    }
    *size.borrow_mut() = drawn.clone();
    let said = received.borrow().join("; ");
    let value = if said.is_empty() {
        drawn.clone()
    } else {
        format!("{drawn}; {said}")
    };
    let ops = format!("[{{\"op\":\"text\",\"x\":4,\"y\":4,\"text\":\"{value}\"}}]");
    let tree = format!(
        "{{\"version\":\"{COMPONENT_SET}\",\"root\":{{\"type\":\"column\",\"children\":[\
         {{\"type\":\"canvas\",\"key\":\"fill\",\"grow\":1,\"role\":\"slider\",\"label\":\"Filler\",\
         \"value\":\"{value}\",\"onResize\":1,\"onKey\":2,\"onPointerDown\":3,\"onPointerUp\":4,\
         \"onPointerMove\":5,\"onPointerEnter\":6,\"onPointerLeave\":7,\"onWheel\":8,\
         \"onDoubleClick\":9,\"onSecondary\":10,\"onIncrement\":11,\
         \"ops\":{ops}}}]}}}}"
    );
    Ok(Rendered {
        tree,
        refresh_after_ms: None,
    })
}

/// The filler's answer to an event: every one but the pop (callback 0)
/// and the resize (callback 1) is recorded, drawn in the next render's
/// value — the resize is the size the value already names.
fn filler_event(received: &RefCell<Vec<String>>, event: &UiEvent) -> Result<Outcome, String> {
    if event.callback <= 1 {
        return Ok(outcome());
    }
    let said = event_summary(&event.payload);
    received.borrow_mut().push(said);
    let excess = received.borrow().len().saturating_sub(6);
    received.borrow_mut().drain(0..excess);
    Ok(outcome())
}

/// What an event's payload says, as the canvas's value draws it: the kind
/// it names and where it happened.
fn event_summary(payload: &str) -> String {
    let kind = payload
        .find("\"event\":\"")
        .map(|at| {
            let rest = &payload[at + 10..];
            let end = rest.find('"').unwrap_or(rest.len());
            rest[..end].to_owned()
        })
        .or_else(|| {
            payload.find("\"key\":\"").map(|at| {
                let rest = &payload[at + 7..];
                let end = rest.find('"').unwrap_or(rest.len());
                format!("key {}", &rest[..end])
            })
        })
        .unwrap_or_else(|| "unknown".into());
    let at = |name: &str| number_after(payload, &format!("\"{name}\":"));
    let point = match (at("x"), at("y")) {
        (Some(x), Some(y)) => format!(" {x},{y}"),
        _ => String::new(),
    };
    format!("{kind}{point}")
}

/// The number after `needle` in `text`, when one is there.
fn number_after(text: &str, needle: &str) -> Option<u32> {
    let at = text.find(needle)?;
    let rest = text[at + needle.len()..].trim_start();
    let end = rest.find([',', '}', ' ']).unwrap_or(rest.len());
    rest[..end].parse().ok()
}

impl Guest for Fixture {
    type View = Designed;

    async fn render(_launch: LaunchRecord) -> Result<String, String> {
        Err("this command opens a designed view".into())
    }

    async fn run(command: String, _launch: LaunchRecord) -> Result<String, String> {
        Err(format!("`{command}` opens a designed view"))
    }

    async fn handle_event(callback: String, _details: String) -> Result<String, String> {
        Err(format!("unknown callback: {callback}"))
    }

    async fn open_view(command: String, _launch: LaunchRecord) -> Result<View, String> {
        match command.as_str() {
            "canvas" => Ok(View::new(Designed::Filler {
                size: RefCell::new(String::from("0x0")),
                received: RefCell::new(Vec::new()),
            })),
            "form" => {
                STATE.count.set(0);
                STATE.next.set(Next::Form);
                STATE.renders.set(0);
                STATE.refresh_ms.set(None);
                STATE.received.borrow_mut().clear();
                STATE.form_error.set(false);
                Ok(View::new(Designed::Screen {
                    root: true,
                    popped: RefCell::new(None),
                }))
            }
            _ => {
                STATE.count.set(0);
                STATE.next.set(Next::Counter);
                STATE.renders.set(0);
                STATE.refresh_ms.set(None);
                STATE.received.borrow_mut().clear();
                Ok(View::new(Designed::Screen {
                    root: true,
                    popped: RefCell::new(None),
                }))
            }
        }
    }
}

#[global_allocator]
static ALLOC: dlmalloc::GlobalDlmalloc = dlmalloc::GlobalDlmalloc;

#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    core::arch::wasm32::unreachable()
}

/// Byte comparison, normally supplied by libc; the compiler emits calls to
/// it for string comparisons.
///
/// # Safety
/// `a` and `b` must be valid for reads of `n` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn memcmp(a: *const c_void, b: *const c_void, n: usize) -> i32 {
    let (a, b) = (a.cast::<u8>(), b.cast::<u8>());
    for i in 0..n {
        // Byte by byte through volatile reads, so that the compiler cannot
        // turn this loop back into a call to memcmp.
        let (x, y) = unsafe { (a.add(i).read_volatile(), b.add(i).read_volatile()) };
        if x != y {
            return i32::from(x) - i32::from(y);
        }
    }
    0
}

/// Equality-only form of [`memcmp`], which the compiler may call instead.
///
/// # Safety
/// `a` and `b` must be valid for reads of `n` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bcmp(a: *const c_void, b: *const c_void, n: usize) -> i32 {
    unsafe { memcmp(a, b, n) }
}

/// Canonical-ABI allocation entry point, normally supplied by `std`.
///
/// # Safety
/// Called only by the component runtime with valid allocation metadata.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cabi_realloc(
    old: *mut u8,
    old_size: usize,
    align: usize,
    new_size: usize,
) -> *mut u8 {
    use alloc::alloc::{Layout, alloc, realloc};
    if new_size == 0 {
        return align as *mut u8;
    }
    let result = if old_size == 0 {
        unsafe { alloc(Layout::from_size_align_unchecked(new_size, align)) }
    } else {
        unsafe {
            realloc(
                old,
                Layout::from_size_align_unchecked(old_size, align),
                new_size,
            )
        }
    };
    if result.is_null() {
        core::arch::wasm32::unreachable();
    }
    result
}
