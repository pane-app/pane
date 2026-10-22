# Native launcher UI prototype

Prototype branch: `prototype/launcher-ui`, based on `748d71e`.
This is the native proof for design review, not a production rollout.

## Question and result

Can the agreed folder layout support shared native styling while retaining
the launcher's existing input and navigation behavior? The structure and
visual integration work: all 51 window/search tests pass, including the
new long-message regression test, and Windows captures show readable dark
and light launchers and forms. The prototype is ready for user design review.

Desktop frost is now **visually verified on this Windows host** after the
user enabled transparency. Initial captures with transparency enabled still
showed a solid panel. Comparing Roboco identified an additive-alpha bug in
the pinned Windows renderer; its source-over correction is carried in the
prototype's local Windows package patch. Removing Pane's outer panel shadows
was also necessary: GPUI paints them under the translucent panel, obscuring
the desktop. The inner highlight and border remain. Light tint is now 80%.

The final dark panel responds to external light/dark test patterns with a
mean RGB difference of 18.086 (previously 0), and visual inspection shows
softened edges of those sharp patterns. The numeric test proves transparency;
the softened edges supply the blur evidence. No global Windows setting was
changed by the agent. See [dark glass](captures/dark-glass.png) and
[light glass](captures/light-glass.png).

## Implemented structure

```text
crates/pane/src/
  main.rs                    startup and window creation
  lib.rs                     entry points and re-exports
  app.rs                     launcher state, dispatch and shared frame
  features/root_search/      query control and search presentation
  extension_views/           form and custom-view adapters
  ui/
    theme.rs                 shared colors, typography and geometry
    material.rs              panel/footer and platform material choice
    icon.rs                  embedded glyphs and gradient tiles
    components/result_row.rs presentation-only result row
  links.rs                   system link opening
```

`ui/` imports no `pane-core` types. The app maps existing stable IDs to
presentation values. Only used components are retained; there are no
placeholder directories for later features. Core is unchanged. The glass
follow-up adds a manifest/lockfile patch for `gpui_ce_windows`; see
[patch provenance](renderer-provenance.md).

The existing GPUI CE editable input remains responsible for editing, IME
and focus. Pane owns visual tokens, surfaces and row chrome. The separate
CE Base compatibility probe passed after a small patch; this prototype's
choice is not a permanent rejection of Base or the styled kit.

## Visual and behavior evidence

Dark follows the supplied reference's palette, Geist typography, panel
geometry, fine edges and gradient icon tiles. Light is a derived palette.
The proof contains the actual installed/sample commands, without fake
pinned slots, recency, action panels, settings or store features.

Main personally inspected native root search in both themes, a narrow
380x420 outer window, command results, unavailable-action explanations,
and form validation in both themes. A light native keyboard sequence also
submitted the form successfully. IME composition and accessibility trees
are covered by the test platform, not by the native SendKeys captures.

Review caught a fixed-height, nonwrapping status footer that clipped long
errors. It now wraps, grows from a 50px minimum to a bounded height, and
scrolls longer messages. The final narrow native capture confirms that the
sample's error is fully readable. The regression test checks horizontal
containment, wrapping, growth, height limits, downward wheel scrolling to
the last line and upward scrolling back to the first line.

Saved native captures (opaque mode, Windows at 96 DPI):

- [Dark launcher](captures/dark.png)
- [Light launcher](captures/light.png)
- [Light form after successful submission](captures/light-form.png)
- [Narrow window with a wrapped error](captures/narrow-error.png)
- [Selected unavailable action and its explanation](captures/narrow-unavailable.png)

## Verification

| Check | Result |
| --- | --- |
| `cargo check -p pane --tests` | Pass, zero warnings |
| `cargo build -p pane` | Pass |
| `cargo fmt --check -p pane` | Pass |
| `cargo test -p pane --test window --test command_search` | 50 window tests + 1 command-search test pass |
| Native dark root search / Escape | Pass |
| Native light form validation / correction / submission | Pass |
| Native narrow selection / unavailable reason / wrapped error | Pass |

The three final capture runs recorded confirmed foreground focus, no
aborted keys, empty stderr logs, scratch caches and successful process
cleanup. Native wheel input and real IME composition were not automated;
their behavior is tested through GPUI's test platform.

## Running the proof on this workspace

From `pane-ui-prototype` in PowerShell, after building with the shared cache:

```powershell
$env:CARGO_TARGET_DIR = '../pane/target'
cargo build -p pane
& ./.scratch/native-review/capture-pane.ps1 -Binary ../pane/target/debug/pane.exe -OutputDir ./.scratch/native-review/manual -Theme dark -Material opaque -LeaveOpen
```

Use `-Theme light` for light mode and `-Material glass` to request native
compositor blur. The helper creates fresh scratch data/cache directories.
Click the opened Pane window to interact. Drag the area around the search
magnifier, or a non-root screen's heading, to move it.

For automated native input, the helper verifies Pane owns foreground focus
before each keystroke. `-ClickToFocus` optionally attempts one guarded click
inside the spawned window when normal activation fails. It does not type
into whichever unrelated application happens to be foreground.

## Limits

- Windows at 96 DPI was exercised; macOS, Linux and other DPI scales were not.
- Linux selects an opaque window and solid panel in source; not runtime-tested here.
- Outer panel shadows are delegated to the native window; the original
  in-scene shadows obscured its glass and were removed.
- Glass tint is not a universal text-contrast guarantee over arbitrary desktops.
- Existing color-dependent native smoke scripts still need updating before reuse.
- Broader specs/tickets and production integration follow user review of this proof.

Raw compiler/test logs, compatibility probes and all capture metadata remain
under `.scratch/`. Capture runs marked `focusAchieved: false` did not execute
their requested keystrokes and are not counted as interaction evidence.
