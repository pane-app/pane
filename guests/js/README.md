# @pane-app/extension

Write [Pane](https://github.com/pane-app/pane) extensions in JavaScript or
TypeScript. This package is the types of Pane's extension contract, the
adapter that turns what a command throws into an answer Pane shows, and
the helpers commands import:

- `@pane-app/extension`: the types (`Command`, `List`, `Item`, `Form`, ...).
- `@pane-app/extension/http`: web requests.
- `@pane-app/extension/feedback`: toasts, HUDs, confirmations and the window.
- `@pane-app/extension/system`: the clipboard, opening, revealing and
  recycling, with the standard actions.
- `@pane-app/extension/preferences`: the package's preferences.
- `@pane-app/extension/icons`: icon helpers.
- `@pane-app/extension/programs`: the system's own programs.

It carries no build tool: a command is bundled and built into a WASI 0.3
component by Pane's tools.

```ts
import type { Command } from "@pane-app/extension";
import { showToast } from "@pane-app/extension/feedback";

export const command: Command = {
  async render() {
    return {
      title: "Hello",
      items: [
        {
          id: "hello",
          title: "Say hello",
          onAction: async () => {
            showToast({ title: "Hello from TypeScript" });
          },
        },
      ],
    };
  },
  async openView() {
    throw new Error("this command opens no designed view");
  },
};
```

## Versions

The package's version follows the extension API it targets: 0.1.x is for
API 0.1 (`"apiVersion": "0.1"` in a package's `pane.json`). A new API
version is a new minor version before 1.0, and a new major version from 1.0.

## More

The guide to writing commands, with an example of every interface, is
[`guests/README.md`](https://github.com/pane-app/pane/blob/main/guests/README.md)
in Pane's repository.

Licensed under either of Apache-2.0 or MIT, at your option.
