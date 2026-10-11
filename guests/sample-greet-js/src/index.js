// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Pane's greet capability provider sample in JavaScript. Its package
// provides the capability `pane-samples:greet@1` (under `provides` in its
// pane.json), served by `publishedOperations` (built with it through
// package.json's `"pane"`); its command only says what it provides. The
// Rust and TypeScript samples (guests/sample-greet and guests/sample-greet-ts)
// provide the same capability, each answering with its own language's name,
// so a consumer installed with any of them works without naming any of them.
//
// The capability's `greet` operation takes `{"name": "<name>"}` and answers
// `{"greeting": "Hello, <name>, from JavaScript"}`, or the error "a name is
// needed". Pane calls it with the operation qualified by its capability —
// `pane-samples:greet@1/greet` — so one component can tell a capability
// call from a call by identity to an operation of the same name; nothing
// else is served.
// @ts-check
import { showToast } from "@pane-app/extension/feedback";

/** The capability this package provides. */
const CAPABILITY = "pane-samples:greet@1";
/** The operation Pane qualifies by it. */
const GREET = "pane-samples:greet@1/greet";

/** @type {import("@pane-app/extension").Command} */
export const command = {
  async render() {
    return {
      title: "JavaScript greet provider",
      items: [
        {
          id: "about",
          title: "What this provides",
          subtitle: "The pane-samples:greet@1 capability, for other extensions",
          onAction: async () => {
            showToast({
              title: `Provides ${CAPABILITY}: other extensions call its greet operation through Pane`,
            });
          },
        },
      ],
    };
  },

  async openView(itemId) {
    throw new Error(`unknown view: ${itemId}`);
  },
};

/** @type {import("@pane-app/extension").PublishedOperations} */
export const publishedOperations = {
  async runOperation(operation, input) {
    if (operation !== GREET) {
      throw new Error(`unknown operation: ${operation}`);
    }
    const { name } = JSON.parse(input);
    if (typeof name !== "string" || name === "") {
      throw new Error("a name is needed");
    }
    return JSON.stringify({ greeting: `Hello, ${name}, from JavaScript` });
  },
};
