# The UI is accepted as built, and the reference comparison is retired

Accepted 2026-10-06 by the user's decision: the user is happy with the launcher and Settings UI as they now are, after the Windows port of #90 and the Raycast-style redesign of #105. The UI's acceptance is the user's judgement of the app itself, no longer a measured match to the authored reference.

So the reference comparison is gone. The visual fixture (the `pane-visual-fixture` program and its scenes), the workbench that captured the reference's boards and the fixture side by side and compared them (`scripts/visual-workbench*`, `docs/visual-workbench.md`), the re-extracted reference boards (`docs/research/ui-port/reference/`) and the retained authored page (`docs/evidence/ui-prototype/reference/`) are removed, with the UI code only the fixture drew. The theme's values stay as they are. Comments that name the reference as where a value came from stay, as its history; links to the removed files lead to the archived copy (tag `archive/impl-ui-91-2026-10-06`).

The evidence #91 to #103 recorded stays under `docs/evidence/`, as the record of what was compared then; closed issues link it. #103 (the integrated acceptance against the reference) and #107 (re-recording that comparison for the redesign) are closed as not planned. The checks that test behaviour in a real window rather than likeness to the reference, #108's background image, compact window and restart persistence, still stand.

A later change to the UI is judged the same way: in the app, by the user, with the window tests guarding its behaviour. The removed tools stay in the history (tag `archive/impl-ui-91-2026-10-06`) if a comparison is ever wanted again.
