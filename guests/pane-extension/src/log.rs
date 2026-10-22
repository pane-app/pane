//! Printing and logging, through `wasi:cli`'s standard output and standard
//! error. What a command writes goes to its package's extension log, which
//! its author sees while developing it ("Logs for <title>" in Pane, and the
//! development session's log file); for a package not being developed, Pane
//! keeps only its most recent lines in memory, for diagnostics.
//!
//! Use the crate's macros, as `std`'s and the `log` crate's:
//! [`debug!`](crate::debug), [`info!`](crate::info), [`warn!`](crate::warn)
//! and [`error!`](crate::error) write a line with its level;
//! [`println!`](crate::println) and [`eprintln!`](crate::eprintln) a line
//! without one (info on standard output, an error on standard error). A
//! panic's message and location are written as an error before the command
//! traps.
//!
//! ```ignore
//! pane_extension::info!("found {} items", items.len());
//! pane_extension::warn!("the service answered {status}; showing the cache");
//! ```
//!
//! Each call writes whole lines: [`print!`](crate::print) without a newline
//! still ends its line. A line longer than 4 KiB is cut, and a package
//! writing more than 1,000 lines a second loses the rest of that second.
//! Writing waits for Pane to take the line, which it does at once; it is
//! meant for a command's calls, not for a custom view's `Drop`, which may
//! not wait.

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::{self, Write};

wit_bindgen::generate!({
    path: "wit",
    world: "output-user",
    default_bindings_module: "pane_extension::log",
    generate_all,
});

/// How much a line matters, which Pane shows with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Debug,
    Info,
    Warn,
    Error,
}

/// Which output a line goes to.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Output {
    Stdout,
    Stderr,
}

/// Writes `args` as a line of `level`: debug and info on standard output,
/// warnings and errors on standard error, each line of it with the level
/// prefix Pane reads. The macros call this.
#[doc(hidden)]
pub fn log(level: Level, args: fmt::Arguments<'_>) {
    let (output, prefix) = match level {
        Level::Debug => (Output::Stdout, "<7>"),
        Level::Info => (Output::Stdout, "<6>"),
        Level::Warn => (Output::Stderr, "<4>"),
        Level::Error => (Output::Stderr, "<3>"),
    };
    let mut message = String::new();
    let _ = message.write_fmt(args);
    let mut text = String::new();
    for (index, part) in message.trim_end_matches('\n').split('\n').enumerate() {
        if index > 0 {
            text.push('\n');
        }
        text.push_str(prefix);
        text.push_str(part);
    }
    line(output, text);
}

/// Writes `args` to `output` without a level, ending the line. The print
/// macros call this.
#[doc(hidden)]
pub fn print(output: Output, args: fmt::Arguments<'_>) {
    let mut text = String::new();
    let _ = text.write_fmt(args);
    line(output, text);
}

/// Writes a panic's message and location as an error. The panic handler
/// calls this once.
pub(crate) fn panicked(info: &core::panic::PanicInfo<'_>) {
    let mut text = String::from("<3>panicked");
    if let Some(location) = info.location() {
        let _ = write!(
            text,
            " at {}:{}:{}",
            location.file(),
            location.line(),
            location.column()
        );
    }
    let _ = write!(text, ": {}", info.message());
    line(Output::Stderr, text);
}

/// Writes `text` to `output` as one line, waiting until Pane took it.
fn line(output: Output, mut text: String) {
    if !text.ends_with('\n') {
        text.push('\n');
    }
    let bytes: Vec<u8> = text.into_bytes();
    let (mut writer, reader) = wit_stream::new::<u8>();
    let written = match output {
        Output::Stdout => wasi::cli::stdout::write_via_stream(reader),
        Output::Stderr => wasi::cli::stderr::write_via_stream(reader),
    };
    wit_bindgen::block_on(async move {
        writer.write_all(bytes).await;
        // The stream ends with the writer, and Pane says when it read it.
        drop(writer);
        let _ = written.await;
    });
}
