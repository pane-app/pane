# Roboco motion reference for Pane Settings

Researched 2026-10-02. Bounded source audit for the current Pane Settings
direction. No Pane or Roboco UI was run, so “seen” below means seen in source,
not visually measured from a recording.

## Provenance

- Local `../roboco` remote: `origin` is `https://github.com/hoangvu12/roboco.git`;
  `upstream` is `https://github.com/zeronsh/zeron.git`.
- Local tracked revision: [`10d78ab`](https://github.com/hoangvu12/roboco/commit/10d78abca61ce800cd06239be70fd129e7d1a6fa)
  (`chore: release v0.6.0`), confirmed with `gh api`; the checkout has one
  unrelated untracked `stripes_check.png`, which was not used.
- All Roboco links below are pinned to that exact commit. The GitHub
  `v0.6.0` tag resolves to a different annotated-tag target, so it is not used
  as the source pin.

## Source-audited motion catalog

The canonical catalog is [`crates/proto/src/motion.rs`](https://github.com/hoangvu12/roboco/blob/10d78abca61ce800cd06239be70fd129e7d1a6fa/crates/proto/src/motion.rs):

- Curves: `easeOutExpo` = `(0.16,1,0.3,1)`; `easeOut` =
  `(0,0,0.58,1)`; `ease` = `(0.25,0.1,0.25,1)`; `easeOutQuint` =
  `(0.22,1,0.36,1)`; `easeInOut` = `(0.42,0,0.58,1)`; Tailwind hover
  curve = `(0.4,0,0.2,1)`.
- Utility timings: fade-in 500ms; quick fade 150ms; menu in/out 140/100ms;
  dialog in 180ms; resize 200ms; tab slide 150ms; collapse 180ms;
  new-thread transition 420ms; chevron 200ms; scroll glide 500ms.
- Brand/ambient timings: Roboco pulse 2400ms; gradient spin 750ms; splash
  out 500ms after a 150ms delay.
- Spring constants in the same file: damping `0.7`, stiffness `0.05`, mass
  `1.25`, 60fps frame step, max catch-up `8` frames, settle grace `500ms`.
  These belong to transcript stick-to-bottom behavior, not every control.

## Consumers

- Native GPUI helpers in [`crates/ui/src/motion.rs`](https://github.com/hoangvu12/roboco/blob/10d78abca61ce800cd06239be70fd129e7d1a6fa/crates/ui/src/motion.rs)
  consume the catalog for fade, menu, dialog, splash, loader pulse and
  hover-color fades. Hover fades are manually driven because GPUI hover
  styles otherwise snap.
- Web theme export publishes the same catalog as `--rb-ease-*` and
  `--rb-motion-*` variables in [`web/packages/theme/src/index.ts`](https://github.com/hoangvu12/roboco/blob/10d78abca61ce800cd06239be70fd129e7d1a6fa/web/packages/theme/src/index.ts).
  The web app consumes them for hover washes, tab transforms, titlebar/resize
  transitions, and sidebar/pane width transitions in `app.css`.
- Transcript-specific consumers are in [`web/packages/app/src/lib/tool-motion.ts`](https://github.com/hoangvu12/roboco/blob/10d78abca61ce800cd06239be70fd129e7d1a6fa/web/packages/app/src/lib/tool-motion.ts):
  fold 140ms `easeOut`, row reveal 360ms `easeOutExpo`, connector reveal
  480ms `easeOutQuint`. Stable row IDs keep fold/reveal state across remounts;
  restored folds render settled rather than replaying arrival motion.
- For Pane Settings, menu/dialog timings are the relevant starting reference.
  Keeping typing, selection, commit, focus return and persistence immediate is
  the proposed Pane contract, not a verified audit of every Roboco Settings path.

## Reduced motion

The [web clock](https://github.com/hoangvu12/roboco/blob/10d78abca61ce800cd06239be70fd129e7d1a6fa/web/packages/app/src/lib/tool-motion.ts) reads live `prefers-reduced-motion: reduce`; reduced mode emits
one snap tick and arms no animation loop, and a mid-flight preference change
lands on the current frame then stops. Fold/reveal progress returns its endpoint.
The native helper exposes `set_reduced_motion`/`reduced_motion`; GPUI's global
flag snaps `with_animation` elements and schedules no frames. This is source
behavior, not a claim that Pane currently receives the OS preference.

## Pane adaptation

Pane's pinned GPUI CE revision [`bcf3a0a`](https://github.com/hoangvu12/gpui-ce/blob/bcf3a0acd047c1873293069d0ed42085a38f699b/crates/gpui/src/elements/animation.rs)
provides `with_animation` and `with_spring`; stable IDs preserve spring
position and velocity when a Settings target changes. Use a small Pane-owned
catalog: 140ms ease-out for popup/chevron, 180ms ease-out for disclosure,
200ms ease-out for shell resize, and 150ms Tailwind-like color fade. These are
proposed adaptations: Roboco's menu/dialog catalog uses `ease`, and its chevron
uses 200ms. Reserve springs for interruptible movement, not text input or save
feedback. Do not transplant transcript spring constants into GPUI's different
integration model or add a slider outside the milestone's scope.

Keep Settings controls on Pane's existing semantic theme/material, Geist
typography, focus ring and row primitives ([component baseline](settings-component-design.md)).
Prefer opacity and positional motion; layout animation needs profiling and
should not resize the native window continuously. Do not delay input,
persistence, focus, or native actions. Use stable control/option IDs, cancel visual exits before
reusing interactive content, and snap all endpoints under reduced motion.

`App.set_reduce_motion` currently defaults false; OS wiring is unverified.
Wire and verify that preference before presenting reduced-motion support as a
product guarantee. This note records source audit only; it contains no UI run,
benchmark, or app change.
