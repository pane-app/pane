// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Pane's files sample in TypeScript: the same contract as the Files default
// extension (guests/files, Rust, "Search Files") and the JavaScript sample.
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
import { type FolderState, type FoundFile, limits, listFolder } from "pane:extension/files@0.1.0";
import type {
  Command,
  CommandSearch,
  CustomView,
  List,
  RootResult,
  RootResults,
  SearchResult,
} from "@pane/extension";

/** The most files one query lists. */
const MAX_RESULTS = 20;

/** The last name of `path`, with `/` between names. */
const lastName = (path: string): string => path.split("/").pop() ?? path;

/** Runs the action of the item `itemId`: none does anything. */
async function act(itemId: string): Promise<void> {
  throw new Error(`unknown item: ${itemId}`);
}

/**
 * The files of the granted folder whose name contains every word of
 * `query`; none while no folder is granted or Pane is still listing it
 * (Pane asks again once it is done).
 */
function found(query: string): FoundFile[] {
  let state: FolderState;
  try {
    state = listFolder();
  } catch (error) {
    throw new Error(String((error as { payload: unknown }).payload));
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

async function render(): Promise<List> {
  const { depth, files } = limits();
  return {
    title: "TypeScript files sample",
    items: [
      {
        id: "policy",
        title: "What is searched",
        subtitle: `Files of the granted folder, ${depth} folders deep, at most ${files} (TypeScript)`,
        onAction: () => act("policy"),
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
    return found(query).map((file) => ({
      id: file.relative,
      title: lastName(file.relative),
      file: file.id,
    }));
  },
};

export const rootResults: RootResults = {
  async resultsFor(query: string): Promise<RootResult[]> {
    return found(query).map((file) => ({
      id: file.relative,
      title: lastName(file.relative),
      action: { tag: "open-file", val: file.id },
    }));
  },
};
