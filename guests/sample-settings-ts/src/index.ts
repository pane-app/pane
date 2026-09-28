// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Pane's settings sample in TypeScript: a command whose chosen greeting style
// Pane keeps between runs, saved with `pane:extension/settings`. Items,
// titles, results and errors match the Rust settings sample
// (guests/sample-settings) and the JavaScript one.
import type { Command, CustomView, FormError, Item, View } from "@pane/extension";
import { get, set } from "pane:extension/settings@0.1.0";

/** The settings key holding the chosen greeting style. */
const STYLE = "greeting-style";

const item = (id: string, title: string, subtitle: string): Item => ({ id, title, subtitle });

async function getView(): Promise<View> {
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
}

async function runAction(itemId: string): Promise<string> {
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
}

async function submitForm(itemId: string): Promise<string> {
  throw { message: `unknown form: ${itemId}` } satisfies FormError;
}

async function openView(itemId: string): Promise<CustomView> {
  throw new Error(`unknown view: ${itemId}`);
}

export const command: Command = { getView, runAction, submitForm, openView };
