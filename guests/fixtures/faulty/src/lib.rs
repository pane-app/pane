//! Test fixture: a guest whose actions and root results fail in each way the
//! host must report, and whose actions grow its memory to either side of
//! the cap Pane puts on it.
#![no_std]

use core::cell::Cell;

use pane_extension::alloc::{format, string::String, vec, vec::Vec};
use pane_extension::commands::{self, CommandRef, LaunchRecord, LaunchType};
use pane_extension::feedback::{Toast, show_toast};
use pane_extension::view::{
    CanvasEvent, CanvasRole, Container, Cx, Draw, IntoAnswer, Length, Paint, TextStyle, View,
    canvas, column,
};
use pane_extension::{Color, Command, Icon, Item, List};

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

/// A designed view that counts the events it handled, refuses "left" and
/// traps on "right". After "down", "home" or "end" it draws a canvas over
/// one of Pane's limits (too many operations, too long a text, too much
/// inline image data), until the next other key.
struct Counter {
    events: Cell<u32>,
    oversize: Cell<Option<&'static str>>,
    /// Whether the next drawing answers an error, as a refused event.
    refuse: Cell<bool>,
}

impl View for Counter {
    fn render(&mut self, cx: &mut Cx<Self>) -> impl IntoAnswer {
        if self.refuse.replace(false) {
            return Err("the view refused".into());
        }
        Ok(self.tree(cx))
    }
}

impl Counter {
    /// The view's tree: a canvas drawing how many events it handled, or
    /// the canvas over one of Pane's limits the last limit key asked for.
    fn tree(&self, cx: &mut Cx<Self>) -> Container {
        let text = |content: String| {
            Draw::text(content)
                .at(0., 0.)
                .style(TextStyle::Body)
                .color(Paint::Color(Color::hex("#ffffff")))
        };
        let mut ops = Vec::new();
        match self.oversize.get() {
            // Too many operations for one canvas.
            Some("down") => {
                for _ in 0..20_001 {
                    ops.push(Draw::rect(0., 0., 1., 1.));
                }
            }
            // A text operation over the character limit.
            Some("home") => ops.push(text("x".repeat(65_537))),
            // An image operation over the inline data limit.
            Some("end") => ops.push(Draw::image(Icon::url(format!(
                "data:image/png;base64,{}",
                "a".repeat(1_048_577)
            )))),
            _ => ops.push(text(format!("{} events", self.events.get()))),
        }
        column().child(
            canvas()
                .key("counter")
                .width(Length::Px(240.))
                .height(Length::Px(20.))
                .role(CanvasRole::Generic)
                .label("Counter")
                .value(format!("{} events", self.events.get()))
                .on_key(cx.value_listener(|this, key| this.keyed(key)))
                .on_pointer_down(cx.canvas_listener(|this, event| this.pointed(event)))
                .on_pointer_move(cx.canvas_listener(|this, event| this.pointed(event)))
                .on_pointer_up(cx.canvas_listener(|this, event| this.pointed(event)))
                .ops(ops),
        )
    }

    /// A key the canvas is focused for: "left" refuses, "right" traps, and
    /// "down", "home" and "end" draw a canvas over one of Pane's limits.
    fn keyed(&self, key: &str) {
        match key {
            "left" => self.refuse.set(true),
            "right" => panic!("view trap"),
            "down" | "home" | "end" => self.oversize.set(Some(key)),
            _ => {
                self.oversize.set(None);
                self.events.set(self.events.get() + 1);
            }
        }
    }

    /// A pointer event the canvas received: each one counts.
    fn pointed(&self, _event: CanvasEvent) {
        self.oversize.set(None);
        self.events.set(self.events.get() + 1);
    }
}

impl Command for Faulty {
    type DesignedView = Counter;

    async fn render() -> Result<List, String> {
        Ok(List::new("Faulty").items([
            acting("ok"),
            acting("error"),
            acting("trap"),
            // A form item of the typed form's shape, which the tree's
            // reading ignores now that a form is a designed view (#241):
            // activating it runs its action like any item's.
            item("form").on_action(|| async { Err("the guest refused the form".into()) }),
            // Declares no operating system, so it is unavailable on every
            // system; activating it runs nothing.
            item("nowhere").on_action(|| async { Ok(()) }).platforms([]),
            // A designed command of this component, opened with its own
            // manifest by the tests: a canvas counting the events it
            // handled.
            item("counter").on_action(|| async {
                let counter = CommandRef {
                    source: None,
                    command: "counter".into(),
                };
                commands::launch(&counter, LaunchType::UserInitiated, &[], None)
                    .map_err(|problem| format!("the guest refused the view: {problem}"))
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

    async fn open_designed_view(command: String, _launch: LaunchRecord) -> Result<Counter, String> {
        if command != "counter" {
            return Err("the guest refused the view".into());
        }
        Ok(Counter {
            events: Cell::new(0),
            oversize: Cell::new(None),
            refuse: Cell::new(false),
        })
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
                let mut sum = 0u64;
                for step in 0..1u64 << 32 {
                    sum = core::hint::black_box(sum.wrapping_add(step));
                }
                Ok(vec![pane_extension::root::RootResult {
                    id: "slow".into(),
                    title: "Slow answer".into(),
                    subtitle: Some(format!("after {sum} steps")),
                    action: pane_extension::root::RootAction::Copy("slow".into()),
                }])
            }
            _ => Ok(Vec::new()),
        }
    }
}
