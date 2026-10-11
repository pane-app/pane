//! Pane's Rust sample command: a list with one action per item, a form, a
//! color picker the command draws itself as a canvas of its designed view,
//! two actions each declared for some operating systems only, and a root
//! result computed from the query ("reverse <text>"). Items, titles,
//! results, errors and drawings match the JavaScript and TypeScript
//! samples.
#![no_std]

use core::cell::{Cell, RefCell};

use pane_extension::alloc::{borrow::ToOwned, format, string::String, vec, vec::Vec};
use pane_extension::commands::{self, CommandRef, LaunchRecord, LaunchType};
use pane_extension::feedback::{Toast, show_toast};
use pane_extension::form;
use pane_extension::root::{RootAction, RootResult};
use pane_extension::view::{
    Answer, CanvasEvent, CanvasRole, Cx, Draw, IntoAnswer, IntoNode, Length, Paint, TextMeasure,
    TextStyle, View, canvas, column, measure_text,
};
use pane_extension::{Color, Command, Item, List, Platform, choice};

struct Sample;
pane_extension::export!(Sample);

/// Settings the "validate" item checks; the port is out of range on purpose.
struct Settings {
    name: &'static str,
    port: u32,
}

impl Settings {
    fn validate(&self) -> Result<(), &'static str> {
        if self.name.is_empty() {
            return Err("name must not be empty");
        }
        if !(1..=65535).contains(&self.port) {
            return Err("port must be between 1 and 65535");
        }
        Ok(())
    }
}

/// The greeting form's options: (id, label).
const GREETINGS: [(&str, &str); 3] = [
    ("hello", "Hello"),
    ("morning", "Good morning"),
    ("welcome", "Welcome"),
];

/// The form command's view (#241): the greeting form — every field the
/// designed tree's forms offer, so the sample shows each kind. A
/// submission validates the name and answers with the greeting; the
/// fields' values arrive with the submission, keyed by their keys, and
/// the name's is remembered: Pane keeps it as the package's settings and
/// prefills it the next time the form opens.
struct Greeting {
    /// The name the last submission answered with, shown as the answer.
    answered: RefCell<String>,
    /// Why the last submission was refused, on the name's field.
    error: RefCell<Option<&'static str>>,
}

impl Greeting {
    /// The form's tree, as the command's view renders it: the listeners the
    /// tree names run with the command's view, whichever command opened it.
    fn render(&self, cx: &mut Cx<Designed>) -> Answer {
        let submit = cx.form_listener(|view, values| {
            let Designed::Form(Greeting { answered, error }) = view else {
                return;
            };
            let name = values.text("name").unwrap_or_default().trim().to_owned();
            let greeting = values
                .text("greeting")
                .and_then(|value| {
                    GREETINGS
                        .iter()
                        .find(|&&(id, _)| id == value)
                        .map(|&(_, label)| label)
                })
                .unwrap_or("Hello");
            if name.is_empty() {
                *error.borrow_mut() = Some("Enter a name");
                return;
            }
            if name.chars().count() > 40 {
                *error.borrow_mut() = Some("Use at most 40 characters");
                return;
            }
            *error.borrow_mut() = None;
            *answered.borrow_mut() = format!("{greeting}, {name}, from the Rust guest");
        });
        let error = self.error.borrow().clone();
        let answered = self.answered.borrow().clone();
        let choice = |value: &str, label: &str| choice(value).label(label);
        // Every field is a builder of its own type, so the list holds the
        // nodes they build.
        let fields = || {
            vec![
                form::text_field("name")
                    .title("Name")
                    .placeholder("Ada Lovelace")
                    .remember()
                    .error(error.clone().map(str::to_owned).unwrap_or_default())
                    .auto_focus()
                    .into_node(),
                form::password_field("secret").title("Secret").into_node(),
                form::text_area("notes").title("Notes").into_node(),
                form::date_picker("day").title("Day").into_node(),
                form::date_time_picker("at").title("At").into_node(),
                form::dropdown("greeting")
                    .title("Greeting")
                    .options([
                        choice("hello", "Hello").section("Plain"),
                        choice("morning", "Good morning").section("Warm"),
                        choice("welcome", "Welcome").section("Warm"),
                    ])
                    .default_value("hello")
                    .into_node(),
                form::tag_picker("tags")
                    .title("Tags")
                    .options([
                        choice("friend", "Friend"),
                        choice("colleague", "Colleague"),
                        choice("family", "Family"),
                    ])
                    .into_node(),
                form::file_picker("file").title("File").into_node(),
                form::folder_picker("folder")
                    .title("Folder")
                    .allow_multiple()
                    .into_node(),
                form::checkbox("updates").label("Send updates").into_node(),
                form::toggle("quiet").label("Quiet mode").into_node(),
            ]
        };
        let mut view = form::Form::new()
            .key("form")
            .navigation_title("Greet someone")
            .submit_title("Greet")
            .on_submit(submit);
        // The answer draws over the form as its first child, the form
        // staying the tree's root: the window keeps a field's state by
        // the path of its key, and wrapping the form in a column would
        // move every field's path — losing the text, the caret and the
        // focus the user left in them (#238's rule).
        if !answered.is_empty() {
            view = view.child(
                pane_extension::view::text(answered.as_str())
                    .style(pane_extension::view::TextStyle::Title)
                    .into_node(),
            );
        }
        for field in fields() {
            view = view.child(field);
        }
        view.into_answer()
    }
}

