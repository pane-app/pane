# Retained launcher UI proof

Evidence for [#62](https://github.com/hoangvu12/pane/issues/62), under
[the UI specification](https://github.com/hoangvu12/pane/issues/61).
Captured from the read-only `pane-ui-prototype` worktree on 2026-10-02.
These files preserve the prototype; they are not production dependencies.

## Primary sources

- [Authored reference](reference/launcher.html), an unchanged self-contained
  HTML snapshot; SHA-256
  `f7e81e030e2216fe61509b9afd98a68147d1998c73f0a168bab02d0bf00f0bb4`.
  [Reference inventory](reference/REFERENCE.md) records extracted design facts
  and limitations. Depicted store, pinned slots and other screens do not
  authorize those features.
- [Recorded interview](reference/ui-rework-interview.md), copied unchanged
  from the main worktree. Its early no-frost checkpoint predates the later
  glass result below.
- [Prototype review and run instructions](prototype-review.md), copied from
  the glass follow-up. Its original relative paths resolve in a restored
  prototype. Reviewed captures are also directly available here:
  [dark](captures/dark.png), [light](captures/light.png),
  [submitted form](captures/light-form.png),
  [narrow error](captures/narrow-error.png),
  [unavailable action](captures/narrow-unavailable.png),
  [dark glass](captures/dark-glass.png), [light glass](captures/light-glass.png).
- [Native capture script](capture-pane.ps1),
  [native validation history](native-validation.md), and
  [external-background comparison](check-backdrop-response.ps1).

## Two separately recoverable stages

1. [initial.bundle](initial.bundle) retains the `prototype/launcher-ui` ref
   at `9bf02e782de359c1d8560cb96a93b9c50da9fc8f`. This Git bundle contains the
   initial proof, fonts/licenses, scripts, screenshots and tests. It requires
   base commit `748d71e3a2969857202d96f594f676976f062765`, retained in Pane's
   history. **The initial commit does not contain the glass fix.**
2. [glass-follow-up.zip](glass-follow-up.zip) is the subsequent, previously
   uncommitted state, saved independently rather than applied to production.
   It includes all six changed tracked files, the complete copied Windows
   renderer and its licenses/provenance, refreshed captures, native capture
   scripts/metadata and review/test logs. Each archived file has its original
   byte count and SHA-256 in [the manifest](glass-follow-up-manifest.json).
   [glass-follow-up.patch](glass-follow-up.patch) makes the tracked source
   changes reviewable; the archive also supplies the untracked renderer and
   captures needed to reproduce them. Runtime data/cache folders and the
   unrelated phone-preview infrastructure are deliberately excluded.

The original branch, index, working files, untracked evidence and services
were not modified or removed. No prototype visuals, copied renderer or
phone-preview service enter the application build through this archive.

## Restore in a separate checkout

From a full Pane clone containing the base commit, in PowerShell (choose an
unused destination; never extract over the retained original worktree):

```powershell
$evidence = (Resolve-Path docs/evidence/ui-prototype).Path
git bundle verify "$evidence/initial.bundle"
git clone --no-checkout . ../pane-proof-restored
git -C ../pane-proof-restored fetch "$evidence/initial.bundle" refs/heads/prototype/launcher-ui
git -C ../pane-proof-restored checkout --detach FETCH_HEAD
# This checkout is now exactly the initial proof.
Expand-Archive -LiteralPath "$evidence/glass-follow-up.zip" -DestinationPath ../pane-proof-restored -Force
# It now includes the separate glass follow-up; do not also apply its patch.
Set-Location ../pane-proof-restored
cargo build -p pane --locked
cargo xtask guests
cargo test -p pane --test window --test command_search --locked
& ./.scratch/native-review/capture-pane.ps1 -Binary ./target/debug/pane.exe -ExtensionsDir ./target/guests -OutputDir ./.scratch/native-review/manual -Theme dark -Material opaque
```

Guest builds require the existing documented Rust/JS/TS toolchains; see
Pane's contributor instructions and `docs/ui-prototype-review.md` in the
restored checkout. The helper uses isolated data/cache folders and guards
input against the spawned window. Coordinate desktop access before running
native capture. `-Theme light` and `-Material glass` select the other modes;
do not change OS transparency settings automatically.

## Provenance and limits

The prototype uses GPUI CE
`17d9c8e8fdb30a329d817ca06bff424e8e848f1a` and the existing
`gpui_ce_elements` editable input. Its local `gpui_ce_windows` copy adapts
ordinary-scene and path-sprite destination alpha to source-over
(`INV_SRC_ALPHA`), following Roboco's Zui
`ec16c62b83caa58b61cd8039db4b5721bacf948f`.
See [renderer provenance](renderer-provenance.md). This temporary dependency
is evidence only; [#63](https://github.com/hoangvu12/pane/issues/63) owns the
maintained production fork.

The glass follow-up also removes full-panel outer shadows and raises light
tint to 80%; dark tint remains 70%. Source-over alone did not establish
useful glass: the no-shadow experiment already showed blur without it.
The recorded dark-region mean RGB response of 18.086 versus the earlier 0
shows transparency. Reviewed softened external pattern edges supply the
Windows blur evidence; the numeric response is not a portable blur threshold.

The archived proof records **50 window tests plus one command-search test**
passing, including its additional long-status regression. The production
prefactor retains the earlier behavior suite; it does not import that visual
regression or the redesigned footer. [Glass test log](glass-final-tests.log).
Native evidence is Windows at 96 DPI: root search/Escape, forms and validation,
successful submission, narrow layouts/errors and unavailable actions.
macOS/Linux and scaled displays were not native-verified by this proof.
Simulated IME and accessibility-tree tests establish no native IME or
assistive-technology operation. Native wheel automation was not established.
Capture runs with failed focus are archived for context, not counted as
successful interaction evidence.
