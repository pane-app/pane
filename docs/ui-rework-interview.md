# UI rework interview log

> Historical: this log scoped the first UI slice (#61). Pinned slots, Actions and Settings, which it deferred, came later with [#70](https://github.com/hoangvu12/pane/issues/70) and the Windows UI port ([#90](https://github.com/hoangvu12/pane/issues/90)).

Dated log of user decisions and open questions for the launcher UI rework.
This is design continuity, not a spec, ADR or ticket; nothing here changes
[CONTEXT.md](../CONTEXT.md) or the [decision index](current-decisions.md), and
no new domain terms are resolved. Status words follow the index: a recorded
choice here supersedes earlier drafts of this rework only, not accepted ADRs.

## 2026-10-02 — decisions recorded (via the orchestration chat)

- **Visual truth** is the user's reference page:
  `.scratch/ui-reference/launcher.html` (byte-identical copy of the user's
  source; extracted tokens and page inventory in its `REFERENCE.md`). The
  earlier screenshot-derived DESIGN.md and all generated preview HTML are
  scrapped — history only, not a source of truth.
- **Launcher first**: the milestone reworks the root launcher surfaces before
  other screens.
- **Shared tokens + frosty materials are required**: one shared token layer and
  frosted/glass surfaces are part of the direction, not optional polish.
- **Maintainable feature/shared-UI organization is required**: shared visual
  code must have a home that feature work does not scatter; module-ownership
  proposal in `.scratch/ui-rework-structure.md` (read-only proposal).
- **Process**: the orchestration chat owns visual judgment; GLM-based workers
  have no vision, so visual fidelity claims must come from the orchestrator or
  the user — worker audits are DOM/geometry only. Workers stop, confirm, idle
  and send before any side-chat followups.

## 2026-10-02 (later) — scope settled

- **First slice only**: a working launcher styling/frost/shared-token slice.
  The native launcher's **actual working search must be preserved** — the real
  query/list/keyboard behavior, not a dummy painted screenshot. Closes the
  earlier first-slice questions: pinned slots, action-panel inclusion, ranking
  changes, settings screens, store and window manager are all **later** work,
  out of this slice.
- **Appearance**: dark reference-first, and **light must work now** through the
  semantic token layer (closes dark-only vs light/dark). A temporary theme
  choice may be an env/CLI switch — no new settings feature in the slice.
- **Native prototype**: an isolated worktree at
  `C:/Users/ADMIN/Desktop/nguyenvu/pane-ui-prototype`, branch
  `prototype/launcher-ui`, created by main; another worker implements there.
  The main checkout carries docs/research only for this rework.

## 2026-10-02 (later) — folder structure agreed

After reviewing the seven-codebase research and a concrete tree, the user
accepted starting with feature folders and shared UI inside `crates/pane`:

- `app.rs` owns window orchestration, navigation and action dispatch;
  `main.rs` owns startup and window creation; `lib.rs` exposes entry points.
- `features/root_search/` owns search presentation and editable-query wiring.
  Other product features gain folders when implemented, not empty placeholders.
- `extension_views/` owns form and custom-view rendering for extensions.
- `ui/` owns semantic dark/light tokens, material/fallback policy, icon treatment
  and reusable visual components such as result rows and keycaps. It does not
  own launcher decisions or depend on `pane-core`.
- `pane-core` stays GPUI-free; existing native opening adapter, assets and
  integration tests retain their distinct roles.
- A separate `pane-ui` crate remains a possible later extraction, not part of
  the agreed starting tree. Split internal files where useful, not mechanically.

This supersedes the earlier `screens/` organization proposal in
`.scratch/ui-rework-structure.md`. Research is recorded in
`.scratch/folder-structure-research.md`; it informs the choice without making
every research claim an accepted design rule.

The ask-matt route is to finish orchestrator technical review, obtain a native
prototype with dark/light and actual frost evidence, and review it with the
user before the broader spec/ticket/implementation flow. The folder decision
does not settle toolkit adoption or authorize deferred product features.

## Open evidence questions (verified by the prototype, not asked of the user)

### Orchestrator checkpoint after resuming technical review

The isolated CE Base patch now passes `cargo check` against Pane's pinned
renderer: nine files, 18 insertions and two deletions. It imports `AppContext`,
adds the renderer's existing dash defaults to four quads, and converts two
border colors to `Background`. This corrects the earlier claim that the
compiler failures justified abandoning Base. Evidence is in the prototype's
`.scratch/ce-compat/logs/`. The styled layer has six remaining errors across
three further categories after the same mechanical fixes: ambiguous
`on_prepaint`, renamed deferred-priority calls, and an asset-source accessor.
These are characterized in `.scratch/ce-compatibility-review.md`; the probe
does not establish an infeasible port or runtime/platform compatibility.

For the first native visual proof, the orchestrator chose the existing
`gpui_ce_elements` input with Pane-owned tokens, materials and visual
components. The preserved input already exposes the required theme colors;
root results retain their existing selection, activation and accessibility
semantics. Base's popup combobox is not a direct fit for always-open root
search, and no new controls or motion engine are needed for this slice.
This is a scoped implementation choice, not a permanent rejection of Base.
The compatibility patch is retained for later adoption evaluation. Source
review: `.scratch/ui-component-fit-review.md`.

- **CE fork Base/Components compatibility**: whether `gpui-ce/gpui-component`
  (pins gpui-ce 0.2.2 from crates.io) compiles and behaves against Pane's
  gpui-ce git rev `17d9c8e8` (same version string, divergent APIs) — and whether the
  headless Base layer, the styled Components layer, or neither is adopted.
  The isolated Base compatibility patch now checks successfully as described
  above. Runtime integration and the further styled-layer port remain open;
  the first visual proof uses the existing input and Pane-owned visuals.
- **Windows frost capability**: actual backdrop blur (Acrylic/D3D) vs a
  graceful opaque fallback on Windows. Linux window-glass stays opaque (no
  guaranteed compositor blur); scene-level in-window blur works on all three
  platforms (roboco evidence, research note).

## Evidence status

### Native proof checkpoint — 2026-10-02

The isolated `prototype/launcher-ui` worktree now captures the folder move
and shared native visuals in commit `9bf02e7`. Review and run instructions:
`C:/Users/ADMIN/Desktop/nguyenvu/pane-ui-prototype/docs/ui-prototype-review.md`.
Saved dark/light and narrow-window screenshots live beside that report in
`docs/ui-prototype/` in the prototype worktree.

The original 50 behavior tests passed after the move and after styling.
A further regression test for long status messages brings the final total
to 51; it verifies wrapping, bounded growth and wheel scrolling to both
ends. Main personally reviewed native dark/light roots, forms, validation,
successful submission, selected unavailable actions and narrow errors.
No dependency or core changes were needed; unused future components were
omitted rather than retained as placeholders.

Desktop frost remains unverified: Windows transparency effects were off
(`EnableTransparency=0`), and external light/dark pattern captures showed
no visible backdrop difference. The system setting was not changed.
Opaque mode is captured and readable. Other platforms and DPI scales are
not runtime-verified. Broader specs/tickets and production integration stay
after user review of this native proof; the prototype has not been merged
into main.

Read-only research (`.scratch/ui-rework-research.md`) distinguishes:

- **Current code** facts (hardcoded colors, minimal screens) — real, but not a
  decision to keep them.
- **Accepted/provisional docs** — ADR 0003 and the decision index remain
  authoritative; earlier *provisional* implementation details (e.g. #45/#46
  preview geometry) stay provisional and are not approved by this rework.
- **Reference-authored visuals** — the token/material values in
  `.scratch/ui-reference/REFERENCE.md` are the visual target; the reference's
  pages do not authorize features (Plugin Store is deferred Q35; the snap HUD
  has no ADR; pinned slots exist in no contract).
