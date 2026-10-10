// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// __TITLE__, started from pane-ext's `detail` template: a command that
// takes a query and shows the detail of the text root search sends it,
// through its alias or as a fallback. `npm run dev` (after `npm install`)
// builds it and hands each build to the running Pane, which reloads it on
// each save; the list below is the detail it shows.
//
// One component (the WebAssembly file pane.json names) serves every
// command the package declares: render and run receive the command's id,
// so this file matches on it. `pane-ext new command .` adds one.
import type { Command, FieldValue, LaunchRecord, List } from "@pane-app/extension";
import { showToast } from "@pane-app/extension/feedback";
import { copy } from "@pane-app/extension/system";

// pane-ext new command adds a command's file here.

/** The command whose screen is open, so `submitForm` reaches the command
 *  whose form was filled in. */
let opened = "__NAME__";

/** What the command shows when root search sent it nothing: it says how to
 *  send it something. */
const NOTHING =
  "Nothing yet: give this command an alias, or make it a fallback, then type into root search";

/** The __NAME__ command's detail: the text it was sent, its measures, and
 *  a copy action on its row. */
async function theDetail(launch: LaunchRecord): Promise<List> {
  const text = launch.fallbackText ?? "";
  return {
    title: "__TITLE__",
    items: [
      {
        id: "text",
        title: text === "" ? NOTHING : text,
        subtitle: "What root search sent, through an alias or as a fallback",
      },
      {
        id: "copy",
        title: "Copy the text",
        subtitle: "Puts what root search sent on the clipboard",
        onAction: async () => {
          copy(text);
          showToast({ title: "Copied" });
        },
      },
      {
        id: "characters",
        title: `${[...text].length} characters`,
        subtitle: "Counted as Unicode characters, not UTF-16 units",
      },
      { id: "words", title: `${text.split(/\s+/).filter(Boolean).length} words` },
    ],
  };
}

export const command: Command = {
  async render(launch: LaunchRecord): Promise<List> {
    opened = launch.command;
    switch (launch.command) {
      case "__NAME__":
        return theDetail(launch);
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
