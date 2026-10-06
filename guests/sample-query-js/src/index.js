// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Echo, the query sample, in JavaScript: a command that takes a query. It
// answers the text the user sends it from root search, through its alias
// ("ec hello" when the user gave it the alias "ec") or by choosing it as a
// fallback; Pane sends the text only when the user invokes it that way.
// Items, answers and errors match the Rust query sample (guests/sample-query)
// and the TypeScript one. "fail" is refused, to show how an error looks;
// "crash" crashes on purpose, and three crashes within five minutes pause
// the extension.

/**
 * Runs the action of the item `itemId`.
 * @param {string} itemId
 * @returns {Promise<string>}
 */
async function act(itemId) {
  if (itemId === "alias" || itemId === "fallback") {
    return "Echo answers the text you send it from root search";
  }
  throw new Error(`unknown item: ${itemId}`);
}

/**
 * An item whose action is `act` with its id.
 * @param {string} id
 * @param {string} title
 * @param {string} subtitle
 * @returns {import("@pane/extension").Item}
 */
const item = (id, title, subtitle) => ({ id, title, subtitle, onAction: () => act(id) });

/** @type {import("@pane/extension").Command} */
export const command = {
  async render() {
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
  },
  async submitForm() {
    throw new Error("Echo has no forms");
  },
  async openView() {
    throw new Error("Echo has no custom views");
  },
};

/** @type {import("@pane/extension").QueryCommand} */
export const queryCommand = {
  async runQuery(command, query) {
    if (command !== "echo") throw new Error(`unknown command: ${command}`);
    if (query === "fail") throw new Error("Echo refuses “fail”, to show how an error looks");
    if (query === "crash") {
      // Resolving with something other than a string is a crash, unlike
      // throwing, which is an error the extension answers with.
      return /** @type {string} */ (/** @type {unknown} */ (undefined));
    }
    return `Echo heard “${query}”`;
  },
};
