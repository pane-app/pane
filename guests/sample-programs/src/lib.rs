//! Pane's programs sample: a command that runs programs installed on the
//! system ([`pane_guest::programs`], ADR 0033). Its program is `pane-echo`
//! (guests/helpers/echo), named by its bare name, so Pane finds it on the
//! user's search path at the time of the call; the tests put the folder
//! holding it on the search path they give Pane.
//!
//! - "Run a program" runs it with input and answers its exit code (3), its
//!   output and its errors.
//! - "Run by its path" asks it for its own path, then runs that absolute
//!   path with arguments a shell would change (spaces, quotes, `$HOME`, a
//!   backslash, `*`, an empty one): each arrives as it was.
//! - "Stream progress" spawns it and shows each line it writes as it
//!   comes, in a toast in progress, then waits for it.
//! - "Talk to a program" writes two lines to a spawned program's input,
//!   closes it and reads its answers.
//! - "Kill a program" spawns one that would run for 30 seconds and kills
//!   it.
//! - "Run with a timeout" gives a 30-second program half a second; the
//!   timeout's error is handled here.
//! - "Start a descendant" spawns one that starts a program of its own, and
//!   returns once it has: Pane ends both with the call.
//! - "Give up after a second" races a run whose program starts a
//!   descendant against a one-second timer, and drops the run when the
//!   timer wins: Pane ends both.
//! - "Run until stopped" notes in its settings that it started, runs a
//!   program with a descendant for a minute, then notes that it finished:
//!   disabling, reloading, updating or uninstalling the package meanwhile,
//!   or quitting Pane, ends both, and it never finishes.
//! - "Write too much" runs one that writes 17 MiB, over what Pane keeps.
//! - "Run elevated" asks Windows to run one as an administrator; elsewhere
//!   that is not available yet.
//! - "Run a missing program" names one that is on no search path.
//! - "Run a shell script" runs `echo` through `sh`, or `cmd` where there is
//!   no `sh`.
//! - "Run in a folder with a variable" runs one in its own folder with a
//!   variable set.
//!
//! The no-view command "Report progress" streams a program's progress into
//! a toast without opening a screen. The JavaScript and TypeScript samples
//! answer the same.
#![no_std]

use core::future::Future;
use core::pin::pin;
use core::task::Poll;

use pane_guest::alloc::{format, string::String, vec::Vec};
use pane_guest::feedback::{Toast, show_toast};
use pane_guest::programs::{self, Options, ProgramError, ProgramErrorKind};
use pane_guest::{Command, Item, LaunchRecord, List, NoCustomView, settings};

/// The program every item runs, by its bare name.
const ECHO: &str = "pane-echo";
/// The settings key where "Run until stopped" notes how far it got.
const LONG: &str = "programs-long";
/// How long "Give up after a second" lets its program run, in nanoseconds.
const LIMIT: u64 = 1_000_000_000;
/// The arguments "Run by its path" passes, which no shell reads.
const ARGS: [&str; 7] = [
    "--args",
    "two words",
    "\"quoted\"",
    "$HOME",
    "a\\b",
    "*",
    "",
];

struct ProgramsSample;
pane_guest::export!(ProgramsSample);

/// `<kind>: <message>`, such as "not-found: there is no program …".
fn explain(error: ProgramError) -> String {
    error.explain()
}

/// An exit code as the answers say it.
fn code(exit: Option<i32>) -> String {
    match exit {
        Some(code) => format!("{code}"),
        None => "none".into(),
    }
}

/// Runs `pane-echo` with `args` and `input`.
async fn echo(args: &[&str], input: &[u8], options: Options) -> Result<programs::Output, String> {
    programs::run(ECHO, args, input, options)
        .await
        .map_err(explain)
}

/// Waits for `first` or `second`, whichever finishes first, and drops the
/// other.
async fn race<A, B>(
    first: impl Future<Output = A>,
    second: impl Future<Output = B>,
) -> Result<A, B> {
    let (mut first, mut second) = (pin!(first), pin!(second));
    core::future::poll_fn(|cx| {
        if let Poll::Ready(value) = first.as_mut().poll(cx) {
            return Poll::Ready(Ok(value));
        }
        if let Poll::Ready(value) = second.as_mut().poll(cx) {
            return Poll::Ready(Err(value));
        }
        Poll::Pending
    })
    .await
}

