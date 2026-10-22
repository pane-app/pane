//! Pane's designed view sample in Rust: a counter whose screen is the
//! tree its `view.render` answers — a column of a text and a row of
//! buttons, drawn by Pane, not by the guest (`docs/designed-tree.md`,
//! ADR 0036). The behaviour matches the JavaScript and TypeScript samples
//! (`sample-view-js`, `sample-view-ts`): the same text, the same buttons.
//!
//! The `components` command of the same package answers a gallery of
//! every component of the UI component set (#237): the layout
//! primitives, the shared components, the tokens and the raw values,
//! with a toggle to show the tree changes. Its fields hold state (#238):
//! typing edits at once, the view echoes the value back, "Clear" sets it
//! (the extension's value wins), and "Reorder" moves the keyed fields
//! around, their state with them. And its `loading` command answers a
//! view that loads something on open (#243): a loading state drawn at
//! once, and what the load answered the moment it arrives — the arrival
//! asks for the drawing itself, with no timer to wait for.
#![no_std]

use core::cell::{Cell, RefCell};
use core::future::Future;

use pane_extension::alloc::{borrow::ToOwned, format, string::String};
use pane_extension::icon::Accessory;
use pane_extension::icon::Tone as Colour;
use pane_extension::view::{
    Color, Cx, DropdownItem, Fit, Icon, IconSize, IntoAnswer, IntoNode, Length, ListAction, Paint,
    Pending, Place, Radius, Space, TextLevel, TextStyle, Tone, View, badge, button, card, cell,
    checkbox, choice, column, detail, divider, dropdown, empty_state, grid, icon, icon_tile, image,
    item, key_sequence, keycap, link, list, loading, markdown, metadata, metadata_list,
    metadata_separator, metadata_tags, password_input, progress, rich_row, row, scroll, section,
    section_header, segmented, select, slider, spacer, span, spans, stack, tag, text, text_area,
    text_input, toggle,
};
use pane_extension::{Command, LaunchRecord};

/// The screen a command opens: the counter, the gallery of components, or
/// the loading sample.
struct Screen {
    /// Which of the package's screens this view is.
    which: Which,
    count: Cell<u32>,
    on: Cell<bool>,
    /// The gallery's fields' values, which the view echoes back (#238):
    /// typing edits at once, and a value the view sets replaces the text.
    name: RefCell<String>,
    notes: RefCell<String>,
    /// Whether the gallery's fields are swapped: "Reorder" flips it,
    /// moving the keyed fields around with their state.
    swapped: Cell<bool>,
    /// The loading sample's data, on its way when the view opens
    /// (`Pending`, #243).
    loading: Pending<String>,
    /// The List sample's state (#240): whether its loading bar is on
    /// (its first item's action toggles it), how many pages of its "More"
    /// section are shown (a load-more appends one), and which of the
    /// dropdown's choices filters it.
    list_loading: Cell<bool>,
    pages: Cell<usize>,
    pinned: Cell<bool>,
}

/// Which of the package's screens a view is.
enum Which {
    Counter,
    Components,
    Loading,
    /// The standard views (#240): a List, a Grid and a Detail.
    List,
    Grid,
    Detail,
}

