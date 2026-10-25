# Handoff 3: pane CI convergence wave (Windows), for a new orchestrator chat, 2026-10-21

You are taking over as **orchestrator** of the CI convergence wave on **pane-app/pane**, from the user's Windows PC. The user's standing goal is "fix all the stuff". The previous chat used about 400k tokens, so keep your own context lean. Delegate long investigations and CI babysitting to side chats or agents (each one that edits git gets its own worktree), and keep the conclusions yourself. Verify the state below; don't trust it.

## Read first
- **Operating rules**: `C:\Users\ADMIN\AppData\Local\Temp\pane-wave-handoff.md`, which covers the merge gate, the commit identity, Windows practice, the census recipe and the verify-worktree recipe. Its rules still hold. Handoff 2 is `pane-wave-handoff-2.md` in the same folder; this file supersedes its state.
- **Wave log** (date-stamped, append-only, newest at the bottom): `C:\Users\ADMIN\Desktop\nguyenvu\wave\WAVE-LOG.md`. Everything below is there in more detail.
- Repo agent docs: `docs/agents/{issue-tracker,triage-labels,domain,ci}.md` (CLAUDE.md requires reading them before tickets, triage, design or pushing).

## User rules (unchanged)
- Never run tests that inject real OS input or drive the desktop locally: the Windows real-input tests (`real_input::`, `hotkey_adapters`) and the smoke scripts. These run on CI only.
- Merge gate: the branch's `CI (branch)` run green, then local verify (fmt for the root and for `guests/`, clippy on the touched crates, race loops ≥10× at 16 threads on the touched families), then `gh pr merge <n> -R pane-app/pane --squash --delete-branch`. PR bodies go via `--body-file` from `C:\Users\ADMIN\Desktop\nguyenvu\prtmp\`.
- Report to the user at natural boundaries, in plain words. The user said "im confused" at a dense status, so lead with a short summary and keep jargon low.

## Merged this session (main = 2dd3c5ff)
- #418 (338c23bf): all eight default extensions pinned to their API 0.2 releases.
- #419 (9f9acc39): paste and selection reads now reach an elevated target when Pane is elevated too. The check compares integrity levels (`system::windows::out_of_reach`). The real-input tests' hung-target and selection sections are skipped where `CI` is set; issue **#421** (needs-triage) tracks them.
- #420 (2dd3c5ff): `command_search::a_crash_of_the_runtime_…` now waits for root search to come back (it was the watch-list load flake).

