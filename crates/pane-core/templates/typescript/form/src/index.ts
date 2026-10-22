// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// __TITLE__, started from pane-ext's `form` template: a command whose
// screen is a form — a name to type and a greeting to choose, with a
// `Greet` button — which the command checks and answers as its view
// renders it again, the problem with a field shown next to it (#241,
// the designed tree's form). `npm run dev` (after `npm install`) builds
// it and hands each build to the running Pane, which reloads it on each
// save.
//
// One component (the WebAssembly file pane.json names) serves every
// command the package declares: render and run receive the command's id,
// so this file matches on it. `pane-ext new command .` adds one.
import type { Command, LaunchRecord, List } from "@pane-app/extension";
import { Column, Form, Text, createView, useState } from "@pane-app/extension/view";
import { jsxs } from "@pane-app/extension/jsx-runtime";

// pane-ext new command adds a command's file here.

/** The greeting form's choices. */
const GREETINGS = [
  { value: "hello", label: "Hello" },
  { value: "morning", label: "Good morning" },
  { value: "welcome", label: "Welcome" },
];

/** The form the __NAME__ command opens (#241): a name to type and a
 *  greeting to choose, with a `Greet` button. The submission is checked:
 *  a problem with the name draws as the field's error, the answer above
 *  the form, and the view renders again. */
function Greeting() {
  const [error, setError] = useState("");
  const [answered, setAnswered] = useState("");
  const submit = (values: {
    [id: string]: string | boolean | string[] | undefined;
  }) => {
    const name = String(values["name"] ?? "").trim();
    if (name === "") {
      setError("Enter a name");
      return;
    }
    if ([...name].length > 40) {
      setError("Use at most 40 characters");
      return;
    }
    setError("");
    const greeting = GREETINGS.find((choice) => choice.value === values["greeting"])?.label;
    setAnswered(`${greeting ?? "Hello"}, ${name}`);
  };
  const form = jsxs(Form, {
    key: "form",
    submitTitle: "Greet",
    onSubmit: submit,
    children: [
      jsxs(Form.TextField, {
        key: "name",
        id: "name",
        title: "Name",
        placeholder: "Ada Lovelace",
        autoFocus: true,
        error: error === "" ? undefined : error,
      }),
      jsxs(Form.Dropdown, {
        key: "greeting",
        id: "greeting",
        title: "Greeting",
        options: GREETINGS,
        defaultValue: "hello",
      }),
    ],
  });
  if (answered === "") {
    return form;
  }
  return jsxs(Column, {
    gap: "m",
    children: [
      jsxs(Text, { style: "title", children: [answered] }),
      form,
    ],
  });
}

export const command: Command = {
  // The form the command opens: a designed view (its pane.json says
  // "mode": "designed").
  async openView(commandId: string) {
    switch (commandId) {
      case "__NAME__":
        return createView(Greeting);
      // pane-ext new command adds a view command's arm here.
      default:
        throw new Error(`unknown command: ${commandId}`);
    }
  },

  async render(launch: LaunchRecord): Promise<List> {
    switch (launch.command) {
      // pane-ext new command adds a view command's arm here.
      default:
        throw new Error(`\`${launch.command}\` opens a designed view; it has no list`);
    }
  },

  async run(command: string, _launch: LaunchRecord): Promise<void> {
    switch (command) {
      // pane-ext new command adds a no-view command's arm here.
      default:
        throw new Error(`\`${command}\` opens a screen; it has no run entry point`);
    }
  },
};