/// The color picker's colors, (hue, [light, medium, dark]): a column of
/// swatches per hue, one row per shade.
const COLORS: [(&str, [u32; 3]); 8] = [
    ("Red", [0xef9a9a, 0xe53935, 0xb71c1c]),
    ("Orange", [0xffcc80, 0xfb8c00, 0xe65100]),
    ("Yellow", [0xfff59d, 0xfdd835, 0xf57f17]),
    ("Green", [0xa5d6a7, 0x43a047, 0x1b5e20]),
    ("Teal", [0x80cbc4, 0x00897b, 0x004d40]),
    ("Blue", [0x90caf9, 0x1e88e5, 0x0d47a1]),
    ("Purple", [0xce93d8, 0x8e24aa, 0x4a148c]),
    ("Pink", [0xf48fb1, 0xd81b60, 0x880e4f]),
];
const SHADES: [&str; 3] = ["Light", "", "Dark"];
/// A swatch's size, and the distance from one swatch to the next.
const SWATCH: u32 = 32;
const STEP: i32 = 36;
const COLUMNS: i32 = COLORS.len() as i32;
const ROWS: i32 = SHADES.len() as i32;

/// An open color picker: a grid of swatches and a preview of the chosen
/// color, drawn as the canvas of its designed view. Arrow keys, Home and
/// End move the choice; pressing or dragging the pointer over the grid
/// chooses the swatch under it. Pane opens one per opened view (the
/// "color" command) and drops it when the view closes.
struct ColorPicker {
    column: Cell<i32>,
    row: Cell<i32>,
    dragging: Cell<bool>,
}

impl ColorPicker {
    fn new() -> ColorPicker {
        // Blue.
        ColorPicker {
            column: Cell::new(5),
            row: Cell::new(1),
            dragging: Cell::new(false),
        }
    }

    /// Chooses the swatch nearest to `x`, `y`.
    fn choose(&self, x: f32, y: f32) {
        self.column
            .set((x as i32).div_euclid(STEP).clamp(0, COLUMNS - 1));
        self.row.set((y as i32).div_euclid(STEP).clamp(0, ROWS - 1));
    }

    /// Where the choice now is.
    fn at(&self) -> (i32, i32) {
        (self.column.get(), self.row.get())
    }

    /// Moves the choice by `columns` and `rows`, clamped at the edges.
    fn moved(&self, columns: i32, rows: i32) {
        let (column, row) = self.at();
        self.column.set((column + columns).clamp(0, COLUMNS - 1));
        self.row.set((row + rows).clamp(0, ROWS - 1));
    }
}

/// The frame colour the chosen swatch and the hex code are drawn in.
fn frame() -> Paint {
    Paint::Color(Color::hex("#f1f3f5"))
}

/// A swatch's own colour, drawn exactly as the sample authored it: the
/// smoke checks the pixels of these.
fn swatch(fill: u32) -> Paint {
    Paint::Exact(Color::hex(hex(fill)))
}

/// "#RRGGBB" for 0xRRGGBB.
fn hex(rgb: u32) -> String {
    format!("#{rgb:06X}")
}

