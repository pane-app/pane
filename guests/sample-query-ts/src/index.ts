// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Echo, the query sample, in TypeScript: a command that takes a query. It
// answers the text the user sends it from root search, through its alias
// ("ec hello" when the user gave it the alias "ec") or by choosing it as a
// fallback; Pane sends the text only when the user invokes it that way.
// Items, answers and errors match the Rust query sample (guests/sample-query)
// and the JavaScript one. "fail" is refused, to show how an error looks;
// "crash" crashes on purpose, and three crashes within five minutes pause
// the extension.
import type { Command, CustomView, Item, List, QueryCommand } from "@pane/extension";

/** Runs the action of the item `itemId`. */
async function act(itemId: string): Promise<string> {
  if (itemId === "alias" || itemId === "fallback") {
    return "Echo answers the text you send it from root search";
  }
  throw new Error(`unknown item: ${itemId}`);
}

/** An item whose action is `act` with its id. */
const item = (id: string, title: string, subtitle: string): Item => ({
  id,
  title,
  subtitle,
  onAction: () => act(id),
});

async function render(): Promise<List> {
  return {
    title: "Echo: send it text from root search",
    items: [
      item(
        "alias",
        "Give Echo an alias in Manage extensions",
        "Then type the alias, a space and your text in root search",
      ),
      item(
        "fallback",
        "Or make Echo a fallback in Manage extensions",
        "Then type anything in root search and choose Echo below the results",
      ),
    ],
  };
}

async function submitForm(_itemId: string): Promise<string> {
  throw new Error("Echo has no forms");
}

async function openView(_itemId: string): Promise<CustomView> {
  throw new Error("Echo has no custom views");
}

export const command: Command = { render, submitForm, openView };

export const queryCommand: QueryCommand = {
  async runQuery(command, query) {
    if (command !== "echo") throw new Error(`unknown command: ${command}`);
    if (query === "fail") throw new Error("Echo refuses “fail”, to show how an error looks");
    // Resolving with something other than a string is a crash, unlike
    // throwing, which is an error the extension answers with.
    if (query === "crash") return undefined as unknown as string;
    return `Echo heard “${query}”`;
  },
};
