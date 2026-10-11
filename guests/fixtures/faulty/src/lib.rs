//! Test fixture: a guest whose actions and root results fail in each way the
//! host must report, and whose actions grow its memory to either side of
//! the cap Pane puts on it.
#![no_std]

use core::cell::Cell;

use pane_extension::alloc::{format, string::String, vec, vec::Vec};
use pane_extension::feedback::{Toast, show_toast};
use pane_extension::{
    Command, CustomView, CustomViewInfo, CustomViewRole, Field, FieldKind, FieldValue, Form,
    FormError, Frame, GuestCustomView, Item, Key, List, Shape, Text, TextField, ViewEvent,
};

struct Faulty;
pane_extension::export!(Faulty);

/// A WebAssembly page, the unit memory grows by.
const PAGE: usize = 64 * 1024;

/// The cap Pane puts on a guest's memory, 128 MiB, in pages: these items
/// pin it from both sides.
const CAP_PAGES: usize = 128 * 1024 * 1024 / PAGE;

/// Grows the memory to two pages under the cap, past which Pane refuses
/// it, and answers its size in bytes. The pages left are the allocator's,
/// for the answer and the next call's arguments.
fn grow_to_just_under_the_cap() -> Result<usize, String> {
    use core::arch::wasm32::{memory_grow, memory_size};
    let pages = memory_size::<0>();
    let wanted = CAP_PAGES - 2;
    if pages < wanted && memory_grow::<0>(wanted - pages) == usize::MAX {
        return Err(format!("could not grow from {pages} pages to {wanted}"));
    }
    Ok(memory_size::<0>() * PAGE)
}

/// An item titled by its id.
fn item(id: &str) -> Item {
    Item::new(id, id)
}

/// An item whose action is [`run`] with its id.
fn acting(id: &'static str) -> Item {
    item(id).on_action(move || run(id))
}

/// Runs the action `id` and shows a toast with what [`outcome`] answers.
async fn run(id: &str) -> Result<(), String> {
    let done = outcome(id).await?;
    show_toast(Toast::success(done));
    Ok(())
}

/// What the action `id` does: "error" is refused, "trap" traps,
/// "grow-near-cap" and "grow-past-cap" grow the memory to either side of
/// the cap, and "hold", which no item lists (tests run it by its callback
/// id), holds the call. It answers what it did.
async fn outcome(id: &str) -> Result<String, String> {
    match id {
        // Holds a stream open to the host (its stdout), with the future of
        // that write pending, saves `holding` as "started", then waits ten
        // seconds before closing them and saving "finished". Tests stop it
        // meanwhile; it is not listed.
        "hold" => {
            let (writer, reader) = wasip3::wit_stream::new::<u8>();
            let written = wasip3::cli::stdout::write_via_stream(reader);
            pane_extension::settings::set("holding", "started")?;
            wasip3::clocks::monotonic_clock::wait_for(10_000_000_000).await;
            drop(writer);
            let _ = written.await;
            pane_extension::settings::set("holding", "finished")?;
            Ok("held".into())
        }
        "error" => Err("the guest refused".into()),
        "trap" => panic!("guest trap"),
        // Ends just under the cap: Pane lets it.
        "grow-near-cap" => Ok(format!("grew to {} bytes", grow_to_just_under_the_cap()?)),
        // Then allocates a mebibyte more, which Pane refuses: the
        // allocation fails, and the guest traps.
        "grow-past-cap" => {
            grow_to_just_under_the_cap()?;
            let block: Vec<u8> = Vec::with_capacity(1024 * 1024);
            core::hint::black_box(&block);
            Ok("allocated past the cap".into())
        }
        _ => Ok("fine".into()),
    }
}

/// A custom view that counts the events it handled, refuses Left and traps
/// on Right. After Down, Home or End it draws a frame over one of Pane's
/// limits (too many shapes, too long a text, too wide), until the next other
/// event.
struct Counter {
    events: Cell<u32>,
    oversize: Cell<Option<Key>>,
}

impl GuestCustomView for Counter {
    async fn render(&self) -> Frame {
        let text = |content: String| {
            Shape::Text(Text {
                x: 0,
                y: 0,
                content,
                color: 0xffffff,
            })
        };
        let (width, shapes) = match self.oversize.get() {
            Some(Key::Down) => (100, vec![text("x".into()); 4097]),
            Some(Key::Home) => (100, vec![text("x".repeat(257))]),
            Some(Key::End) => (4097, Vec::new()),
            _ => (100, Vec::new()),
        };
        Frame {
            width,
            height: 20,
            shapes,
            value: format!("{} events", self.events.get()),
        }
    }

