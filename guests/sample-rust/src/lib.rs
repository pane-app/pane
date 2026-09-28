//! Pane's Rust sample command: one list and one action per item. Items,
//! titles and results match the JavaScript and TypeScript samples.
#![no_std]

use pane_guest::alloc::{format, string::String, vec};
use pane_guest::{Guest, Item, View};

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

impl Guest for Sample {
    async fn get_view() -> Result<View, String> {
        let item = |id: &str, title: &str, subtitle: &str| Item {
            id: id.into(),
            title: title.into(),
            subtitle: Some(subtitle.into()),
        };
        Ok(View {
            title: "Rust sample".into(),
            items: vec![
                item("greet", "Say hello", "Answer from the Rust guest"),
                item(
                    "wait",
                    "Wait briefly",
                    "Await a WASI 0.3 clock, then answer",
                ),
                item(
                    "validate",
                    "Validate settings",
                    "Reject settings with an out-of-range port",
                ),
                item(
                    "random",
                    "Roll a number",
                    "A random number from this instance",
                ),
            ],
        })
    }

    async fn run_action(item_id: String) -> Result<String, String> {
        match item_id.as_str() {
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
            other => Err(format!("unknown item: {other}")),
        }
    }
}
