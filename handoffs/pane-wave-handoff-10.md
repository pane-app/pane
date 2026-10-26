# Handoff 10: pane wave, 2026-10-26 (new PC)

Supersedes handoff 9. Wave 9 ran on the new PC. Read this, then `wave/WORKER-BRIEF-9.md`, and grep `wave/WAVE-LOG.md` (newest entries at the bottom).

## This PC
- Folder `C:\Users\HP MEDIA\Desktop\nguyenvu` (note the space: quote every path). 20 threads, 32 GB RAM, Windows 11 25H2 at 150%.
- `pane` is the main checkout (on main). `pane-notes` is this branch, `notes/wave`, as a worktree of `pane`. `wave9\` holds the worker brief, the PR and issue texts (`wave9\msg`) and the hand-check scripts (`wave9\hc`).
- `pane\target\guests` has the guests built. Workers junction it; never rebuild guests into it.
- Rust 1.98.1 (from rust-toolchain.toml) and wasm32-wasip2 are installed. `gh` is logged in as hoangvu12. npm has not been logged in on this PC.
- Pane 0.1.4 is installed (`%LOCALAPPDATA%\Pane`) and running as the user's own launcher, on default settings. Don't stop it or touch its data, except for a hand check the user asked for. Quit it with its own Quit Pane command, never by killing it.
- Other projects share this folder (`kyoko*`, `vexly`, `oxidcord-ref`, `.orchestrator`). Leave them alone.

## State
- main = `bdfaa1d4` (#551). Full main CI 40260000639 is green.
- **Released:** v0.1.4 is published as the latest release, with 14 assets. 0.1.3 is out too.
- **Done in wave 9:**
  - Fixes and 0.1.3: #527, #104 (code), #117 (macOS key symbols).
  - A hand check of 0.1.3 on this PC: #270 and #108 closed, plus #451.
  - Five fixes from that check, each rechecked by hand: #538 hook watchdog, #539 recording a double tap, #540 punctuation matching, #541 Settings title bar double-click, #542 single instance (ADR 0052).
  - Test fixes: #549 runs the title bar tests one at a time with the other real-window tests, and #551 adds logs for hand-offs and lost previews.
  - #550 was closed by the user ("it works for me").
- **Open:**
  - The new alerts spec #553 and its sub-issues #554–#559 (filed by the user on 2026-10-26).
  - #552: a macOS smoke flake where an `--install` preview is lost. It's needs-triage; the new logs should catch it.
  - #104 and #68: need a real Mac and Linux machine.

## Work queue (user, 2026-10-26: "implement them")
Spec #553, "Let an extension alert the user when its timer fires". Respect the blockers in each issue; `#554` comes first.
1. **#554** (prerequisite): ADR 0053, the glossary and the WIT contract. The others build on it, so merge it before starting them.
2. Then in parallel, each in its own worktree: **#555** (Windows system notification), **#556** (Windows ringing sound), **#557** (show the launcher on an extension's command from a call with no window), **#558** (Rust and JS/TS bindings, a sample, docs/alerts.md). Check each issue's "Blocked by" first.
3. **#559** (macOS and Linux) is labelled ready-for-human. Ask the user before an agent takes it. The code can be written and CI-checked, but the hand checks need a Mac and Linux.
4. After merging, a hand check on this PC is possible: Windows notifications and sound. Ask the user first, because it drives their desktop.
- Merging green PRs is fine. A release (0.1.5) needs the user's go. The npm packages (`@pane-app/cli`) need a republish if `pane-ext` or the templates change. Ask first.

## Talking to the user
- Plain, short words, with a one-line summary first. The user reads in the Roboco Android app.
- "Do like Raycast" is a standing instruction.
- Confirm anything outward-facing unless they already said go for that exact thing.
- Run work as Roboco side chats, each in its own worktree. Give each worker the brief `wave9\WORKER-BRIEF-9.md` and its own `CARGO_TARGET_DIR` under `nguyenvu\targets\<lane>`. At most 3 builds at once.

## Lessons from wave 9 (add these to the rules)
- **Merge commit subjects:** strip `Title: ` and `# ` from a PR text's first line before using it as the title. `107a798` has a stray "# " in its subject.
- **Updating a PR's branch:** when two PRs touch the same file, update the second one's branch with main (`gh pr update-branch`) and wait for CI before merging.
- **New real-window tests:** they must join the one-at-a-time group in `.config/nextest.toml` (the `real-clipboard` test group). Otherwise they steal each other's foreground. That's what main run 40234693621 was.
- **ci-fast on Windows-only tests:** the Linux and macOS legs "fail" with "no tests to run". Read the Windows leg.
- **Line endings:** `git show HEAD | grep -c $'\r'` lies in this shell. Count real CR bytes with `git cat-file -p HEAD:<file> | tr -cd '\r' | wc -c`.
- **Keys in hand checks:** check that the right window is in front before every key. A Notepad message took the focus once, and the keys went to the wrong place.
- **Workers' local tests:** something ended the user's installed Pane during the workers' runs. Workers must never stop pane.exe processes they didn't start.
- **Pane's hook and focus:** the hook watchdog fix (#538) means the Windows key no longer dies. If it ever does, `pane.log` says so.
- **Disk:** about 215 GB free. The workers' targets were ~50 GB and are deleted after each wave.
