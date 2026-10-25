# Handoff 8: pane wave orchestrator (Windows), 2026-10-24

You take over as orchestrator for pane-app/pane on the user's Windows PC from chat "Pane CI Convergence Wave Windows (7)". This supersedes handoffs 6 and 7 where they differ; read handoff 7 (`%TEMP%\pane-wave-handoff-7.md`) for the standing setup. Keep your own context lean: delegate long work to background agents, each with its own worktree, and keep only the conclusions. Verify state; don't trust it.

## Read first
- `%TEMP%\pane-wave\WORKER-RULES.md`: give it to every worker.
- `%TEMP%\pane-wave\WAVE-LOG.md`: append-only, newest at the bottom. Grep it; don't read all of it.
- Repo docs: AGENTS.md and `docs/agents/{issue-tracker,triage-labels,domain,ci}.md`.

## Talking to the user
- Plain, short words. Lead with a one-line summary. They read in the Roboco Android app.
- They compare Pane with **Raycast** (Windows and macOS) and want Raycast's behaviour. "Do like Raycast" is a standing instruction.
- Confirm before anything outward-facing or hard to undo, unless they already said go for that exact thing.

## Standing rules (unchanged)
- Merge gate: `CI (branch)` green plus local fmt/clippy on touched crates, then `gh pr merge <n> -R pane-app/pane --squash --delete-branch`. Commit as `git -c user.name=hoangvu12 -c user.email=hggaming91@gmail.com commit -s`, with LF commits. PR bodies go through `--body-file` from `%TEMP%\pane-wave\msg\`.
- Never dispatch ci.yml on main. Smoke tests and real-input tests run only in CI.
- **PC load:** parallel builds froze the PC. Run at most TWO building workers at a time. Each worker waits for no other cargo/rustc before a full build, uses `-p` builds, and never runs workspace-wide clippy/ci-lints locally (CI covers them). Read-only research workers can run alongside.
- Tags: release tags are force-moved only, never deleted (deleting one turns its release into a draft). A `v*` push starts release.yml, which stops on an existing release; cancel that run.
- "Closes #a and #b" closes only #a. Close the others by hand.

## State (verify)
- main = `13d097f7`. History was regrouped into meaningful commits (54 up to v0.1.1), then #487 and #489. The pre-rewrite backup is `%TEMP%\pane-history-backup.git`; keep it for now.
- v0.1.1 is published as a full release and marked latest. **The user is on 0.1.1** (the in-app update worked, but the flow was clunky). The old data backups are deleted.
- Merged since 0.1.1:
  - #487 (closes #486): dev builds now stage icon.png, HELP.md and assets/. The user's own `test-extension` failed until this ships.
  - #489 (closes #488): Wasmtime's cache cleanup was deleting `cache\application-icons` and `cache\file-index`. Code cache moved to `cache\code`, and the icon worker is faster.
- Worktrees: only the main checkout. When this handoff was written, 2 cargo/rustc processes were running that weren't ours (probably the gpui-go chats). Leave them alone.

## Work queue (the user said "do it" to all of this)
File the GitHub issues first (per docs/agents/issue-tracker.md and triage-labels.md), then run the workers, at most two building at once.

**A. Keyboard-first everywhere.**
- Report: `%TEMP%\pane-wave\research-keyboard-audit.md`. It has 9 items in Part 3 and Appendix A, which lists all 163 mouse actions (59 have no keyboard route, 48 of them in Settings).
- Build all 9, in this order:
  1. Extension page switches, Actions menu and install controls by keyboard.
  2. Settings confirmations and previews work with Enter, arrows and Escape. Escape cancels, not closes Settings.
  3. Ctrl+K gets Uninstall / Disable / Configure Extension, with Raycast's chords.
  4. Backspace clears a recorded hotkey, and Reset is reachable.
  5. Move between Settings pages and into a page by keyboard.
  6. A "Quit Pane" command.
  7. Forms: Ctrl+Enter from any field, Ctrl+P opens dropdowns, chips and paths removable by keyboard, "Choose…" focusable, File Search link and Exclude field.
  8. Every remaining Settings control reachable with Tab, plus a test that checks it.
  9. Detail links, long details and scrolling text reachable by keyboard.
- Item 1 has a direct user complaint: they had to use the mouse to uninstall their test extension.

**B. Updates like Raycast.**
- Report: `%TEMP%\pane-wave\research-raycast-updates.md`. The user said "yes, do like Raycast".
- First write an ADR that supersedes the "never downloads or restarts on its own" parts of ADR 0048 / US76.
- Then build:
  - a periodic background check (every few hours, and when the launcher opens after a stale check), with an ETag and the last check remembered across restarts;
  - an automatic background download and verify;
  - a highlighted "Restart to Update Pane" item pinned at the top of root search, plus a toast;
  - Enter swaps the program and Pane relaunches itself. Use a single-instance handoff: the new pane.exe waits for the old one's process to exit. Keep a fallback if the relaunch fails, and keep the Windows Apps entry's DisplayVersion;
  - after the restart, a "What's New in x" item from the GitHub release notes, shown for about a day;
  - "Check for Updates" stays as a command.
- Whether to also auto-install while idle (newer Raycast on Mac): the orchestrator's recommendation was no, because the binaries are unsigned, and the user just said "do like Raycast". Default to Enter-to-restart, and ask the user if it matters.

**C. Icons at start, like Raycast.**
- Prewarm the app list and icon cache when Pane starts, so the very first search after a fresh install or update has real icons.
- The #489 worker's caveat: this starts extension instances earlier and changes the idle-memory measurements. Measure it and update the recorded targets/evidence.
- Notes: `%TEMP%\pane-wave\research-app-icons.md`.

**D. Duplicate apps (Discord twice).**
- One Discord entry comes from the Desktop .lnk and one from the Start Menu .lnk, with different arguments. The user asked "does Raycast do this?". Unconfirmed: check how Raycast for Windows lists apps (one entry per app?) and tell the user.
- If Raycast shows one, dedupe in Pane's discovery (`applications/start_menu.rs`) by AppUserModelID, else by same program and name. Migrate the pinned and learned ids that depend on app ids.

**E. Republish the npm CLI.**
- The user said OK. `@pane-app/cli` and its platform packages carry `pane-ext dev`, which gets the #487 fix.
- Follow `docs/npm.md`, and check that npm auth works on this PC (`npm whoami`). If it isn't logged in, tell the user what's needed. `@pane-app/create` and `extension-template` didn't change.

**F. After A–E: release 0.1.2.**
- The user said release after everything is fixed.
- Steps: bump every crate version that `cargo xtask` and release.yml check (see the 0.1.1 bump PR #483 for the file list; the smoke scripts read the version from `crates/pane/Cargo.toml` now). Rewrite the "What's new" section in `.github/release-notes.md`. Wait for main CI, tag `v0.1.2`, wait for the release run, then publish it as a full release marked latest (the user approved publishing after the fixes).
- Then ask the user to try the new update flow from 0.1.1. They will need one manual update to get the new flow.

## Still open with the user
- Should Pane's own heading screens (package preview, confirmations) get the top bar too? Never answered.
- Tooltip-only text (audit item 8 part): Raycast's tooltips are hover-only too, so this is optional.
- An idea, not filed: a "Paused" label for broken extensions.
- Delete `%TEMP%\pane-history-backup.git` when the user is happy with the history.

## Cleanup at the end
Remove the worker worktrees once their branches merge. Keep `%TEMP%\pane-wave` (rules, log, research reports). Leave `nguyenvu\roboco-wt` and the user's other projects alone.
