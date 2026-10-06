// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Pane's preferences sample in JavaScript: a package that declares
// preferences of every type in `pane.json`, for the whole extension and for
// single commands, and commands that read their effective values with
// `getPreferenceValues` from `@pane/extension/preferences`. Commands and
// answers match the Rust preferences sample (guests/sample-preferences) and
// the TypeScript one.
//
// - The package declares an API key (a password, required, no default),
//   units (a dropdown, required, with a default), a greeting (text,
//   optional) and "Verbose" (a checkbox). Until the API key is set every
//   command needs setup: Pane shows its Setup screen, with the package's
//   HELP.md, before the first run.
// - "Show preferences" is a view command that also declares a notes folder
//   (a folder, required), a notes file (a file) and an editor (an
//   application): its list shows every value it received.
// - "Report preferences" is a no-view command that also declares "Loud" (a
//   checkbox): it answers where it was launched from and its values,
//   shouted when Loud is on.
// - "Tick" runs every minute on its own schedule, in the background, once
//   the package is set up, counting its runs; "Last tick" answers the count.
// @ts-check
import { getPreferenceValues } from "@pane/extension/preferences";
import { get, set } from "pane:extension/settings@0.1.0";

/** The settings key holding how many times "Tick" ran. */
const TICKS = "ticks";

/**
 * "none" for a value that is absent.
 * @param {unknown} value
 * @returns {string}
 */
function orNone(value) {
  return typeof value === "string" ? value : "none";
}

/**
 * The number of characters of `text`, as Rust counts them.
 * @param {unknown} text
 * @returns {number}
 */
function characters(text) {
  return typeof text === "string" ? Array.from(text).length : 0;
}

/**
 * What "Report preferences" answers, launched as `record` says.
 * @param {import("@pane/extension").LaunchRecord} record
 * @returns {string}
 */
function report(record) {
  const values = getPreferenceValues();
  const answer =
    `Report from ${record.source}: API key of ${characters(values.apiKey)} characters; units: ${values.units}; ` +
    `greeting: ${orNone(values.greeting)}; verbose: ${values.verbose === true}`;
  return values.loud === true ? answer.toUpperCase() : answer;
}

/**
 * What "Tick" answers, counting the run.
 * @returns {string}
 */
function tick() {
  const saved = get(TICKS);
  const counted = saved === null ? 0 : Number.parseInt(saved, 10);
  const ticks = (Number.isNaN(counted) ? 0 : counted) + 1;
  set(TICKS, String(ticks));
  return `Ticked ${ticks} times`;
}

/**
 * What "Last tick" answers: how many times "Tick" ran.
 * @returns {string}
 */
function last() {
  return `Ticks: ${get(TICKS) ?? "0"}`;
}

/** @type {import("@pane/extension").Command} */
export const command = {
  async render() {
    const values = getPreferenceValues();
    return {
      title: "Preferences",
      items: [
        { id: "api-key", title: `API key: ${characters(values.apiKey)} characters` },
        { id: "units", title: `Units: ${values.units}` },
        { id: "greeting", title: `Greeting: ${orNone(values.greeting)}` },
        { id: "verbose", title: `Verbose: ${values.verbose === true}` },
        { id: "folder", title: `Notes folder: ${values.folder}` },
        { id: "notes", title: `Notes file: ${orNone(values.notes)}` },
        { id: "editor", title: `Editor: ${orNone(values.editor)}` },
      ],
    };
  },
  async run(id, record) {
    switch (id) {
      case "report":
        return report(record);
      case "tick":
        return tick();
      case "last":
        return last();
      default:
        throw new Error(`unknown command: ${id}`);
    }
  },
};
