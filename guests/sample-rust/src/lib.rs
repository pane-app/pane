//! Pane's Rust sample command: a list with one action per item, a form, a
//! color picker the command draws itself, two actions each declared for
//! some operating systems only, and a root result computed from the query
//! ("reverse <text>"). Items, titles, results, errors and drawings match the
//! JavaScript and TypeScript samples.
#![no_std]

use core::cell::Cell;

use pane_guest::alloc::{format, string::String, vec, vec::Vec};
use pane_guest::feedback::{Toast, show_toast};
use pane_guest::root::{RootAction, RootResult};
use pane_guest::{
    Choice, Command, CustomView, CustomViewInfo, CustomViewRole, Field, FieldKind, FieldValue,
    Form, FormError, Frame, GuestCustomView, Item, Key, List, Platform, Rect, Shape, Text,
    TextField, ViewEvent,
};

struct Sample;
pane_guest::export!(Sample);

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

/// The "form" item's form: a name to greet and a greeting to choose.
fn greeting_form() -> Form {
    Form {
        title: "Greet someone".into(),
        fields: vec![
            Field {
                id: "name".into(),
                label: "Name".into(),
                kind: FieldKind::Text(TextField {
                    placeholder: Some("Ada Lovelace".into()),
                }),
            },
            Field {
                id: "greeting".into(),
                label: "Greeting".into(),
                kind: FieldKind::Choice(
                    GREETINGS
                        .iter()
                        .map(|&(id, label)| Choice {
                            id: id.into(),
                            label: label.into(),
                        })
                        .collect(),
                ),
            },
        ],
        submit_label: "Greet".into(),
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
/// color. Arrow keys, Home and End move the choice; pressing or dragging the
/// pointer over the grid chooses the swatch under it. Pane creates one per
/// opened view (`open_view`) and drops it when the view closes.
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
    fn choose(&self, x: i32, y: i32) {
        self.column.set(x.div_euclid(STEP).clamp(0, COLUMNS - 1));
        self.row.set(y.div_euclid(STEP).clamp(0, ROWS - 1));
    }
}

fn rect(x: i32, y: i32, size: u32, fill: u32) -> Shape {
    Shape::Rect(Rect {
        x,
        y,
        width: size,
        height: size,
        fill,
    })
}

/// "#RRGGBB" for 0xRRGGBB.
fn hex(rgb: u32) -> String {
    format!("#{rgb:06X}")
}

impl GuestCustomView for ColorPicker {
    async fn render(&self) -> Frame {
        let (column, row) = (self.column.get(), self.row.get());
        let (hue, shades) = COLORS[column as usize];
        let chosen = shades[row as usize];
        // A light frame around the chosen swatch, then the swatches.
        let mut shapes = vec![rect(column * STEP, row * STEP, STEP as u32, 0xf1f3f5)];
        for (x, (_, column)) in (0..).zip(COLORS) {
            for (y, fill) in (0..).zip(column) {
                shapes.push(rect(x * STEP + 2, y * STEP + 2, SWATCH, fill));
            }
        }
        shapes.push(rect(COLUMNS * STEP + 12, 2, 64, chosen));
        shapes.push(Shape::Text(Text {
            x: COLUMNS * STEP + 12,
            y: 74,
            content: hex(chosen),
            color: 0xf1f3f5,
        }));
        let name = match SHADES[row as usize] {
            "" => String::from(hue),
            shade => format!("{shade} {}", hue.to_lowercase()),
        };
        Frame {
            width: (COLUMNS * STEP + 88) as u32,
            height: (ROWS * STEP) as u32,
            shapes,
            value: format!("{name}, {}", hex(chosen)),
        }
    }

    async fn handle_event(&self, event: ViewEvent) -> Result<(), String> {
        match event {
            ViewEvent::Key(key) => {
                let (column, row) = (self.column.get(), self.row.get());
                let (column, row) = match key {
                    Key::Left => (column - 1, row),
                    Key::Right => (column + 1, row),
                    Key::Up => (column, row - 1),
                    Key::Down => (column, row + 1),
                    Key::Home => (0, row),
                    Key::End => (COLUMNS - 1, row),
                };
                self.column.set(column.clamp(0, COLUMNS - 1));
                self.row.set(row.clamp(0, ROWS - 1));
            }
            // Only a press on the grid chooses a swatch and starts a drag.
            ViewEvent::PointerDown(at) => {
                if (0..COLUMNS * STEP).contains(&at.x) && (0..ROWS * STEP).contains(&at.y) {
                    self.dragging.set(true);
                    self.choose(at.x, at.y);
                }
            }
            ViewEvent::PointerMove(at) => {
                if self.dragging.get() {
                    self.choose(at.x, at.y);
                }
            }
            ViewEvent::PointerUp(_) => self.dragging.set(false),
        }
        Ok(())
    }
}

/// An error about the field `field`.
fn invalid(field: &str, message: &str) -> FormError {
    FormError {
        field: Some(field.into()),
        message: message.into(),
    }
}

/// Runs the action of the item `id` and shows a toast with what [`outcome`]
/// answers; each item's action is this with its id.
async fn act(id: &str) -> Result<(), String> {
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
    type CustomView = ColorPicker;

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
            item("form", "Greet someone", "Fill in a form the guest checks").form(greeting_form()),
            item(
                "color",
                "Choose a color",
                "Pick a color in a view the guest draws",
            )
            .custom_view(CustomViewInfo {
                title: "Choose a color".into(),
                label: "Color".into(),
                role: CustomViewRole::ColorWell,
            }),
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

    async fn submit_form(item_id: String, values: Vec<FieldValue>) -> Result<String, FormError> {
        if item_id != "form" {
            return Err(FormError {
                field: None,
                message: format!("unknown form: {item_id}"),
            });
        }
        let value = |id: &str| {
            values
                .iter()
                .find(|field| field.id == id)
                .map_or("", |field| field.value.as_str())
        };
        let name = value("name").trim();
        if name.is_empty() {
            return Err(invalid("name", "Enter a name"));
        }
        if name.chars().count() > 40 {
            return Err(invalid("name", "Use at most 40 characters"));
        }
        let greeting = value("greeting");
        let Some(&(_, greeting)) = GREETINGS.iter().find(|&&(id, _)| id == greeting) else {
            return Err(invalid("greeting", "Choose a greeting"));
        };
        Ok(format!("{greeting}, {name}, from the Rust guest"))
    }

    async fn open_view(item_id: String) -> Result<CustomView, String> {
        if item_id != "color" {
            return Err(format!("unknown view: {item_id}"));
        }
        Ok(CustomView::new(ColorPicker::new()))
    }
}

pane_guest::root::export!(Sample);

/// The query that lists [`WEBSITE`], which invoking opens.
const WEBSITE_QUERY: &str = "pane website";
const WEBSITE: &str = "https://github.com/hoangvu12/pane";

impl pane_guest::root::Guest for Sample {
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
