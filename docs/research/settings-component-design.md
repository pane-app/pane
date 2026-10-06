# Settings component design research

Researched 2026-10-02. This note is repository evidence for the Settings
milestone in [issue #70](https://github.com/hoangvu12/pane/issues/70). It does
not add product decisions, duplicate the Roboco motion work, or repeat the
Raycast/shadcn research.

## Design rule

New Settings components should be Pane-owned GPUI presentation in `crates/pane`
and consume the existing semantic theme/material layer. They should not bring a
second token authority, a web-style design system, or `pane-core` dependencies
into `ui/`. Local transient control state may live in `ui`; navigation/dispatch,
persistence, native integration, and domain policy stay in feature/app seams.
The retained UI rework decision puts shared visual code in `ui/` ([`docs/ui-rework-interview.md`](../ui-rework-interview.md), lines 46–57).

## Exact reusable contract

### Tokens

- **Text:** `text_title`, `text_body`, `text_muted`, `text_query`, and
  `text_placeholder`.
- **Surfaces:** `panel_tint`, `panel_solid`, `panel_sheen`,
  `panel_top_highlight`, and `footer_tint`.
- **Edges and states:** `hairline`, `hairline_soft`, `row_hover`,
  `row_selected`, `row_selected_border`, `focus_ring`, `accent_text`,
  `danger`, `warning`, and `success`.
- **Control chrome:** `tile_background`, `tile_foreground`, `tile_border`,
  `tile_highlight`, `tile_app_edge`, `tile_app_highlight`, and `tile_drop`.

These are the current semantic roles; Settings controls should map to them by
meaning rather than add per-control colors. Source: [`theme.rs`](../../crates/pane/src/ui/theme.rs#L42-L126).

### Typography and geometry

Use the embedded Geist family and existing type roles (`search_size` 19px,
row title 14px, subtitle 13px, annotation/footer 12.5px, medium weight).
The shared geometry is panel radius 18px where the platform exposes rounded
corners, 64px search header, 44px minimum row, 10px row radius, 12px row gap,
28px tile/7px tile radius, and a 50px footer. Source:
[`theme.rs`](../../crates/pane/src/ui/theme.rs#L129-L184) and the concrete values
at [`theme.rs`](../../crates/pane/src/ui/theme.rs#L286-L333).

### Surface primitive

Use `Material::panel(theme, content)` for the Settings window root and
`Material::footer(theme)` where a status/action strip is needed. The panel owns
the tint-or-solid choice, clipping, radius, hairline, and top sheen; the sheen
is explicitly non-interactive. `Material::new` normalizes glass to opaque on
platforms without compositor frost, so a Settings “glass” control must report
effective fallback honestly. Sources: [`material.rs`](../../crates/pane/src/ui/material.rs#L102-L170)
and [`material.rs`](../../crates/pane/src/ui/material.rs#L42-L64).

### Input primitive

Use the existing `gpui_ce_elements::editable_text::text_input` through the
current field wiring. It already receives Pane colors for placeholder, caret,
selection, marked IME text, and value text. Preserve its focus handle, normal
editing, and IME behavior; do not introduce a second input stack for Settings.
Source: [`form.rs`](../../crates/pane/src/extension_views/form.rs#L251-L285).

The root-search implementation is the closest accessibility pattern for a
Settings search field: a wrapper tracks the input focus, exposes
`Role::EditableComboBox`, and supplies label/value/placeholder semantics while
the real text input remains the editable child. Source:
[`root_search/mod.rs`](../../crates/pane/src/features/root_search/mod.rs#L150-L210).

### Row/list primitive

Use `result_row` only for result-like navigation entries, translating Settings
metadata into presentation values. It owns the current 44px-floor row geometry,
hover/selection wash, title/subtitle typography, and unavailable-reason
wrapping, but it deliberately registers no handlers or focus. Settings search
and sidebar code must own selection, activation, and focus restoration. Source:
[`result_row.rs`](../../crates/pane/src/ui/result_row.rs#L55-L147).

### Existing form controls

The current extension form is useful visual prior art, not a complete Settings
component kit: text fields use the shared border/tile/focus roles, submit uses
selected-row styling, and choices are rendered as a focusable `RadioGroup` of
radio-button rows. Source: [`form.rs`](../../crates/pane/src/extension_views/form.rs#L214-L237)
and [`form.rs`](../../crates/pane/src/extension_views/form.rs#L288-L372).

## Gaps: searchable dropdown / GPUI

Pane currently has no reusable Settings `Select`/searchable-dropdown primitive.
The existing `FieldKind::Choice` is a radio group, so it is appropriate for a
short visible set but not a long searchable list. The existing root search is
an always-open `EditableComboBox`, not a popup select.

No current pinned-renderer integration of a GPUI Base/Components Combobox is
established here; any historical scratch fit review is unverified evidence, not
a capability claim.

Therefore the Settings gap is bounded: define one Pane-owned, stateful
searchable-select primitive only when a real Settings consumer needs it. It
must own popup open/close, query filtering, highlighted option, selection,
keyboard escape/enter/arrow behavior, outside-click dismissal, focus return,
and accessible expanded/active-option semantics. It should compose the
existing `text_input`, tokens, and row-like option presentation; popup material
should use the reference's L2 role only if its exact values are added to Pane's
material API. The retained reference documents L2 as a 14px-radius popover with
its own tint, inset edge, and shadow, but says other pages were not yet
token-mined ([`REFERENCE.md`](https://github.com/hoangvu12/pane/blob/archive/impl-ui-91-2026-10-06/docs/evidence/ui-prototype/reference/REFERENCE.md#L37-L46),
[`REFERENCE.md`](https://github.com/hoangvu12/pane/blob/archive/impl-ui-91-2026-10-06/docs/evidence/ui-prototype/reference/REFERENCE.md#L81-L87)).

Do not silently treat the current radio group or root search as this missing
primitive, and do not claim the fork's popup Combobox is already integrated.

## Issue integration map

The parent specification says Settings should use the retained reference for
typography, surfaces, sidebar, and responsive layout, while Raycast screenshots
are organization guidance only. It also requires functioning sections rather
than placeholder controls ([issue #70](https://github.com/hoangvu12/pane/issues/70)).

| Issue | Component/design consequence |
| --- | --- |
| [#71](https://github.com/hoangvu12/pane/issues/71) | Reuse tokens, keycap treatment when implemented, and status/footer material; keep action state in launcher feature code. |
| [#72](https://github.com/hoangvu12/pane/issues/72) | Establish the Settings window shell: `Material::panel`, Geist/sidebar typography, responsive layout, titlebar hit-test seam, and page-registration seam. |
| [#73](https://github.com/hoangvu12/pane/issues/73) | Replace startup-only fixed visuals with observable host settings; reuse `Theme`/`Material`, but keep persistence/effective-state logic outside visual primitives. |
| [#74](https://github.com/hoangvu12/pane/issues/74) | General needs a recorder control; use existing focus/border/focus-ring roles and keep native registration/rollback in application/native seams. |
| [#75](https://github.com/hoangvu12/pane/issues/75) | Shortcuts needs searchable grouped rows and inline alias editing; reuse `text_input` and row/list presentation, with aliases/hotkeys supplied by existing core records. |
| [#76](https://github.com/hoangvu12/pane/issues/76) | Reuse the shared recorder presentation from #74; do not make the component own global registration or conflict policy. |
| [#77](https://github.com/hoangvu12/pane/issues/77) | Keycaps and visible hints must read effective bindings; action definitions and context remain app-owned. |
| [#78](https://github.com/hoangvu12/pane/issues/78) | Launcher placement/reopening controls use normal Settings fields and search registration; display/window policy remains native/application code. |
| [#79](https://github.com/hoangvu12/pane/issues/79) | General toggle is a semantic control backed by native tray/menu state; failure/effective-state messaging belongs above the visual primitive. |
| [#80](https://github.com/hoangvu12/pane/issues/80) | Launch-at-login uses the same field/toggle styling and host settings search registration; native registration is not a UI concern. |
| [#81](https://github.com/hoangvu12/pane/issues/81) | Extensions page must compose existing management operations and retained-data policy; do not invent a generic extension-settings form. |
| [#82](https://github.com/hoangvu12/pane/issues/82) | About uses typography/link/status primitives and existing update diagnostics; no invented telemetry or release state. |
| [#83](https://github.com/hoangvu12/pane/issues/83) | Defines the separate Settings search/catalog seam: registered host settings only, dynamic registration, keyboard/pointer navigation, and no arbitrary extension-data indexing. It is not dropdown search. |
| [#84](https://github.com/hoangvu12/pane/issues/84) | Integration validation must cover the shared visual contract across all pages, narrow/scaled layouts, accessibility/focus, persistence, and platform fallback; it is not a reason to add speculative controls. |

The child issue bodies and blockers were read with `gh issue view` and the
native sub-issue relationship from issue #70. No app or GitHub changes were
made. The reusable baseline is sufficient for the first Settings shell and
ordinary fields; the missing shared primitive is a searchable select, to be
added only for a concrete consumer.
