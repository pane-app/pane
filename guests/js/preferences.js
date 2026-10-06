// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// The preferences of JS/TS commands (`@pane/extension/preferences`): the
// typed values the package's `pane.json` declares under `preferences`, for
// the whole extension or for one command, which the user sets in Pane (on
// the Setup screen before the command's first run, and on the extension's
// card in Settings). Pane stores them; a command only reads them. Bundled
// into the command that imports it, like any npm module.

import { values } from "pane:extension/preferences@0.1.0";

/**
 * The effective preference values of the command Pane is running, or of
 * the command with id `command` of the same package, as an object: each
 * declared name to the value the user set or else its default. A
 * checkbox's value is a boolean, every other kind's a string; a preference
 * with no value and no default is absent. Throws an `Error` with Pane's
 * reason when it refuses.
 * @param {string} [command]
 * @returns {Record<string, string | boolean>}
 */
export function getPreferenceValues(command) {
  let text;
  try {
    text = values(command ?? null);
  } catch (error) {
    const payload = error !== null && typeof error === "object" && "payload" in error
      ? error.payload
      : undefined;
    throw typeof payload === "string" ? new Error(payload) : error;
  }
  return JSON.parse(text);
}
