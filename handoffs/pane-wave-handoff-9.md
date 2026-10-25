# Handoff 9: pane wave, 2026-10-25 (for moving to another PC)

Supersedes handoff 8. The user stopped the wave here and may continue on a different PC. These notes live on branch `notes/wave` of pane-app/pane (a parentless branch, no code and no CI). On the new PC, clone it beside the repo:
`git clone -b notes/wave --single-branch https://github.com/pane-app/pane pane-notes`.
Paths in older notes (C:\Users\ADMIN\..., %TEMP%\pane-wave) belong to the old Windows PC; map them to the new one.

## Layout of this branch
- `wave/`: WORKER-RULES.md, WORKER-BRIEF-8.md, WAVE-LOG.md (append-only, newest at the bottom), research-*.md, plan-updates.md, history-groups.*, helper scripts (linkcheck.py, tolf.py, jsdigest.py, ...).
- `handoffs/`: handoffs 1–9 (this file is the newest).
- `test-extension/`: the user's own test extension (TS list), without node_modules and dist. Run `npm install` there; it should pin `@pane-app/cli` 0.1.2.

## State
- main = `179385a6` (#533), and the full main CI 40158661133 was green on it: every test and the smokes on all three systems.
- **Released:** v0.1.2 is published, the latest full release, with 14 assets. The user is on 0.1.2; the update from 0.1.1 worked.
- **npm:** `@pane-app/cli` 0.1.2 and its 6 platform packages, and `@pane-app/create` 0.1.1. `extension-template` is private, so it isn't on npm. npm was logged in as `hoangnguyenvu12` on the old PC.
- **On main, not released yet** (they go in the next release):
  - #529: top bar with a back button on Pane's own page screens (package preview etc.)
  - #530 and #531: Pane restarts into a downloaded update by itself after the launcher has been hidden 10 minutes and nothing is running. A changed form holds it. No opt-out. ADR 0051.
  - #532: "Pop to root search" defaults to After 90 seconds, like Raycast. Saved as `popToRoot`, and an old `reopening: restore-view` reads as 90 s.
  - #533: Pane's own questions are Raycast-style alert pop-ups (Uninstall + "Delete saved data" checkbox, unticked). It also fixed the main smoke, which had been red since #529.
  - Commit e6e50de5 (#532) has a stray "# " at the start of its subject line; it was left as is.
- **Open issues:** #527 (a runtime-stopped error can be hidden by a toast from the crashed call, which makes the macOS smoke flaky), plus the older #451, #270, #117, #108, #104 and #68.

## Talking to the user
- Plain, short words, with a one-line summary first. The user reads in the Roboco Android app.
- "Do like Raycast" is a standing instruction.
- Confirm anything outward-facing unless they already said go for that exact thing.
- Run work as Roboco side chats, each in its own worktree.

## Lessons from wave 8 (add these to the rules)
- **Disk, not CPU, froze the old PC:** a budget NVMe that was almost full. Keep free space high, and never give every worker a fresh cold target.
- **Never share one CARGO_TARGET_DIR between two workers building at the same time:** their workspace crates overwrite each other. Give each concurrent lane its own warm target, and reuse it for that lane's next worker.
- **Strict load rules:** CARGO_BUILD_JOBS=4, `--test-threads` 4 or fewer, local tests run once, CI for repeats, no workspace-wide lints locally.
- **Roboco side chats end their turn for good:** nothing wakes them when a background task finishes, so they must wait in the foreground, in loops of 9 minutes or less.
- **Check the full main CI** (ci.yml on push) after merges. A newer push cancels the older run, so several merges in a row can hide a red smoke. That's how #529's smoke break went unnoticed until #533.
- **Release notes template:** `.github/release-notes.md`'s "What was checked" Windows line claims hand-testing. Fix it for each release.
- **Tags:** release tags are force-moved, never deleted. Pushing a v* tag runs release.yml, which makes a draft. Publish it with `gh release edit vX --draft=false --prerelease=false --latest`.

## Possible next steps (none requested)
- Release 0.1.3 with #529–#533: bump versions as #526 did, rewrite the notes, wait for main CI, tag, publish.
- Fix #527.
