# @pane-app/extension

Write [Pane](https://github.com/pane-app/pane) extensions in JavaScript or
TypeScript. This package has the types of Pane's extension contract and the
helpers your commands import. It also has the adapter that turns what a
command throws into an answer Pane shows.

There is no build tool in it. Pane's tools bundle a command and build it
into a WASI 0.3 component.

## A first command

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

## What you can import

| Import | What it has |
| --- | --- |
| `@pane-app/extension` | The types (`Command`, `List`, `Item`, `Form`, ...). |
| `@pane-app/extension/http` | Web requests. |
| `@pane-app/extension/feedback` | Toasts, HUDs, confirmations and the window. |
| `@pane-app/extension/system` | The clipboard, opening, revealing and recycling, with the standard actions. |
| `@pane-app/extension/preferences` | The package's preferences. |
| `@pane-app/extension/icons` | Icon helpers. |
| `@pane-app/extension/programs` | The system's own programs. |

## Versions

The package's version follows the extension API it targets. 0.2.x is for
API 0.2 (`"apiVersion": "0.2"` in a package's `pane.json`).

A new API version gets a new minor version before 1.0, and a new major
version from 1.0.

## More

The guide to writing commands, with an example of every interface, is
[`guests/README.md`](https://github.com/pane-app/pane/blob/main/guests/README.md)
in Pane's repository.

## License

Licensed under either of Apache-2.0 or MIT, at your option.
