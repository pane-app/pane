# Breaking changes

The record of the extension API's breaking changes, the committed source
each Pane release's notes carry: one **Breaking changes** section per
extension API version, each change shown with code before and after, for
Rust and for JS/TS, so updating an extension is mechanical
([ADR 0046](adr/0046-every-breaking-change-to-the-extension-api-gets-a-new-version.md)).
Each Pane release whose extension API moved carries the sections of the
versions it moved, as they are written here.

The shape is the rule (ADR 0046): breaking changes are gathered into
deliberate API versions, never shipped in a patch release, so the record
holds one section per version, not per change. A section is written when
its version moves — 0.2's was written with
[#320](https://github.com/pane-app/pane/issues/320)'s bump — and
`cargo xtask ci-lints` refuses a version without its section
([xtask/src/release_notes.rs](../xtask/src/release_notes.rs)): the WIT
guard's sibling ([#319](https://github.com/pane-app/pane/issues/319)
refuses a breaking change without a new version; this refuses a new
version without its announced changes).

A section's heading is a line of its own, exactly `## Extension API X.Y`
with the version's MAJOR.MINOR and nothing more: the level-two headings
of this file are its sections, one per version, and the check looks for
that line alone — a heading of another level, one with more on its line,
or the heading quoted inside a code block is not a section. Under the
heading, one entry per change: what changed and why, then the code before
and after — the WIT first, then the SDKs an extension author calls it
through, the Rust `pane-extension` crate and the `@pane-app/extension`
npm package — each snippet true to the code as it is in this repository.
A section holds only its version's breaking changes; everything else a
release's notes say is that release's own.

## Extension API 0.2

`show-hud`'s `style` parameter gets its own `hud-style` type
([#320](https://github.com/pane-app/pane/issues/320)): a HUD was styled
by `toast-style`, whose semantics differ for a HUD. A success toast hides
after 3 seconds, pausing while the pointer is over it; a success HUD
hides after 1.2. An `animated` toast stays until it is updated or hidden;
an `animated` HUD hides like a success does. A toast can carry actions
and replaces the one shown; a HUD has no actions and never queues.
Toasts keep `toast-style`: nothing that shows a toast changes.

**WIT** ([wit/feedback.wit](../wit/feedback.wit); the 0.1 world is
[wit/previous/feedback.wit](../wit/previous/feedback.wit)) — the style of
`show-hud` names the new type:

```wit
// before (0.1)                        // after (0.2)
enum toast-style {                     enum toast-style {
  animated,                              animated,
  success,                               success,
  failure,                               failure,
}                                      }
                                       enum hud-style {
                                         animated,
                                         success,
                                         failure,
                                       }
show-hud: func(title: string,          show-hud: func(title: string,
  style: toast-style);                   style: hud-style);
```

**Rust** (the [pane-extension](../guests/pane-extension/src/feedback.rs)
SDK) — `feedback::show_hud`'s style parameter is a `HudStyle`:

```rust
// before                               // after
use pane_extension::feedback::{         use pane_extension::feedback::{
    ToastStyle, show_hud,                   HudStyle, show_hud,
};                                      };

show_hud("Saved", ToastStyle::Success); show_hud("Saved", HudStyle::Success);
```

**JS/TS** (the [@pane-app/extension](../guests/js/feedback.d.ts) SDK) —
`showHUD`'s style parameter is a `HudStyle`, and `ToastStyle` is a
toast's own:

```ts
// before                                // after
import { showHUD, type ToastStyle }      import { showHUD, type HudStyle }
  from "@pane-app/extension/feedback";     from "@pane-app/extension/feedback";

const style: ToastStyle = "success";     const style: HudStyle = "success";
showHUD("Saved", style);                 showHUD("Saved", style);
```

A JS/TS call that passes the style string itself —
`showHUD("Saved", "success")` — is unchanged, the values being the same
strings in both versions: what changed is which type the SDK's
declarations name for the parameter, so a style kept in a
`ToastStyle`-typed variable where a HUD's style is meant is kept in a
`HudStyle`-typed one.

Extensions built for 0.1 keep running: this Pane runs them through the
adapter until the next Pane release — the window ADR 0046 gives their
authors — and their page names the version they were built for. Official
extensions need 0.2 releases before the Pane release that pins them.
