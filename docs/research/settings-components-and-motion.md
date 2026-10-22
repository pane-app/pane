# Settings components, motion, and searchable selects

Research supplement, 2026-10-02. This records the user's clarification that new components must adopt Pane's current design, with Roboco as the motion reference and Raycast searchable selects as a behavior reference. It does not implement or publish changes to [specification #70](https://github.com/hoangvu12/pane/issues/70).

## Recommended direction

Keep Pane's visual language throughout Settings: embedded Geist, semantic dark/light colors, restrained lime accent, thin borders, rounded control chrome, selection washes, and visible keyboard focus. Source these from the existing shared theme rather than creating a second palette for Settings. The retained Appearance board guides composition; controls outside the published scope remain reference-only. See [component evidence](settings-component-design.md), [current appearance](../launcher-appearance.md), and [retained reference](https://github.com/hoangvu12/pane/blob/archive/impl-ui-91-2026-10-06/docs/evidence/ui-prototype/reference/REFERENCE.md).

Use a shared searchable-select control, styled as Pane, for actual dropdown consumers. Search is an option of that control: long or variable option sets benefit most; tiny fixed choices can retain direct selection. This placement is a recommendation, not a user-approved threshold. Keep theme previews as cards if that best matches the retained design; do not invent new settings merely to demonstrate a combobox. See [Raycast/shadcn evidence](searchable-settings-selects.md).

Keep the option being browsed separate from the saved setting. Opening and filtering must not change the preference; Enter or clicking an enabled option commits it. Escape dismisses without changing the prior choice and restores focus. Tab should close and continue normal traversal without an implicit commit. An empty result cannot be committed. Preserve IME composition and ordinary text editing, and prevent dropdown keys from reaching launcher actions. These are proposed Pane semantics, informed by the [WAI combobox pattern](https://www.w3.org/WAI/ARIA/apg/patterns/combobox/), rather than claims about Raycast's closed-source Settings implementation. Native accessibility needs GPUI/AccessKit semantics and native verification; HTML ARIA attributes cannot simply be transplanted.

## Motion implementation boundary

Use Roboco's concrete motion evidence to select a small set of shared timings and easing curves; see [motion research](roboco-motion-settings.md). Keep frequent typing and keyboard selection immediate. Settings transitions must remain interruptible and must not defer input, persistence, or focus until animation completion. Retain an exiting popup visually only if its interactive lifecycle is already closed, so keys and pointer events cannot reach stale controls.

| Roboco source behavior | Proposed Pane use |
| --- | --- |
| Menu in/out: 140/100ms, `ease` `(0.25,0.1,0.25,1)` | Starting timings for ellipsis and dropdown popups; consider ease-out for Pane entrances after visual review. |
| Dialog entrance: 180ms, `ease` | Short confirmation/recorder overlay transition where an overlay is actually needed. |
| Hover color: 150ms, `(0.4,0,0.2,1)` | Pointer feedback; keyboard focus and active option remain immediate. |
| Collapse: 180ms ease-out; resize: 200ms ease-out | Optional disclosure continuity; profile any animated layout in GPUI. |
| 500ms entrance, 420ms new-thread morph, transcript scrolling spring | Reference-specific effects; do not adopt these as default launcher interaction timings. |

These values come from Roboco's [pinned catalog](https://github.com/hoangvu12/roboco/blob/10d78abca61ce800cd06239be70fd129e7d1a6fa/crates/proto/src/motion.rs). Its [native helpers](https://github.com/hoangvu12/roboco/blob/10d78abca61ce800cd06239be70fd129e7d1a6fa/crates/ui/src/motion.rs) explicitly approximate web menu/dialog scaling with fade and translation at their renderer revision. Match the resulting native experience deliberately; source timings alone do not establish visual parity. Transcript spring constants use their own integration model and should not be copied numerically into GPUI spring configuration.

The renderer already provides the needed foundations. Pane pins GPUI CE revision `bcf3a0acd047c1873293069d0ed42085a38f699b` in [its manifest](../../crates/pane/Cargo.toml). At that exact revision, [`AnimationExt`](https://github.com/hoangvu12/gpui-ce/blob/bcf3a0acd047c1873293069d0ed42085a38f699b/crates/gpui/src/elements/animation.rs) provides duration/easing animation and stateful springs; stable spring IDs preserve position and velocity across target changes. This establishes API availability, not a tested Settings implementation.

The same animation wrapper respects reduced motion and stops scheduling animation frames for its static state. However, [`App`](https://github.com/hoangvu12/gpui-ce/blob/bcf3a0acd047c1873293069d0ed42085a38f699b/crates/gpui/src/app.rs) initializes the flag to false and exposes `set_reduce_motion`; this research does not establish OS preference synchronization. Wire and verify the preference before claiming support. Pane's existing `request_animation_frame` in [app.rs](../../crates/pane/src/app.rs) repairs scroll positioning after relayout; it is not an existing decorative-motion system.

## Fit into existing work

| Existing issue | Integration guidance |
| --- | --- |
| [#71](https://github.com/hoangvu12/pane/issues/71) footer | Reuse theme and shared keycap/action chrome; keep invocation immediate. |
| [#72](https://github.com/hoangvu12/pane/issues/72) Settings shell | Establish themed controls and popup focus/lifecycle behavior with real consumers; add restrained shell/page motion. |
| [#73](https://github.com/hoangvu12/pane/issues/73) Appearance | Carry shared tokens into both windows and previews; separate tentative dropdown navigation from saved preferences and save errors. |
| [#75](https://github.com/hoangvu12/pane/issues/75), [#76](https://github.com/hoangvu12/pane/issues/76), [#77](https://github.com/hoangvu12/pane/issues/77) command/local bindings | Reuse control and focus treatment; preserve recorder cancellation and text-input ownership. |
| [#78](https://github.com/hoangvu12/pane/issues/78) Launcher | Reuse selection control for supported placement/reopening choices; do not add a display inventory outside its scope. |
| [#83](https://github.com/hoangvu12/pane/issues/83) Settings search | Keep global Settings search distinct from filtering one dropdown's options. |
| [#84](https://github.com/hoangvu12/pane/issues/84) validation | Check rapid open/close, interruption, focus return, IME, no-results, save failure, reduced motion, narrow/scaled layouts, and dark/light/opaque/glass behavior. |

No new ticket decomposition is necessary to explain this direction. Updating the published acceptance criteria is separate from this research. No application code, builds, tests, native animation recordings, or GitHub mutations were performed by this research task.

## Evidence scope

Pane source was inspected at `a74beae136b257bd41ef2688823c1d68925cc106`. GitHub issue bodies and renderer source were read through authenticated `gh` CLI; official web documentation was checked separately. The linked focused notes distinguish source behavior from proposed Pane adaptations. Existing untracked work was preserved.
