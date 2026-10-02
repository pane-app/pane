# Behavior smoke appearance

The Windows, macOS and Linux smoke scripts force `PANE_THEME=dark` and
`PANE_MATERIAL=opaque`, including their installed-application and update phases.
They record that choice in `system.txt`. This keeps behavior screenshots
independent of the desktop background and the caller's appearance environment.
These runs do not establish light-theme readability or native blur; those remain
separate native acceptance work for [#61](https://github.com/hoangvu12/pane/issues/61).

The macOS/Linux installed and updated GUI launches explicitly supply both
appearance variables after `env -i`; exporting them at script startup does not
survive that environment reset. The clean HOME, empty tool PATH and local
artifact source remain isolated. In
[CI run 36952982441](https://github.com/hoangvu12/pane/actions/runs/36952982441),
the missing variables caused macOS's installed launch to use default glass.
Artifact `11206345698`, `installed-stderr.log`, records an abort from
`gpui_macos/src/window.rs:3794` at fork `2b9e644`: the blurred-view callback
sent `setBackgroundColor:` an Objective-C object (`@`) where the method expects
a CoreGraphics color pointer (`^{CGColor=}`). The resulting panic crossed a
non-unwinding callback boundary. System Events' missing PID was a consequence,
not the original failure. Restoring the smoke's explicit opaque mode fixes
its configuration error; it does **not** fix or certify production macOS glass.
The renderer owner must address that independently before glass acceptance.
The separate `smoke-macos-default-startup.sh` CI step launches with appearance
variables absent, requires a native window and checks the process survives
ten more seconds of native layer updates. It records stderr and startup evidence
under `smoke/default-startup`, uses a fresh HOME/data/cache and stops only its
own child. It sends no input and does not establish blur quality or broader
native material acceptance. Its native execution remains a final-CI check.

`scripts/check_screenshot.py` recognizes the shared dark panel's connected
neutral surfaces, including its sheen, inset edges and darker footer. A flat
background crop would omit parts of the new header and footer. Like the previous
checker, this assumes the launcher is visible and its panel is the largest
connected matching region, without an adjoining desktop surface of the same
colors. Captures must retain that controlled condition.

Text checks follow the dark semantic roles. The `selected` check requires a
broad connected row wash and retains the 3,000-pixel minimum; a sheen, hover or
small neutral icon alone cannot pass. The `progress` check is confined to the
footer because progress and unavailable reasons now share the warning color.
The `subtitle` check excludes the heading and footer because subtitles and idle
hints now share the muted color. Extension drawing checks still use their
authored hex colors, with the original pixel thresholds and pointer lookup.
Result/error, absent-color, distinct-frame and same-frame checks remain active.

The Linux resource and latency workload's two screenshot checks follow the
same roles. `scripts/measure-linux.sh` was missed when the smoke scripts'
hex colors were translated: its cold-root check still named the retired hint
color and its calculator check still named the retired selected-row wash
`364355`. In
[CI run 36960914528](https://github.com/hoangvu12/pane/actions/runs/36960914528),
artifact `resource-measurements` frame `4-calculator-answer.png` held only
429 pixels near the old wash, under the 3,000 minimum, while the frame itself
shows the new selected answer row; the cold-root check had passed only
incidentally, 94 stray pixels near the old hint color against the default
minimum of 20. Both checks now use the `hint` and `selected` roles with the
same thresholds, and rechecking that artifact's frames passes (463 hint
pixels, a 29,702-pixel selected wash).

The same-frame comparison permits at most one interior pixel whose channels
each differ by at most one 8-bit level. This bound comes from
[CI run 36951745142](https://github.com/hoangvu12/pane/actions/runs/36951745142):
`4-result-2.png` and `25-search-result.png` both show the same TypeScript
command and answer, but exact equality failed on each platform. In Windows
artifact `11204826276`, the only difference within the existing 12-pixel edge
crop is `(178, 92, 46)` versus `(178, 92, 47)` at panel coordinate `(310, 406)`.
In macOS artifact `11204369046`, it is `(54, 55, 58)` versus `(53, 54, 58)` at
`(160, 116)`; other changes are confined to the already-excluded outer edge.
This is consistent with final color rounding. All other interior pixels must
still match exactly, and image dimensions must match. There is no text mask
or percentage allowance. Comparing the captured Rust or JavaScript answers
against the TypeScript answer still fails on both platforms. Offline tests
also reject a two-level change in one pixel and one-level changes in two pixels.

Windows captures park the pointer in the foreground Pane window's header
before capturing, then wait briefly for its hover repaint. This does not click,
restore, refocus or change selection; captures while Pane is not foreground
leave the pointer alone. The hotkey foreground assertions remain unchanged.
In [CI run 36952982441](https://github.com/hoangvu12/pane/actions/runs/36952982441),
Windows artifact `11204779190` showed why this is necessary: the color-picker
click left the pointer at `(263, 292)`, over the JavaScript row in root search.
Restoring the window after the released-hotkey check repainted that row's
hover wash. Frames `57-disabled.png` and `58-disabled-pressed.png` differed in
31,438 interior pixels, all within the unselected row's 44-pixel height; its
background changed from `(22, 23, 26)` to `(30, 31, 34)`, matching the theme's
white hover wash at alpha 9/255. The title, selected Rust row, query and footer
were unchanged. All four earlier same-frame pairs in that artifact were exact
matches except the already-documented one-pixel TypeScript rounding difference.
The checker still rejects this hover difference: deterministic input conditions
are restored in the Windows capture helper instead of masking row backgrounds
or expanding image tolerance. Native verification of pointer normalization is
left to the integrated smoke run.

The install-preview wait now requires positive metadata evidence below the
package heading. The old root-search blue border no longer exists. This check
is only for the initial `--install` window: it assumes the prototype's default
760-logical-pixel width and scales the sampling band with the captured width
for display scaling. It is not a general resized-window screen classifier.

The macOS quit-with-helper phase also waits for positive progress evidence
instead of assuming its two-second delay produced a fresh frame. In
[CI run 36957014594](https://github.com/hoangvu12/pane/actions/runs/36957014594),
artifact `11207102192` frame `94-helper-before-quit.png` shows the correct
selected wait action but the idle footer, with zero warning pixels anywhere.
The preceding process check passed and the isolated helper-quit settings
record contains `helper-wait: started`, with a heartbeat file present. There
is no associated error in stderr. This is consistent with a stale frame;
it does not establish which native scheduling stage delayed the update.
The existing bounded capture helper now requires the unchanged progress
assertion within five more seconds and rechecks that the helper is still
running before requesting quit. If the UI never shows progress, or the helper
finishes first, the smoke still fails. The ten-second guest wait, heartbeat
and shutdown checks remain unchanged; final native CI must verify the fix.

Offline checker regression tests use synthetic frames to exercise missing
results, desktop changes, missing selection, wrong-region text, preview waiting
at 1x/2x and unchanged custom swatches:

```sh
python -m unittest discover -s scripts -p test_check_screenshot.py
```

Since the Desktop Window Manager now rounds the Windows window's own
corners (`pane::prefer_rounded_window_corners`) and the panel fills the
window without a painted radius, Windows smoke captures show the system's
~8px compositor curve at the window edge instead of the panel's former
18px arc; the corner plate of raw acrylic (glass) or white clear (opaque)
that the painted arc left behind it is gone. Checks are unaffected: the
panel-surface region the checker finds only grows, and every color,
wash, same-frame and distinct-frame assertion is interior.

This script adaptation has not been validated by running the integrated
launcher. Per the Windows-first integration order, native desktop smokes and
the final all-OS CI run are deferred until the presentation and dependency
changes are integrated. In particular, confirm the preview sampling band
against real package captures on each OS before treating it as native evidence.
