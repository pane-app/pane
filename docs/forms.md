# Extension forms

A form is a designed view now (#241): a `form` node of the tree whose
children are the author's own layout — the fields anywhere in it — and
whose submission is an action. The typed form this document used to
describe (the list tree's `form`/`form` view, the `submit-form` export of
the command interface) is gone; a component built for that older shape is
refused before it runs, naming the change.

Everything a form is now is described by
[`docs/designed-tree.md`](designed-tree.md): the [form node and its
fields](designed-tree.md#forms) (text, password, text area, checkbox,
toggle, date and date-time pickers, dropdowns with sections and search,
tag pickers, file and folder pickers, descriptions, separators, links —
each with a title, note, error, default value, auto-focus and a
remembered value), the submission's payload and keyboard rules, and the
SDK surfaces (`Form` and `Form.*` in JSX, `pane_extension::form` in
Rust). Pane's own forms — the argument form, the Setup screen, the
alias form and the npm and Git install forms — are the same components
in a tree the launcher itself builds, answered by the launcher instead
of an extension.