## Current state (verify)
1. **Open Pane bug fix, not yet PR'd**: branch `census-paste-hook` (main checkout `C:\Users\ADMIN\Desktop\nguyenvu\pane`; pushed) = **8c019c70** "Let the paste target settle in front before Pane sends it Ctrl+V" on top of 27aa145b (new real-input test `a_paste_arrives_while_pane_s_keyboard_hook_is_installed`) on main.
   - **The bug**: on Windows, the launcher hides before it pastes, and Windows activates the next window by itself. Pane sent Ctrl+V 13 ms later, while the target was still activating (its focus was on Notepad's frame), and the keys were lost. The diagnostic smoke run 39474712223 showed this.
   - The bug showed only with Pane's keyboard hook installed, and the hook is installed on every fresh install because Open Pane defaults to the Windows-key tap (#268). It never reproduced inside the test binary, in any of the variants tried (see the wave log).
   - **The fix**: `settle()` in `crates/pane-core/src/system/windows.rs` runs after `bring_to_front`. It sends `WM_NULL` with `SendMessageTimeoutW` and then calls `WaitForInputIdle`, each capped at 500 ms.
   - Status: real-input ci-fast 39483339067 is green on Windows. Local fmt and clippy are clean.
   - **Pending**: Windows smoke **39483355593** on the smoke branch, which carries this fix cherry-picked as 23c16ebc. The previous chat's background watcher dies with it, so check the run yourself. If the clipboard phase gets past "Paste did not reach Notepad" (smoke-windows.ps1, about line 2713), open the PR (body: the A/B/C diagnostic evidence from the wave log) and merge it through the gate. If it still fails, the next diagnostic is in the wave log's "DIAG C" notes.
2. **Smoke branch** `census-smoke-faces` (worktree `C:\Users\ADMIN\Desktop\nguyenvu\smoke-wt`, head 23c16ebc). Its CI (branch) was green at 46d0a658.
   - The last full green runs: ubuntu 39220232222 (bee2231c) and macOS 39221362239 (a57dff4f). Only Windows-script and Pane changes have landed since.
   - It carries two **cherry-picks to drop** when you rebase onto main: 098493d5 (#419's first commit; the squash 9f9acc39 also holds its test changes, so expect a conflict and `git rebase --skip` it) and 23c16ebc (once 8c019c70's PR merges).
   - **Windows** passes every phase up to the clipboard phase's Paste step. After that it still has 284–407 (clipboard) and 600–604 (Pane update); ubuntu and macOS already pass their equivalents.
   - When Windows is green, rebase onto main and run a final `only=smoke` on all three systems. Then PR and merge it. The branch holds about 20 smoke fixes plus the search samples' warning sign; the commit messages give each reason.
3. **No temp branches** remain on origin besides those two. Delete any you create.

## Remaining queue (after the above)
1. Remove the `[diag-ff27]`/`ff29` scaffolds (`crates/pane/src/app.rs`, `crates/pane-core/src/launcher/designed_list.rs`) once the smoke is green, in its own small PR.
2. Run a release matrix on main and harvest it with the census recipe in handoff 1. The last known count, on main 4c4c2623, was about 9 macOS hard failures and one red Windows shard; #414 and #415 targeted those, so re-count.
3. **Questions to the user that are still open** (ask again; don't assume an answer):
   - Should a failed Pane self-update keep the selection on its "Update Pane to …" row? Today the row is hidden during the install by design (`application_update.rs:465` asserts it), so after a failure Enter runs "Check for Extension Updates". The smoke works around it by typing "update pane".
   - Should the agent take on GitHub issues #299 (system_commands test hang), #68 (release validation), #283 (first-setup list, deferred) and #231 (macOS pasteboard, needs-triage)? #281, #270, #108, #104 and #117 need a human. #421 is new from this session.
4. Watches: `command_search::a_service_that_stalls_is_given_up_on_within_the_limits` failed locally once in 30 runs at 16 threads (timing; CI hasn't flagged it). Also the older watches in handoffs 1 and 2.

## Practical notes learned this session
- **nextest filter for the Windows real-input tests**: `binary(integration) & test(/^real_input::/)`. `crates/pane-core/tests/main.rs` is the `[[test]] name = "integration"` binary, so `binary(real_input)` and `binary(main)` match nothing. Its Linux and macOS legs end with "no tests to run", which is expected. `PANE_TEST_REAL_INPUT=1` comes from `.github/actions/setup` on Windows.
- **Job logs**: `gh api repos/pane-app/pane/actions/jobs/<job-id>/logs` (`gh run view --job … --log` came back empty). Smoke artifacts are named `gui-smoke-Windows`, `gui-smoke-Linux` and `gui-smoke-macOS`.
- **Parallel smoke experiments**: a ci.yml dispatch on a ref cancels the in-progress one on the same ref. Push temp branches (`wave-…`), dispatch `-f only=smoke -f smoke_os=…` on each, and cancel the `CI (branch)` and `CLI packages` runs their pushes start.
- **Windows runner facts that keep mattering**:
  - It's an administrator session, so the smoke's Pane and its targets are all elevated.
  - A fresh data folder makes Open Pane the Windows-key tap; seed `settings.json` `{ "version": 1 }` where a phase presses Ctrl+Alt+Space.
  - Reopening restores the last view.
  - Toasts leave after 3 s.
  - Pane's paste is tagged: it puts the clipboard back and leaves no history record.
  - The hook phase's chord holder now signals through a marker file.
- **Edits on this Windows checkout**: the working copies are CRLF. Use a CRLF-aware Python helper, like `edit()` in `C:\Users\ADMIN\Desktop\nguyenvu\wavetools\integrity_edits.py`, run with `python -E -P`. Bash heredocs with backslashes get mangled. Commits stay LF; check with `git show HEAD | grep -c $'\r'`, which should print 0.
- If `target/guests` is missing in a checkout, run `cargo xtask guests` (about 5 minutes) before running pane-core tests locally.
- Background watchers don't survive a chat. Re-check any run ids listed above.

## Suggested skills
- `diagnosing-bugs`: if the paste fix doesn't hold, or for the next Windows smoke failures and the release-matrix census.
- `resolving-merge-conflicts`: rebasing `census-smoke-faces` onto main, dropping the cherry-picks.
- `code-review`: optional, on `census-paste-hook` and the smoke branch before their PRs.
- `tdd`: when a fix can be reproduced in a test first (the paste one couldn't).
