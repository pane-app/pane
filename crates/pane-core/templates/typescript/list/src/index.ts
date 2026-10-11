// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// __TITLE__, started from pane-ext's `list` template: a command that opens
// a list of items, each running an action when chosen. `npm run dev`
// (after `npm install`) builds it and hands each build to the running
// Pane, which reloads it on each save; the items below are the list it
// opens.
//
// One component (the WebAssembly file pane.json names) serves every
// command the package declares: render and run receive the command's id,
// so this file matches on it. `pane-ext new command .` adds one.
import type { Command, FieldValue, LaunchRecord, List } from "@pane-app/extension";
import { showToast } from "@pane-app/extension/feedback";

// pane-ext new command adds a command's file here.

/** The command whose screen is open, so `submitForm` reaches the command
 *  whose form was filled in. */
let opened = "__NAME__";

/** What the "Say hello" item shows in a toast. */
const GREETING = "Hello from __TITLE__";

/** The __NAME__ command's list: what shows at Enter on its row in root
 *  search. Pane asks for it again after each item's action runs. */
async function theList(): Promise<List> {
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
        subtitle: "The items are here in theList; save an edit and Pane reloads it",
      },
    ],
  };
}

export const command: Command = {
  async render(launch: LaunchRecord): Promise<List> {
    opened = launch.command;
    switch (launch.command) {
      case "__NAME__":
        return theList();
      // pane-ext new command adds a view command's arm here.
      default:
        throw new Error(`unknown command: ${launch.command}`);
    }
  },

  async run(id: string, _launch: LaunchRecord): Promise<void> {
    switch (id) {
      // pane-ext new command adds a no-view command's arm here.
      default:
        throw new Error(`\`${id}\` opens a screen; it has no run entry point`);
    }
  },

  async submitForm(itemId: string, _values: FieldValue[]): Promise<string> {
    switch (opened) {
      // pane-ext new command adds a form command's arm here.
      default:
        throw { message: `\`${opened}\` has no forms: ${itemId}` };
    }
  },
};
