# Brief for wave-9 workers (new PC)

This PC: `C:\Users\HP MEDIA\Desktop\nguyenvu` (note the space: always quote paths). 20 threads, 32 GB RAM, ~300 GB free disk.
- Main checkout: `...\nguyenvu\pane` (on main). Don't edit it or switch its branch.
- Notes: `...\nguyenvu\pane-notes\wave\` (WORKER-RULES.md, WAVE-LOG.md, helper scripts like tolf.py, linkcheck.py).
- Wave scratch: `...\nguyenvu\wave9\` (put PR text in `wave9\msg\<branch>-pr.md`).

Read `pane-notes\wave\WORKER-RULES.md` and follow it, but map the old paths:
- `C:\Users\ADMIN\Desktop\nguyenvu\...` -> `C:\Users\HP MEDIA\Desktop\nguyenvu\...`
- `C:\Users\ADMIN\AppData\Local\Temp\pane-wave\...` -> `C:\Users\HP MEDIA\Desktop\nguyenvu\pane-notes\wave\...` (scripts, wave log) or `...\nguyenvu\wave9\` (msg files, scratch).
- Older handoffs are in `pane-notes\handoffs\` (pane-wave-handoff.md "Windows-specific practice", -3.md "Practical notes", -9.md is the latest).

Your worktree is already made by the orchestrator (named in your prompt), branched from origin/main at 179385a6.

## Load rules (from wave 8, still apply)
- Every cargo command: `CARGO_BUILD_JOBS=4`, tests `--test-threads 4` or fewer.
- Use your OWN target dir, named in your prompt, via `CARGO_TARGET_DIR=<dir>` on every cargo command. Never share it with another worker.
- At most 3 workers build at once on this PC. Before a build, check `tasklist | grep -iE "cargo|rustc"`. If 3 other workers' builds are running, wait in a foreground loop (sleep 60 s).
- Pane 0.1.3 is installed and running on this PC as the user's own launcher. Don't stop it, don't touch `%LOCALAPPDATA%\Pane`, and don't start a debug `pane.exe` that installs hotkeys or a keyboard hook. Window tests in `cargo test` are fine.
- Guests: already built into `...\nguyenvu\pane\target\guests`. Make the junction both in your target dir and in `<worktree>\target`:
  `mkdir "<dir>"` then `cmd //c mklink //J "<dir>\guests" "C:\Users\HP MEDIA\Desktop\nguyenvu\pane\target\guests"` (same for `<worktree>\target\guests`). Never build guests into the junction. Remove the worktree junction with `cmd //c rmdir` when done.
- Write the whole change first, then build. Run only the tests you added or changed plus their family, ONCE locally. Clippy once at the end, on touched crates only (`-p`). No workspace-wide clippy/lints. CI covers breadth (`ci-fast.yml` with a filter).
- Never run smoke scripts or real-input tests locally.

## Git / CI
- Commit identity: `git -c user.name=hoangvu12 -c user.email=hggaming91@gmail.com commit -s`. LF commits (check `git show HEAD | grep -c $'\r'` is 0).
- Push your branch, cancel the `CLI packages` run, wait for `CI (branch)` to go green, fix red runs. If you touched a smoke script or something a smoke checks, also dispatch `gh workflow run ci.yml -R pane-app/pane --ref <branch> -f only=smoke -f smoke_os=<os>`. Never dispatch ci.yml on main.
- Do NOT open or merge the PR. Write a ready title + body (plain English, why, how tested, `Closes #N`) to `wave9\msg\<branch>-pr.md`.
- Comment on the GitHub issue only with findings via `--body-file`; don't close issues or change labels.
- Keep docs in step (docs/, user stories, glossary, ADRs) if behaviour changes.
- The user wants Pane to behave like Raycast. When unsure, do what Raycast does.

## You are a Roboco side chat
Your turn ending is final; nothing wakes you when a background task finishes. Wait for builds and CI in the FOREGROUND, in loops that sleep 60 s between checks, each tool call 9 minutes or less, repeated as needed. Don't end your turn until the report is ready.

## Report back (short, plain words)
Branch, head sha, worktree, CI (branch) run id + result, other run ids, local verify results, one line per change, open questions for the user. Also append a short dated entry to `pane-notes\wave\WAVE-LOG.md` (don't commit it).
