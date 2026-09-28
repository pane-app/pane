//! Test fixture: a guest whose actions and root results fail in each way the
//! host must report.
#![no_std]

use core::cell::Cell;

use pane_guest::alloc::{format, string::String, vec, vec::Vec};
use pane_guest::{
    CustomView, CustomViewInfo, CustomViewRole, Field, FieldKind, FieldValue, Form, FormError,
    Frame, Guest, GuestCustomView, Item, Key, Shape, Text, TextField, View, ViewEvent,
};

struct Faulty;
pane_guest::export!(Faulty);

fn item(id: &str) -> Item {
    Item {
        id: id.into(),
        title: id.into(),
        subtitle: None,
        form: None,
        platforms: None,
        custom_view: None,
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

impl Guest for Faulty {
    type CustomView = Counter;

    async fn get_view() -> Result<View, String> {
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
        Ok(View {
            title: "Faulty".into(),
            items: vec![
                item("ok"),
                item("error"),
                item("trap"),
                Item {
                    form: Some(form.clone()),
                    ..item("form")
                },
                // Declares no operating system, so it is unavailable on
                // every system; activating it must not open its form.
                Item {
                    form: Some(form),
                    platforms: Some(vec![]),
                    ..item("nowhere")
                },
                Item {
                    custom_view: Some(CustomViewInfo {
                        title: "Counter".into(),
                        label: "Counter".into(),
                        role: CustomViewRole::ColorWell,
                    }),
                    ..item("view")
                },
                Item {
                    custom_view: Some(CustomViewInfo {
                        title: "Refused view".into(),
                        label: "Refused".into(),
                        role: CustomViewRole::ColorWell,
                    }),
                    ..item("no-view")
                },
            ],
        })
    }

    async fn submit_form(_item_id: String, _values: Vec<FieldValue>) -> Result<String, FormError> {
        Err(FormError {
            field: None,
            message: "the guest refused the form".into(),
        })
    }

    async fn run_action(item_id: String) -> Result<String, String> {
        match item_id.as_str() {
            // Holds a stream open to the host (its stdout), with the future of
            // that write pending, saves `holding` as "started", then waits ten
            // seconds before closing them and saving "finished". Tests stop it
            // meanwhile; it is not listed.
            "hold" => {
                let (writer, reader) = wasip3::wit_stream::new::<u8>();
                let written = wasip3::cli::stdout::write_via_stream(reader);
                pane_guest::settings::set("holding", "started")?;
                wasip3::clocks::monotonic_clock::wait_for(10_000_000_000).await;
                drop(writer);
                let _ = written.await;
                pane_guest::settings::set("holding", "finished")?;
                Ok("held".into())
            }
            "error" => Err("the guest refused".into()),
            "trap" => panic!("guest trap"),
            _ => Ok("fine".into()),
        }
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

pane_guest::root::export!(Faulty);

/// Root results that fail: the query "error" is refused and "trap" traps.
/// The query "0 + 0" is answered slowly, after about a second of busy work,
/// with one result titled "Slow answer". Any other query has no results.
impl pane_guest::root::Guest for Faulty {
    async fn results_for(query: String) -> Result<Vec<pane_guest::root::RootResult>, String> {
        match query.as_str() {
            "error" => Err("the guest refused the query".into()),
            "file link" => Ok(vec![pane_guest::root::RootResult {
                id: "file".into(),
                title: "A local file".into(),
                subtitle: None,
                action: pane_guest::root::RootAction::OpenUrl("file:///etc/hosts".into()),
            }]),
            "trap" => panic!("trap requested"),
            "0 + 0" => {
                let mut sum = 0u64;
                for step in 0..1u64 << 32 {
                    sum = core::hint::black_box(sum.wrapping_add(step));
                }
                Ok(vec![pane_guest::root::RootResult {
                    id: "slow".into(),
                    title: "Slow answer".into(),
                    subtitle: Some(format!("after {sum} steps")),
                    action: pane_guest::root::RootAction::Copy("slow".into()),
                }])
            }
            _ => Ok(Vec::new()),
        }
    }
}
