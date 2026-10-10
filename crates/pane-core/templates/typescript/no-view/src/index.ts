// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// __TITLE__, started from pane-ext's `no-view` template: a command that
// runs without opening a screen ("mode": "no-view" in pane.json), telling
// the user what happened with a toast. Root search sends it text through
// its alias or as a fallback, which `run` answers in the toast. `npm run
// dev` (after `npm install`) builds it and hands each build to the
// running Pane, which reloads it on each save.
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

/** What the command says when root search sent it nothing: it says how to
 *  send it something. */
const NOTHING =
  "Nothing yet: give this command an alias, or make it a fallback, then type into root search";

/** The __NAME__ command's work: what it says in its toast. A command Pane
 *  or another command launches in the background does its work but shows
 *  nothing. */
async function theRun(launch: LaunchRecord): Promise<void> {
  if (launch.launchType === "background") {
    return;
  }
  const heard = launch.fallbackText ? `__TITLE__ heard “${launch.fallbackText}”` : NOTHING;
  showToast({ title: heard });
}

export const command: Command = {
  async render(launch: LaunchRecord): Promise<List> {
    opened = launch.command;
    switch (launch.command) {
      // pane-ext new command adds a view command's arm here.
      default:
        throw new Error(`unknown command: ${launch.command}`);
    }
  },

  /** Runs the __NAME__ command: it answers in a toast with the text root
   *  search sent it, if any. */
  async run(id: string, _launch: LaunchRecord): Promise<void> {
    switch (id) {
      case "__NAME__":
        return theRun(_launch);
      // pane-ext new command adds a no-view command's arm here.
      default:
        throw new Error(`unknown command: ${id}`);
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
