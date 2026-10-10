// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Pane's sample of the keywords a command declares in `pane.json`, in
// JavaScript (#197): "Empty the Bin" is found by its keywords ("trash",
// "rubbish") rather than its title, and the "Moons" command (`"mode":
// "provider"`, `"indexedResults": true`, built with it through
// package.json's `"pane"`) supplies one indexed result, "The Moon", found
// by its alternate title ("Luna") and its keywords ("satellite", "rock")
// as a command's title and subtitle are found. Toasts and errors match
// the Rust keywords sample (guests/sample-keywords) and the TypeScript
// one.
// @ts-check
import { showToast } from "@pane-app/extension/feedback";

/** @type {import("@pane-app/extension").Command} */
export const command = {
  // Empties the bin, saying so. The provider command is never run.
  async run(id, launch) {
    switch (id) {
      case "bin":
        showToast({ title: "Emptied the bin" });
        return;
      case "moons":
        throw new Error("the Moons command only answers root search");
      default:
        throw new Error(`unknown command: ${id}`);
    }
  },
};

/** @type {import("@pane-app/extension").IndexedResults} */
export const indexedResults = {
  async results() {
    return [
      {
        id: "moon",
        title: "The Moon",
        subtitle: "What the keywords sample supplies",
        alternateTitles: ["Luna"],
        keywords: ["satellite", "rock"],
        action: { tag: "open", val: { target: "https://example.com/moon" } },
      },
    ];
  },
};
