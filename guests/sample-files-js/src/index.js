// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Pane's files sample in JavaScript: the same contract as the Files default
// extension (guests/files, Rust, "Search Files") and the Rust and
// TypeScript samples. Its pane.json sets `"fileIndex": true`, so Pane keeps
// its file index of the home folder current while the sample is enabled;
// the sample searches it with `pane:extension/file-index` (imported because
// package.json sets `"pane": { "fileIndex": true }`). Once open, the command
// owns the launcher's search field (`"search": true`) and answers each text
// typed with the entries the index finds, named by the ids Pane gave them;
// root search gets the same entries as `open-file` results, and a query
// that is a path ending in a separator lists the folder it names (#204):
// Pane's host lists the folder's entries for the sample
// (`pane:extension/typed-folder`, imported because package.json sets
// `"pane": { "typedFolder": true }`, at most 500, folders first and each
// in name order), and it answers them as it does the index's, by the ids
// Pane gave them. Pane shows each entry's own name and folder and gives it
// its file actions (Open, Show in Explorer, Open With…, Copy Path, Copy
// File, Move to Recycle Bin; for a program, Enter shows it and only Run
// runs it), which it performs itself.
// @ts-check
import { search, status } from "pane:extension/file-index@0.1.0";
import { listEntries } from "pane:extension/typed-folder@0.1.0";

/** The most entries one query lists. */
const MAX_RESULTS = 20;

/**
 * Runs the action of the item `itemId`: none does anything.
 * @param {string} itemId
 * @returns {Promise<void>}
 */
async function act(itemId) {
  throw new Error(`unknown item: ${itemId}`);
}

/**
 * The entries of the folder the path typed into root search names, when
 * `query` is a path ending in a separator (#204): `null` when it is not
 * one, or the folder cannot be listed — a missing folder lists nothing.
 * @param {string} query
 * @returns {import("pane:extension/typed-folder@0.1.0").FolderEntry[] | null}
 */
function typed(query) {
  const path = query.trim();
  if (!path.endsWith("/") && !path.endsWith("\\")) return null;
  try {
    return listEntries(path).entries;
  } catch {
    return null;
  }
}

/**
 * The entries the index finds for `query`, best first; none for a blank
 * query.
 * @param {string} query
 */
function found(query) {
  if (!query.trim()) return [];
  try {
    return search(query, { sort: "relevance", limit: MAX_RESULTS, offset: 0 });
  } catch (error) {
    throw new Error(String(/** @type {any} */ (error).payload));
  }
}

/** @type {import("@pane-app/extension").Command} */
export const command = {
  async render() {
    const { state, entries } = status();
    return {
      title: "JavaScript files sample",
      items: [
        {
          id: "status",
          title: "What is searched",
          subtitle: `Pane's file index, ${state}: ${entries} entries (JavaScript)`,
          onAction: () => act("status"),
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

/** @type {import("@pane-app/extension").CommandSearch} */
export const commandSearch = {
  async search(_command, query) {
    return found(query).map((entry) => ({
      id: entry.path,
      title: entry.name,
      file: entry.id,
    }));
  },
};

/** @type {import("@pane-app/extension").RootResults} */
export const rootResults = {
  async resultsFor(query) {
    // A path ending in a separator lists the folder it names (#204).
    const entries = typed(query);
    if (entries) {
      return entries.map((entry) => ({
        // Pane shows the entry's own name and folder, whatever these say.
        id: entry.id,
        title: entry.name,
        action: { tag: "open-file", val: entry.id },
      }));
    }
    return found(query).map((entry) => ({
      id: entry.path,
      title: entry.name,
      action: { tag: "open-file", val: entry.id },
    }));
  },
};
