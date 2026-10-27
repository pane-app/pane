# Handoff 11: pane wave, 2026-10-27 (same PC as handoff 10)

Supersedes handoff 10. Wave 10 built the alerts spec #553. Read this, then `wave9\WORKER-BRIEF-9.md` (with the wave-10 updates below), and grep `wave/WAVE-LOG.md` (newest entries at the bottom).

## This PC
Unchanged from handoff 10: `C:\Users\HP MEDIA\Desktop\nguyenvu` (quote every path), `pane` = main checkout, `pane-notes` = `notes/wave` worktree, guests in `pane\target\guests` (they predate `guests/fixtures/alerts` and `guests/sample-alerts`: a worker that needs those replaces its worktree's junction with its own folder). Pane 0.1.4 runs as the user's own launcher. npm is not logged in on this PC: the user publishes from their main PC, which is also Windows.

## State
- main = `c4afecd` (#567). Full main CI 40425505587 is green (one hotkey flake, rerun green).
- **Done in wave 10** (all squash-merged):
  - #554 → PR #560: ADR 0053 (proposed), the glossary terms System notification, Ringing sound, Attention launch, and `wit/alerts.wit`. Additive, so the API stays 0.2.0. The orchestrator added the rule that an attention launch opens only the caller's own commands, as #557 asks.
  - #557 → PR #561: attention launch (a queue, waiting for typing to stop/an Alert/Game mode, focus, own commands only).
  - #556 → PR #562: ringing sound on Windows (WASAPI plus Media Foundation, owned registration, 2 per package and 8 in all, 10 min cap, the bundled sounds are tones made in code).
  - #555 → PR #563: Windows toast notification. The AppUserModelID `PaneApp.Pane` (dev: `.Development`) is set on the process, registered under HKCU at every start, and carried by the setup's shortcut. A notification click is a windowed launch (it opens at once).
  - #558 → PR #564: `pane_extension::alerts`, `@pane-app/extension/alerts`, the `"pane": {"alerts": true}` option, `guests/sample-alerts` (Rust/JS/TS), `docs/alerts.md`.
  - #559 → PR #566: macOS (UserNotifications from Pane.app only; the unsigned build was refused on CI, UNErrorDomain 1; AudioToolbox) and Linux (D-Bus via zbus, ALSA loaded at run time), input idle on macOS/X11, and Symphonia (MPL-2.0) for MP3 off Windows and Ogg everywhere. The smokes cover the calls. **#559 stays open (ready-for-human)** for a person's check on a real Mac and Linux desktop; the steps are in its comments.
  - PR #567: SDKs moved to 0.2.1 (npm `@pane-app/extension`, crates.io `pane-extension`), and the prebuilt samples rebuilt.
- **Filed:** #565 (JS `provide()` handle lacks `dispose()`), #568 (Windows real-input hotkey tests flake on main). Both are needs-triage.

## Waiting on the user
1. **Publishing SDK 0.2.1** from their main PC. The commands are in PR #567's body and were sent in chat. Check with `npm view @pane-app/extension version`.
2. **After that publish:** a small PR moving the TypeScript templates' pin (`crates/pane-core/templates/typescript/*/package.json`, `packages/extension-template/package.json`) from "0.2.0" to "0.2.1". It changes the templates, so `@pane-app/cli` needs a republish too (ask). Rust templates pin "0.2" and need nothing.
3. **Windows hand check of alerts on this PC** (notification, sound, attention launch with Enter/Escape, whether the launcher really comes to the front from a timer). Ask first: it drives the desktop and plays sound. Toasts are off for all apps here (`ToastEnabled = 0`), so they must be turned on for it. Record the results on #555, #556 and #557.
4. **Release 0.1.5:** needs the user's go.
5. **Open choices noted to the user:** the worker-picked numbers in ADR 0053 (10 min cap, 2/8 sounds, 10 notifications a minute, the 1 s/10 s typing wait, ringing under Do Not Disturb), and macOS signing (needs a Developer ID; open like #104).

## Lessons from wave 10 (add these to the rules)
- **Each push to main cancels the in-progress `ci.yml` run**, and rerunning an older main run cancels the newer one. Merge in batches, and only judge the newest run.
- **A macOS lints job can be cancelled while it waits for a runner**; `gh run rerun <id> --failed` fixes it.
- **Any edit to `wit/` or `guests/js/`** (a comment or a version number too) makes every prebuilt JS/TS sample stale. Batch such edits and rebuild once (`cargo xtask js-guests`; Node is on this PC). Delete `guests/prebuilt/*.wasm` and `*.wasm.map` first, or it fails with os error 1224.
- **`cargo xtask guests` copies from `guests/target`, `guests/fixtures/mixed-p2/target` and `guests/helpers/echo/target`**, ignoring CARGO_TARGET_DIR. Workers junction them to their own target and remove the junctions after.
- **Parallel tickets that each need a test guest** may create the same fixture. Name the fixture per ticket in the prompts, or plan the second merge as a combine.
- **A guard test refuses `eprintln!`** anywhere in pane-core or pane outside `diagnostics.rs`, test code included.
- **Worktree folders stay locked** while their worker chat lives. `git worktree remove` fails with "Permission denied"; delete the folders at wave end.
- **Don't close a ticket whose acceptance needs a human check** with "Closes #N". Use "Part of #N" and keep it open.
- **Ask when the user's short reply is ambiguous.** "1 is my main pc" meant the npm publish, not item 1 of the list just sent.
