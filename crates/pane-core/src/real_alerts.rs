//! For the tests of the real alerts adapters (#559): whether they are opted
//! in, and the record of what this system answered them.

/// Whether the tests that play through the real sound output and post
/// through the real notifier run: `PANE_TEST_REAL_ALERTS`, which CI's
/// Linux and macOS test jobs set (`.github/actions/setup`), so a
/// developer's own computer never sounds or shows a notification.
pub(crate) fn opted_in() -> bool {
    std::env::var_os("PANE_TEST_REAL_ALERTS").is_some()
}

/// Records what the system answered: in the test's output and, on a CI
/// runner, in the job's summary, where it shows though the test passed.
pub(crate) fn record(text: &str) {
    use std::io::Write;

    // Standard output: only `diagnostics` writes to standard error.
    println!("{text}");
    if let Some(summary) = std::env::var_os("GITHUB_STEP_SUMMARY")
        && let Ok(mut file) = std::fs::OpenOptions::new().append(true).open(summary)
    {
        let _ = writeln!(
            file,
            "- {} ({}): {text}",
            std::env::consts::OS,
            env!("CARGO_PKG_NAME")
        );
    }
}
