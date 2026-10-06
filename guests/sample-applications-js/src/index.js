// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Pane's applications sample in JavaScript: it finds the installed
// applications through `pane:extension/applications`, which Pane's host
// provides since a WASI guest cannot see or start them, supplies "Launch
// <name>" for each to root search as indexed results (`indexedResults`,
// built with it through package.json's `"pane"`), and its command lists
// them and opens one. Titles, results and errors match the TypeScript
// sample (guests/sample-applications-ts). The JSDoc types let TypeScript
// check this file against Pane's contract; they are optional.
// @ts-check
import { installed, open } from "pane:extension/applications@0.1.0";

const SAMPLE = "JavaScript applications sample";

/**
 * Calls a host function, turning its error into a plain message.
 * @template T
 * @param {() => T} call
 * @returns {T}
 */
function host(call) {
  try {
    return call();
  } catch (error) {
    const payload = /** @type {{ payload?: unknown }} */ (error).payload;
    throw typeof payload === "string" ? new Error(payload) : error;
  }
}

/**
 * The installed applications, by name.
 * @returns {import("pane:extension/applications@0.1.0").Application[]}
 */
function applications() {
  return host(installed).sort((a, b) =>
    a.name.toLowerCase().localeCompare(b.name.toLowerCase()),
  );
}

/**
 * Opens the application with id `itemId`, the action of its item.
 * @param {string} itemId
 * @returns {Promise<string>}
 */
async function act(itemId) {
  host(() => open(itemId));
  const name = applications().find((app) => app.id === itemId)?.name ?? itemId;
  return `Opened ${name}`;
}

/** @type {import("@pane/extension").Command} */
export const command = {
  async render() {
    return {
      title: SAMPLE,
      items: applications().map((app) => ({
        id: app.id,
        title: app.name,
        subtitle: app.location,
        onAction: () => act(app.id),
      })),
    };
  },

  async submitForm() {
    throw { message: "this sample has no forms" };
  },

  async openView() {
    throw new Error("this sample has no custom views");
  },
};

/** @type {import("@pane/extension").IndexedResults} */
export const indexedResults = {
  async results() {
    return applications().map((app) => ({
      id: app.id,
      title: `Launch ${app.name}`,
      subtitle: SAMPLE,
      action: { tag: "open-application", val: app.id },
    }));
  },
};
