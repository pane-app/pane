// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Pane's helper sample in JavaScript: a command that runs a native helper
// its package ships, with `pane:extension/helpers`. The helper, `pane-echo`
// (guests/helpers/echo), is an ordinary program built for each system; the
// package's pane.json names its file for each target under `helpers`, and
// Pane runs the one for the system it runs on. Items, results and errors
// match the Rust sample (guests/sample-helper) and the TypeScript one.
//
// "Echo within a second" races the slow helper against a one-second timer.
// A JavaScript promise cannot be cancelled, so the run is left behind when
// the timer wins; Pane ends the helper's process as soon as the call that
// started it returns.
// @ts-check
import { run } from "pane:extension/helpers@0.1.0";
import { set } from "pane:extension/settings@0.1.0";
import { waitFor } from "wasi:clocks/monotonic-clock@0.3.0";

/** The helper's name in the package's pane.json. */
const ECHO = "echo";
/** The settings key where "Echo after waiting" notes how far it got. */
const WAITING = "helper-wait";
/** The settings key where "Echo after a long wait" notes that it finished. */
const LONG_WAIT = "helper-long-wait";
/** How long "Echo within a second" lets the helper run, in nanoseconds. */
const LIMIT = 1_000_000_000;

/**
 * Runs the helper `helper` with `args` and `input`. A failed run throws
 * "<kind>: <message>".
 * @param {string} helper
 * @param {string[]} args
 * @param {string} input
 * @returns {Promise<string>}
 */
async function helperRun(helper, args, input) {
  try {
    return await run(helper, args, input);
  } catch (error) {
    /** @type {import("pane:extension/helpers@0.1.0").HelperError} */
    const { kind, message } = /** @type {any} */ (error).payload;
    throw new Error(`${kind}: ${message}`);
  }
}

/**
 * @param {string} id
 * @param {string} title
 * @param {string} subtitle
 * @returns {import("@pane/extension").Item}
 */
const item = (id, title, subtitle) => ({ id, title, subtitle });

/** @type {import("@pane/extension").Command} */
export const command = {
  async getView() {
    return {
      title: "JavaScript helper sample",
      items: [
        item("echo", "Echo through the helper", "Runs the package's native helper for this system"),
        item(
          "wait",
          "Echo after waiting",
          "The helper waits 10 seconds; disabling or reloading stops it",
        ),
        item("limit", "Echo within a second", "Cancels the slow helper after one second"),
        item(
          "long",
          "Echo after a long wait",
          "The helper waits 40 seconds; other extensions answer meanwhile",
        ),
        item("fail", "Make the helper fail", "The helper exits with an error"),
        item(
          "undeclared",
          "Run an undeclared helper",
          "The package's pane.json declares no helper by that name",
        ),
      ],
    };
  },

  async runAction(itemId) {
    switch (itemId) {
      case "echo":
        return helperRun(ECHO, [], "hello from Pane");
      case "wait": {
        set(WAITING, "started");
        // If Pane stops the call meanwhile, the helper's process ends and
        // nothing after this line runs.
        const answer = await helperRun(ECHO, ["--wait", "10"], "after waiting");
        set(WAITING, "finished");
        return answer;
      }
      case "limit": {
        const slow = helperRun(ECHO, ["--wait", "10"], "too late");
        const timer = waitFor(LIMIT).then(() => null);
        const answer = await Promise.race([slow, timer]);
        // Returning ends the call, and with it the helper's process.
        return answer ?? "Stopped the helper after one second";
      }
      case "long": {
        // Longer than the thirty seconds Pane once allowed a helper: other
        // extensions' calls are served while this one waits.
        const answer = await helperRun(ECHO, ["--wait", "40"], "after a long wait");
        set(LONG_WAIT, "finished");
        return answer;
      }
      case "fail":
        return helperRun(ECHO, ["--fail"], "");
      case "undeclared":
        return helperRun("absent", [], "");
      default:
        throw new Error(`unknown item: ${itemId}`);
    }
  },

  async submitForm(itemId) {
    throw { message: `unknown form: ${itemId}` };
  },

  async openView(itemId) {
    throw new Error(`unknown view: ${itemId}`);
  },
};
