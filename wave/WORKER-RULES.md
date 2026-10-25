# Rules for worker agents in the pane CI convergence wave (Windows PC)

Repo: GitHub pane-app/pane. Main checkout: C:\Users\ADMIN\Desktop\nguyenvu\pane. Don't switch its branch or edit files in it. The orchestrator opens and merges PRs, so workers don't (unless their prompt says otherwise).

## Read first
- Repo docs: CLAUDE.md, docs/agents/issue-tracker.md, docs/agents/triage-labels.md, docs/agents/domain.md (glossary + docs/adr/), docs/agents/ci.md.
- Operating rules: C:\Users\ADMIN\AppData\Local\Temp\pane-wave-handoff.md ("Windows-specific practice") and C:\Users\ADMIN\AppData\Local\Temp\pane-wave-handoff-3.md ("Practical notes learned this session").
- Wave log (append-only, newest at bottom): C:\Users\ADMIN\AppData\Local\Temp\pane-wave\WAVE-LOG.md. Grep it; don't read all of it.

## Hard rules
- NEVER run locally: the smoke scripts (scripts/smoke-*), or tests that inject real OS input or drive the desktop (`real_input::`, `hotkey_adapters`). They are CI-only. Ordinary cargo build, test, clippy and fmt are fine.
- Process-spawning test families (programs::, helpers::, build::, develop_builds::) hang locally; use CI for them. Run pane window families at `--test-threads 8` locally.
- Each worker that edits git gets its OWN worktree:
  `git -C C:\Users\ADMIN\Desktop\nguyenvu\pane worktree add -q C:\Users\ADMIN\Desktop\nguyenvu\<name>-wt -b <branch> origin/main`
  then `mkdir <wt>\target` and `cmd //c mklink //J "<wt>\target\guests" "C:\Users\ADMIN\Desktop\nguyenvu\pane\target\guests"` (don't rebuild guests into that junction; if you change guests, remove the junction with `cmd //c rmdir` and run `cargo xtask guests` into your own target). A cold target takes ~6 min to build.
- Commit identity: `git -c user.name=hoangvu12 -c user.email=hggaming91@gmail.com commit -s`. Commits must be LF (`git show HEAD | grep -c $'\r'` prints 0). Working copies are CRLF: use the Edit tool, or a CRLF-aware helper; C:\Users\ADMIN\AppData\Local\Temp\pane-wave\tolf.py converts a file to LF. No bash heredocs with backslashes for file contents. Commit messages and PR bodies go in files under C:\Users\ADMIN\AppData\Local\Temp\pane-wave\msg\ (create it; delete the files after use).
- Commit style: one plain-English subject line like the repo's `git log`, and a body giving the reason. Code style: match the surrounding code (doc comments that explain why, plain names, SAFETY comments).
- Run gh with jq through bash, not PowerShell. Job logs: `gh api repos/pane-app/pane/actions/jobs/<id>/logs`.
- CI: pushing a branch starts `CI (branch)` (the merge gate) and `CLI packages` (cancel the packages run). Targeted rechecks on all three systems: `gh workflow run ci-fast.yml -R pane-app/pane --ref <branch> -f tests='<nextest filter>'`. The pane-core and pane test binaries are both named `integration`: filter `binary(integration) & test(/^family::/)`, and locally run `cargo test -p <crate> --test integration -- <filter>`. A smoke-only run: `gh workflow run ci.yml -R pane-app/pane --ref <branch> -f only=smoke [-f smoke_os=windows-2025|macos-15|ubuntu-24.04]`. A dispatch on a ref cancels an in-progress one on the same ref, so use temp branches named `wave-â€¦` for parallel experiments, and delete every temp branch you push. Wait on runs with a background `until` loop (sleep 60+), not a busy poll. To save CI time (user rule), dispatch full `ci.yml` on a branch only when the change touches what its smoke/real-app jobs check, and then prefer `-f only=smoke -f smoke_os=<os>`. Rerun only failed jobs (`gh run rerun <id> --failed`).
- Docs-only branches (only .md files and images): no CI wait; run the link checker (C:\Users\ADMIN\AppData\Local\Temp\pane-wave\linkcheck.py) instead. User decision, 2026-10-23.
- Local verify before reporting a branch ready: `cargo fmt --all -- --check` in the root and in `guests/`, `cargo clippy -p <touched crates> --all-targets` clean, and the touched test families 10Ã— at `--test-threads 16` (8 for pane window families), all green.
- GitHub issues: comment with findings via `--body-file`. Don't close issues or change their labels unless the prompt says so; report the recommendation instead.
- Append short date-stamped entries (`## 2026-10-21 â€” <topic>`, a few bullets) to the wave log at milestones.

## Report back (short, plain words)
The branch name, head sha and worktree; the CI (branch) run id and result; the other run ids; local verify results; one line per change; and anything that needs a user decision. If you're blocked, say why and what you'd need.

