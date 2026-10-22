# Native validation — Keyboard page, in-app navigation rebinding (#77)

**Status: plan only.** Per the ticket, native and visual validation is
#84's pass; this branch does not run the app natively. The automated
window-harness results that *were* run are on CI (this branch's Check
legs); they exercise the Keyboard page's recorders, the launcher's
rebound keys, the footer and menu hints, and the Settings window's
isolation through GPUI's test platform, which is not native key delivery,
a real input method, or the real window manager. Everything below is the
capture plan the native run should follow, and this file must say
"passed/failed, evidence" per row once that run happens.

## What this ticket ships

The Keyboard page ([#77](https://github.com/hoangvu12/pane/issues/77)):
rebinding for the bounded set of host navigation actions — previous/next
result, invoke selected action, back, return to root, dismiss launcher
and open Settings — with the provisional defaults Up/Down, Enter,
Escape, Cmd+Esc (macOS) / Shift+Esc (elsewhere), Cmd+W / Ctrl+W, and
Cmd+, / Ctrl+,. The set is closed: no arbitrary key, no extension
action, and none of the text-editing keys a focused field owns
(`Binding::protected` refuses them, and the platform's
select-all/copy/paste/word-navigation combinations with it). Changes are
applied to every window's keys at once (the keymap is re-made over the
new set), saved to `settings.json` under the host settings' rules, and
re-read by a fresh start; a save that fails rolls the effective bindings
back to what the record holds. Escape's order — cancel an active IME
composition, dismiss the open footer menu, then back, clearing a command
search's text before leaving it and root search's before hiding an empty
root — is in the launcher window's back handler; return-to-root and
dismiss are new actions of the bounded set, and the Settings window
closes on the platform's close-window shortcut with the launcher
untouched.

The window-level behavior is tested in
`crates/pane/tests/keyboard.rs` through real keystrokes, clicks and the
accessibility tree, with a fake native registration where the global
hotkey's survival is observed: recording, collision and protected-key
refusals, save-failure rollback, restart, per-action reset (including a
refused reset), the footer keycap and the menu hint following the
effective bindings, window-local isolation, composition cancellation
(driven on the query field's editing state), the menu-first back order,
nested back, forms, custom views, command search, and root dismissal
keeping Pane running.

## What the harness cannot observe (why native evidence is required)

- **The real IME.** The tests drive composition on the query field's
  editing state directly; on a real system the input method takes the
  keys itself while composing, and its cancel (the first Escape of the
  order) must be seen to leave the composed text handled as designed.
  A native run with a Japanese or Vietnamese input method must compose
  in root search, press the back key, and capture the composition
  cancelled without the query clearing or the window hiding.
- **The real key delivery for the rebound keys.** The test platform
  parses keystroke strings; a real keyboard's layouts (where `,` is
  shifted, where the platform modifier is Command/Win/Super) must show
  the recorded binding actually delivered — including a binding
  recorded as `ctrl-,` on a layout where comma is not a direct key.
- **Hide and show against the real window manager.** The empty-root
  Escape and the dismiss binding call `set_visible(false)`; a real run
  must show focus returning to the previous application, the Settings
  window staying open, and the Open Pane hotkey still summoning the
  hidden launcher.
- **macOS's Cmd-spelled defaults.** The harness runs the platform's own
  defaults; a native macOS run must show Cmd+Esc, Cmd+W and Cmd+,
  working and rebindable, with the platform modifier displayed as
  Command.
- **Wayland.** Nothing in this ticket registers global shortcuts, but
  the dismissal-keeps-running check must be seen on a Wayland session
  too (the in-app keys are window-local and must work there).

## Capture plan (the next native run)

Reuse the harness of
[`settings-72/capture-settings.ps1`](../settings-72/capture-settings.ps1)
(scratch `PANE_DATA_DIR`, guarded clicks, focus confirmation with the
real foreground HWND), extended with:

1. Start Pane with a scratch data dir; open Settings' Keyboard page;
   capture the seven rows with their default bindings.
2. Record a new next-result binding (guarded click on the row, then
   `Ctrl+N`): capture the page showing it, the scratch `settings.json`
   holding `"next-result": "ctrl-n"`, and the launcher's list moving
   with `Ctrl+N` while `Down` no longer does — with the query field
   focused, typing still editing the query.
3. Record a protected key (a plain `J`): capture the refusal note and
   the recorder still listening; Escape cancels.
4. Record a colliding binding (Back on `Ctrl+N`): capture the refusal
   naming the other action; reset Back (the pointer-only row) and
   capture `Escape` working again.
5. Rebind invoke to another key: capture the footer's keycap and the
   menu's Settings hint showing it (the `Ctrl+,` default too, before
   the rebind), and the new key opening the selected result.
6. With an input method composing in root search, press the back key:
   capture the composition cancelled, the query intact, the window
   neither navigated nor hidden; the next press clears the query; the
   one after hides the launcher (capture the process alive, the Open
   Pane hotkey still summoning it, a Settings window if one was open
   still present).
7. Dismiss the launcher with the platform's close-window shortcut while
   Settings has focus: capture only Settings closing. Then, with the
   launcher focused, dismiss it: capture only the launcher hidden.
8. Restart Pane over the same scratch data dir: capture the recorded
   bindings working before any Settings window opens.
9. (macOS) repeat 1–8 with the Cmd-spelled defaults.

Record per-row passed/failed with the capture files, as `settings-72`'s
table does; nothing here mutates the user's real shortcuts or settings
(the scratch data dir keeps every change inside the run).
