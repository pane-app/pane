# Handoff — pane CI convergence wave (Windows), 2026-10-19

You continue the failing-test census wave on **pane-app/pane**, driven from a **Windows** PC.
The user said "fix all the stuff". The full operating rules (the merge gate, the bracket pattern,
conflict replay, wave log) are in the original briefing, which the user will paste again or which
you should ask for. The key rules are summarised below. Verify the state; don't trust it.

## Where the history lives (read these, don't re-derive)

- **Wave log** (date-stamped, keep appending): `C:\Users\ADMIN\Desktop\nguyenvu\wave\WAVE-LOG.md`
- Merged in this wave: #410 (693572a1), #411 (fda5beb9), **#412** (d5165731, canvas: text-only canvas size probe, keyboard-made pointer-leave, tree reader's recursive frame/ABRT), **#413** (4c4c2623, operations samples draw their answer in success/danger tone; diag scaffolds ff22/23/25/34/60 removed). PR bodies have the details.
- CI docs: `docs/agents/ci.md`. Branch pushes run `ci-branch.yml` (`Lints (os)` ×3 + `Tests (ubuntu-24.04, N/3)`), which is the merge gate. A pull-request-event run verifies nothing. Rechecks: `gh workflow run ci-fast.yml --ref <branch> -f tests='test(/…/)'` runs chosen tests on Linux, Windows **and macOS**. `gh workflow run ci.yml --ref <branch> -f only=smoke` (optional `-f smoke_os=macos-15|windows-2025|ubuntu-24.04`) runs the smoke.
- Harvested logs: `wave\main-4c4c\` (main 4c4c2623's test jobs), `wave\smokeS\` (smoke-only run 39165753328 on the #413 branch: ubuntu/windows/macos job logs).

## Current state (verify, don't trust)

- `main` = **4c4c2623**. The main checkout `C:\Users\ADMIN\Desktop\nguyenvu\pane` is on `main`, clean. No open wave branches.
- main's release matrix (run 39166905486): Lints ×3 green, **Tests ubuntu 3/3 green (ubuntu census = 0)**, Package green, Smoke ×3 red, Tests windows shard 2 red, Tests macOS 3/3 red.
- `C:\Users\ADMIN\Desktop\nguyenvu\verify-C` is a worktree detached at c979ef2d (the #413 branch commit) with a warm target dir. Its `target\guests` is a junction to the main checkout's. Reuse it for the next verify: `git -C verify-C checkout --detach <sha>`. Don't `rm -rf` it (the junction); use `git worktree remove` only after deleting the junction with `cmd //c rmdir`.

## Remaining queue

1. **macOS hard faces** (failed all 3 tries on main 4c4c2623; the first macOS harvest of the wave). They look like the designed-era migrations/harness races #410 fixed on ubuntu. They can't run locally (Windows PC), so diagnose by reading, then check with `ci-fast.yml -f tests=…` (which runs macOS too):
   - `backspace_sections::` ×5: `ctrl_arrows_cross_root_searches_sections_scrolling_the_label_into_view`, `the_vertical_pinned_rows_count_as_the_first_section`, `the_horizontal_strip_is_not_entered`, `the_actions_panel_jumps_entries_and_groups`, `a_command_list_moves_five_rows_and_jumps_to_its_ends`
   - `loading::a_slow_command_search_shows_the_line_under_its_own_rule`
   - `navigation_stack::shift_escape_drops_the_whole_stack`
   - `designed_components::the_components_announce_their_roles_and_names`
   - `keyboard::a_form_still_submits_with_the_rebound_key_and_the_footer_button`
   - macOS flakes (retry-clean): `application_icons::an_applications_row_draws_its_own_icon_bare…`, `unresponsive::a_{rust,typescript,javascript}_guest_that_stops_responding…`, `clipboard_adapter_macos::the_watcher_reports…`
   - Failure messages: `grep -A30 "<test name>" wave\main-4c4c\job-*.log`.
