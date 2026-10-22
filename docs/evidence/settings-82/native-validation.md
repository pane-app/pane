# Native validation — About: updates and diagnostics (#82)

**Status: DEFERRED — this is the capture plan, not a record of a run.**
Ticket [#82](https://github.com/hoangvu12/pane/issues/82) lands the About
page's update flow and diagnostics copy with simulated-window integration
coverage only (the local artifact source `pane-core`'s application-update
tests use, driven through the real Settings window on GPUI's test platform);
the native captures below are the plan for the follow-up native validation
run, to be recorded here once it happens (the ticket's parent
[#84](https://github.com/hoangvu12/pane/issues/84) will run the milestone's
combined native evidence).

The plan follows the established pattern of
[`docs/evidence/settings-81/native-validation.md`](../settings-81/native-validation.md)
and the script it describes
([`docs/evidence/settings-72/capture-settings.ps1`](../settings-72/capture-settings.ps1)):
launch an explicit development build with per-run scratch `PANE_DATA_DIR` and
scratch `LOCALAPPDATA` (spawn-scoped environment variables, restored
immediately), focus confirmed with the real foreground HWND before input,
guarded real clicks whose `WindowFromPoint` guards prove the point belongs to
the spawned window tree, and close only the process the script started.

The update flows need a real release source on this computer, which a
development build will not invent: point the child's `PANE_ARTIFACTS` at a
local artifact source serving an index with an `application` entry for this
system's target, exactly as `pane-core`'s `tests/support/artifacts.rs` serves
one (the packaging tasks' `cargo xtask package-windows` output beside a
hand-written index is the same shape). A second run with `PANE_ARTIFACTS`
cleared captures the no-source state; nothing reaches Pane's published
downloads or the network in either run.

## What the simulated coverage already pins

`crates/pane/tests/settings.rs` drives every state the flow has through the
real Settings window — the version row's real build version, the
documentation entry's opening (and its refusal, where no handler is
installed), no-source, a check that answers up to date, an unreachable
source, a failed check retried, the offer installed by the user's click,
an install whose download is interrupted partway and retried, an install
whose package fails its integrity check with the offer kept, a check left
running while the page is left, and the diagnostics copy to the clipboard —
against the local artifact source, never a real release service. The native
run below adds what a simulated window cannot: the real program swap on a
real install folder, the real system handler opening the documentation, and
the real clipboard.

## What to capture natively (Windows first)

1. **The page with no release source.** `Ctrl+,` opens Settings, walk the
   sidebar to About with `PANE_ARTIFACTS` cleared for the child: the version
   row, the update section's explanation that no artifact source is
   configured (no check row, no claim of a release), the documentation row
   and the diagnostics row are visible at the opened size and at the
   560×400 floor, where the page scrolls. Capture both sizes.
2. **A check that says up to date.** With the local source serving an index
   whose `application` entry names this Pane's own version (or none), click
   **Check for updates**: capture the page saying Pane is up to date, the
   launcher window's status line saying the same, and the check row still
   offered.
3. **An offered update and its install.** Publish a newer version's package
   on the local source (a real `pane-<version>-<target>.zip`, so the swap is
   the real one), click **Check for updates**, capture the offer — the
   version, the row's explanation that extensions and settings are kept and
   the new version is used the next time Pane starts — and the same row in
   root search. Then click **Update Pane to <version>**: capture the
   download progress on the page, the result when it lands, and the install
   folder's program replaced with `pane.exe.old` beside it. Restart the
   spawned Pane and capture `pane --version` reporting the new version, the
   old program removed by the start, and the About page showing the new
   version.
4. **A failed check, explained and retried.** Stop the local source between
   the offer and a re-check (or serve a failing status for the index): click
   **Check for updates**, capture the failure explained on the page with the
   check row offered again, and the root row listing the same retry. Bring
   the source back and retry from the page: capture the offer found again.
5. **A failed install, explained, the offer kept.** Corrupt the served
   package's bytes (as `corrupt_application` does), click the offer's row:
   capture the failure explained on the page with the offer still offered,
   the status line saying the same, and the install folder untouched. Serve
   the intact package again and click the offer again: capture the install
   completing.
6. **The diagnostics copy.** Click **Copy diagnostics**: capture the
   completion status on the page, and the clipboard's content (pasted into
   Notepad in the capture) — the real version, the target, the data folder
   and the update-check state — with nothing of any extension's settings,
   data or credentials in it.
7. **The documentation link.** Click the documentation row: capture the
   browser opened at Pane's repository, and the page's status saying it
   opened. (A run where no handler is installed captures the refusal
   explained instead.)

## What is explicitly not claimed until run

- macOS and Linux equivalents (the same flows against their package formats,
  the macOS bundle's binary swap included) are part of the same plan on
  those platforms; no Windows run establishes them.
- The install's swap itself is `pane-core`'s platform-independent code
  already covered by its tests and the packaging smokes; the capture here
  verifies the page drove it and showed it, not the swap's mechanics.
- Simulated accessibility checks in `tests/settings.rs` do not establish
  native assistive-technology operation; the capture run should at least
  verify the page's rows and statuses are named and announced in Windows's
  Accessibility Insights, as #72's run did for the window controls.
- Perceived smoothness of any transition on the page is out of scope here
  (#87/#88 own Settings section and popup motion).
