# Native review — launch/capture helper and validation checklist

Bounded review support for the forthcoming native proof. The helper
(`capture-pane.ps1`, this directory) launches a real Pane build and captures
real desktop pixels; **main executes it and visually inspects the PNGs**.
This file is the checklist for that inspection. Nothing here runs the app,
builds it, or modifies app source; the helper is presentation-capture only.

Helper: `capture-pane.ps1` — Windows PowerShell 5.1+ (also runs under pwsh).
Each run writes `<OutputDir>\run-<stamp>-<id>\` containing the PNGs,
`pane-run.json` (PID, window bounds, DPI, theme/material, keys, focus and
Z-order results, cache location) and `pane-stderr.log`.

## Safety rules the helper enforces (verify before running)

1. `-Binary` and `-OutputDir` are **mandatory**; the binary is launched from
   the explicit path only. Default binary is never guessed.
2. Per-run scratch, spawn-scoped: `PANE_DATA_DIR` points at a unique new
   folder under the run directory, and the child's `LOCALAPPDATA` is a
   per-run scratch folder — Pane's cache is `%LOCALAPPDATA%\Pane\cache` on
   Windows (`cache_dir()` in `crates/pane/src/lib.rs`; `PANE_CACHE_DIR` is
   **not** read), so the cache also lands in the run folder, recorded as
   `cacheLocation` with `cacheDirExisted` proving the child used it.
   `PANE_ARTIFACTS` is cleared for the child only, so no default-extension
   downloads or update checks run. All of these are set only around the
   spawn and restored immediately after (child keeps its inherited copy);
   the operator's environment is never left modified, and nothing is
   deleted (no `Remove-Item -Recurse`; the only removals are the `PANE_*` /
   `LOCALAPPDATA` env vars the helper itself set, restored afterwards).
3. `PANE_EXTENSIONS_DIR` defaults to
   `C:\Users\ADMIN\Desktop\nguyenvu\pane\target\guests` (absolute, not
   CWD-relative — see the fixture-path note below).
4. Window crop by default (`DWMWA_EXTENDED_FRAME_BOUNDS`, physical pixels,
   DPI-aware process). Whole-virtual-screen capture only with explicit
   `-FullScreen`.
5. Keys are sent only after focus is confirmed with `GetForegroundWindow`
   compared to the spawned HWND by explicit **Int64** equality (`IntPtr
   -eq` is not trusted), **re-checked before every SendWait token**: if
   Pane loses the foreground mid-sequence, the remaining keys are not sent
   (`keysAbortedAt` records where, together with the foreground HWND/PID at
   that moment; exit code 1). If plain `SetForegroundWindow` fails, the
   helper tries `WScript.Shell.AppActivate` with **only the spawned
   process Id** — AppActivate's boolean return is never treated as focus;
   confirmation is always the real foreground HWND check, made before any
   input. No synthetic Alt key, no `AttachThreadInput`, no weakened guard.
   Metadata records the diagnosis: `foregroundHwnd`, `foregroundPid`,
   `foregroundIsPane`, `foregroundBeforeFallback` (snapshot right after
   the plain loop) and `activationMethod` ('SetForegroundWindow' |
   'AppActivate' | 'guarded-click' | null) — so a mismatch is
   attributable: another window of the same process (`foregroundIsPane`
   true, HWND differs) vs a foreign process holding the foreground.
   Last resort, **explicit opt-in `-ClickToFocus`** (default OFF): one
   REAL mouse click at a known interior point of Pane's window
   (center-x, visible top + 32 physical px, the search-header area), as
   normal native UI testing. Hard guards: `WindowFromPoint` must resolve
   the point to the spawned window tree (root ancestor = spawned HWND)
   BEFORE the cursor moves, and again immediately BEFORE the click; a
   foreign window covering the point cancels the click (no cursor move
   at all when the first guard fails). The cursor is saved and restored
   afterwards only if nothing moved it — the user's hand always wins.
   'guarded-click' is recorded only when the real foreground check then
   confirms Pane's exact HWND; the per-key guard is unchanged.
6. The helper closes only the PID it started (graceful `CloseMainWindow`,
   then `Stop-Process` on that PID if needed) — unless `-LeaveOpen`.
   Existing windows are never touched.
7. The optional backdrop is a helper-owned WinForms window shown **only**
   with `-Backdrop` (no hidden helper windows exist in any mode; it is the
   one intentional visible test window). It is arranged **immediately
   behind Pane in Z-order** (`hWndInsertAfter` = Pane's HWND, with
   NOSIZE|NOMOVE|NOACTIVATE — not `HWND_BOTTOM`, which could bury it behind
   unrelated windows) and the arrangement is **verified** (the window
   directly above the backdrop must be Pane; `zOrderVerified` in metadata).
   It is external to Pane: a pattern behind the window for compositor-blur
   evidence, never wallpaper injected into Pane. It lives in the helper's
   process, so it **always closes when the helper ends**; `-LeaveOpen`
   keeps only Pane itself running (PID printed and recorded).
8. Optional explicit outer-window size (`-WindowWidthPixels` /
   `-WindowHeightPixels`, **physical** pixels, both given together, each
   > 100): resizes **only the spawned Pane HWND** (`SetWindowPos` with
   NOMOVE|NOZORDER|NOACTIVATE) after its handle appears and before backdrop
   geometry/capture, then lets the layout settle (metadata records the
   requested size beside the actual rect). Default: no resize; no other
   window is ever resized. Main uses this for the 380x420 horizontal layout
   the 640px-wide test window never exercises.
9. Backdrop patterns differ by **luminance base**, not by inversion:
   `light` = `#ECECEC` base with black accent blocks/bands, `dark` =
   `#161616` base with white accents (same accent geometry). ~3/4-bright
   vs ~3/4-dark average coverage, so their blurred averages are clearly
   different and the blocks/bands remain recognizable through blur; the
   pattern is recorded in metadata. (Inverted 50/50 checkerboards would
   blur to the same average and prove nothing.)