impl View for Screen {
    fn render(&mut self, cx: &mut Cx<Self>) -> impl IntoAnswer {
        // The loading sample: the loading state, then what the load
        // answered, drawn the moment it arrives.
        if matches!(self.which, Which::Loading) {
            return match self.loading.ready() {
                Some(what) => column()
                    .gap(Space::M)
                    .child(text("Loaded").style(TextStyle::Title))
                    .child(text(what.as_str()).level(TextLevel::Secondary))
                    .into_answer(),
                None => text("Loading…").level(TextLevel::Secondary).into_answer(),
            };
        }
        if matches!(self.which, Which::List) {
            return self.list_tree(cx).into_answer();
        }
        if matches!(self.which, Which::Grid) {
            return self.grid_tree(cx).into_answer();
        }
        if matches!(self.which, Which::Detail) {
            return self.detail_tree(cx).into_answer();
        }
        if matches!(self.which, Which::Counter) {
            return column()
                .gap(Space::M)
                .child(
                    text(format!("Count: {}", self.count.get()))
                        .style(TextStyle::Title)
                        .level(TextLevel::Primary),
                )
                .child(
                    row().gap(Space::S).children([
                        button("Increment").on_click(cx.listener(|this: &mut Self| {
                            this.count.set(this.count.get() + 1);
                        })),
                        button("Decrement").on_click(cx.listener(|this: &mut Self| {
                            this.count.set(this.count.get().saturating_sub(1));
                        })),
                        button("Reset")
                            .tone(Tone::Destructive)
                            .on_click(cx.listener(|this: &mut Self| this.count.set(0))),
                    ]),
                )
                .into_answer();
        }
        scroll()
            .key("gallery")
            .grow(1.)
            .child(
                column()
                    .gap(Space::L)
                    .child(
                        text("The UI component set")
                            .style(TextStyle::Heading)
                            .level(TextLevel::Primary),
                    )
                    .child(
                        // A stack: a badge over an icon tile, placed.
                        stack()
                            .align(Place::TopEnd)
                            .child(icon_tile(Icon::builtin("layers")))
                            .child(
                                badge("4")
                                    .place(Place::BottomEnd)
                                    .offset(Length::Px(4.), Length::Px(4.)),
                            ),
                    )
                    .child(
                        // A text with spans, one a link.
                        spans([
                            span("Accept the "),
                            span("terms").on_click(cx.listener(|_: &mut Self| {})),
                            span(" before continuing.").code(),
                        ]),
                    )
                    .child(
                        row()
                            .gap(Space::S)
                            .children([
                                icon(Icon::builtin("star")).size(IconSize::L).into_node(),
                                // A raw blue, corrected for contrast.
                                icon(Icon::builtin("bell").tint(Color::Raw("#88ccff".into())))
                                    .into_node(),
                                keycap("ctrl").into_node(),
                                key_sequence(["ctrl", "shift", "p"]).into_node(),
                                tag("beta").color(Colour::Blue).into_node(),
                                badge("3").into_node(),
                            ])
                            .name("Marks"),
                    )
                    .child(
                        card().gap(Space::S).child(
                            rich_row("Pane")
                                .subtitle("A tree Pane renders")
                                .icon(Icon::builtin("layers"))
                                .tag("new")
                                .on_click(cx.listener(|_: &mut Self| {})),
                        ),
                    )
                    .child(
                        // Controls, one of them live.
                        column()
                            .gap(Space::S)
                            .child(section_header("Controls").note("Every one focusable"))
                            .child(
                                toggle(self.on.get())
                                    .label("Dark mode")
                                    .on_click(cx.listener(|this: &mut Self| {
                                        this.on.set(!this.on.get());
                                    })),
                            )
                            .child(
                                checkbox(self.on.get())
                                    .label("Remember")
                                    .on_click(cx.listener(|this: &mut Self| {
                                        this.on.set(!this.on.get());
                                    })),
                            )
                            .child(
                                segmented([
                                    choice("daily").label("Daily"),
                                    choice("weekly").label("Weekly"),
                                ])
                                .value("daily")
                                .label("Digest")
                                .on_click(cx.listener(|_: &mut Self| {})),
                            )
                            .child(
                                select([
                                    choice("daily").label("Daily"),
                                    choice("weekly").label("Weekly"),
                                ])
                                .value("daily")
                                .label("Pick")
                                .on_click(cx.listener(|_: &mut Self| {})),
                            )
                            .child(
                                slider(0.4)
                                    .label("Volume")
                                    .on_click(cx.listener(|_: &mut Self| {})),
                            )
                            .child(progress(0.7).label("Installed"))
                            .child(loading().label("Checking")),
                    )
                    .child(self.fields(cx))
                    .child(password_input().label("Secret"))
                    .child(
                        // Markdown.
                        markdown(concat!(
                            "# Markdown\n\n",
                            "Some *prose*, `code` and [a link](https://pane.dev).\n\n",
                            "- [x] drawn\n- [ ] still to do\n"
                        )),
                    )
                    .child(metadata_list([
                        metadata("Author", "Vu").on_click(cx.listener(|_: &mut Self| {})),
                        metadata_tags("Tags", ["one", "two"]),
                        metadata_separator(),
                        metadata("Kind", "sample"),
                    ]))
                    .child(
                        empty_state("Nothing here")
                            .description("The gallery is over")
                            .icon(Icon::builtin("search-minus"))
                            .action(button("Start over").on_click(cx.listener(|_: &mut Self| {}))),
                    )
                    .child(
                        // An image, with a placeholder while it stands in.
                        image(Icon::builtin("image"))
                            .size(IconSize::Xl)
                            .fit(Fit::Cover)
                            .placeholder(text("Loading…").level(TextLevel::Tertiary)),
                    )
                    .child(
                        // Raw values: a surface with a variant, a tone, a
                        // corrected colour and an exact one.
                        row().gap(Space::S).children([
                            text("Surface")
                                .level(TextLevel::Secondary)
                                .background(Paint::Color(Color::Tone(Colour::Danger)))
                                .radius(Radius::M)
                                .hover(
                                    pane_extension::view::Surface::default()
                                        .background(Paint::Color(Color::Tone(Colour::Accent))),
                                )
                                .into_node(),
                            link("A link")
                                .on_click(cx.listener(|_: &mut Self| {}))
                                .into_node(),
                        ]),
                    )
                    .child(row().gap(Space::S).children([
                        text("Corrected").color(Color::Raw("#88ccff".into())),
                        text("Exact").color(Paint::Exact(Color::Raw("#ff6363".into()))),
                    ]))
                    .child(divider())
                    .child(spacer()),
            )
            .into_answer()
    }
}

