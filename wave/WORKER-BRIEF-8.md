# Brief for wave-8 workers (read with WORKER-RULES.md)

- Rules: C:\Users\ADMIN\AppData\Local\Temp\pane-wave\WORKER-RULES.md. Follow them exactly (own worktree, guests junction, commit identity, LF commits, local verify, wave log entry).
- Context: the "Work queue" section of C:\Users\ADMIN\AppData\Local\Temp\pane-wave-handoff-8.md. The user wants Pane to behave like Raycast (Windows and macOS); when unsure, do what Raycast does.
- PC load: at most two building workers run at once. Before any full build, wait until no cargo/rustc you didn't start is running for more than a few minutes, except the one other wave worker (check with `tasklist | grep -iE "cargo|rustc"`; if more than one other build is running, wait with a background loop). Use `-p` builds. Never run workspace-wide clippy or ci-lints locally; CI covers them.
- **PC load, strict (user, 2026-10-24: builds freeze the PC).** These override WORKER-RULES' local verify:
  - Every cargo command runs with `CARGO_BUILD_JOBS=4`, and tests with `--test-threads 4` or fewer.
  - Compile as few times as you can: write the whole change first, then build. No speculative builds, and no compiling after each edit.
  - Locally, run only the tests you added or changed, plus their own family, ONCE. No 10× loops, and no full integration-binary or workspace test runs. Use CI for breadth and repeats (`ci-fast.yml` with a filter).
  - Run clippy once, on touched crates only, at the end.
  - Disk is the real bottleneck: the SSD is ~95% full and freezes at 100% disk. A cold target writes many GB, so never build into a fresh `<worktree>\target`. Use the warm shared target dir your prompt names, with `CARGO_TARGET_DIR=<dir>` on every cargo command. It already has its `guests` junction, so skip the rules' target/junction step. Cargo's lock makes a second build in the same dir wait, which is fine.
  - Tests still read guests from `<worktree>\target\guests`, so make that one junction (`mkdir target`, then mklink /J to the main checkout's target\guests). Put no build output there, and remove it with `cmd //c rmdir` when you're done.
  - Delete scratch files and temp branches' build output when you're done. Never delete the shared target dir.
- You push your branch and wait for `CI (branch)` to go green (cancel the `CLI packages` run). Fix red runs. You do NOT open or merge the PR; the orchestrator does. Put a ready PR title and body in C:\Users\ADMIN\AppData\Local\Temp\pane-wave\msg\<branch>-pr.md (plain English, the why, how it was tested; leave a `Closes #?` line for the orchestrator to fill).
- Never dispatch ci.yml on main.
- Keep docs in step: if behaviour changes, update the relevant docs/ pages, user stories and glossary (see docs/agents/domain.md).
- If you run as a Roboco side chat: your turn ending is final, and nothing wakes you when a background task or Monitor finishes. Wait for builds and CI in the foreground, with a loop that sleeps 60 s between checks and a long timeout. Don't end your turn until the report is ready.
- Report back (short): branch, head sha, worktree path, CI (branch) run id + result, local verify results, one line per change, open questions for the user.
