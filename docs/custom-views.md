# Custom views

Added for [#21](https://github.com/hoangvu12/pane/issues/21) (US37, US38, T05,
G2). A custom view is an interactive view the extension draws itself, for
what standard controls such as [forms](forms.md) cannot show. This slice is
the minimum a small color picker needs: filled rectangles and text in a
fixed-size area, a few keys, and pointer press, drag and release. It is not a
drawing or layout framework, and it does not settle the rest of the G2 UI
contract.

## Contract

Defined in [`wit/extension.wit`](../wit/extension.wit), identically for Rust
([`pane-guest`](../guests/pane-guest/src/lib.rs)) and JavaScript/TypeScript
([`@pane/extension`](../guests/js/pane.d.ts)):

- An item may carry a `customView` (screen title, accessible label, role;
  the WIT record `custom-view-info`, carried in the list's tree, see
  [list-tree.md](list-tree.md)). Activating it calls `open-view(item-id)`
  instead of running the item's action (a `form`, if also set, wins).
- `open-view` returns a `custom-view`, a WIT **resource** the extension
  implements and in which it keeps the view's state. Each call opens a new
  view with its own state. Pane owns the handle: it drops it when the view
  closes, and never calls it again. In Rust, the resource's value is dropped
  (`Drop` runs); in JS/TS the object is released to the garbage collector,
  and no method is called on it.
- `render() -> frame` draws the view as it is now. A `frame` is a width and
  height in logical pixels, a list of `shape`s painted in order (later over
  earlier, clipped to the frame), and a `value` string: the view's current
  value for assistive technology ("Blue, #1E88E5").
  - `rect { x, y, width, height, fill }`: a filled rectangle.
  - `text { x, y, content, color }`: one line in Pane's font and size, its
    top-left corner at `x`, `y`.
  - Coordinates are `s32` logical pixels from the view's top-left corner;
    colors are `rgb`, a `u32` 0xRRGGBB (`pane_core::Rgb` on the host). There
    are no strokes, paths, images, fonts, alpha, scrolling or nested layout.
  - Pane bounds each frame: at most 4096 shapes, 256 characters per text and
    4096 x 4096 pixels (`MAX_FRAME_SHAPES`, `MAX_TEXT_CHARS`,
    `MAX_FRAME_SIZE` in `pane_core`). A frame over a limit is not drawn: it
    is the extension's error ("The extension reported an error: the frame
    has 4097 shapes; at most 4096 are drawn"), the view keeps its last
    drawing and stays open for the next event (a first frame over a limit
    opens nothing).
- `handle-event(view-event) -> result<_, string>` receives the user's input,
  after which Pane calls `render` again. The events are:
  - `key(key)`: `left`, `right`, `up`, `down`, `home` or `end` while the view
    has keyboard focus;
  - `pointer-down(point)`: the primary button pressed over the view's
    drawing area (a press on the focus ring around it only focuses the
    view);
  - `pointer-move(point)`: the pointer moved while that button is held,
    anywhere in the window (the point can then lie outside the view);
  - `pointer-up(point)`: that button released. When the window cannot see
    the release (the button went up outside it, or the window lost
    activation), Pane sends `pointer-up` at the last point it saw during the
    press, on the first move without the button or on deactivation.
  Moves and the release are sent only after a press over the view; plain
  hovering sends nothing. Moves are coalesced: while one is being handled,
  only the latest further move waits, and it is sent when the moves in flight
  are answered, or before the next other event, so a drag never lags behind
  the pointer by more than one move and the release comes after the last
  move.
- An `Err` from `open-view` or `handle-event` is shown to the user as the
  extension's error ("The extension reported an error: ..."); after a
  `handle-event` error the view stays open with its last drawing. A trap
  (crash) in any of these calls drops the instance, as for every call, and
  with it every view open in it: Pane leaves the view for the command's list
  and shows "The extension crashed: ...".
