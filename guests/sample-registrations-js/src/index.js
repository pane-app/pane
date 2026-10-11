// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Pane's registrations sample in JavaScript: a command that registers at
// run time what it owns (ADR 0041, #158) — a dynamic root item under its
// own command, updated by a timer; a folder watcher on the folder its
// settings name; and a provision of the `pane-samples:greet@1` capability,
// made after a "Sign in" action and held until "Sign out" or the code's
// replacement. The package's `pane.json` declares the activation entry
// point (its package.json sets `"pane": { "activate": true }`), so the
// registrations exist as soon as Pane may run the code; each handle lives
// as long as the code that made it, so a disable, a reload, an update, an
// uninstall or a pause undoes them all, and activating again makes them
// afresh. Items, titles, results and errors match the Rust and TypeScript
// registrations samples.
// @ts-check
import { showToast } from "@pane-app/extension/feedback";
import * as registrations from "@pane-app/extension/registrations";
import { get, set } from "pane:extension/settings@0.1.0";
import * as content from "pane:extension/content@0.1.0";

/** The command's id in `pane.json`, which the dynamic root item is under. */
const COMMAND = "registrations";

/** The content key holding how many timer firings there have been, ever. */
const TICKS = "ticks";

/** The content key holding how many folder watcher changes there have
 * been, ever. */
const CHANGES = "changes";

/** The settings key holding the folder the watcher watches, as an
 * absolute path; a test or a user writes it. Empty for none. */
const FOLDER = "folder";

/** How often the timer fires, in seconds. */
const EVERY = 1;

/** The dynamic root item the activation registered, and the handles of
 * the timer that updates it and the folder watcher: this module's own
 * state, dropped with it when the generation ends. A handle that is not
 * kept undoes its registration, so all three live as long as the code.
 * @type {import("@pane-app/extension/registrations").RootItem | undefined} */
let item;

/** @type {import("@pane-app/extension/registrations").Handle | undefined} */
let timer;

/** @type {import("@pane-app/extension/registrations").Handle | undefined} */
let watcher;

/** The provision of the capability, held while signed in.
 * @type {import("@pane-app/extension/registrations").Handle | undefined} */
let provision;

/** The events entry point: the timers and watchers registered call back
 * into this module's callbacks, which the SDK keeps. */
export const events = { handleEvent: registrations.handleEvent };

/** The activation entry point: registers the dynamic root item, the timer
 * that updates it and the folder watcher. */
export const lifecycle = {
  async activate() {
    item = registrations.rootItem(COMMAND, countedItem());
    timer = registrations.every(EVERY, async () => {
      const ticks = counted(TICKS) + 1;
      content.set(TICKS, String(ticks));
      item?.update(countedItem());
    });
    const folder = get(FOLDER)?.trim();
    if (folder) {
      watcher = registrations.watchFolder(folder, false, async (changes) => {
        const paths = changes.tag === "paths" ? changes.val.length : 1;
        const changed = counted(CHANGES) + paths;
        content.set(CHANGES, String(changed));
        item?.update(countedItem());
      });
    }
  },
  // This sample opts into the state handoff's interface without keeping
  // state: nothing is handed over, so nothing is ever restored.
  async snapshot() {},
  async restore() {
    throw new Error("this sample keeps no state across a replacement");
  },
};

/**
 * The count kept in the command's content, zero before the first.
 * @param {string} key
 * @returns {number}
 */
function counted(key) {
  const saved = content.get(key);
  const count = saved === null ? 0 : Number.parseInt(saved, 10);
  return Number.isNaN(count) ? 0 : count;
}

/**
 * The dynamic root item as it stands: a row of root search under this
 * command, its subtitle saying where the counts stand.
 * @returns {import("@pane-app/extension/registrations").Item}
 */
function countedItem() {
  return {
    id: "counted",
    title: "Registrations: counting",
    subtitle: `${counted(TICKS)} timer firings, ${counted(CHANGES)} watcher changes (JavaScript)`,
    actions: [
      {
        title: "Add one",
        onAction: async () => {
          const ticks = counted(TICKS) + 1;
          content.set(TICKS, String(ticks));
          item?.update(countedItem());
        },
      },
    ],
  };
}

/**
 * The operations the capability is made of, served through the export
 * that serves published operations: `greet` arrives qualified by the
 * capability, answered only while the provision is held.
 * @param {string} operation
 * @param {string} input
 * @returns {Promise<string>}
 */
export const publishedOperations = {
  async runOperation(operation, input) {
    if (operation !== "pane-samples:greet@1/greet") {
      throw new Error(`this component serves no \`${operation}\``);
    }
    const name = JSON.parse(input || "{}").name ?? "everyone";
    return JSON.stringify({ greeting: `JavaScript registrations sample greets ${name}` });
  },
};

/** @type {import("@pane-app/extension").Command} */
export const command = {
  async render() {
    const signedIn = provision !== undefined;
    return {
      title: `Registrations: ${counted(TICKS)} firings, ${counted(CHANGES)} changes`,
      items: [
        {
          id: "sign",
          title: signedIn ? "Sign out of the provision" : "Sign in to the provision",
          subtitle:
            "Provides pane-samples:greet@1 while held; the capabilities sample answers from it",
          onAction: async () => {
            if (provision !== undefined) {
              provision.dispose();
              provision = undefined;
              showToast({ title: "Signed out; the provision is dropped" });
            } else {
              provision = registrations.provide("pane-samples:greet@1");
              showToast({ title: "Signed in; the provision is held until signed out" });
            }
          },
        },
        {
          id: "watched",
          title: "The watched folder",
          subtitle: get(FOLDER)
            ? "The folder this settings key names; changes update the row in root search"
            : "None: the settings key `folder` is empty or unset",
        },
      ],
    };
  },
};