/// The absolute path `pane-echo` was found at.
async fn echo_path() -> Result<String, String> {
    let found = echo(&["--where"], b"", Options::default()).await?;
    Ok(String::from(found.stdout_text().trim()))
}

/// Spawns `pane-echo --lines 3`, shows each line it writes in a toast in
/// progress, then waits for it.
async fn stream() -> Result<String, String> {
    let toast = show_toast(Toast::animated("Starting…"));
    let process = programs::spawn(ECHO, &["--lines", "3"], Options::default())
        .await
        .map_err(explain)?;
    let mut lines = process.lines();
    let mut seen = Vec::new();
    while let Some(line) = lines.next().await.map_err(explain)? {
        toast.update(Toast::animated(line.clone()));
        seen.push(line);
    }
    let exit = process.wait().await.map_err(explain)?;
    Ok(format!(
        "Streamed {}; exit code {}",
        seen.join(", "),
        code(exit)
    ))
}

/// Runs the action of the item `item_id` and shows a toast with what
/// [`outcome`] answers.
async fn act(item_id: &str) -> Result<(), String> {
    let done = outcome(item_id).await?;
    show_toast(Toast::success(done));
    Ok(())
}

/// What the item `item_id` does, answering what became of its program.
async fn outcome(item_id: &str) -> Result<String, String> {
    match item_id {
        "run" => {
            let output = echo(&["--status", "3"], b"hello", Options::default()).await?;
            Ok(format!(
                "Exit code {}, output \"{}\", errors \"{}\"",
                code(output.exit_code),
                output.stdout_text().trim(),
                output.stderr_text().trim()
            ))
        }
        "path" => {
            let path = echo_path().await?;
            let output = programs::run(&path, &ARGS, b"", Options::default())
                .await
                .map_err(explain)?;
            let text = output.stdout_text();
            let arrived: Vec<&str> = text.lines().collect();
            Ok(format!(
                "By its path, the arguments arrived as {}",
                arrived.join(" ")
            ))
        }
        "stream" => stream().await,
        "input" => {
            let process = programs::spawn(ECHO, &["--echo-lines"], Options::default())
                .await
                .map_err(explain)?;
            process.write(b"one\n").await.map_err(explain)?;
            process.write(b"two\n").await.map_err(explain)?;
            process.close_input().await.map_err(explain)?;
            let mut lines = process.lines();
            let mut answers = Vec::new();
            while let Some(line) = lines.next().await.map_err(explain)? {
                answers.push(line);
            }
            let exit = process.wait().await.map_err(explain)?;
            Ok(format!(
                "Answered {}; exit code {}",
                answers.join(", "),
                code(exit)
            ))
        }
        "kill" => {
            let process = programs::spawn(ECHO, &["--hold", "30", "killed"], Options::default())
                .await
                .map_err(explain)?;
            process.kill().await.map_err(explain)?;
            process.wait().await.map_err(explain)?;
            Ok("Killed the program".into())
        }
        "timeout" => {
            let options = Options::default().timeout(500);
            match programs::run(ECHO, &["--hold", "30", "timeout"], b"", options).await {
                Ok(output) => Ok(format!(
                    "It finished first, with exit code {}",
                    code(output.exit_code)
                )),
                Err(error) if error.kind == ProgramErrorKind::TimedOut => {
                    Ok(format!("The timeout ended it: {}", error.message))
                }
                Err(error) => Err(explain(error)),
            }
        }
        "descendant" => {
            let process = programs::spawn(ECHO, &["--parent", "30"], Options::default())
                .await
                .map_err(explain)?;
            let first = process.lines().next().await.map_err(explain)?;
            // Returning ends the call, and with it the program and its
            // descendant.
            Ok(format!(
                "It said \"{}\"; returned without waiting",
                first.unwrap_or_default()
            ))
        }
        "give-up" => {
            let slow = echo(&["--parent", "30"], b"", Options::default());
            let timer = wasip3::clocks::monotonic_clock::wait_for(LIMIT);
            match race(slow, timer).await {
                Ok(finished) => finished.map(|output| {
                    format!(
                        "It finished first, with exit code {}",
                        code(output.exit_code)
                    )
                }),
                // The run was dropped when the timer won: Pane ended the
                // program and its descendant.
                Err(()) => Ok("Gave up after a second".into()),
            }
        }
        "long" => {
            settings::set(LONG, "started")?;
            // If Pane stops the call meanwhile, the program and its
            // descendant end and nothing after this line runs.
            let output = echo(&["--parent", "60"], b"", Options::default()).await?;
            settings::set(LONG, "finished")?;
            Ok(format!(
                "Ran to the end, with exit code {}",
                code(output.exit_code)
            ))
        }
        "flood" => {
            let output = echo(&["--flood-mib", "17"], b"", Options::default()).await?;
            Ok(format!("It wrote {} bytes", output.stdout.len()))
        }
        "elevated" => {
            let options = Options::default().as_administrator();
            let output = echo(&["--where"], b"", options).await?;
            Ok(format!(
                "The elevated run ended with exit code {}",
                code(output.exit_code)
            ))
        }
        "missing" => programs::run("pane-no-such-program", &[], b"", Options::default())
            .await
            .map(|output| format!("It ran, with exit code {}", code(output.exit_code)))
            .map_err(explain),
        "script" => {
            let script = "echo script ran";
            let output = match programs::sh(script, Options::default()).await {
                Err(error) if error.kind == ProgramErrorKind::NotFound => {
                    programs::cmd(script, Options::default()).await
                }
                ran => ran,
            }
            .map_err(explain)?;
            Ok(format!("The script said {}", output.stdout_text().trim()))
        }
        "context" => {
            let path = echo_path().await?;
            let folder = match path.rfind(['/', '\\']) {
                Some(end) => String::from(&path[..end]),
                None => return Err(format!("{path} is in no folder")),
            };
            let options = Options::default()
                .in_folder(folder)
                .with_env("PANE_SAMPLE_VALUE", "set by the sample");
            let output = echo(&["--context", "PANE_SAMPLE_VALUE"], b"", options).await?;
            Ok(String::from(output.stdout_text().trim()))
        }
        other => Err(format!("unknown item: {other}")),
    }
}

