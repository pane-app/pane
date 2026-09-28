// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Pane's settings sample in JavaScript: a command whose chosen greeting style
// Pane keeps between runs, saved with `pane:extension/settings`. Items,
// titles, results and errors match the Rust settings sample
// (guests/sample-settings) and the TypeScript one.
// @ts-check
import { get, set } from "pane:extension/settings@0.1.0";

/** The settings key holding the chosen greeting style. */
const STYLE = "greeting-style";

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
    // A settings error (get throws) is shown to the user as the command's error.
    const style = get(STYLE);
    return {
      title: style === null ? "Greeting" : `Greeting: ${style}`,
      items: [
        item("formal", "Use a formal greeting", "Saved in Pane's settings"),
        item("casual", "Use a casual greeting", "Saved in Pane's settings"),
        item("greet", "Greet me", "Answer in the saved style"),
      ],
    };
  },

  async runAction(itemId) {
    switch (itemId) {
      case "formal":
      case "casual":
        set(STYLE, itemId);
        return `Saved the ${itemId} greeting`;
      case "greet":
        switch (get(STYLE)) {
          case "formal":
            return "Good day to you";
          case "casual":
            return "Hi there";
          default:
            throw new Error("No greeting style is saved yet; choose one first");
        }
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
