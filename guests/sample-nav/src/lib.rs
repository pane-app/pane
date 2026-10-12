//! Pane's navigation sample in Rust: a designed view with a navigation
//! stack (#239) — a list-like view whose rows push a detail view, which
//! itself pushes deeper; the detail pops with a result the rows answer
//! through `onPop`, and the deeper view drops the whole stack through the
//! `window.pop-to-root` host function. The behaviour matches the
//! JavaScript and TypeScript samples (`sample-nav-js`, `sample-nav-ts`):
//! the same texts, the same buttons.
#![no_std]

use pane_extension::alloc::string::ToString as _;
use pane_extension::alloc::{borrow::ToOwned, format, string::String};
use pane_extension::view::{
    Cx, IntoNode, Space, TextLevel, TextStyle, View, button, column, row, text,
};
use pane_extension::{Command, LaunchRecord};

/// The sample's screen: the rows, the detail one of them pushed, or the
/// deeper view the detail pushed.
enum Screen {
    /// The rows, with what the last popped view answered.
    Rows {
        /// The text the last `onPop` answered, when one ran: "Picked: …",
        /// or "Picked: nothing" for a pop that carried no result.
        picked: Option<String>,
    },
    /// The detail of `name`, pushed above the rows.
    Detail { name: String },
    /// A deeper view, pushed above the detail or replacing it.
    Deeper { name: String },
}

impl View for Screen {
    fn render(&mut self, cx: &mut Cx<Self>) -> impl IntoNode {
        match self {
            Screen::Rows { picked } => column()
                .navigation_title("Navigation sample")
                .gap(Space::M)
                .child(
                    text(picked.as_deref().unwrap_or("Nothing picked yet"))
                        .style(TextStyle::Title)
                        .level(TextLevel::Primary),
                )
                .child(
                    row()
                        .gap(Space::S)
                        .children(["One", "Two", "Three"].map(|name| {
                            let name = name.to_owned();
                            button(name.clone()).on_click(cx.push_with(
                                move |_| Screen::Detail { name: name.clone() },
                                |this, result| {
                                    let Screen::Rows { picked } = this else {
                                        return;
                                    };
                                    *picked = Some(match result {
                                        Some(result) => format!("Picked: {result}"),
                                        None => "Picked: nothing".into(),
                                    });
                                },
                            ))
                        })),
                ),
            Screen::Detail { name } => column()
                .navigation_title(name.clone())
                .gap(Space::M)
                .child(
                    text(format!("Detail: {name}"))
                        .style(TextStyle::Title)
                        .level(TextLevel::Primary),
                )
                .child(row().gap(Space::S).children([
                    button("Deeper").on_click(cx.push(|this| deeper_of(this))),
                    button("Swap for deeper").on_click(cx.replace(|this| deeper_of(this))),
                    button("Done").on_click(cx.pop_with(|this| match this {
                        Screen::Detail { name } => format!("done:{name}"),
                        _ => String::new(),
                    })),
                ])),
            Screen::Deeper { name } => column()
                .navigation_title("Deeper")
                .gap(Space::M)
                .child(
                    text(format!("Deeper: {name}"))
                        .style(TextStyle::Title)
                        .level(TextLevel::Primary),
                )
                .child(row().gap(Space::S).children([
                    button("Push another").on_click(cx.push(|this| deeper_of(this))),
                    button("Pop to root").on_click(cx.listener(|_| {
                        pane_extension::window::pop_to_root(false);
                    })),
                ])),
        }
    }
}

/// The deeper view of the view on screen, as a push or a replace opens.
fn deeper_of(on_screen: &Screen) -> Screen {
    match on_screen {
        Screen::Detail { name } | Screen::Deeper { name } => Screen::Deeper { name: name.clone() },
        Screen::Rows { .. } => Screen::Deeper {
            name: String::new(),
        },
    }
}

struct Sample;
pane_extension::export!(Sample);

impl Command for Sample {
    type DesignedView = Screen;

    async fn open_designed_view(_command: String, _launch: LaunchRecord) -> Result<Screen, String> {
        Ok(Screen::Rows { picked: None })
    }
}