impl Screen {
    /// A screen opening: the counter, or the gallery.
    fn opening(which: Which) -> Screen {
        Screen {
            which,
            count: Cell::new(0),
            on: Cell::new(false),
            name: RefCell::new("typed".into()),
            notes: RefCell::new("two lines".into()),
            swapped: Cell::new(false),
            loading: Pending::loading(async { String::new() }),
            list_loading: Cell::new(false),
            pages: Cell::new(0),
            pinned: Cell::new(false),
        }
    }

    /// The gallery's fields, live and keyed (#238): the name field hears
    /// its value as the user types (the view echoing it back, which never
    /// fights the typing), the notes field on its commits, "Clear" sets
    /// both (the extension's value replacing the text), and "Reorder"
    /// swaps the two fields, whose keys keep their state.
    fn fields(&self, cx: &mut Cx<Self>) -> impl IntoNode {
        let name = text_input(self.name.borrow().clone())
            .key("name")
            .placeholder("Type here")
            .label("Name")
            .on_input(cx.value_listener(|this: &mut Self, value| {
                *this.name.borrow_mut() = value.to_owned();
            }));
        let notes = text_area(self.notes.borrow().clone())
            .key("notes")
            .label("Notes")
            .on_change(cx.value_listener(|this: &mut Self, value| {
                *this.notes.borrow_mut() = value.to_owned();
            }));
        // "Reorder" swaps the fields' places, not their state: the keys
        // keep each field's text, caret and focus.
        let fields = if self.swapped.get() {
            [notes, name]
        } else {
            [name, notes]
        };
        column()
            .gap(Space::S)
            .child(section_header("Fields").note("Live, keyed"))
            .children(fields)
            .child(row().gap(Space::S).children([
                button("Clear").on_click(cx.listener(|this: &mut Self| {
                    *this.name.borrow_mut() = String::new();
                    *this.notes.borrow_mut() = String::new();
                })),
                button("Reorder").on_click(cx.listener(|this: &mut Self| {
                    this.swapped.set(!this.swapped.get());
                })),
            ]))
            .child(text(format!("Echo: {}", self.name.borrow().as_str())))
    }
}

