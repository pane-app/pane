// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// The __NAME__ command: a list of items, each running an action when
// chosen, as the `list` template's own command is. Added to the package by
// `pane-ext new command`; the entry file (`src/index.ts`) calls its
// `render`.
import type { LaunchRecord, List } from "@pane-app/extension";
import { showToast } from "@pane-app/extension/feedback";

/** What the "Say hello" item shows in a toast. */
const GREETING = "Hello from __TITLE__";

/** The command's list: what shows at Enter on its row in root search. Pane
 *  asks for it again after each item's action runs. `_launch` is how the
 *  command was opened, unused here and kept for the commands that want
 *  it. */
export async function render(_launch: LaunchRecord): Promise<List> {
  return {
    title: "__TITLE__",
    items: [
      {
        id: "greet",
        title: "Say hello",
        subtitle: "Shows a toast, as an item's action does",
        onAction: async () => {
          showToast({ title: GREETING });
        },
      },
      {
        id: "edit",
        title: "Edit this list",
        subtitle: "The items are here; save an edit and Pane reloads the command",
      },
    ],
  };
}
