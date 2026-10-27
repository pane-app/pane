# Handoff 13: pane wave, 2026-10-27 (same PC)

Supersedes handoff 12. Read handoff 11 for wave 10's lessons, then this. Brief for workers: `wave9\WORKER-BRIEF-9.md` with the updates below. Log: `wave/WAVE-LOG.md` (newest at the bottom). Wave 11 scratch: `wave11\` (PR text in `wave11\msg`).

## State
- main = `0e37dd8` (#575). Full main CI 40498734894 green, CLI packages 40498734864 green.
- **Pane 0.1.5 is released**: tag `v0.1.5` (annotated, "Pane 0.1.5 preview") at 0e37dd8, release.yml 40505103360, 14 assets, published as a full release and Latest (as 0.1.3 and 0.1.4 were; release.yml drafts a prerelease, so untick it on publish). The orchestrator published it on the user's "just publish it".
- **npm published by the user** from their main PC: `@pane-app/cli` + 6 platform packages 0.1.3, `@pane-app/create` 0.1.2. The prompt used is `wave11\msg\publish-0.1.5-prompt.md`.
- **Done in wave 11** (squash-merged):
  - #570 → PR #572: no code cause (the Windows adapter caches no notifier or setting). A refused or failed `notify` is now written to `pane.log` (package title, answer, reason; never the text), and a toast's `Failed` event on Windows too. #570 stays open for a desktop recheck.
  - #571 → PR #573: Pane never hid the launcher on blur, however it was shown. On Windows it now hides when another application owns the foreground (checked ~100 ms after deactivation and on every system foreground change; only if it had the focus since shown). Pane's own windows (Settings, file dialogs) keep it. New real-window test `launcher_blur`. #571 stays open for a desktop recheck.
  - PR #574: version 0.1.5 (pane-ext follows), CLI 0.1.3, create 0.1.2, pins, release notes.
  - PR #575: test race in `ringing::tests::assert_stopped` (the thread drops its output before its `Finished` guard leaves the registry); it failed main CI on Windows 3 tries in a row.
- Cleanup: all worker chats archived, worktrees, targets and branches removed. `pane\target\debug` and wave9's old CI downloads deleted for disk space. `pane\target\guests` kept (workers junction it). Only `pane` and `pane-notes` worktrees remain.

## Open
1. **Desktop recheck of #570 and #571**, best on the user's main PC with 0.1.5 installed: Pane started with notifications off, turned on, an alarm fired, then `pane.log` read; the notification banner and its click with the sample alarm (`guests/packages`, sample-alerts); click the taskbar clock over a Ringing screen and over a hotkey-summoned launcher (both hide), open Settings from the launcher (stays). Close the issues with the results. Offered to the user, no answer yet.
2. **macOS and Linux hide on blur** (Raycast does): asked the user whether to file follow-ups, no answer yet. macOS could check whether the app is still active; Linux must not hide for portal file dialogs.
3. **Flake seen once**: `settings_titlebar::windows::a_real_double_click_on_the_settings_titlebar_maximizes_and_restores_it` failed 3 tries on main CI 40474935881 (the drag moved the window 40,20 of 80,40), green on rerun. Not filed. File it (or add to #568) if it comes back.
4. Unchanged from handoff 12: #559 (real Mac and Linux), #565, #568, #552, #104, #68; ADR 0053's worker-picked numbers and macOS signing still unanswered.

## Lessons from wave 11
- The user's "clean up" may go to several orchestrators at once (kyoko, Roboco). Clean only Pane's things; report the others' sizes.
- A worker's worktree folder stays locked until its chat is archived; after archiving, removing it a little later works.
- A tag push was confirmed with "yeah just release it"; publishing the draft took a second "just publish it". Ask for each.

## Talking to the user
Unchanged: plain, short words, one-line summary first (Roboco Android app). "Do like Raycast" is a standing instruction. Waves of parallel tickets. Ask when a short reply is ambiguous.
