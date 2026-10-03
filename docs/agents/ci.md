# CI: verification tiers and pacing

All verification that gates a merge happens on GitHub Actions, paced in tiers so that no work waits for a run it does not need. Local commands are each developer's own choice — never required, never assumed, never a substitute: a branch is done when its CI tier is green.

## The tiers

- **`ci-fast.yml` — ticket worktrees (`pi-subagent/**`).** A plain push runs `cargo fmt --all --check` and `cargo check --workspace --all-targets` on Linux and Windows (~13 minutes). A push whose head commit message contains `[verify]` runs the full check leg instead: the guests, clippy `-D warnings` and the workspace tests under cargo-nextest with two retries (~30 minutes). An implementer iterates on plain pushes and asks for `[verify]` once, when it believes the branch is done. A superseded run is cancelled automatically, so iterating is cheap.
- **`ci-branch.yml` — every other branch push.** Two Linux jobs run beside each other — `cargo xtask ci-lints` (formatting, the prebuilt-samples check, clippy) and `cargo xtask ci-tests` (the guests and the tests, run by cargo-nextest with two retries) — while Windows and macOS run the quick compile check; the wall time is the slower Linux job (~20 minutes). The full test suite on Windows and macOS runs at the `[verify]` tier and in the release matrix, not per push.
- **`ci.yml` — the release matrix: the smokes, renderer output, packaging, JS/TS guests from source, and licenses.** It runs on pushes to `main` and on demand (`workflow_dispatch`), and nowhere else: not on branch pushes, not on pull-request events (no duplicate runs), not when a pull request is marked ready for review. It verifies a release, not a milestone: its evidence feeds the milestone's release-validation ticket (for #70, #84) when a release is actually made.

## How merges are paced

- Land ticket merges in waves of 3–4 with a single final push. Pushes to a branch auto-cancel superseded runs, and a green run on the newest commit verifies every ancestor: intermediate pushes cost nothing.
- Do not dispatch the release matrix on a ticket branch, and do not wait for any release leg during a milestone. The branch tier is the merge gate.
- Poll runs with small commands (`gh run list --branch <branch> --repo hoangvu12/pane`), never long sleeps.
- When a change alters what a leg runs or how long it takes, update this file with it.

## Flakes

Two tests fail occasionally without a code cause: the macOS claim-window test in `pane-core`'s `update.rs` (fires only where macOS runs tests) and `command_search.rs`'s `a_service_that_stalls_is_given_up_on_within_the_limits` under runner load. The tests run under cargo-nextest with two retries, which absorbs them. A failure that survives its retries is real: rerun the failed jobs once (`gh run rerun <id> --repo hoangvu12/pane --failed`) before diagnosing.

## What is traded

Per branch push, Windows and macOS get compile checks only; their full test runs happen at `[verify]` and in the release matrix. The release legs — the app actually launched and screenshotted, the packages built, the renderer's output checked — never run per push: a milestone's native and visual validation is its own open release-validation ticket, fed by the run on `main`.