impl Screen {
    /// The List sample's tree (#240): sections, keywords, accessories,
    /// a host-filtered search field, the empty view, the detail pane
    /// (built for the selected item, as the render context names it), the
    /// search-bar dropdown, the loading bar and pagination. The first
    /// item's action toggles the loading bar, so its 300 ms threshold can
    /// be seen.
    fn list_tree(&mut self, cx: &mut Cx<Self>) -> impl IntoAnswer {
        // The detail pane's content: only the selected item's is built,
        // as the context's `selected` names it — the lazy detail.
        let selected = cx.selected().unwrap_or_default().to_owned();
        let detail = |key: &str, title: &str| {
            if key != selected {
                return None;
            }
            Some(
                detail().child(
                    markdown(format!(
                        "# {title}\n\n*Selected:* {key} \u{2014} the pane's content is \
                         built for it alone.\n\n- [x] The render context names it\n- [ ] No \
                         other item's detail is built\n\n| Field | Value |\n| --- | --- |\n|                          key | {key} |\n| title | {title} |\n\n![Pane](data:image/svg+xml,                         %3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 12'%3E%3C                         rect width='24' height='12' rx='3' fill='%233d5afe'/%3E%3C/svg%3E =48x24 \"Pane's mark\")\n\nSee [the tree](https://example.com/tree)."
                    ))
                    .into_node(),
                ),
            )
        };
        let note = |cx: &mut Cx<Self>,
                    key: &str,
                    title: &str,
                    subtitle: &str,
                    keyword: &str,
                    tag: &str,
                    colour: Colour| {
            let mut held = item(key)
                .title(title)
                .subtitle(subtitle)
                .icon(Icon::builtin("document"))
                .keyword(keyword)
                .accessory(Accessory::tag(tag).color(colour))
                .accessory(Accessory::text("1"))
                .on_press(cx.listener(move |this: &mut Self| {
                    // The first item's press toggles the loading bar.
                    this.list_loading.set(!this.list_loading.get());
                }))
                .action(ListAction::new("Copy", cx.listener(|_: &mut Self| {})));
            if let Some(detail) = detail(key, title) {
                held = held.detail(detail);
            }
            held
        };
        let mut list = list()
            .search_placeholder("Search notes…")
            .is_loading(self.list_loading.get())
            .is_showing_detail(true)
            .has_more(self.pages.get() < 3)
            .page_size(6)
            .on_load_more(cx.listener(|this: &mut Self| {
                this.pages.set(this.pages.get() + 1);
            }))
            .child(
                dropdown()
                    .value(if self.pinned.get() { "pinned" } else { "all" })
                    .on_change(cx.value_listener(|this: &mut Self, value: &str| {
                        this.pinned.set(value == "pinned");
                    }))
                    .item(DropdownItem::new("all").label("All notes"))
                    .item(DropdownItem::new("pinned").label("Pinned")),
            )
            .child(
                section("Notes")
                    .subtitle("Three notes")
                    .child(note(
                        cx,
                        "first",
                        "First note",
                        "The first of the notes",
                        "opening",
                        "new",
                        Colour::Green,
                    ))
                    .child(note(
                        cx,
                        "second",
                        "Second note",
                        "The second of the notes",
                        "middle",
                        "kept",
                        Colour::Blue,
                    ))
                    .child(note(
                        cx,
                        "third",
                        "Third note",
                        "The third of the notes",
                        "closing",
                        "done",
                        Colour::Red,
                    )),
            );
        if !self.pinned.get() {
            list = list.child(section("More").subtitle("A page at a time").children(
                (0..self.pages.get() * 6).map(|at| {
                    item(format!("more-{at}"))
                        .title(format!("More {at}"))
                        .subtitle("One page of a longer list")
                        .keyword("page")
                        .on_press(cx.listener(|_: &mut Self| {}))
                }),
            ));
        } else {
            list = list.child(section("Pinned").child(note(
                cx,
                "pinned",
                "Pinned note",
                "The one that is pinned",
                "kept",
                "pinned",
                Colour::Yellow,
            )));
        }
        list.child(
            empty_state("No notes")
                .description("Nothing matches the search.")
                .child(button("Clear the search").on_click(cx.listener(|_: &mut Self| {}))),
        )
        .navigation_title("Notes")
        .into_answer()
    }