impl ColorPicker {
    /// The picker's tree, as the command's view renders it.
    fn render(&mut self, cx: &mut Cx<Designed>) -> Answer {
        let (column, row) = self.at();
        let (hue, shades) = COLORS[column as usize];
        let chosen = shades[row as usize];
        // A light frame around the chosen swatch, then the swatches, the
        // preview and its hex code. The hex code is measured
        // (`pane:extension/view.measure-text`), so the text drawn under
        // the preview fits what it says.
        let code = hex(chosen);
        let measured = measure_text(
            &code,
            TextMeasure {
                style: Some(TextStyle::Caption),
                ..TextMeasure::default()
            },
        );
        let mut ops = vec![
            Draw::rect(
                (column * STEP) as f32,
                (row * STEP) as f32,
                STEP as f32,
                STEP as f32,
            )
            .filled(frame()),
        ];
        for (x, (_, column)) in (0..).zip(COLORS) {
            for (y, fill) in (0..).zip(column) {
                ops.push(
                    Draw::rect(
                        (x * STEP + 2) as f32,
                        (y * STEP + 2) as f32,
                        SWATCH as f32,
                        SWATCH as f32,
                    )
                    .filled(swatch(fill)),
                );
            }
        }
        ops.push(Draw::rect((COLUMNS * STEP + 12) as f32, 2., 64., 64.).filled(swatch(chosen)));
        ops.push(
            Draw::text(code)
                .at((COLUMNS * STEP + 12) as f32, 68. + measured.1)
                .style(TextStyle::Caption)
                .color(frame()),
        );
        let name = match SHADES[row as usize] {
            "" => String::from(hue),
            shade => format!("{shade} {}", hue.to_lowercase()),
        };
        pane_extension::view::column()
            .key("picker")
            .navigation_title("Choose a color")
            .child(
                canvas()
                    .key("grid")
                    .width(Length::Px((COLUMNS * STEP + 88) as f32))
                    .height(Length::Px((ROWS * STEP) as f32))
                    .role(CanvasRole::ColorWell)
                    .label("Color")
                    .value(format!("{name}, {}", hex(chosen)))
                    .on_key(cx.value_listener(|view, key| {
                        if let Designed::Color(picker) = view {
                            picker.keyed(key);
                        }
                    }))
                    .on_pointer_down(cx.canvas_listener(|view, event| {
                        if let Designed::Color(picker) = view {
                            picker.pointed(event);
                        }
                    }))
                    .on_pointer_move(cx.canvas_listener(|view, event| {
                        if let Designed::Color(picker) = view {
                            picker.pointed(event);
                        }
                    }))
                    .on_pointer_up(cx.canvas_listener(|view, _| {
                        if let Designed::Color(picker) = view {
                            picker.dragging.set(false);
                        }
                    }))
                    .ops(ops),
            )
            .into_answer()
    }
}

impl ColorPicker {
    /// A key the canvas is focused for: the arrows, Home and End move the
    /// choice.
    fn keyed(&self, key: &str) {
        match key {
            "left" => self.moved(-1, 0),
            "right" => self.moved(1, 0),
            "up" => self.moved(0, -1),
            "down" => self.moved(0, 1),
            "home" => {
                self.column.set(0);
            }
            "end" => {
                self.column.set(COLUMNS - 1);
            }
            _ => {}
        }
    }

    /// A pointer event the canvas is dragged with: a press on the grid
    /// chooses a swatch and starts the drag; a move during one chooses
    /// under the pointer.
    fn pointed(&self, event: CanvasEvent) {
        let (x, y, pressed) = match event {
            CanvasEvent::PointerDown { x, y, .. } => (x, y, true),
            CanvasEvent::PointerMove { x, y } => (x, y, false),
            _ => return,
        };
        let on_grid =
            (0..COLUMNS * STEP).contains(&(x as i32)) && (0..ROWS * STEP).contains(&(y as i32));
        if pressed {
            // Only a press on the grid chooses a swatch and starts a drag.
            if on_grid {
                self.dragging.set(true);
                self.choose(x, y);
            }
        } else if self.dragging.get() {
            self.choose(x, y);
        }
    }
}

/// The view either designed command of this package opens: the greeting
/// form's, or the color picker's, whichever command Pane launched
/// ([`Command::open_designed_view`] hands one of these to Pane).
enum Designed {
    Form(Greeting),
    Color(ColorPicker),
}

impl View for Designed {
    fn render(&mut self, cx: &mut Cx<Self>) -> impl IntoAnswer {
        match self {
            Designed::Form(form) => form.render(cx),
            Designed::Color(picker) => picker.render(cx),
        }
    }
}

/// Runs the action of the item `id` and shows a toast with what [`outcome`]
/// answers; each item's action is this with its id. The "Choose a color"
/// item launches the command of its designed view instead.
async fn act(id: &str) -> Result<(), String> {
    if id == "color" {
        let color = CommandRef {
            source: None,
            command: "color".into(),
        };
        return commands::launch(&color, LaunchType::UserInitiated, &[], None)
            .map_err(|problem| format!("could not open the color picker: {problem}"));
    }
    let done = outcome(id).await?;
    show_toast(Toast::success(done));
    Ok(())
}

