# Handoff 5: pane wave orchestrator (Windows), 2026-10-23

You take over as orchestrator for the pane-app/pane work on the user's Windows PC. The previous chat got too big. Keep your own context lean: delegate long work to background agents (each one that edits git gets its own worktree), and keep only the conclusions.

## Read first
- **Shared worker rules:** `C:\Users\ADMIN\Desktop\nguyenvu\wave\WORKER-RULES.md`. Give it to every worker, and point workers at it instead of repeating it.
- **Older handoffs:** `pane-wave-handoff.md` and `pane-wave-handoff-4.md` in this folder. Their rules still hold except where this file differs.
- **Wave log:** `C:\Users\ADMIN\Desktop\nguyenvu\wave\WAVE-LOG.md`. It is append-only, newest at the bottom. Grep it; don't read all of it.
- **Repo agent docs:** AGENTS.md, and `docs/agents/{issue-tracker,triage-labels,domain,ci}.md`.

## How to talk to the user
- Plain, short words. Lead with a one-line summary. The user gets lost in dense status updates.
- The user reads replies in the **Roboco Android app**.
  - **Images:** the app shows an image only when the file is inside this chat's project folder and is written as a plain absolute path: `![x](C:/Users/ADMIN/Desktop/nguyenvu/pane/target/readme-shots/x.png)`. No `file://`, `data:` or https.
  - **When the user is at the PC:** open files with `Start-Process <path>`.
- Confirm before any action that is outward-facing or hard to undo, unless the user already said go for that exact thing.

## User rules and decisions (standing)
- **CI only:** never run the smoke scripts or real-OS-input tests locally (`real_input::`, `hotkey_adapters`). Exception: on 2026-10-23 the user allowed winuse for README screenshots while away.
- **Merge gate:**
  - A branch's `CI (branch)` run is green, plus local fmt/clippy/race loops for code changes, then `gh pr merge <n> -R pane-app/pane --squash --delete-branch`.
  - PR bodies go through `--body-file`, from `C:\Users\ADMIN\Desktop\nguyenvu\prtmp\`.
  - Commit with `git -c user.name=hoangvu12 -c user.email=hggaming91@gmail.com commit -s`.
- **Docs-only branches** (only .md or image files) **skip the CI wait**. The user asked "why do we need CI on a readme". Run the link checker instead: `python -E C:\Users\ADMIN\Desktop\nguyenvu\prtmp\linkcheck.py <repo>`. Exception: `crates/pane-core/templates/**/README.md` is embedded in Pane and tested, so it needs CI.
- **Never dispatch `ci.yml` on main while main's run is in progress.** The per-ref concurrency group cancels the running one.
- **Publishing:** done (see below). Publishing anything new still needs the user's go.
- **Product decisions this wave:**
  - A failed Pane update re-selects its row so Enter retries.
  - Uninstalled defaults stay uninstalled.
  - Cmd+W or closing the window hides the macOS launcher.
  - The Windows icon worker's ~5 s stall under full load is left as is.
  - Docs use blader/humanizer's rules. The README uses the **dark** "window" screenshot.

## State (verify, don't trust)
- **main = 28079ba0** (#447, the README rewrite with screenshot `docs/media/readme/pane.jpg`). Merged this stretch: #422 to #447.
- **Published:**
  - crates.io: `pane-extension` 0.2.0.
  - npm (org `pane-app`, account hoangnguyenvu12): `@pane-app/extension` 0.2.0, `@pane-app/cli` 0.1.0 plus its 6 platform packages, and `@pane-app/create` 0.1.0. End to end, `npm create @pane-app` → `pane-ext pack` works.
  - #281 is closed.
- **The 8 official extension repos** (pane-app/{calculator,applications,quicklinks,files,clipboard-history,run,system-commands,switch-windows}) build from crates.io `pane-extension` 0.2. Their PRs are merged.
  - #287 is still open for its last step: tag new releases and move Pane's pins in `crates/pane/defaults.json`. The user agreed to skip that until each extension's next real release.
- **Cleanup merged (#446):**
  - Archive tag `archive/docs-history-2026-10-23` holds the removed docs/evidence, research, HANDOFF and current-decisions.
  - CONTRIBUTING is now in `.github/`.
  - CLAUDE.md is `@AGENTS.md`.
- **IN FLIGHT: `census-docs-humanize`, head 599ad40d** (worktree `C:\Users\ADMIN\Desktop\nguyenvu\docs-wt`). It is the humanizer rewrite of 56 human-facing docs, plus the licensing fix that create and extension-template are Apache-2.0 OR MIT. It is rebased on 28079ba0.
  - Its CI (branch) run **39878678764** is in progress. CI is needed because it touches the template READMEs.
  - When it's green: open the PR (write a body: what it does, the link check results, the CI run), squash-merge it, and remove the worktree. Delete the `target\guests` junction first with `cmd /c rmdir`.
  - If template tests fail, compare against the tests that read `crates/pane-core/templates/**/README.md` (grep pane-ext and pane-core tests for "README").
  - Link check: 29 broken (all old, in research or ADRs) versus 44 on main. None are new.
- **Open issues:**
  - #68: needs human native checks; the agent evidence is recorded there.
  - #287: the releases-and-pins step.
  - #270 and #108: human checks on Windows 11.
  - #104 (reduced motion) and #117 (macOS footer chord overflow): marked ready-for-human. The user hasn't answered whether agents should try them.
- **Unanswered user question:** credit for the blue topographic desktop wallpaper in the README screenshot. Its source is unknown. The meadow art (alinalal_) is already credited.

## Practical notes
- **Line endings:** check committed blobs with `python -E C:\Users\ADMIN\Desktop\nguyenvu\wavetools\blobcr.py <repo> <base>`. Git Bash's `grep -c $'\r'` is unreliable here.
  - Fix stray CRs with `wavetools\tolf.py <repo> <listfile>`.
  - Commit docs with `-c core.autocrlf=false` after normalising.
  - Python in heredocs: `C:\Users` in a normal string breaks (`\U` escape). Use raw strings, or the Edit tool.
- **README screenshot kit:**
  - Scratch data folder: `wave\shot-data`. Wallpaper: `wave\wallhaven\`.
  - Scripts in `wavetools\`: `capture.ps1`, `crop.py`, `variant.ps1`, `setopts.py`, `q.ps1`.
  - EVKey (a Vietnamese IME) swallows injected letters, so type into Pane by clipboard paste. Restore the clipboard afterwards.
  - PowerShell 5.1 `Set-Content -Encoding utf8` writes a BOM that breaks Pane's `settings.json`.
- **Background agents and waits die when the session restarts.** Resume an agent with SendMessage, or check its worktree and finish the work yourself.
- **Usage limits** can kill agents mid-task. Resume them after the reset.
