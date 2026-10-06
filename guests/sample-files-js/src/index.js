// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Pane's files sample in JavaScript: the same contract as the Files default
// extension (guests/files, Rust) and the TypeScript sample. The user grants
// the package a folder with Pane's own "Choose folder…" row (its pane.json
// sets `"folderAccess": true`); Pane lists it with `pane:extension/files`
// (imported because package.json sets `"pane": { "files": true }`), and the
// command answers root search with an `open-file` result, by the file's id,
// for each file whose name contains every word typed. Pane shows the file's
// own name and opens it once it has checked it again.
// @ts-check
import { limits, listFolder } from "pane:extension/files@0.1.0";

/** The most files one query lists. */
const MAX_RESULTS = 20;

/**
 * The last name of `path`, with `/` between names.
 * @param {string} path
 */
const lastName = (path) => path.split("/").pop() ?? path;

/**
 * Runs the action of the item `itemId`: none does anything.
 * @param {string} itemId
 * @returns {Promise<string>}
 */
async function act(itemId) {
  throw new Error(`unknown item: ${itemId}`);
}

/** @type {import("@pane/extension").Command} */
export const command = {
  async render() {
    const { depth, files } = limits();
    return {
      title: "JavaScript files sample",
      items: [
        {
          id: "policy",
          title: "What is searched",
          subtitle: `Files of the granted folder, ${depth} folders deep, at most ${files} (JavaScript)`,
          onAction: () => act("policy"),
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

/** @type {import("@pane/extension").RootResults} */
export const rootResults = {
  async resultsFor(query) {
    let state;
    try {
      state = listFolder();
    } catch (error) {
      throw new Error(String(/** @type {any} */ (error).payload));
    }
    if (state.tag !== "ready") return [];
    const words = query.toLowerCase().split(/\s+/).filter(Boolean);
    return state.val.files
      .filter((file) => {
        const name = lastName(file.relative).toLowerCase();
        return words.every((word) => name.includes(word));
      })
      .slice(0, MAX_RESULTS)
      .map((file) => ({
        id: file.relative,
        title: lastName(file.relative),
        action: { tag: "open-file", val: file.id },
      }));
  },
};
