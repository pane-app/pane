# A command declares its mode, and host functions decide what happens after it runs

Accepted 2026-10-06 by the user's decision, after the [Raycast deep dive](../research/raycast-deep-dive.md#command-kinds-and-lifecycle). It changes Pane's current command contract, under which every command opens a list (`get-view`) and an item's action answers with text that Pane shows in the status line (`run-action`). A command now declares whether it opens a screen. What happens once it has run is decided by the command, through host functions, rather than implied by Pane. It amends [ADR 0016](0016-host-registers-global-hotkeys-for-commands.md) for commands that open no screen. It leaves [ADR 0011](0011-extension-call-and-result-api.md) unchanged: launching a command returns no result, and operations remain the way one package calls another for one.

So each command declares `"mode"` in its `pane.json` entry. The mode is `view` or `no-view`. Raycast's third mode, `menu-bar`, is not offered, since Raycast itself has none on Windows. A view command renders its UI ([ADR 0036](0036-extension-ui-is-a-tree-pane-renders-written-with-a-gpui-like-api.md)). A no-view command exports a run entry point. Invoking it (Enter in root search, its alias, a fallback, or another command launching it) calls that entry point with the command's launch record, as the extension contract specification defines them, and opens no screen. Its global hotkey runs it without showing Pane's window, where ADR 0016 opened every command in the window.

After that, the command decides what happens through host functions every command has, whether view or no-view:

- **Close the window.** The command can ask for the query to be cleared, and choose how root search returns: by default, immediately, or suspended (Raycast's three pop-to-root types).
- **Pop to root, and clear the search field.**
- **Show a HUD**, which closes the window first.
- **Show a toast.** Its style is animated, success or failure. The command can update it and give it primary and secondary actions. When the window is hidden, it is shown as a HUD instead.
- **Ask for confirmation.** A confirm dialog can be destructive in style and can offer "don't ask again".
- **Use the clipboard.** The command can copy, and a concealed copy keeps the text out of clipboard history. It can paste into the application that was in front before Pane, and it can read the clipboard.
- **Open things.** The command can open a URL, open a path (optionally with an application it names), reveal a path in the file manager (File Explorer on Windows) and move a path to the Recycle Bin. As in Raycast, Pane opens what the command names, without filtering: a URL of any scheme (`https:`, `mailto:`, `ms-settings:`, an application's own scheme), any file or folder, or an application, including programs. The command is trusted ([ADR 0002](0002-trusted-extensions-and-open-distribution.md)) and can already run programs ([ADR 0033](0033-extensions-may-run-system-programs.md)), so a filter here would protect nothing. The quicklinks default extension follows the same rule, so a quicklink may name any scheme, a file, a folder or an application. File search's own Enter still never runs a program by accident: a program found by file search opens through an explicit action, as Raycast shows such results as files with actions.
- **Launch another command**, passing it context.

Beyond reporting a run that fails, Pane does nothing after a run that the command did not ask for. Raycast's convenience actions, such as copy, then close, then a "Copied to Clipboard" HUD, become SDK helpers composed of these functions. They are not part of the contract, which stays small, and an author can compose the steps differently.

The extension contract specification settles the rest:

- what an action's answered text becomes beside these functions;
- how the query-taking command's `run-query` folds into the launch record' fallback text;
- whether launching another package's command asks first, as it does in Raycast.

Raycast's model is mature and well used. In its corpus, 30% of commands are no-view, and 42% of extensions close the window, show a HUD or pop to root themselves. Pane must know a command's mode before running it, to decide at Enter whether to open a screen. Functions the command calls are more flexible than a fixed set of built-in outcomes. Pane's single answered text cannot express closing the window, a HUD or a confirmation at all.
