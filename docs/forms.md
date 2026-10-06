# Extension forms

Added for [#20](https://github.com/hoangvu12/pane/issues/20) (US36, US38, T05,
G2). A form is the first standard control set an extension can use beyond the
list view. It is deliberately small: a single-line text field, a choice of one
option, a submit button and a validation/error flow. It is not a widget
library, and it does not settle the rest of the G2 UI contract (custom views,
drawing, other controls).

## Contract

Defined in [`wit/extension.wit`](../wit/extension.wit), identically for Rust
([`pane-guest`](../guests/pane-guest/src/lib.rs)) and JavaScript/TypeScript
([`@pane/extension`](../guests/js/pane.d.ts)):

- An item may carry a `form` (title, fields, submit label; the WIT record
  `form`, carried in the list's tree, see [list-tree.md](list-tree.md)).
  Activating such an item opens the form instead of running its action.
- A `field` has an `id`, a `label` and a `kind`: `text` (single line, starts
  empty, optional placeholder) or `choice` (a list of options, the first
  starts chosen; the value is the chosen option's id).
- Submitting calls `submit-form(item-id, values)`, with one `field-value` per
  field in form order. The command answers with text, shown as the result, or
  a `form-error { field, message }`. A form error is the command's
  validation message to the user, not a failure, so Pane shows it as written:
  with a `field` that names one of the form's fields, the message is shown
  under that field and the status line reads `<label>: <message>`; without
  one (or with an unknown field), the status line is the message itself.
  Only failures, an `Err` from `render`/`handle-event` or a trap, are
  prefixed ("The extension reported an error: ...", "The extension crashed:
  ...").
- The launcher submits only choices the form offers; validating text (empty,
  too long, ...) is the command's job.
- In JS/TS, `submitForm` rejects by throwing a plain `FormError` object. Any
  other exception, such as an `Error`, traps the call (checked with a scratch
  build of the sample that throws `new Error`): Pane shows "The extension
  crashed" and the next call starts a fresh instance.

The runnable example is the "Greet someone" item of the three samples
([Rust](../guests/sample-rust/src/lib.rs),
[JavaScript](../guests/sample-js/src/index.js),
[TypeScript](../guests/sample-ts/src/index.ts)); author instructions are in
[guests/README.md](../guests/README.md#forms).

## Host behavior

The public host interface is [`pane_core::Launcher`](../crates/pane-core/src/launcher.rs):
`Screen::Form`, which carries the form (fields, values and errors),
`set_field_value`, `submit_form` and `back`. The window only renders that
state and maps input to those calls.

| Input | On the form screen |
| --- | --- |
| Typing, Backspace/Delete, arrows, Home/End (Cmd-arrows on macOS), word moves, select all, cut/copy/paste, undo/redo | Edit the focused text field (GPUI CE's editable text element and its default bindings) |
| Tab / Shift-Tab | Next / previous control: text fields and choices in order, then the submit button, wrapping around |
| Up/Left, Down/Right on a choice | Previous / next option |
| Enter anywhere | Submit |
| Space on the submit button, or a click on it | Submit |
| Click on an option | Choose it and focus the choice |
| Escape | Back to the command's list, with the form's item still selected |

When a form opens, focus is on its first field. After a rejected submission,
focus moves to the rejected field. Editing a field clears its error. Each
control is one tab stop (the text field's wrapper node holds the stop; the
editable text element inside only shares its focus handle).

While a submission waits for its reply the status is "running", the fields
stay editable, and submitting again (Enter, Space, a click) does nothing, so
the command sees one `submit-form` call at a time. The reply is applied as if
it had arrived before any edit made meanwhile: a rejected field the user has
changed since is not marked (editing clears its error) and focus is not moved
there, though the status line still shows the rejection. A reply that arrives
after the user went back is discarded.

## Accessibility

Established, and checked through GPUI's accessibility tree
(`Window::debug_a11y_tree_json`), not through private element layouts:

- The form is a `Form` node labelled with the form title.
- A text field is a `TextInput` named by its label, with its current text as
  value, its placeholder, and after a rejection its error as description. It
  is the node reported as focused while the field has keyboard focus.
- A choice is a `RadioGroup` named by its label (error as description), whose
  options are `RadioButton`s with label, toggled state and position in set.
  The group holds keyboard focus and the chosen option is its active
  descendant, so the chosen option is reported as focused.
- The submit button is a `Button` named by its submit label.
- The result or error is the existing `Status` node.

Not supported, and not claimed:

- **Invalid state.** GPUI CE has no setter for AccessKit's `invalid` flag, so
  a rejected field is marked only by its description and the status text.
- **Text details.** The editable text element has no accessibility support of
  its own (listed in its backlog); Pane exposes only the field's whole value.
  Caret position, selection and text ranges are not reported, so screen
  reader character/word review inside the field is not expected to work.
- **Assistive-technology actions.** No accessibility action handlers are
  registered, so "press" or "set value" requests from a screen reader or
  automation client do nothing; only keyboard and pointer input work.
- **Label association.** The visible label is plain text; the control's name
  is set directly rather than linked to the label element.
- **Live announcements.** No screen reader (Narrator/NVDA, VoiceOver,
  Orca) was run on any platform. Whether the status change or the focus move
  after a rejection is announced is unverified.

## Checks

Contract checks, run for each of the Rust, JavaScript and TypeScript samples
through the launcher's public interface
([`crates/pane-core/tests/samples.rs`](../crates/pane-core/tests/samples.rs)):
the form's fields and initial values, a valid submission's answer, an empty
name rejected on its field with values kept and a corrected form accepted, a
too-long name rejected, and an unknown choice rejected by the guest itself.
Host behavior with the Rust sample and the faulty fixture
([`launcher.rs`](../crates/pane-core/tests/launcher.rs)): Back from a form,
clearing an error by editing, ignoring options the form does not offer, a
form-level error, a stale reply after Back, and a second submission while
one is pending being ignored (with an edit made meanwhile not marked by the
older rejection).

Window checks through GPUI's test platform, with real key and mouse events
dispatched to the window and real guests
([`crates/pane/tests/window.rs`](../crates/pane/tests/window.rs)): filling in
and submitting by keyboard and the rejected-field flow for all three
languages; Tab/Shift-Tab visiting each control exactly once in both
directions over two rounds, editing keys, input-method composition, clicks,
Space on the button and the accessibility nodes with the Rust sample.

What the composition test proves, and what it does not: GPUI CE's test
platform (at the pinned revision) has no public way to reach the window's
platform input handler, and its keystroke simulation only commits text, never
marks it. The test therefore calls `replace_and_mark_text_in_range` and then
`replace_text_in_range` on the name field's editing state
(`EntityInputHandler`), after checking that this state's focus handle is the
focused one. It proves the field's editing state handles marked (composing)
text and its commit, and that the focused field is the one composed into.
Typed text going through the window's input handler to the focused field is
proven separately, by the typing tests. It does not prove that the window
routes platform IME calls (NSTextInputClient, TSF, XIM/Wayland text-input) to
that field. The window exposes the editing state for this test only
(`LauncherWindow::text_field`, hidden from the docs).

Native checks: the GUI smoke scripts open the Rust form, submit it empty,
type a name, Tab, Down and Enter, and assert the error and result colors.
Only the Rust form is driven natively, to keep the smokes short (each JS or TS
component is about 4 MB of code to compile on first use); the host and window code is the
same for every language, and the JavaScript and TypeScript forms are shown
equivalent only by the contract checks and window checks above, not by a
native run. On
Linux X11 this ran on 2026-09-28 ([findings](platforms/linux.md#text-input-and-accessibility-findings));
the macOS and Windows steps are written but have not run yet
([macOS](platforms/macos.md#text-input-and-accessibility-findings),
[Windows](platforms/windows.md#text-input-and-accessibility-findings)). No
real input method was used on any platform.
