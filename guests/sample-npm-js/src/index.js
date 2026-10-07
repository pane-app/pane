// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// The component of Pane's npm-distributed sample, `@pane-samples/greeter`
// (guests/npm/greeter): a command and the operation `greet`, whose answers
// name the npm package, so that what runs is visibly the copy Pane
// downloaded from npm rather than another sample.
//
// Its command's one item, "Say hello", shows a toast saying "Hello from the
// npm package". `greet` version 1 takes `{"name": "<name>"}` and answers
// `{"greeting": "Hello, <name>, from the npm package"}`, or the error "a
// name is needed".
// @ts-check
import { showToast } from "@pane/extension/feedback";

/** @type {import("@pane/extension").Command} */
export const command = {
  async render() {
    return {
      title: "Greeter from npm",
      items: [
        {
          id: "greet",
          title: "Say hello",
          subtitle: "Answer from the npm package",
          onAction: async () => {
            showToast({ title: "Hello from the npm package" });
          },
        },
      ],
    };
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
    return JSON.stringify({ greeting: `Hello, ${name}, from the npm package` });
  },
};
