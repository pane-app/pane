# Native validation — Extensions in Settings (#81)

**Status: DEFERRED — this is the capture plan, not a record of a run.**
Ticket [#81](https://github.com/hoangvu12/pane/issues/81) lands the
Extensions page with simulated-window integration coverage only; the native
captures below are the plan for the follow-up native validation run, to be
recorded here once it happens (the ticket's parent
[#84](https://github.com/hoangvu12/pane/issues/84) will run the milestone's
combined native evidence).

The plan follows the established pattern of
[`docs/evidence/settings-72/capture-settings.ps1`](../settings-72/capture-settings.ps1):
launch an explicit development build with per-run scratch `PANE_DATA_DIR` and
scratch `LOCALAPPDATA` (spawn-scoped environment variables, restored
immediately), `PANE_ARTIFACTS` cleared for the child, focus confirmed with
the real foreground HWND before input, guarded real clicks whose
`WindowFromPoint` guards prove the point belongs to the spawned window tree,
install the settings sample through `--install` first so the page has real
content, and close only the process the script started.

## What to capture natively (Windows first)

1. **The Extensions page lists real extensions.** `Ctrl+,` opens Settings on
   the Extensions page: the settings sample's row (state `Enabled · …`), its
   management rows, the `Greeting` command row and the three install rows are
   visible at the opened size and at the 560×400 floor, where the page
   scrolls. Capture both sizes.
2. **A disable from Settings.** Click the package row; capture the list
   showing `Enabled` replaced by `Disabled`, and the launcher window (brought
   to the same extension list by the shared flow) showing the same state.
3. **A dependent confirmation.** With a package installed that requires
   another (the operations fixture pair), click the dependency's row and
   capture the confirmation — `Disable all N` / `Cancel` — rendered in
   Settings, then the outcome in both windows after `Disable all N`.
4. **An uninstall with the saved-data choice.** Click `Uninstall …`, capture
   the confirmation with its saved-data lines, choose **keep**, then capture
   the retained-data row; verify `extensions/settings.json` on disk still
   holds the package's key (the saved-data policy, checked through the
   existing integration boundary, as `tests/settings.rs` does).
5. **Opening a settings command from the page.** Click the `Greeting`
   command row: capture the launcher window summoned and focused with the
   command open, and Settings still open beside it.
6. **An install row delegates to the launcher.** Click
   `Install extension from npm…`: capture the launcher window focused with
   its form open.
7. **A failure and its recovery.** Install the package from a folder whose
   component is replaced with `failing_start.wasm` (as
   `tests/settings.rs`'s reload-failure test does), click `Reload …` from
   Settings, capture the error status and the `Retry …` row on the page, then
   capture the retry's outcome.
8. **A background change reaches the page by itself.** Start development from
   the page (the `Develop …` row), save a source that does not build (the
   FakeBuilder path needs a stand-in build command a native build can run —
   use a package whose build command fails, as `develop_builds.rs`'s
   fixtures do), and capture the page showing the failed-build status and the
   `Why … did not build` row without any input into Settings.

## What is explicitly not claimed until run

- macOS and Linux equivalents (the launcher window's summoning/focusing
  behavior, the titlebar at the small size, and the same flows) are part of
  the same plan on those platforms; no Windows run establishes them.
- Perceived smoothness of any transition on the page is out of scope here
  (#87/#88 own Settings section and popup motion).
- Simulated accessibility checks in `tests/settings.rs` do not establish
  native assistive-technology operation; the capture run should at least
  verify the page's rows are named in Windows's Accessibility Insights, as
  #72's run did for the window controls.
