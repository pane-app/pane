// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Pane's operations sample in JavaScript. Its package publishes the operation
// `greet` (under `operations` in its pane.json), served by
// `publishedOperations` (built with it through package.json's `"pane"`), and
// its command calls the `greet` operation the Rust operations sample
// publishes, with `pane:extension/operations`. Items, titles, results and
// errors match the Rust sample (guests/sample-operations) and the TypeScript
// one.
//
// `greet` version 1 takes `{"name": "<name>"}` and answers
// `{"greeting": "Hello, <name>, from JavaScript"}`, or the error "a name is
// needed".
// @ts-check
import { call } from "pane:extension/operations@0.1.0";

/**
 * The packages this command calls, by source: relative to this package's own
 * source folder, so the samples work side by side wherever they are.
 */
const RUST = "local:../sample-operations";
const MISSING = "local:../no-such-extension";

/**
 * Calls `greet` version 1 of the package with `source` for `name`, and
 * returns its greeting. A failed call throws "<kind>: <message>".
 * @param {string} source
 * @param {string} name
 * @returns {Promise<string>}
 */
async function greet(source, name) {
  let result;
  try {
    result = await call(source, "greet", 1, JSON.stringify({ name }));
  } catch (error) {
    /** @type {import("pane:extension/operations@0.1.0").CallError} */
    const { kind, message } = /** @type {any} */ (error).payload;
    throw new Error(`${kind}: ${message}`);
  }
  const { greeting } = JSON.parse(result);
  if (typeof greeting !== "string") {
    throw new Error("the answer has no greeting");
  }
  return greeting;
}

/**
 * @param {string} id
 * @param {string} title
 * @returns {import("@pane/extension").Item}
 */
const item = (id, title) => ({ id, title });

/** @type {import("@pane/extension").Command} */
export const command = {
  async getView() {
    return {
      title: "Call from JavaScript",
      items: [
        item("ask-rust", "Ask Rust to greet"),
        item("ask-empty", "Ask Rust with no name"),
        item("ask-missing", "Ask an extension that is not installed"),
      ],
    };
  },

  async runAction(itemId) {
    switch (itemId) {
      case "ask-rust":
        return `Rust answered: ${await greet(RUST, "JavaScript")}`;
      case "ask-empty":
        return `Rust answered: ${await greet(RUST, "")}`;
      case "ask-missing":
        return `Nobody answered: ${await greet(MISSING, "JavaScript")}`;
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

/** @type {import("@pane/extension").PublishedOperations} */
export const publishedOperations = {
  async runOperation(operation, input) {
    if (operation !== "greet") {
      throw new Error(`unknown operation: ${operation}`);
    }
    const { name } = JSON.parse(input);
    if (typeof name !== "string" || name === "") {
      throw new Error("a name is needed");
    }
    return JSON.stringify({ greeting: `Hello, ${name}, from JavaScript` });
  },
};
