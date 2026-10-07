# CI: verification tiers and pacing

All verification that gates a merge happens on GitHub Actions, paced in tiers so that no work waits for a run it does not need. Local commands are each developer's own choice — never required, never assumed, never a substitute: a branch is done when its CI tier is green.

## The tiers

- **`ci-fast.yml` — ticket worktrees (`pi-subagent/**`).** A plain push runs `cargo fmt --all --check` and `cargo check --workspace --all-targets` on Linux and Windows (~13 minutes). A manual run, `gh workflow run ci-fast.yml --ref <branch> -f verify=true`, runs the full check leg instead: the guests, clippy `-D warnings` and the workspace tests under cargo-nextest with two retries (~30 minutes), and a macOS leg compiles the macOS-gated code (the autostart, tray, placement and hotkey adapters, which the Linux and Windows legs never compile) and runs the macOS clipboard adapter test. An implementer iterates on plain pushes and asks for a verify run once, when it believes the branch is done; it makes no commit to ask (the old `[verify]` commit-message trigger left empty retry commits on `main`, and is gone). A manual run needs the workflow on `main`, which has it since this change landed. A superseded run is cancelled automatically, so iterating is cheap. A plain push whose committed JS/TS sample components (`guests/prebuilt`) no longer match their sources, the SDK or the WIT also rebuilds them on Linux with the pinned toolchain and uploads them as the `js-guests` artifact; the implementer commits that (`gh run download <run> -n js-guests -D guests/prebuilt`) before asking for a verify run, whose prebuilt-samples check would otherwise fail, so no agent's machine builds the toolchain.
- **`ci-branch.yml` — every other branch push.** Two Linux jobs run beside each other — `cargo xtask ci-lints` (formatting, the prebuilt-samples check, clippy) and `cargo xtask ci-tests` (the guests and the tests, run by cargo-nextest with two retries) — while Windows and macOS run the quick compile check; the wall time is the slower Linux job (~20 minutes). The full test suite on Windows and macOS runs at the verify tier and in the release matrix, not per push.
- **`ci.yml` — the release matrix: the smokes, renderer output, packaging, JS/TS guests from source, and licenses.** It runs on pushes to `main` and on demand (`workflow_dispatch`), and nowhere else: not on branch pushes, not on pull-request events (no duplicate runs), not when a pull request is marked ready for review. It verifies a release, not a milestone: its evidence feeds the milestone's release-validation ticket (for #70, #84) when a release is actually made.

## How merges are paced

- Land each ticket on the milestone branch as one commit: `git merge --squash <ticket-branch>`, then a commit titled for the ticket with its number (`… (#NN)`) and a body saying what it does, signed off. The ticket branch's iterations, fixups and merges stay on the ticket branch, out of `main`'s history; the ticket's results comment cites the squashed commit. A ticket whose branch is already a curated series may keep it.
- A milestone's pull request to `main` lands with a merge commit, so the commits its tickets cite stay reachable; the repository does not offer rebase merging, which would rewrite them. A small pull request that is not a milestone lands squashed.
- Land ticket merges in waves of 3–4 with a single final push. Pushes to a branch auto-cancel superseded runs, and a green run on the newest commit verifies every ancestor: intermediate pushes cost nothing.
- Do not dispatch the release matrix on a ticket branch, and do not wait for any release leg during a milestone. The branch tier is the merge gate.
- Poll runs with small commands (`gh run list --branch <branch> --repo hoangvu12/pane`), never long sleeps.
- When a change alters what a leg runs or how long it takes, update this file with it.

## Flakes

Two tests fail occasionally without a code cause: the macOS claim-window test in `pane-core`'s `update.rs` (fires only where macOS runs tests) and `command_search.rs`'s `a_service_that_stalls_is_given_up_on_within_the_limits` under runner load. The tests run under cargo-nextest with two retries, which absorbs them. A failure that survives its retries is real: rerun the failed jobs once (`gh run rerun <id> --repo hoangvu12/pane --failed`) before diagnosing.

The Linux clipboard adapter test (`clipboard_adapter_linux.rs`'s `the_watcher_reports_this_tests_changes_until_dropped`) runs in its own Xvfb step under plain `cargo test`, so nothing retries it. Once (release run 37426152516) Xvfb reset the test's first connection ("Pane could not reach the X11 display :99: Connection reset by peer"), before any clipboard work. That panic is the display, not the adapter: rerun the failed job.

## What is traded

Per branch push, Windows and macOS get compile checks only; their full test runs happen at the verify tier and in the release matrix. The release legs — the app actually launched and screenshotted, the packages built, the renderer's output checked — never run per push: a milestone's native and visual validation is its own open release-validation ticket, fed by the run on `main`.