2. **Smoke faces** (CI-only; never run the smoke locally). Step 32 is fixed. The new stopping points are:
   - **ubuntu**: `--distinct 40-kept.png 43-kept-after-clear.png` says they're the same window. After the cache clear, the view never redraws to "… Cached greeting: none" (`scripts/smoke-linux.sh:771-773`). Windows passes this check.
   - **windows**: `45-opened.png` has 0 px of success green, expected ≥20 (`scripts/smoke-windows.ps1`, after `44-application.png`'s selected row).
   - **macos**: fails early. `--distinct 4-result-2.png 25-search-result.png` says they're DIFFERENT, where linux/windows say they're the same (`scripts/smoke-macos.sh`). It never reaches step 32.
   - The smoke artifacts (screenshots) are uploaded as `gui-smoke-<OS>` on the run; download them to look.
   - Remove `[diag-ff27]`/`ff29` (crates/pane/src/app.rs, crates/pane-core/src/launcher/designed_list.rs) once the smoke is green.
3. **Windows-only**: `pane-core::hotkey_adapters windows::a_recording_session_holds_the_keys_back_and_reports_what_was_pressed` (hotkey_adapters.rs:~909, "the session reports the tap; the hook below saw []"). It needs `PANE_TEST_REAL_INPUT=1` and **injects real OS keystrokes**. Do NOT run it on the user's PC. Research by reading; it's verified by CI's Windows legs only.
4. **Watches**: ubuntu was clean on main 4c4c2623. Old watches: `command_search::a_crash_of_the_runtime…`, `view_push::…slow_service`, `services::reloading_…`, Windows `backspace_sections::a_held_backspace…`, Windows `clipboard_adapter::a_commands_concealed_copy…` (1 try on main 4c4c2623). Two consecutive matrices at 3/3 promote a watch to the hunt.
5. **Local-only watch (Windows PC)**: pane `window::clipboard_split::` fails 8–12/13 when its 13 tests run in parallel ("Test timed out after 15 seconds while parking", gpui test_scheduler.rs:506), the same on main. It passes at `--test-threads` 1–8 and alone. Ubuntu CI is green. Run pane window families at `--test-threads 8` locally.
6. Pre-existing, low priority: the clippy warning in `crates/pane-build/src/js.rs:226` (useless conversion) is Windows-local. Guest warnings in sample-operations (unused `vec`/`FormValues`, dead `greet`) don't gate: ci-lints only fmt-checks `guests/`.

## Windows-specific practice

- Paths: repo `C:\Users\ADMIN\Desktop\nguyenvu\pane`; use `C:\Users\ADMIN\Desktop\nguyenvu\` for verify worktrees, `wave\` and `prtmp\` (PR bodies/commit messages). There's no /tmp.
- Fresh verify worktree recipe: `git worktree add -q <dir> <sha> --detach`, `mkdir <dir>/target`, `cmd //c mklink //J "<dir>\target\guests" "C:\Users\ADMIN\Desktop\nguyenvu\pane\target\guests"`. A cold target takes about 6 min to build pane tests.
- **Prebuilt JS/TS samples**: `cargo xtask js-guests` rebuilds ALL 59 and they're never byte-identical (QuickJS snapshot). Commit only the samples whose sources changed: restore the rest with `git checkout HEAD -- guests/prebuilt/<others>` and patch only their entries into HEAD's `manifest.json` (script: `wave\patch_manifest.py <repo> <full-manifest>`). Check with `cargo xtask js-guests --check`. After that, run `cargo xtask guests` so `target\guests` holds the committed builds again.
- Process-spawning families (programs::, helpers::, build::, develop_builds::) hang locally. `develop::` in pane runs (fake builder).
- `core.autocrlf` "LF will be replaced by CRLF" warnings are harmless; commits stay LF (check: `git show HEAD | grep -c $'\r'` gives 0). After `git checkout HEAD -- <files>`, `git status` can list files that have no content diff (stale stat); `git add -u` settles it.
- PowerShell 5.1 mangles `gh … -q '"\(.x)"'` jq strings; run gh-with-jq through the Bash tool.
- GPUI facts: hover is hidden after keyboard input (`HitboxId::is_hovered`, gpui window.rs:872). Test windows open inactive. The test display is 1920×1080. A test window must call `launcher.with_changes(sender)` and `follow_changes(…)`. Wait for designed push/pop in state (`until_state`), not with a bare `settle`.
- Merge gate:
  - commit: `git -c user.name=hoangvu12 -c user.email=hggaming91@gmail.com commit -s`
  - PR from `prtmp` with `--body-file`, then `gh pr merge <N> -R pane-app/pane --squash --delete-branch` once the branch push's `CI (branch)` run is green
  - before merging: fmt check in the root and in `guests/`, `cargo clippy -p <crates> --all-targets`, race loops ≥10× with `--test-threads 16`, and no `#[cfg(any(test, debug_assertions))]` helpers called from release code.
- Census recipe: harvest `gh api 'repos/pane-app/pane/actions/runs?head_sha=<sha>'`, then the failed Tests jobs' logs. Match `(FAIL|SIGABRT|ABRT|SIGSEGV|TIMEOUT) \[`, and count per test: 4 lines = hard (3 tries + summary); fewer = retry-clean flake.

## Suggested skills

- `diagnosing-bugs`: for the macOS faces, the smoke faces and the Windows recording session.
- `code-review`: optional, on a branch before its PR.
- `resolving-merge-conflicts`: if parallel branches conflict (rebase onto main, resolve by intent, re-verify).
