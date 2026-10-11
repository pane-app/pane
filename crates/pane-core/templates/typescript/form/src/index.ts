// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// __TITLE__, started from pane-ext's `form` template: a command whose item
// opens a form — a name to type and a greeting to choose — which
// `submitForm` answers, field problems included. `npm run dev` (after
// `npm install`) builds it and hands each build to the running Pane,
// which reloads it on each save.
//
// One component (the WebAssembly file pane.json names) serves every
// command the package declares: render and run receive the command's id,
// so this file matches on it. `pane-ext new command .` adds one.
import type { Command, FieldValue, Form, LaunchRecord, List } from "@pane-app/extension";

// pane-ext new command adds a command's file here.

/** The command whose screen is open, so `submitForm` reaches the command
 *  whose form was filled in. */
let opened = "__NAME__";

/** The greeting form's choices. */
const GREETINGS = [
  { id: "hello", label: "Hello" },
  { id: "morning", label: "Good morning" },
  { id: "welcome", label: "Welcome" },
];

/** The item's form: a name to type and a greeting to choose. */
const GREETING_FORM: Form = {
  title: "Greet someone",
  fields: [
    { id: "name", label: "Name", kind: { tag: "text", val: { placeholder: "Ada Lovelace" } } },
    { id: "greeting", label: "Greeting", kind: { tag: "choice", val: GREETINGS } },
  ],
  submitLabel: "Greet",
};

/** The __NAME__ command's list: the form's item. Choosing it opens the
 *  form; submitting it calls `submitForm` with the item's id. */
async function theList(): Promise<List> {
  return {
    title: "__TITLE__",
    items: [
      {
        id: "greet",
        title: "Greet someone",
        subtitle: "Fill in a form the command checks and answers",
        form: GREETING_FORM,
      },
    ],
  };
}

export const command: Command = {
  async render(launch: LaunchRecord): Promise<List> {
    opened = launch.command;
    switch (launch.command) {
      case "__NAME__":
        return theList();
      // pane-ext new command adds a view command's arm here.
      default:
        throw new Error(`unknown command: ${launch.command}`);
    }
  },

  async run(id: string, _launch: LaunchRecord): Promise<void> {
    switch (id) {
      // pane-ext new command adds a no-view command's arm here.
      default:
        throw new Error(`\`${id}\` opens a screen; it has no run entry point`);
    }
  },

  /** The form's answer: the chosen greeting and the name, or the problem
   *  with a field, shown next to it. */
  async submitForm(itemId: string, _values: FieldValue[]): Promise<string> {
    switch (opened) {
      case "__NAME__":
        return submitted(itemId, _values);
      // pane-ext new command adds a form command's arm here.
      default:
        throw { message: `\`${opened}\` has no forms: ${itemId}` };
    }
  },
};

/** Answers the form: the greeting and the name, or the problem with a
 *  field. */
function submitted(itemId: string, values: FieldValue[]): string {
  if (itemId !== "greet") {
    throw { message: `unknown form: ${itemId}` };
  }
  const value = (id: string): string => values.find((field) => field.id === id)?.value ?? "";
  const name = value("name").trim();
  if (name === "") {
    throw { field: "name", message: "Enter a name" };
  }
  if ([...name].length > 40) {
    throw { field: "name", message: "Use at most 40 characters" };
  }
  const greeting = GREETINGS.find((choice) => choice.id === value("greeting"))?.label;
  if (greeting === undefined) {
    throw { field: "greeting", message: "Choose a greeting" };
  }
  return `${greeting}, ${name}`;
}
