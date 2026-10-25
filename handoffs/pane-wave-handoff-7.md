# Handoff 7: pane wave (Windows), 2026-10-24

Supersedes handoff 6 where they differ. The old `nguyenvu\wave`, `wavetools` and `prtmp` folders are gone (user's request); everything a future session needs is in `%TEMP%\pane-wave\`:
- `WORKER-RULES.md` (paths updated; give it to every worker), `WAVE-LOG.md` (append-only, newest at the bottom; grep it), `linkcheck.py`, `blobcr.py`, `tolf.py`.
- Commit messages and PR bodies: write them under `%TEMP%\pane-wave\msg\` and delete them after use.

## State at handoff
- **0.1.1 released.** #475 (calculator currency, calculator v0.6.4) and #482 (top bar with back button on forms/detail/canvas) merged; Pane bumped to 0.1.1; v0.1.1 published as a full release, marked latest. v0.1.0 also still published. 14 assets each.
- **History rewritten (user's go, 2026-10-24).** main is 54 grouped commits (each a contiguous run of the old first-parent history; list in `%TEMP%\pane-wave\history-groups.md`), head `069bfab0`. Tags v0.1.0 -> fdd9ffa1, v0.1.1 -> 069bfab0. Every commit sha in older handoffs and the wave log is gone from main. Full pre-rewrite backup (all 204 refs): `%TEMP%\pane-history-backup.git`.
  - GitHub has only `main`, `v0.1.0`, `v0.1.1` and `archive/docs-history-2026-10-23` (restored as a parentless snapshot; the docs link to it 92 times, so never delete it).
  - Moving a release tag: force-push the tag, never delete it (a deleted tag turns its release into a draft). A `v*` tag push starts release.yml, which stops on an existing release; cancel the run.
- **Not done:** the user skipped the in-app update test (0.1.0 -> 0.1.1) and said it isn't needed. Their PC still runs 0.1.0. Pane data backups `%TEMP%\pane-data-backup-before-setup` and `-before-v0.1.0` are kept until they update.
- **Open question for the user:** should Pane's own heading screens (package preview, confirmations) also get the top bar?
- **Open issues:** #451 (release spec; its children #475/#482 are closed), #68/#270/#108 (human checks), #104/#117 (leave them). Idea not filed: a "Paused" label for broken extensions.
- **This PC:** Defender excludes `nguyenvu`, `.cargo`, `.rustup` and `%LOCALAPPDATA%\go-build`. Builds from parallel workers froze the PC once; keep at most two full Pane builds at a time and don't run local workspace lints when CI covers them.
- Leave `nguyenvu\roboco-wt` and the user's other projects alone.