## Run recipes

```powershell
# 1. Plain launch + initial window crop (dark default today)
powershell -NoProfile -ExecutionPolicy Bypass -File .\capture-pane.ps1 `
  -Binary C:\Users\ADMIN\Desktop\nguyenvu\pane\target\debug\pane.exe `
  -OutputDir C:\captures

# 2. Type 'rust' (root search), then Escape, capturing after each
... -Keys 'rust','{ESC}'

# 3. Activation evidence: typing + Enter opens the best match
... -Keys 'rust','{ENTER}'

# 4. Labeled mode runs (PANE_THEME/PANE_MATERIAL are prototype selectors;
#    verify the build under test supports them before trusting differences)
... -Theme dark  -Material glass
... -Theme light -Material opaque

# 5. Frost evidence: external pattern directly behind Pane
... -Backdrop -BackdropPattern light        # or 'dark'; run both
... -Backdrop -BackdropPattern light -FullScreen   # only if the crop is not enough

# 6. Explicit outer-window size (physical pixels): 380x420 horizontal layout
... -WindowWidthPixels 380 -WindowHeightPixels 420

# 7. Manual inspection (Pane only; the backdrop always closes)
... -LeaveOpen                                # prints/records the PID

# 8. Focus last resort (explicit opt-in; REAL pointer - hands off!)
... -ClickToFocus                             # guarded click inside Pane
```

SendKeys syntax: literal text as-is (`rust`), special keys braced
(`{ESC}`, `{ENTER}`, `{DOWN}`); escape `+ ^ % ~ ( ) { }` inside literal
text or the token is misparsed (the reference smoke script notes `{+}` for
a plus sign).

## Visual validation checklist (per run)