    async fn handle_event(&self, event: ViewEvent) -> Result<(), String> {
        match event {
            ViewEvent::Key(Key::Left) => Err("the view refused".into()),
            ViewEvent::Key(Key::Right) => panic!("view trap"),
            _ => {
                self.oversize.set(match event {
                    ViewEvent::Key(key @ (Key::Down | Key::Home | Key::End)) => Some(key),
                    _ => None,
                });
                self.events.set(self.events.get() + 1);
                Ok(())
            }
        }
    }
}

impl Command for Faulty {
    type CustomView = Counter;

    async fn render() -> Result<List, String> {
        // A form whose submission is always refused as a whole.
        let form = Form {
            title: "Refused".into(),
            fields: vec![Field {
                id: "text".into(),
                label: "Text".into(),
                kind: FieldKind::Text(TextField { placeholder: None }),
            }],
            submit_label: "Submit".into(),
        };
        Ok(List::new("Faulty").items([
            acting("ok"),
            acting("error"),
            acting("trap"),
            item("form").form(form.clone()),
            // Declares no operating system, so it is unavailable on every
            // system; activating it must not open its form.
            item("nowhere").form(form).platforms([]),
            item("view").custom_view(CustomViewInfo {
                title: "Counter".into(),
                label: "Counter".into(),
                role: CustomViewRole::ColorWell,
            }),
            item("no-view").custom_view(CustomViewInfo {
                title: "Refused view".into(),
                label: "Refused".into(),
                role: CustomViewRole::ColorWell,
            }),
            acting("grow-near-cap"),
            acting("grow-past-cap"),
        ]))
    }

    /// A callback no item names runs as an action of that id, so tests can
    /// run "hold", which is not listed.
    async fn run_search_result(id: String) -> Result<(), String> {
        run(&id).await
    }

    async fn submit_form(_item_id: String, _values: Vec<FieldValue>) -> Result<String, FormError> {
        Err(FormError {
            field: None,
            message: "the guest refused the form".into(),
        })
    }

    async fn open_view(item_id: String) -> Result<CustomView, String> {
        match item_id.as_str() {
            "view" => Ok(CustomView::new(Counter {
                events: Cell::new(0),
                oversize: Cell::new(None),
            })),
            _ => Err("the guest refused the view".into()),
        }
    }
}

pane_extension::root::export!(Faulty);

/// Root results that fail: the query "error" is refused and "trap" traps.
/// The query "0 + 0" is answered slowly, after about a second of busy work,
/// with one result titled "Slow answer". Any other query has no results.
impl pane_extension::root::Guest for Faulty {
    async fn results_for(query: String) -> Result<Vec<pane_extension::root::RootResult>, String> {
        match query.as_str() {
            "error" => Err("the guest refused the query".into()),
            "file link" => Ok(vec![pane_extension::root::RootResult {
                id: "file".into(),
                title: "A local file".into(),
                subtitle: None,
                action: pane_extension::root::RootAction::OpenUrl("file:///etc/hosts".into()),
            }]),
            // Files it names by a path of its own, not an id Pane gave it:
            // Pane must list and open neither.
            "forged file" => Ok(vec![pane_extension::root::RootResult {
                id: "forged".into(),
                title: "hosts".into(),
                subtitle: None,
                action: pane_extension::root::RootAction::OpenFile("/etc/hosts".into()),
            }]),
            // Each file of its granted folder under a harmless title: Pane
            // must show the file's own name instead.
            "spoof" => match pane_extension::files::list_folder()? {
                pane_extension::files::FolderState::Ready(listing) => Ok(listing
                    .files
                    .into_iter()
                    .map(|file| pane_extension::root::RootResult {
                        id: file.relative,
                        title: "harmless.txt".into(),
                        subtitle: Some("File in Documents".into()),
                        action: pane_extension::root::RootAction::OpenFile(file.id),
                    })
                    .collect()),
                _ => Ok(Vec::new()),
            },
            "trap" => panic!("trap requested"),
            "0 + 0" => {
                // About a second of busy work, bounded by the clock rather
                // than a count of steps: the wait must outlast the loading
                // bar's threshold on a fast machine while staying well
                // inside the runtime's unresponsive limit on a slow one
                // (a fixed count stretched past the limit on CI's slower
                // runners, and Pane stopped the call before it answered).
                let now = wasip3::clocks::monotonic_clock::now;
                let end = now() + 1_000_000_000;
                let mut sum = 0u64;
                let mut steps = 0u64;
                while now() < end {
                    for step in 0..1_000_000u64 {
                        sum = sum.wrapping_add(step);
                        steps += 1;
                    }
                    sum = core::hint::black_box(sum);
                }
                Ok(vec![pane_extension::root::RootResult {
                    id: "slow".into(),
                    title: "Slow answer".into(),
                    subtitle: Some(format!("after {steps} steps")),
                    action: pane_extension::root::RootAction::Copy("slow".into()),
                }])
            }
            _ => Ok(Vec::new()),
        }
    }
}
