#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use gpui::{
    App, Bounds, Context, IntoElement, Render, Window, WindowBounds, WindowOptions, div,
    prelude::*, px, rgb, size,
};
use serde::Deserialize;
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
};

#[derive(Clone, Deserialize)]
struct Item {
    id: String,
    title: String,
    #[serde(default)]
    subtitle: String,
}

#[derive(Deserialize)]
struct View {
    title: String,
    items: Vec<Item>,
}

struct Probe {
    input: PathBuf,
    events: PathBuf,
    view: View,
    status: String,
}

impl Probe {
    fn reload(&mut self) {
        match fs::read(&self.input)
            .map_err(|e| e.to_string())
            .and_then(|bytes| serde_json::from_slice::<View>(&bytes).map_err(|e| e.to_string()))
        {
            Ok(view) => {
                self.status = format!("Loaded {} rows", view.items.len());
                self.view = view;
            }
            Err(error) => self.status = format!("Reload failed: {error}"),
        }
    }

    fn activate(&mut self, id: &str) {
        let event = serde_json::json!({"type": "activate", "id": id});
        let result = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.events)
            .and_then(|mut file| writeln!(file, "{event}"));
        self.status = match result {
            Ok(()) => format!("Emitted activation: {id}"),
            Err(error) => format!("Event write failed: {error}"),
        };
    }
}

impl Render for Probe {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap_3()
            .p_5()
            .bg(rgb(0x20252d))
            .text_color(rgb(0xf1f3f5))
            .font_family("Segoe UI")
            .child(div().text_xl().child(self.view.title.clone()))
            .child(
                div()
                    .id("reload")
                    .p_2()
                    .bg(rgb(0x364355))
                    .rounded_md()
                    .cursor_pointer()
                    .child("Reload JSON")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.reload();
                        cx.notify();
                    })),
            )
            .children(
                self.view
                    .items
                    .clone()
                    .into_iter()
                    .enumerate()
                    .map(|(index, item)| {
                        div()
                            .id(("row", index))
                            .p_3()
                            .bg(rgb(0x2a3440))
                            .rounded_md()
                            .cursor_pointer()
                            .child(div().child(item.title))
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(rgb(0xaabbcc))
                                    .child(item.subtitle),
                            )
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.activate(&item.id);
                                cx.notify();
                            }))
                    }),
            )
            .child(div().text_sm().child(self.status.clone()))
    }
}

fn main() {
    eprintln!("probe: initializing native platform");
    let mut args = std::env::args_os().skip(1);
    let input = PathBuf::from(args.next().unwrap_or_else(|| "view.json".into()));
    let events = PathBuf::from(args.next().unwrap_or_else(|| "events.jsonl".into()));
    gpui_platform::application().run(move |cx: &mut App| {
        eprintln!("probe: opening native window");
        cx.on_window_closed(|cx, _| {
            cx.quit();
        })
        .detach();
        let bounds = Bounds::centered(None, size(px(620.), px(440.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(gpui::TitlebarOptions {
                    title: Some("Kyoko GPUI view probe".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            move |_, cx| {
                cx.new(|_| {
                    let mut probe = Probe {
                        input,
                        events,
                        view: View {
                            title: "View probe".into(),
                            items: vec![],
                        },
                        status: String::new(),
                    };
                    probe.reload();
                    probe
                })
            },
        )
        .expect("open GPUI window");
        eprintln!("probe: native window open");
        cx.activate(true);
    });
}
