# Handoff 4: pane CI convergence wave (Windows), 2026-10-21

Supersedes handoff 3's state. Operating rules: pane-wave-handoff.md (same folder) plus wave\WORKER-RULES.md (shared rules for worker agents). Wave log: C:\Users\ADMIN\Desktop\nguyenvu\wave\WAVE-LOG.md (newest at the bottom).

## State (verify)
- main = 048811de. Release matrix 39594267641: every job green, no test needed a retry.
- Merged this session: #422, #423, #425–#431, #436–#438, #440–#443.
- Closed: #231, #283, #299, #421, #424, #432–#435, #439.
- Open: #68, waiting only on native checks that need a person (blur, IME, assistive tech, final visual review). Agent evidence is in its comments.
- Worktrees: only the main checkout, smoke-wt and verify-A/B/C (all detached). No census-* or wave-* branches on origin.

## User decisions this session
- After a failed Pane update, its row comes back selected so Enter retries (#426).
- Take on all agent-doable issues.
- An uninstalled default stays uninstalled (#427).
- Cmd+W or closing the window hides the macOS launcher (#442).
- The Windows icon worker's ~5 s stall under full load stays as is (ADR 0038 unchanged).

## Watches
- macOS `file_index::` one-offs.
- Windows smoke one-offs at 304 and 246.
- Local only: `file_search_settings::a_folder_taken_out_for_churn…` under load.

## Lesson
Never dispatch ci.yml on main while main's run is in progress: the per-ref concurrency group cancels it, even if the dispatch itself is cancelled.
