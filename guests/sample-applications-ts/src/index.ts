// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Pane's applications sample in TypeScript: it finds the installed
// applications through `pane:extension/applications`, which Pane's host
// provides since a WASI guest cannot see or start them, supplies "Launch
// <name>" for each to root search as indexed results (`indexedResults`,
// built with it through package.json's `"pane"`), and its command lists
// them and opens one. Titles, results and errors match the JavaScript
// sample (guests/sample-applications-js).
import type {
  Command,
  CustomView,
  FieldValue,
  FormError,
  IndexedResult,
  IndexedResults,
  List,
} from "@pane/extension";
import { installed, open, type Application } from "pane:extension/applications@0.1.0";

const SAMPLE = "TypeScript applications sample";

/** Calls a host function, turning its error into a plain message. */
function host<T>(call: () => T): T {
  try {
    return call();
  } catch (error) {
    const payload = (error as { payload?: unknown }).payload;
    throw typeof payload === "string" ? new Error(payload) : error;
  }
}

/** The installed applications, by name. */
function applications(): Application[] {
  return host(installed).sort((a, b) =>
    a.name.toLowerCase().localeCompare(b.name.toLowerCase()),
  );
}

/** Opens the application with id `itemId`, the action of its item. */
async function act(itemId: string): Promise<string> {
  host(() => open(itemId));
  const name = applications().find((app) => app.id === itemId)?.name ?? itemId;
  return `Opened ${name}`;
}

async function render(): Promise<List> {
  return {
    title: SAMPLE,
    items: applications().map((app) => ({
      id: app.id,
      title: app.name,
      subtitle: app.location,
      onAction: () => act(app.id),
    })),
  };
}

async function submitForm(_itemId: string, _values: FieldValue[]): Promise<string> {
  throw { message: "this sample has no forms" } satisfies FormError;
}

async function openView(_itemId: string): Promise<CustomView> {
  throw new Error("this sample has no custom views");
}

export const command: Command = { render, submitForm, openView };

export const indexedResults: IndexedResults = {
  async results(): Promise<IndexedResult[]> {
    return applications().map((app) => ({
      id: app.id,
      title: `Launch ${app.name}`,
      subtitle: SAMPLE,
      action: { tag: "open-application", val: app.id },
    }));
  },
};
