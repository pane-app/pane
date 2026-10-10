// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// A minimal JavaScript command to develop with Pane's development mode:
// build it once, install this folder, choose "Develop" in the Actions menu
// of Hello JavaScript's page in Settings, then edit GREETING and save. Pane builds the package
// with pane-build's JavaScript build and reloads it while it keeps
// running; "Say hello" then shows the new text in a toast. See
// guests/README.md.
// @ts-check

import { showToast } from "@pane-app/extension/feedback";

/**
 * What "Say hello" shows in a toast.
 * @type {string}
 */
const GREETING = "Hello from JavaScript";

/** @type {import("@pane-app/extension").Command} */
export const command = {
  async render() {
    return {
      title: "Hello",
      items: [
        {
          id: "hello",
          title: "Say hello",
          onAction: async () => {
            showToast({ title: GREETING });
          },
        },
      ],
    };
  },
  async submitForm() {
    throw { message: "this command has no forms" };
  },
  async openView() {
    throw new Error("this command has no custom views");
  },
};
