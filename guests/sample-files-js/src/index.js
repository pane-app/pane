// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Pane's files sample in JavaScript: the same contract as the Files default
// extension (guests/files, Rust, "Search Files") and the TypeScript sample.
// The user grants the package a folder with Pane's own "Choose folder…" row
// (its pane.json sets `"folderAccess": true`); Pane lists it with
// `pane:extension/files` (imported because package.json sets
// `"pane": { "files": true }`). Once open, the command owns the launcher's
// search field (`"search": true`) and answers each text typed with the
// files whose name contains every word, named by the ids Pane gave them;
// root search gets the same files as `open-file` results. Pane shows each
// file's own name and gives it its file actions (Open, Reveal in Explorer,
// Open With…, Copy Path, Copy File, Move to Recycle Bin; for a program,
// Enter reveals it and only Run runs it), which it performs itself.
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
 * @returns {Promise<void>}
 */
async function act(itemId) {
  throw new Error(`unknown item: ${itemId}`);
}

/**
 * The files of the granted folder whose name contains every word of
 * `query`; none while no folder is granted or Pane is still listing it
 * (Pane asks again once it is done).
 * @param {string} query
 */
function found(query) {
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
    .slice(0, MAX_RESULTS);
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

/** @type {import("@pane/extension").CommandSearch} */
export const commandSearch = {
  async search(_command, query) {
    return found(query).map((file) => ({
      id: file.relative,
      title: lastName(file.relative),
      file: file.id,
    }));
  },
};

/** @type {import("@pane/extension").RootResults} */
export const rootResults = {
  async resultsFor(query) {
    return found(query).map((file) => ({
      id: file.relative,
      title: lastName(file.relative),
      action: { tag: "open-file", val: file.id },
    }));
  },
};
