// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Pane's files sample in TypeScript: the same contract as the Files default
// extension (guests/files, Rust) and the JavaScript sample. The user grants
// the package a folder with Pane's own "Choose folder…" row (its pane.json
// sets `"folderAccess": true`); Pane lists it with `pane:extension/files`
// (imported because package.json sets `"pane": { "files": true }`), and the
// command answers root search with an `open-file` result, by the file's id,
// for each file whose name contains every word typed. Pane shows the file's
// own name and opens it once it has checked it again.
import { type FolderState, limits, listFolder } from "pane:extension/files@0.1.0";
import type { Command, CustomView, List, RootResult, RootResults } from "@pane/extension";

/** The most files one query lists. */
const MAX_RESULTS = 20;

/** The last name of `path`, with `/` between names. */
const lastName = (path: string): string => path.split("/").pop() ?? path;

/** Runs the action of the item `itemId`: none does anything. */
async function act(itemId: string): Promise<string> {
  throw new Error(`unknown item: ${itemId}`);
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

export const rootResults: RootResults = {
  async resultsFor(query: string): Promise<RootResult[]> {
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
      .slice(0, MAX_RESULTS)
      .map((file) => ({
        id: file.relative,
        title: lastName(file.relative),
        action: { tag: "open-file", val: file.id },
      }));
  },
};
