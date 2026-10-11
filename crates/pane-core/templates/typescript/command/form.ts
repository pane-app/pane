// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// The __NAME__ command: an item that opens a form the command answers, as
// the `form` template's own command does. Added to the package by
// `pane-ext new command`; the entry file (`src/index.ts`) calls its
// `render` and `submit`.
import type { FieldValue, Form, LaunchRecord, List } from "@pane-app/extension";

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

/** The command's list: the form's item. Choosing it opens the form;
 *  submitting it calls `submit` with the item's id. */
export async function render(_launch: LaunchRecord): Promise<List> {
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

/** The form's answer: the chosen greeting and the name, or the problem
 *  with a field, shown next to it. */
export function submit(itemId: string, values: FieldValue[]): string {
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
