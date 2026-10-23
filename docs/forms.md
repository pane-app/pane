# Extension forms

A form is a designed view now (#241). This page points to where forms
are described.

In the tree, a form is a `form` node. Its children are the author's own
layout, with the fields anywhere in it, and its submission is an action.
[`docs/designed-tree.md`](designed-tree.md) describes everything a form
is now:

- the [form node and its fields](designed-tree.md#forms): text, password,
  text area, checkbox, toggle, date and date-time pickers, dropdowns with
  sections and search, tag pickers, file and folder pickers,
  descriptions, separators and links. Each has a title, note, error,
  default value, auto-focus and a remembered value.
- the submission's payload and keyboard rules.
- the SDK surfaces: `Form` and `Form.*` in JSX, `pane_extension::form` in
  Rust.

Pane's own forms (the argument form, the Setup screen, the alias form and
the npm and Git install forms) are the same components, in a tree the
launcher itself builds. The launcher answers them instead of an
extension.

## The older typed form

The typed form this document used to describe is gone: the list tree's
`form`/`form` view, and the `submit-form` export of the command
interface. A component built for that older shape is refused before it
runs, with a message naming the change.
