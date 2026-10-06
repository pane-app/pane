# Approved UI port scope and data contracts

Authoritative tracker record: [#100](https://github.com/hoangvu12/pane/issues/100); specification: [#90](https://github.com/hoangvu12/pane/issues/90). This local document retains the research decision record, not duplicate implementation tickets.

Approved 2026-10-04 when the user accepted the proposed specification/ticket breakdown and delegated the remaining choices. This records bounded implementation choices, not completed application work.

## Verification and scope

Windows UI only. Use the production-component comparison workbench, real feature-adapter scenarios, and scoped local Windows checks. No CI, other-OS validation, release matrix, push or packaging gate. Retain GPUI CE, the current feature/shared-UI organization, existing editable input/select state, stable operation identities and extension-owned custom drawing. Match the concrete reference board over conflicting design-language summaries. Windows modifiers, extension terminology, light/narrow adaptations and existing safety/status affordances are intentional adaptations. Exact material parity still requires the local rendering proof; this approval does not certify an unmeasured approximation.

## Pinned home contract

> Superseded in part by [ADR 0027](../../adr/0027-quick-slots-are-an-ordered-list.md) (#105): the quick slots are now an ordered list of pins with no gaps and no limit, so the five places, the explicit replacement choice, empty slot outlines, Move left/right as the only moves and Ctrl+1 to Ctrl+5 as the only slot chords below are history. Identity, resolution and the record rules still hold. The current contract is in [root search § pinned home](../../root-search.md#the-pinned-home).

Include five ordered host-owned quick slots. Each optional slot contains either a registered command identity or an indexed-result identity scoped to its owning command/package. Persist identity and ordering in a versioned host record alongside launcher settings, not extension data. Never persist a row index, title-derived key, ephemeral computed answer or granted-file handle as a pin. Resolve identities through the current enabled command/indexed-result registry; a cold root visit may request the existing indexed provider for a saved pin without pretending a query was typed or introducing a new search/indexing engine.

Pin to Quick Slot appears in the contextual menu for eligible real results. It fills the first empty slot; when full, show the five slots and require an explicit replacement choice. Re-pinning an existing target is a no-op that focuses its current slot. A slot's contextual action can remove it or move it left/right; first/last moves are unavailable at boundaries. Empty positions remain empty slot outlines with an accessible hint to pin a search result; they invoke nothing. Fresh installation has no fabricated pins. Fixtures contain five authored sample entries only for visual comparison.

Five-slot order is stable across restart. Disabled, paused, missing and temporarily unresolved targets retain their slot, show the actual unavailable reason, and do not dispatch a stale target; users can still remove them. Enabling/reinstalling the same identity can resolve the pin again. Revalidate immediately on invocation, and reject late results from a replaced/uninstalled generation. Save atomically; on failure restore the saved arrangement and report the failure. An unreadable record is retained and reported, not silently overwritten.

Use local Ctrl+1 through Ctrl+5 for slot activation in the root window, not global registrations. They act only when root search owns focus, no IME composition or overlay is consuming the key, and the slot resolves to an available target; otherwise no command runs. Display these actual Windows chords. A nonblank trimmed query hides the strip; clearing restores it. Keep the same section-header and row visuals, but populate the remaining blank-query list from existing real root results in their existing order. Label it Commands/Results as appropriate; do not claim recent-use suggestions or introduce frecency/telemetry.

## Actions and root behavior

Include real primary action, existing command alias/hotkey configuration when applicable, and the quick-slot operations above when their ticket lands. Do not expose fake-enabled quit/new-window/hide operations. Keep the app-wide Settings entry distinct from the contextual Actions panel. D05 can ship its supported action subset before the pin ticket adds pin operations.

Adopt reference mouse-movement selection and click sequencing for root search. A stationary pointer does not undo keyboard selection; the selected identity drives the footer and Enter. Freeze the underlying target while Actions is open. An unselected click without preceding movement selects first, while selected-row click invokes once. Command/command-search rows share visuals and retain existing invocation semantics. Fallbacks remain unselected until deliberate selection. Preserve IME, keyboard focus, stale-query cancellation and unavailable reasons.

## Clipboard split-view contract

Keep the existing text-only capture, opt-in, pause/resume, retention, exclusions, copy, delete and disable semantics. No images/colors acquisition, clipboard pinning, paste-to-previous-app, syntax guessing or network preview is added. Production offers All and Text filters over the supported text records; reference Links/Images/Colors and Pinned grouping remain fixture-only. Plain text is the default preview; code/image/color/link fixture variants establish visuals but do not classify a user's content automatically.

Add a narrow host-owned read-only presentation projection for the registered Clipboard History default-extension command. Identify it by verified package/command identity, never localized title or a matching row string. The projection exposes the owning package, opaque record ID, full stored text, copied timestamp/age, optional actual source application, actual capture state, and actual operation availability. It reads the existing package-scoped clipboard store; it creates no second history store, clipboard watcher or new capture capability. No new general-purpose guest/WIT view protocol is required for this bounded default-extension presentation. Generic third-party command views retain their existing rendering.

The feature adapter owns query, selected record and preview state. Search case-insensitively within stored text and known source; preserve the store's newest-first order and use Today/Yesterday/Older groups in local time. Select the first visible record for preview after filtering removes the old selection; zero records means no preview and no primary action. Pointer click selects, Up/Down move selection and keep it visible; Enter and the footer's Copy perform the existing copy operation. Delete is explicit and uses the existing delete operation. Revalidate owner/record/generation before any operation; expiry/deletion invalidates stale selection. Preserve or route to the current management controls for opt-in, pause/resume, retention, exclusions, clear and turn-off/delete. Only registered Clipboard History receives this presentation; a similarly titled extension gets no access to another package's records.

## Appearance and deferred capabilities

Production retains system/light/dark theme and glass/solid material, with live propagation, persistence, save-failure rollback, environment overrides and truthful Windows fallback feedback. Port those controls into the reference's segmented-choice/page/preview families. Frost, accents, custom color, blur/tint sliders, density, pinned visibility and footer-tip preferences remain development-fixture variants, not new saved settings in this milestone. In the full reference fixture Solid disables both sliders, retains their values and uses the authored disabled opacity. A persisted value with no actual renderer effect is not a working setting.

Calculator uses existing computed data/copy operations. Unit/time conversion, calculation history and paste are not added. Empty-state extension suggestions require real data and otherwise stay fixture-only. Store and snap HUD are fully catalogued reference screens, not production features in this milestone. Their catalog/acquisition/window-manipulation backends, mock permission/sandbox claims and fictitious metadata are excluded.

## Dependency disposition

The planning prerequisite records these resolved choices and is completed as planning only. Pin and clipboard implementation tickets contain their respective full contracts and become ready-for-agent subject to their real visual blockers. No human-answer blocker remains. Final acceptance covers the agreed current-feature UI plus pins and supported text clipboard; it must enumerate deferred reference-only capabilities and cannot claim an all-nine-feature implementation.