/// What the action of the item `id` does, answering what it found.
async fn outcome(id: &str) -> Result<String, String> {
    match id {
        "greet" => Ok("Hello from the Rust guest".into()),
        "wait" => {
            // A native component-model async import; the guest suspends here.
            wasip3::clocks::monotonic_clock::wait_for(50_000_000).await;
            Ok("Waited 50 ms inside the Rust guest".into())
        }
        "validate" => {
            let settings = Settings {
                name: "Pane",
                port: 70000,
            };
            settings
                .validate()
                .map_err(|problem| format!("Invalid settings: {problem}"))?;
            Ok(format!(
                "Settings are valid: {} on port {}",
                settings.name, settings.port
            ))
        }
        "random" => {
            // A number in [0, 1) from 53 random bits, like `Math.random()`.
            let bits = wasip3::random::random::get_random_u64() >> 11;
            Ok(format!("{}", bits as f64 / (1u64 << 53) as f64))
        }
        "windows-only" => Ok("Ran the Windows-only action in the Rust guest".into()),
        "not-windows" => Ok("Ran the macOS and Linux action in the Rust guest".into()),
        other => Err(format!("unknown item: {other}")),
    }
}

impl Command for Sample {
    type DesignedView = Designed;

    async fn render() -> Result<List, String> {
        let item =
            |id: &'static str, title: &str, subtitle: &str| Item::new(id, title).subtitle(subtitle);
        let acting = |id: &'static str, title: &str, subtitle: &str| {
            item(id, title, subtitle).on_action(move || act(id))
        };
        Ok(List::new("Rust sample").items([
            acting("greet", "Say hello", "Answer from the Rust guest"),
            acting(
                "wait",
                "Wait briefly",
                "Await a WASI 0.3 clock, then answer",
            ),
            acting(
                "validate",
                "Validate settings",
                "Reject settings with an out-of-range port",
            ),
            acting(
                "random",
                "Roll a number",
                "A random number from this instance",
            ),
            item(
                "form",
                "Greet someone",
                "A form the guest checks, on the designed tree",
            )
            .on_action(|| async {
                let form = CommandRef {
                    source: None,
                    command: "form".into(),
                };
                commands::launch(&form, LaunchType::UserInitiated, &[], None)
                    .map_err(|problem| format!("could not open the form: {problem}"))
            }),
            acting(
                "color",
                "Choose a color",
                "Pick a color in a view the guest draws",
            ),
            // Elsewhere Pane lists these as unavailable, says why, and
            // never runs their actions.
            acting(
                "windows-only",
                "Windows-only action",
                "Declared to work on Windows only",
            )
            .platforms([Platform::Windows]),
            acting(
                "not-windows",
                "macOS and Linux action",
                "Declared to work on macOS and Linux only",
            )
            .platforms([Platform::Macos, Platform::Linux]),
        ]))
    }

    async fn open_designed_view(
        command: String,
        _launch: LaunchRecord,
    ) -> Result<Designed, String> {
        match command.as_str() {
            "form" => Ok(Designed::Form(Greeting {
                answered: RefCell::new(String::new()),
                error: RefCell::new(None),
            })),
            "color" => Ok(Designed::Color(ColorPicker::new())),
            _ => Err(format!("unknown designed view: {command}")),
        }
    }
}

pane_extension::root::export!(Sample);

/// The query that lists [`WEBSITE`], which invoking opens.
const WEBSITE_QUERY: &str = "pane website";
const WEBSITE: &str = "https://github.com/hoangvu12/pane";

impl pane_extension::root::Guest for Sample {
    /// "reverse <text>" typed into root search lists the text reversed,
    /// which Enter copies, and "pane website" lists Pane's website, which
    /// Enter opens; other queries have no results.
    async fn results_for(query: String) -> Result<Vec<RootResult>, String> {
        if query == WEBSITE_QUERY {
            return Ok(vec![RootResult {
                id: "website".into(),
                title: "Pane's website".into(),
                subtitle: Some("Opened by the Rust guest".into()),
                action: RootAction::OpenUrl(WEBSITE.into()),
            }]);
        }
        let text = query.strip_prefix("reverse ").unwrap_or_default().trim();
        if text.is_empty() {
            return Ok(Vec::new());
        }
        let reversed: String = text.chars().rev().collect();
        Ok(vec![RootResult {
            id: "reversed".into(),
            title: reversed.clone(),
            subtitle: Some("Reversed by the Rust guest".into()),
            action: RootAction::Copy(reversed),
        }])
    }
}
