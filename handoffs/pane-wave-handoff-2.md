# Handoff 2 — pane CI convergence wave (Windows), 2026-10-19 (later)

You continue the CI convergence wave on **pane-app/pane** from a **Windows** PC. The user's standing goal is "fix all the stuff". This replaces `pane-wave-handoff.md` (same folder); read that one for the operating rules it lists (the merge gate, Windows practice, census recipe), which still hold. Verify the state; don't trust it.

## Where the history lives
- **Wave log** (date-stamped; append to it): `C:\Users\ADMIN\Desktop\nguyenvu\wave\WAVE-LOG.md`. Everything below is in it in more detail.
- Merged since the first handoff: #414 (macOS chords in tests), #415 (Windows keyboard hook: raw input follows the foreground, posts reach the session, recorded-chord packing, left-hand modifiers record side-free), #416 (hotkey cell's route note is an a11y status), #417 (install-time type check of root results' `results-for`, both API worlds, + fixture `guests/fixtures/old-root-results`). main = **a95cead1** (#417).

## Rules from the user (unchanged)
- Never run tests that inject real OS input or drive the desktop (Windows `hotkey_adapters` real-input tests, the smoke scripts). CI only: `gh workflow run ci-fast.yml --ref <b> -f tests='<nextest filter>'` (note `test()` matches test names; use `binary(hotkey_adapters)` for that binary), `gh workflow run ci.yml --ref <b> -f only=smoke [-f smoke_os=windows-2025|ubuntu-24.04|macos-15]`.
- **Both ci.yml and ci-fast.yml cancel in-progress runs on the same ref** (concurrency = workflow+ref). One dispatch per ref at a time; use temp branches for parallel experiments, and cancel the `CI (branch)`/`CLI packages` runs their pushes trigger.
- Merge gate: branch `CI (branch)` green + local verify (fmt root and `guests/`, clippy, race loops ≥10× @16 threads on the touched families) + squash merge via `gh pr merge <n> -R pane-app/pane --squash --delete-branch`. Commit as `git -c user.name=hoangvu12 -c user.email=hggaming91@gmail.com commit -s`; PR bodies via `--body-file` from `C:\Users\ADMIN\Desktop\nguyenvu\prtmp\`.
- Report to the user at natural boundaries; append date-stamped log entries.

## Current state (verify)
1. **Main checkout** `C:\Users\ADMIN\Desktop\nguyenvu\pane` is on branch **`census-default-pins`** (local only, not pushed) with an **uncommitted edit to `crates/pane/defaults.json`**: all eight default extensions re-pinned to their new API 0.2 releases (below). The user interrupted while I was checking what else reads `defaults.json` (`crates/pane-core/src/defaults.rs`, `crates/pane/src/lib.rs`, `crates/pane-core/tests/{application_update.rs,support/artifacts.rs,support/defaults.rs}`, `crates/pane/tests/settings.rs`, `xtask/src/{main.rs,package.rs}`): confirm nothing else holds per-component hashes or versions that must change, run the affected tests, then commit, push, PR, gate, merge.
2. **Default extensions re-released** (user authorized "Re-release them"; ADR 0046: official extensions are released for an API version before the Pane release that pins them). All built against the SDK at pane b8995933, apiVersion 0.2, merged + Release workflow green. New pins (tag → commit):
   - calculator v0.6.0 → f604ea4a0eec0509600da3dd1a94fcb9c7ec67cf
   - applications v0.6.0 → b40de4fca2b9dcda3b2a3857693a1a52828b377e
   - quicklinks v0.6.0 → 5689ffa0f63db345991ca808b17e1ee9842af9de
   - files v0.9.0 → 83b986d95f773e7be8a556d9c04cdcb7b914153a
   - clipboard-history v0.7.0 → 85b08857c4d29fb95fd0ed53e15fee0c1480809d
   - run v0.3.0 → 9a87dc1b2b5189b9b89a9fedbc89e0bdf8161aed (windows)
   - system-commands v0.2.0 → e2f5736da526749826e1bb922603d69b65bae837 (windows)
   - switch-windows v0.2.0 → 9d14e156eeeda0e3177636be28f816ab030c4aa5 (windows)
   Clones in `C:\Users\ADMIN\Desktop\nguyenvu\defaults\<repo>`. Verified in Pane with a scratch pane-core test (never committed): all eight install with no mark; calculator answers "6*7" → "42"; applications answers "notepad". Behaviour changes the porting agent made (tell the user): quicklinks' create form → designed form view, run's "Run with Completions" and switch-windows → designed Lists (`"mode": "designed"`), files dropped its command-level `search` export (as Pane did in #240), so a folder-installed Files has no search field of its own.
3. **Smoke branch** `census-smoke-faces` (worktree `C:\Users\ADMIN\Desktop\nguyenvu\smoke-wt`, head d6cb58e1, rebased on b8995933; rebase onto a95cead1 + the pins next). It holds every smoke-script fix of this session plus the search samples' danger warning sign (Rust/JS/TS + rebuilt `sample_search_{js,ts}` prebuilts). Not yet merged — merge it once the smoke is green (or decide with the user to merge earlier, since its CI (branch) is green).
   - **ubuntu/macOS** stop at **501-calculator-answer** (the incompatible defaults). With the pins merged into the branch, they should pass 501; then continue.
   - **Windows** (round 11, run 39207283354) recorded `tap:win` and stopped at `--distinct 82-tap-unfocused 83-tap-opened` (smoke-windows.ps1:~996): 82 shows no Pane window on purpose. **Next fix:** drop that comparison as was done for 78/79 (Check-Pane-In-Front + 83's selected check prove it). Expect more stale steps after it.
4. **Temp branch** `wave-smoke-recording` on origin: delete it (`git push origin --delete wave-smoke-recording`), it is superseded.
5. Remove the `[diag-ff27]`/`ff29` scaffolds (crates/pane/src/app.rs, crates/pane-core/src/launcher/designed_list.rs) once the smoke is green.

## Remaining queue after the above
- Watches: `windows::the_watchdog_repairs_a_hook_removed_behind_the_adapter_s_back` (1 retry-clean flake; likely pre-existing test race: the first chord after removal triggers a reinstall from real raw input while the 8 posted WM_INPUT are still queued); `command_search::a_crash_of_the_runtime…` (load flake); the older watches in handoff 1; local-only `window::clipboard_split::` parallel timeouts.
- Run a release matrix on main once the smoke branch merges, and harvest (census recipe in handoff 1).

## Practical notes learned this session
- Heredocs with backslashes break in this Bash tool; put multi-line edit scripts in `C:\Users\ADMIN\Desktop\nguyenvu\wavetools\*.py` and run them with `python -E -P`.
- Pillow is installed for the user (`pip --user`); `python -I` hides it, use `python -E -P`.
- A git push of a branch created with `git switch -C` may lose its upstream; push explicitly (`git push origin <sha>:refs/heads/<b>`), and check `git ls-remote`.
- Smoke failures since the designed era are mostly: toasts that leave after 3 s (capture with `capture_until`/`Capture-Until` from the press), designed views replacing lists/forms, a11y names (prefix vs exact), and fresh-data-folder defaults (#268: Open Pane = Windows key on a fresh Windows folder).