    /// The Grid sample's tree (#240): cells of images and colours in two
    /// sections, each with its own columns, aspect ratio, fit and inset.
    fn grid_tree(&mut self, cx: &mut Cx<Self>) -> impl IntoAnswer {
        grid()
            .search_placeholder("Search cells…")
            .child(
                section("Warm")
                    .columns(3)
                    .aspect_ratio(2.)
                    .fit(Fit::Cover)
                    .children([
                        cell()
                            .key("warm-0")
                            .title("Amber")
                            .subtitle("A colour cell")
                            .color(Paint::from(Color::hex("#ffb300")))
                            .on_press(cx.listener(|_: &mut Self| {})),
                        cell()
                            .key("warm-1")
                            .title("Coral")
                            .subtitle("A colour cell")
                            .color(Paint::from(Color::hex("#ff7043")))
                            .on_press(cx.listener(|_: &mut Self| {})),
                        cell()
                            .key("warm-2")
                            .title("Document")
                            .subtitle("An image cell")
                            .image(Icon::builtin("document"))
                            .on_press(cx.listener(|_: &mut Self| {})),
                    ]),
            )
            .child(
                section("Cool")
                    .columns(4)
                    .aspect_ratio(1.)
                    .inset(true)
                    .children([
                        cell()
                            .key("cool-0")
                            .title("Indigo")
                            .subtitle("A colour cell")
                            .color(Paint::from(Color::hex("#3d5afe")))
                            .on_press(cx.listener(|_: &mut Self| {})),
                        cell()
                            .key("cool-1")
                            .title("Teal")
                            .subtitle("A colour cell")
                            .color(Paint::from(Color::hex("#00897b")))
                            .on_press(cx.listener(|_: &mut Self| {})),
                        cell()
                            .key("cool-2")
                            .title("Star")
                            .subtitle("An image cell")
                            .image(Icon::builtin("star"))
                            .on_press(cx.listener(|_: &mut Self| {})),
                        cell()
                            .key("cool-3")
                            .title("Typed")
                            .subtitle("A subtree cell")
                            .child(text("A cell of the author's own").level(TextLevel::Secondary))
                            .on_press(cx.listener(|_: &mut Self| {})),
                    ]),
            )
            .navigation_title("Cells")
            .into_answer()
    }

    /// The Detail sample's tree (#240): Markdown with an image, a task
    /// list and a table, a metadata panel and actions.
    fn detail_tree(&mut self, cx: &mut Cx<Self>) -> impl IntoAnswer {
        detail()
            .child(
                markdown(
                    "# Pane\n\nA screen an extension designs as a **tree** Pane renders.\n\n\
                     ![Pane](data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg'                      viewBox='0 0 24 12'%3E%3Crect width='24' height='12' rx='3' fill='%23                     3d5afe'/%3E%3C/svg%3E =48x24 \"Pane's mark\")\n\n- [x] Drawn by Pane\n- [ ]                      Drawn by the extension\n\n| Field | Value |\n| --- | --- |\n| version | 0.1 \
                     |\n| renderer | GPUI |\n\nSee [the tree](https://example.com/tree).",
                ),
            )
            .child(
                metadata_list([
                    metadata("Version", "0.1.0"),
                    metadata_tags("Tags", ["designed", "tree"]),
                    metadata_separator(),
                    metadata("Repository", "pane")
                        .on_click(cx.listener(|_: &mut Self| {})),
                ]),
            )
            .child(
                row()
                    .gap(Space::S)
                    .children([button("Done").on_click(cx.listener(|_: &mut Self| {}))]),
            )
            .navigation_title("About Pane")
            .into_answer()
    }
}

struct Sample;
pane_extension::export!(Sample);

impl Command for Sample {
    type DesignedView = Screen;

    async fn open_designed_view(command: String, _launch: LaunchRecord) -> Result<Screen, String> {
        let which = match command.as_str() {
            "sample" => Which::Counter,
            "components" => Which::Components,
            "loading" => Which::Loading,
            "list" => Which::List,
            "grid" => Which::Grid,
            "detail" => Which::Detail,
            _ => return Err("this command opens no designed view".into()),
        };
        let loading = matches!(which, Which::Loading);
        let mut screen = Screen::opening(which);
        if loading {
            screen.loading = Pending::loading(load());
        }
        Ok(screen)
    }
}

/// What the loading sample loads: held back for a moment, as work from a
/// service would be, so the loading state shows once.
fn load() -> impl Future<Output = String> {
    async {
        wasip3::clocks::monotonic_clock::wait_for(300_000_000).await;
        String::from("Pane drew this the moment it arrived")
    }
}