- [ ] `00-initial-window.png`: root search renders with its query field;
      window bounds in `pane-run.json` match the crop (frame bounds ≈ the
      visible window; `windowRect` includes DWM's invisible borders).
- [ ] After `'rust'`: the list narrows to the **Rust sample** row (tests:
  `typing_in_root_search_narrows_the_results_and_enter_opens_the_best_match`,
  `coming_back_to_root_search_starts_an_empty_search_with_focus`). Root
  search opens ready to type — with focus confirmed, keys land in the query
  field without clicking.
- [ ] After `'{ESC}'`: an empty query with the full row list again
  (`a_query_that_matches_nothing_says_so_and_escape_clears_it`).
- [ ] Activation evidence: after `'rust'` + `'{ENTER}'` the **Rust sample
  command screen** opens (same root typing+Enter test). If focus was lost
  mid-sequence, `keysAbortedAt` in metadata names the step and no further
  keys were sent.
- [ ] `-Theme dark` vs `-Theme light`: compare crops side by side.
      `PANE_THEME` / `PANE_MATERIAL` are **prototype selectors** (their
      implementation is in progress): **verify the build under test
      supports them** before trusting mode differences — if it does not,
      the two captures will look identical, which is a property of that
      build, not of the helper. The run metadata records the label either
      way, so captures stay comparable once the selectors land.
- [ ] Frost evidence: run the same recipe with `-BackdropPattern light` and
  `-BackdropPattern dark`. A **luminance difference inside Pane's window**
  (brighter with the light pattern, darker with the dark one, accent
  blocks/bands visible through any blur) = Pane's surface samples the
  desktop — evidence of transparency, not sufficient evidence of blur.
  Separately inspect the known sharp blocks/bands: softened edges or
  averaged detail through the panel support compositor blur; sharp edges
  indicate transparency alone. Compare with opaque mode as a control.
  If tint hides all pattern detail, report blur as unverified. Check
  `zOrderVerified: true` first, or the evidence is invalid. The backdrop
  pattern must never appear as Pane's own content.
- [ ] DPI: `dpi` in metadata matches the display's scaling (96 = 100%);
  crop size in pixels matches `frameBounds` (physical pixels, not logical).
- [ ] Explicit size (when `-WindowWidthPixels`/`-WindowHeightPixels` given):
      `requestedWindowSize` is recorded, the actual `windowRect` matches the
      requested outer size in physical pixels (`frameBounds` is smaller by
      DWM's invisible borders), and the capture covers the resized window.
- [ ] `cacheLocation` = `<run>\localappdata\Pane\cache` and
  `cacheDirExisted: true` (the child really used the scratch cache, not
  the user's); `pane-stderr.log` has no panics;
  `processExited`/`exitCode` recorded.

## Reuse notes from the prototype's tests (do not re-derive)

The interactive helper complements the test platform, it does not replace
it: `window.rs`/`command_search.rs` drive GPUI's **test platform** with
simulated input; the helper drives the **real platform** with SendKeys.

- **IME**: `input_method_composition_searches_root` and
  `input_method_composition_commits_into_the_text_field` cover composition.
  SendKeys bypasses IME composition entirely — disable any active IME
  before running the helper, and keep IME validation on the test-platform
  tests.
- **Accessibility**: `assistive_technology_sees_the_search_field_and_the_selected_result`
  (and the form/color equivalents) check the a11y tree; the helper cannot
  see it — visual capture only.
- **Activation**: for real-world activation evidence use the root
  typing+Enter flow (`typing_in_root_search_narrows_the_results_and_enter_opens_the_best_match`,
  recipe 3 above). `leaving_the_window_during_a_drag_ends_it` is **drag
  lifecycle**, not command activation — do not use it as the activation
  reference.
- **Shrink/scroll**: `the_selected_row_stays_visible_when_the_window_shrinks`,
  `the_list_scrolls_to_keep_the_selected_row_visible`,
  `the_mouse_wheel_scrolls_away_until_the_rows_reload` — relevant filters
  once the reworked list is proven on screen.
- Filters: `cargo test -p pane --test window <name>` /
  `cargo test -p pane --test command_search`.

## Fixture-path note (resolved by a junction)

`window.rs`/`command_search.rs` resolve sample components relative to
`CARGO_MANIFEST_DIR` (`../../target/guests`, `../../target/guests/packages/…`),
which `CARGO_TARGET_DIR` does not redirect — but main has already created a
**junction** at `pane-ui-prototype/target/guests` → `pane/target/guests`
(verified present), so the prototype's tests resolve the main checkout's
guests and **no guest rebuild is needed**. Only if that junction is absent
does `cargo xtask guests` need to run in the prototype. The runtime helper
is unaffected either way: it points the app at the absolute
`PANE_EXTENSIONS_DIR` (`pane/target/guests`, verified to hold
`sample_*.wasm` fixtures).

`scripts/smoke-windows.ps1` (main checkout) is the reference for
Win32/SendKeys/DPI/capture patterns but is a **large destructive fixture
workflow** (it `Remove-Item -Recurse`s its data dir and drives installs,
uninstalls and updates). Do not run it wholesale; this helper is the safe
subset for the native proof.

## Known operator cautions

- Don't touch mouse/keyboard during a run (real input goes to whatever is
  focused; focus is checked before every key token, but a key can still
  race a hand movement between check and send — hands off is the rule).
  With `-ClickToFocus` this is doubly true: the helper moves and clicks
  the REAL system pointer once, inside Pane's own window only, briefly —
  keep hands off the mouse and opt in only when testing your own window.
- Windows can deny `SetForegroundWindow` from a background script; the
  helper retries ~2 s, then tries `AppActivate` on the spawned PID (up to
  3 times). If focus is still not confirmed by the real foreground HWND,
  keys are skipped and the run exits 1 with the diagnosis in metadata —
  which window is foreground, which PID owns it, and whether that PID is
  Pane's. A custom-titlebar app can also hold the foreground in a
  DIFFERENT HWND of the same process: `foregroundIsPane` true with a
  different `foregroundHwnd` identifies exactly that case (the guard still
  refuses to type — the HWND must match — and main can then decide which
  window is the real target). Launch from an interactive session, not a
  hidden service.
- Multi-monitor: window coordinates can be negative; `CopyFromScreen`
  handles it. `-FullScreen` captures the whole virtual screen — only when
  the crop is insufficient.
- Leaving runs behind is intended (no cleanup deletes); tidy old run
  folders by hand if needed.

## Static sanity check (performed, no app run)

- PowerShell parser: file parses with zero errors (5.1 parser).
- The embedded C# (`Add-Type`, including `GetWindow`) compiles cleanly in a
  scratch session.
- `Remove-Item` appears only for `Env:` variables (no filesystem deletes);
  `-Recurse` never appears.
- `Binary`/`OutputDir` are mandatory in the param block (AST-verified).
- Focus re-check (`GetForegroundWindow`) is present immediately before
  every `SendWait` in the key loop; abort path sets `keysAbortedAt`.
- Focus diagnostics: `GetWindowThreadProcessId` P/Invoke present; the
  foreground HWND/PID are recorded after the activation attempts and at
  any key abort; every HWND equality check compares `.ToInt64()`
  explicitly (focus loop, AppActivate recheck, per-key guard, z-order
  verification).
- Activation fallback is `WScript.Shell.AppActivate` with the spawned
  PID only and is confirmed by the real foreground check; no
  `AttachThreadInput` and no `keybd_event`/synthetic Alt appear anywhere
  (the opt-in real click uses `mouse_event`, the sanctioned real-pointer
  path, inside the guarded block only).
- `-ClickToFocus` is a switch (default OFF); its click path checks
  `WindowFromPoint` root-ancestry twice (before the cursor moves and again
  immediately before the click), clicks only at the interior point
  (center-x, top+32 physical px), saves and conditionally restores the
  cursor, and records 'guarded-click' only on a confirmed exact-HWND
  foreground; metadata carries clickToFocusPoint/clickToFocusGuards/
  cursorSaved/cursorRestored.
- Window-size parameters are validated (both together, each > 100; any
  non-zero non-positive value is rejected) and the
  resize call uses `SetWindowPos` with NOMOVE|NOZORDER|NOACTIVATE on the
  spawned HWND only, before backdrop geometry/capture.

Execution and visual inspection belong to main, per the task split.
