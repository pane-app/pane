# Handoff 6: pane wave orchestrator (Windows), 2026-10-24

You take over as orchestrator for pane-app/pane on the user's Windows PC. The previous chat used about 300k tokens. Keep your own context lean: delegate long work to background agents, each with its own worktree, and keep only the conclusions.

## Read first
- `C:\Users\ADMIN\Desktop\nguyenvu\wave\WORKER-RULES.md`: give it to every worker. It includes the CI-time rule: dispatch full ci.yml on a branch only when the change touches what smoke/real-app jobs check, and rerun only failed jobs.
- `C:\Users\ADMIN\Desktop\nguyenvu\wave\WAVE-LOG.md`: append-only, newest at the bottom. Grep it; don't read all of it.
- The older handoffs in this folder (`pane-wave-handoff*.md`): their rules hold except where this file differs.
- Repo docs: AGENTS.md and `docs/agents/{issue-tracker,triage-labels,domain,ci}.md`.

## Talking to the user
- Plain, short words. Lead with a one-line summary. The user reads replies in the Roboco Android app.
- Images show only when the file is inside the project folder (`C:/Users/ADMIN/Desktop/nguyenvu/pane/...`, e.g. under `target/`), written as a plain absolute path.
- Confirm before anything outward-facing or hard to undo, unless the user already said go for that exact thing.
- The user compares Pane with **Raycast for Windows** and wants Raycast's behaviour.

## Standing rules
- **Merge gate:** `CI (branch)` green, plus local fmt/clippy for code changes, then `gh pr merge <n> -R pane-app/pane --squash --delete-branch`.
  - PR bodies go through `--body-file`, from `C:\Users\ADMIN\Desktop\nguyenvu\prtmp\`.
  - Commit with `git -c user.name=hoangvu12 -c user.email=hggaming91@gmail.com commit -s`.
- **Docs-only branches:** skip the CI wait and run `python -E C:\Users\ADMIN\Desktop\nguyenvu\prtmp\linkcheck.py <repo>`. Exception: template READMEs under `crates/pane-core/templates/` are tested, so they need CI.
- **ci.yml on main:** never dispatch it, and never while main's run is in progress.
- **CI only:** no smoke scripts or real-input tests locally (`real_input::`, `hotkey_adapters`), and no injected input locally.
- **Tags:** tags are never rewritten, with one exception: the v0.1.0 tag and draft were deleted once before publishing, with the user's OK.
- **Closing issues:** a PR body that says "Closes #a and #b" closes only #a. Close the others by hand.

## State (verify, don't trust)
- **main = 989a8021.** **v0.1.0 is PUBLISHED** as a full release, marked latest: https://github.com/pane-app/pane/releases/tag/v0.1.0
  - It has the Windows `Pane-Setup-0.1.0.exe` (Inno Setup), the macOS `Pane-0.1.0.dmg`, the Linux `Pane-0.1.0-x86_64.AppImage`, the zips/tar.gz, a source archive and `.sha256` files.
  - The release workflow (#461) runs on a `v*` tag push and creates a DRAFT prerelease. A person publishes it (`gh release edit vX --draft=false --prerelease=false --latest` was used for v0.1.0, with the user's go). Dry run: `gh workflow run release.yml -R pane-app/pane --ref <branch>`.
  - The updater reads GitHub Releases (ADR 0048), takes the newest by version including prereleases, and swaps pane.exe from the zip. On Windows it then updates the Apps entry's DisplayVersion (#465). It **cannot see drafts**.
- **The user's PC:**
  - Pane 0.1.0 is installed from the published draft's setup, in `%LOCALAPPDATA%\Pane`, with data in `%LOCALAPPDATA%\Pane\data`.
  - Hotkey: Alt+Space. Raycast is also installed and running, with its own Left-Win double-tap hotkey.
  - Data backups: `%TEMP%\pane-data-backup-before-setup` and `%TEMP%\pane-data-backup-before-v0.1.0`. Delete them after the update test.
- **IN FLIGHT** (two background agents started by the old chat; you can't message them):
  - **#475** currency in the calculator. Worktree `currency-wt`, plus a clone in `extfix3\calculator`. The user chose the fawazahmed0 currency-api as primary and open.er-api.com as fallback, fetched daily, cached offline, with the rates' date shown. It ends with calculator v0.6.4 and a Pane pin move.
  - **#482** a top bar with a back button on forms, detail and canvas views. Worktree `topbar-wt`.
  - They merge themselves. Check with `gh pr list -R pane-app/pane --state all -L 5`, `git worktree list`, and the bottom of the wave log. If one stalls (session restarts kill agents), finish its work from its worktree yourself or start a fresh worker on it.
- **NEXT (the user agreed):** when #475 and #482 are merged:
  1. Bump Pane to **0.1.1** in a PR: every crate version that `cargo xtask` and the release workflow check. The tag must match the version.
  2. Wait for main CI, tag `v0.1.1`, and wait for the release run.
  3. Publish it as a full release (the user said to release it).
  4. Have the user test the **in-app update** from the installed 0.1.0 (Pane checks at start), and confirm Settings → Apps shows 0.1.1 afterwards.
- **Open issues:**
  - #451: the release spec, with #475 and #482 as its open children.
  - #68, #270, #108: human checks.
  - #104 and #117: the user said leave them.
  - Idea not filed yet: a "Paused" label for broken or incompatible extensions in the extensions list.
- **User decisions this stretch:** everything pane.sh is gone, and updates come from GitHub Releases. Pane installs like a normal app on every platform. The launcher stays off the taskbar and Alt+Tab, with no Dock icon (like Raycast, no setting). The first-setup list fetches the real icons, never bundled ones. Pane closes after opening an app or file. There's a back button on pushed views. Each command has its own icon. The calculator works like Raycast's. Linux AppImage menu self-integration: skip.

## Cleanup the user asked for (when all current work is done)
- Delete `C:\Users\ADMIN\Desktop\nguyenvu\wave`, `wavetools` and `prtmp` entirely; the user says that's not a place to keep these. First save what a future session needs (WORKER-RULES and a short log summary, plus `linkcheck.py`, `blobcr.py` and `tolf.py` if still useful) under `%TEMP%` next to the handoffs, and update any handoff paths.
- Also delete any leftover worker folders (`extfix*`, `*-wt` once merged) and the data backups above.
- Leave the user's other project folders in `nguyenvu` alone (e.g. `roboco-wt` is not Pane's).
