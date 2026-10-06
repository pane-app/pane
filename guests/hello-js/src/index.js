// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// A minimal JavaScript command to develop with Pane's development mode:
// build it once, install this folder, choose "Develop Hello JavaScript" in
// Manage extensions, then edit GREETING and save. Pane builds the package
// with tools/componentize-js/pane_js.py and reloads it while it keeps
// running; "Say hello" then answers with the new text. See
// guests/README.md.
// @ts-check

/**
 * What "Say hello" answers.
 * @type {string}
 */
const GREETING = "Hello from JavaScript";

/** @type {import("@pane/extension").Command} */
export const command = {
  async render() {
    return { title: "Hello", items: [{ id: "hello", title: "Say hello", onAction: async () => GREETING }] };
  },
  async submitForm() {
    throw { message: "this command has no forms" };
  },
  async openView() {
    throw new Error("this command has no custom views");
  },
};