impl Command for ProgramsSample {
    type CustomView = NoCustomView;

    async fn render() -> Result<List, String> {
        let item = |id: &'static str, title: &str, subtitle: &str| {
            Item::new(id, title)
                .subtitle(subtitle)
                .on_action(move || act(id))
        };
        Ok(List::new("Programs sample").items([
            item(
                "run",
                "Run a program",
                "Answers its exit code, output and errors",
            ),
            item(
                "path",
                "Run by its path",
                "Arguments reach the program as they are",
            ),
            item(
                "stream",
                "Stream progress",
                "Shows each line the program writes as it comes",
            ),
            item(
                "input",
                "Talk to a program",
                "Writes to its input and reads its answers",
            ),
            item("kill", "Kill a program", "Ends a program that would run on"),
            item(
                "timeout",
                "Run with a timeout",
                "Ends the program after half a second",
            ),
            item(
                "descendant",
                "Start a descendant",
                "Returns while the program and the one it started run",
            ),
            item(
                "give-up",
                "Give up after a second",
                "Drops the run when a timer wins",
            ),
            item(
                "long",
                "Run until stopped",
                "Runs for a minute; disabling or reloading ends it",
            ),
            item(
                "flood",
                "Write too much",
                "The program writes more than Pane keeps",
            ),
            item(
                "elevated",
                "Run elevated",
                "Asks Windows to run it as an administrator",
            ),
            item(
                "missing",
                "Run a missing program",
                "Names a program that is on no search path",
            ),
            item(
                "script",
                "Run a shell script",
                "Runs echo through the system's shell",
            ),
            item(
                "context",
                "Run in a folder with a variable",
                "Its own folder, and a variable set",
            ),
        ]))
    }

    async fn run(command: String, _launch: LaunchRecord) -> Result<(), String> {
        match command.as_str() {
            "progress" => {
                let done = stream().await?;
                show_toast(Toast::success(done));
                Ok(())
            }
            other => Err(format!("`{other}` opens a screen")),
        }
    }
}
