# Handoff 12: pane wave, 2026-10-27 (same PC)

Supersedes handoff 11. Read handoff 11 first for wave 10's work and lessons. This file says only what changed since then and what's next. Then read `wave9\WORKER-BRIEF-9.md`, and grep `wave/WAVE-LOG.md` (newest entries at the bottom).

## State
- main = `dd6549e` (#569). Full main CI 40448965541 is green.
- **Since handoff 11:**
  - The user published SDK 0.2.1 from their main PC; npm and crates.io show 0.2.1.
  - PR #569 pins the TypeScript templates and `packages/extension-template` to `@pane-app/extension` 0.2.1.
  - **Windows hand check of alerts** on this PC. The results are posted on #555, #556 and #557, and the files are in `wave10\hc`.
    - Sound: works.
    - Attention launch: works, including the typing wait and Enter/Escape.
    - Notification: the content shows right in Windows' toast history. The banner and clicking it are unchecked, because this PC has the policy `HKCU\Software\Policies\Microsoft\Windows\Explorer\DisableNotificationCenter = 1`. Don't change it.
    - Method: a release build of main beside the user's Pane, with its own `PANE_DATA_DIR`, `PANE_CHANNEL` and `LOCALAPPDATA` (under `wave10\hc`) and an unused hotkey (Ctrl+Alt+Shift+F11), so no keyboard hook. The user's Pane was untouched. Notifications were turned on through Settings > System > Notifications (the registry value alone isn't picked up) and turned back off after.
  - Filed from the check: **#570**, the first toast after turning notifications on was not posted (once, not reproduced), and **#571**, the launcher stayed visible after the taskbar (the clock) took the focus, following an attention launch. Both are needs-triage.
- The workers' chats are archived and the leftover worktree folders deleted. Only `pane` and `pane-notes` remain.

## Work queue (user, 2026-10-27: "fix first, handoff new orchestrator chat")
1. **Fix #570 and #571** before the release, side by side as one wave of two workers, each in its own worktree and target. Triage them first: confirm or relabel to ready-for-agent, as the issue-tracker doc says.
   - #570: look for notifier state kept from when notifications were off (the `ToastNotifier`, its `Setting`, the AppUserModelID registration). Also consider writing a refused `notify` to `pane.log`, since an installed package's extension log can't be read.
   - #571: find out whether losing the focus to the taskbar fails to hide the launcher only after an attention launch, or always. Raycast hides on blur.
   - If a fix needs a real-desktop recheck, ask the user first. It can run beside their Pane the same way.
2. **Then release 0.1.5.** The user said yes once things work: "yes after we can confirm it works". Still confirm before pushing the tag, and before the user publishes the draft.
   - PR "Move Pane to version 0.1.5" (as #548 and #537 did; `pane-ext` follows the app's version).
   - In the same release prep: `@pane-app/cli` and its 6 platform packages go from 0.1.2 to 0.1.3, the templates, `packages/extension-template` and `packages/create` pin the CLI at 0.1.3, and `@pane-app/create` goes from 0.1.1 to 0.1.2. See #513 and #528 for the shape. The CLI carries the templates, which now pin SDK 0.2.1.
   - Tag `v0.1.5`. `release.yml` drafts a prerelease, the user reviews and publishes it, and you check its assets (0.1.4 had 14).
   - npm is not logged in on this PC. Give the user a prompt for their main PC, also Windows, to publish the 8 npm packages: the 7 CLI tarballs from the `CLI packages` run on main (artifacts `npm-cli` and `npm-cli-<target>`), and `@pane-app/create` from `packages/create`. The prompt for the SDK publish worked well: fresh clone, checks, dry runs, publish, verify, report.
3. After 0.1.5 is installed on their main PC, the user can check the notification banner and its click there with the sample alarm (`guests/packages`, sample-alerts). Offer the steps.
4. Open and not urgent: #559 (needs a real Mac and Linux desktop), #565, #568 (hotkey test flakes), #552, #104, #68. The worker-picked numbers in ADR 0053 and macOS signing were shown to the user, with no answer yet.

## Talking to the user
Unchanged: plain, short words, one-line summary first (they read in the Roboco Android app). "Do like Raycast" is a standing instruction. Run work in waves of parallel tickets (user, 2026-10-26). When a short reply is ambiguous, ask.