- `render` and `handle-event` are `async` like the other exports. In JS/TS
  both must be `async` methods (return a promise). This is a limit of the
  pinned toolchain (componentize-qjs with Pane's P3 patches), not of the
  contract: a synchronous `render` came back with the frame's first field
  overwritten (the width read as the `value` string's length), and a
  non-`async` implementation of another `async` export traps when called
  ("The extension crashed: ..."). `@pane/extension` types both methods as
  returning a `Promise`, so the build's type check (TypeScript, or
  JavaScript with `// @ts-check` and a typed `command`) reports a
  synchronous one before anything runs: "The types returned by 'render()'
  are incompatible between these types. Type 'Frame' is missing the
  following properties from type 'Promise<Frame>'". Unchecked JavaScript
  gets no such error. The object `openView` returns needs only these two
  methods; it need not be a class the module exports.

The runnable example is the "Choose a color" item of the three samples
([Rust](../guests/sample-rust/src/lib.rs),
[JavaScript](../guests/sample-js/src/index.js),
[TypeScript](../guests/sample-ts/src/index.ts)): a grid of 8 hues x 3 shades,
a frame around the chosen swatch, a preview and its hex code. Arrow keys move
the choice (clamped at the edges), Home and End go to the first and last hue,
and pressing or dragging over the grid chooses the swatch under the pointer
(a drag past the edge chooses the nearest one). Author instructions are in
[guests/README.md](../guests/README.md#custom-views).

## Host behavior

The public host interface is [`pane_core::Launcher`](../crates/pane-core/src/launcher.rs):
`Screen::CustomView`, which carries the view's snapshot (label, role and
the latest `Frame`), `send_view_event` and `back`. The runtime
([`Runtime`](../crates/pane-core/src/runtime.rs)) keeps each open view's
resource on its thread and hands out only a `ViewId`: `open_view`,
`view_event`, `close_view` and, for diagnostics and tests only,
`view_count` (the launcher never consults it; it keeps its own open view). No
renderer or engine object crosses into the guest or the SDKs: guests see only
the WIT records above, and the window turns frames into GPUI CE elements
([`crates/pane/src/extension_views/custom_view.rs`](../crates/pane/src/extension_views/custom_view.rs)).

| Input | On the custom view screen |
| --- | --- |
| Left, Right, Up, Down, Home, End | Sent to the view as `key` events |
| Primary button pressed over the drawing area | Focuses the view and sends `pointer-down` |
| Primary button pressed on the focus ring around it | Focuses the view only |
| Pointer moved while that button is held | `pointer-move`, even outside the view; coalesced |
| That button released | `pointer-up`, even outside the view |
| Move without the button, or window deactivated, during a press | `pointer-up` at the last point seen |
| Tab / Shift-Tab | Stay on the view, the screen's only focus stop |
| Escape | Closes the view and returns to the command's list |

The view opens with keyboard focus; its focus ring is drawn by Pane around
the drawing area. Enter, Tab and Escape are not sent to the view.

**Which view events and answers belong to.** Each event is sent to the
runtime when the window receives it, and the runtime handles a view's events
one at a time in that order. The launcher numbers them and shows an answer
only if no later event's drawing is already shown, so a late older answer
cannot replace a newer one. An answer is also applied only while the screen
it was requested from is still shown: after Back (or any navigation), the
view is closed and later answers for it are discarded. A view whose
`open-view` answer arrives after the user has left the command is closed as
soon as it arrives, so it never lingers in the guest. Events cannot reach a
closed view: the launcher sends nothing when no view is open, and the
runtime answers `ViewClosed` for a closed or dropped view. No "Running…"
status is shown while an event is handled.

## Accessibility

Checked through GPUI's accessibility tree (`Window::debug_a11y_tree_json`):

- The view is one node, with the role the item declares (`color-well` is
  AccessKit's `ColorWell`), named by its label ("Color") and with the
  frame's `value` as its value ("Blue, #1E88E5"). It updates with each
  drawing, and is the focused node while the view has focus.
- The drawing's rectangles and text add no nodes of their own.

Not supported, and not claimed: AccessKit's color value property (GPUI CE has
no setter, so the color is only in the text value), accessibility actions (a
screen reader cannot change the color; only keys and the pointer can), and
announcements: no screen reader was run on any platform. Only one role
exists; other kinds of custom control need their own.

## Checks

Contract checks, run for each of the Rust, JavaScript and TypeScript samples
([`crates/pane-core/tests/samples.rs`](../crates/pane-core/tests/samples.rs)):
the first frame's size, value and shapes; keys moving and clamping the
choice; pointer press, drag (including past the edge), release and a press
outside the grid; a reopened view starting afresh; two views open at once
keeping their own state, and a closed view refusing events; an unknown view
being the guest's error. Host behavior
([`launcher.rs`](../crates/pane-core/tests/launcher.rs)), with the Rust
sample and the faulty fixture's counting view: Back closing the view in the
guest; a view that opens after the user left being closed; an event answer
after Back being discarded and not shown in a reopened view; a late older
answer not replacing a newer one; moves and releases sent only while
pressed; a view error keeping the view open; a crash closing it while the
command keeps working; a refused view; a package preview closing an open
view; the component's code being replaced behind the launcher's back closing
the view on its next event; a frame over each limit being an error while the
view stays usable; whether a press is held; a drag sending only the latest
waiting move, and a waiting move going out before the release. Package
checks ([`packages.rs`](../crates/pane-core/tests/packages.rs)): an update
finishing in the background, and a disable, closing the installed package's
open view at once.

Window checks through GPUI's test platform, with real key and mouse events
([`crates/pane/tests/window.rs`](../crates/pane/tests/window.rs)), for all
three languages: keys changing the color, focus and the accessible node and
its value, Tab staying on the view, Escape returning to the list with the
item selected; pointer press, drag past the view's edge, release, and a
click choosing a swatch. With the faulty fixture: a press on the focus ring
sending nothing, and a release outside the window (a move without the
button) and a deactivated window each ending the drag.

Native checks: the GUI smoke scripts open the Rust sample's color picker,
press Right, and click the dark green swatch, located in the screenshot by
its color (`check_screenshot.py --locate`: the largest region of that
color inside the Pane window), asserting each time that the chosen color fills its swatch and the
preview (screenshots 21 to 23). As with forms, only the Rust view is driven
natively; the JavaScript and TypeScript views rest on the contract and window
checks above. On Linux X11 this ran on 2026-09-28
([evidence](platforms/linux.md#custom-view-21)); the macOS and Windows steps,
including their click helpers (Quartz events through Python `ctypes`; `user32`
`SetCursorPos`/`mouse_event` in a DPI-aware process), are written but have
not run yet.

## Limits

- One view per screen, with one focus stop; no standard controls inside a
  custom view, no text input, modifiers, scroll wheel, double clicks, hover
  or secondary buttons.
- The frame is shown at the size the extension asks for; it is neither
  scaled nor scrolled, so a frame larger than the window is cut off.
- Replacing or disabling the package of an open view (an update finishing
  in the background, or a disable) drops its instance and the view with it,
  and the launcher leaves the view at once for root search, with the update's
  or disable's message; the view's state is lost (#11, #14 own carrying it
  over). Only code that drops the component without the launcher (a direct
  `Runtime::forget`) leaves the screen to close on the view's next event,
  with "The extension's view is no longer open". Reload does not exist yet.
- A drag's pointer position is not reported between the last move Pane saw
  and a release it could not see; the release uses the last point.
- A `render` or `handle-event` that never returns blocks the later calls
  into its own instance, which runs one call at a time; other extensions'
  calls are served meanwhile (#136). One waiting at an `await` is stopped when its package is disabled, reloaded
  or updated ([generations](generations.md)); one computing without
  waiting is stopped too, at once then, and after 5 seconds of computing
  otherwise, as unresponsive (#18,
  [pausing](pausing.md#when-an-extension-stops-responding)).
- The contract adds required exports (`open-view` and the `custom-view`
  resource) without changing the extension API version (0.1), as #20 did for
  `submit-form`: components built against the earlier contract must be
  rebuilt. Pane refuses them at install as built for an older extension API
  shape ([current decisions](current-decisions.md#explicitly-unresolved-or-deferred),
  item 5).
