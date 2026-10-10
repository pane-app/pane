// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// The __NAME__ command: it runs without opening a screen and answers in a
// toast, as the `no-view` template's own command does. Added to the
// package by `pane-ext new command`; the entry file (`src/index.ts`) calls
// its `run`.
import type { LaunchRecord } from "@pane-app/extension";
import { showToast } from "@pane-app/extension/feedback";

/** What the command says when root search sent it nothing: it says how to
 *  send it something. */
const NOTHING =
  "Nothing yet: give this command an alias, or make it a fallback, then type into root search";

/** Runs the command: it answers in a toast with the text root search sent
 *  it, if any. A command Pane or another command launches in the
 *  background does its work but shows nothing. */
export async function run(launch: LaunchRecord): Promise<void> {
  if (launch.launchType === "background") {
    return;
  }
  const heard = launch.fallbackText ? `__TITLE__ heard “${launch.fallbackText}”` : NOTHING;
  showToast({ title: heard });
}
