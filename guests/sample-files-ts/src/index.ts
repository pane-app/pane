// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Pane's files sample in TypeScript: the same contract as the Files default
// extension (guests/files, Rust, "Search Files") and the Rust and
// JavaScript samples. Its pane.json sets `"fileIndex": true`, so Pane keeps
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
import { type FileEntry, search, status } from "pane:extension/file-index@0.1.0";
import { type FolderEntry, list } from "pane:extension/typed-folder@0.1.0";
import type {
  Command,
  CommandSearch,
  CustomView,
  List,
  RootResult,
  RootResults,
  SearchResult,
} from "@pane-app/extension";

/** The most entries one query lists. */
const MAX_RESULTS = 20;

/** Runs the action of the item `itemId`: none does anything. */
async function act(itemId: string): Promise<void> {
  throw new Error(`unknown item: ${itemId}`);
}

/**
 * The entries of the folder the path typed into root search names, when
 * `query` is a path ending in a separator (#204): `null` when it is not
 * one, or the folder cannot be listed — a missing folder lists nothing.
 */
function typed(query: string): FolderEntry[] | null {
  const path = query.trim();
  if (!path.endsWith("/") && !path.endsWith("\\")) return null;
  try {
    return list(path).entries;
  } catch {
    return null;
  }
}

/** The entries the index finds for `query`, best first; none for a blank query. */
function found(query: string): FileEntry[] {
  if (!query.trim()) return [];
  try {
    return search(query, { sort: "relevance", limit: MAX_RESULTS, offset: 0 });
  } catch (error) {
    throw new Error(String((error as { payload: unknown }).payload));
  }
}

async function render(): Promise<List> {
  const { state, entries } = status();
  return {
    title: "TypeScript files sample",
    items: [
      {
        id: "status",
        title: "What is searched",
        subtitle: `Pane's file index, ${state}: ${entries} entries (TypeScript)`,
        onAction: () => act("status"),
      },
    ],
  };
}

async function submitForm(itemId: string): Promise<string> {
  throw { message: `unknown form: ${itemId}` };
}

async function openView(itemId: string): Promise<CustomView> {
  throw new Error(`unknown view: ${itemId}`);
}

export const command: Command = { render, submitForm, openView };

export const commandSearch: CommandSearch = {
  async search(_command: string, query: string): Promise<SearchResult[]> {
    return found(query).map((entry) => ({
      id: entry.path,
      title: entry.name,
      file: entry.id,
    }));
  },
};

export const rootResults: RootResults = {
  async resultsFor(query: string): Promise<RootResult[]> {
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
