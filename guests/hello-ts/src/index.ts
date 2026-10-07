// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// A minimal TypeScript command to develop with Pane's development mode:
// build it once, install this folder, choose "Develop Hello TypeScript" in
// Manage extensions, then edit GREETING and save. Pane type-checks and
// builds the package with tools/componentize-js/pane_js.py and reloads it
// while it keeps running; "Say hello" then shows the new text in a toast.
// See guests/README.md.
import type { Command } from "@pane/extension";
import { showToast } from "@pane/extension/feedback";

/** What "Say hello" shows in a toast. */
const GREETING: string = "Hello from TypeScript";

export const command: Command = {
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
