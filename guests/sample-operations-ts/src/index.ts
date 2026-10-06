// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Pane's operations sample in TypeScript. Its package publishes the operation
// `greet` (under `operations` in its pane.json), served by
// `publishedOperations` (built with it through package.json's `"pane"`), and
// its command calls the `greet` operation of another package with
// `pane:extension/operations`: a form asks for that package's source (its
// identity, as Pane shows it: `local:` and the folder it was installed
// from), a name, and whether to ask once or twice at once. Items, titles,
// results and errors match the Rust sample (guests/sample-operations) and
// the JavaScript one.
//
// `greet` version 1 takes `{"name": "<name>"}` and answers
// `{"greeting": "Hello, <name>, from TypeScript"}`, or the error "a name is
// needed".
//
// `wait` version 1 shows a call Pane stops: it saves `waiting` as "started"
// in its settings, waits ten seconds, saves "finished" and answers
// `{"waited": true}`. Disabling or reloading either package meanwhile stops
// it; the "wait" item calls it.
import type {
  Command,
  CustomView,
  FieldValue,
  Form,
  FormError,
  List,
  PublishedOperations,
} from "@pane/extension";
import { call, type CallError } from "pane:extension/operations@0.1.0";
import { set } from "pane:extension/settings@0.1.0";
import { waitFor } from "wasi:clocks/monotonic-clock@0.3.0";

/** `greet`'s input and result, version 1. */
interface GreetInput {
  name: string;
}
interface GreetResult {
  greeting: string;
}

/**
 * Calls `greet` version 1 of the package with `source` for `name`, and
 * returns its greeting. A failed call throws "<kind>: <message>".
 */
async function greet(source: string, name: string): Promise<string> {
  let result: string;
  try {
    const input: GreetInput = { name };
    result = await call(source, "greet", 1, JSON.stringify(input));
  } catch (error) {
    const { kind, message } = (error as { payload: CallError }).payload;
    throw new Error(`${kind}: ${message}`);
  }
  const { greeting } = JSON.parse(result) as Partial<GreetResult>;
  if (typeof greeting !== "string") {
    throw new Error("the answer has no greeting");
  }
  return greeting;
}

const greetForm: Form = {
  title: "Greet through another extension",
  fields: [
    {
      id: "source",
      label: "Package source",
      kind: { tag: "text", val: { placeholder: "local:/path/to/sample-operations" } },
    },
    { id: "name", label: "Name", kind: { tag: "text", val: { placeholder: "TypeScript" } } },
    {
      id: "times",
      label: "Ask",
      kind: {
        tag: "choice",
        val: [
          { id: "once", label: "Once" },
          { id: "twice", label: "Twice at once" },
        ],
      },
    },
  ],
  submitLabel: "Greet",
};

const waitForm: Form = {
  title: "Wait in another extension",
  fields: [
    {
      id: "source",
      label: "Package source",
      kind: { tag: "text", val: { placeholder: "local:/path/to/sample-operations" } },
    },
  ],
  submitLabel: "Wait",
};

async function render(): Promise<List> {
  return {
    title: "Call from TypeScript",
    items: [
      {
        id: "greet",
        title: "Greet through another extension",
        subtitle: "Calls its greet operation through Pane",
        form: greetForm,
      },
      {
        id: "wait",
        title: "Wait in another extension",
        subtitle: "Calls its wait operation, which takes ten seconds",
        form: waitForm,
      },
    ],
  };
}

async function submitForm(itemId: string, values: FieldValue[]): Promise<string> {
  if (itemId !== "greet" && itemId !== "wait") {
    throw { message: `unknown form: ${itemId}` } satisfies FormError;
  }
  const value = (id: string) => values.find((field) => field.id === id)?.value ?? "";
  const source = value("source").trim();
  if (source === "") {
    throw { field: "source", message: "Enter the package's source" } satisfies FormError;
  }
  if (itemId === "wait") {
    try {
      await call(source, "wait", 1, "{}");
    } catch (error) {
      const { kind, message } = (error as { payload: CallError }).payload;
      throw { message: `${kind}: ${message}` } satisfies FormError;
    }
    return "Waited in the other extension";
  }
  const name = value("name");
  try {
    if (value("times") === "twice") {
      // Both calls are made at once; Pane serves them one after another.
      const [first, second] = await Promise.all([greet(source, name), greet(source, name)]);
      return `${first} / ${second}`;
    }
    return await greet(source, name);
  } catch (error) {
    throw { message: (error as Error).message } satisfies FormError;
  }
}

async function openView(itemId: string): Promise<CustomView> {
  throw new Error(`unknown view: ${itemId}`);
}

async function runOperation(operation: string, input: string): Promise<string> {
  if (operation === "wait") {
    set("waiting", "started");
    // If Pane stops the call meanwhile, nothing after this runs.
    await waitFor(10_000_000_000);
    set("waiting", "finished");
    return JSON.stringify({ waited: true });
  }
  if (operation !== "greet") {
    throw new Error(`unknown operation: ${operation}`);
  }
  const { name } = JSON.parse(input) as Partial<GreetInput>;
  if (typeof name !== "string" || name === "") {
    throw new Error("a name is needed");
  }
  const result: GreetResult = { greeting: `Hello, ${name}, from TypeScript` };
  return JSON.stringify(result);
}

export const command: Command = { render, submitForm, openView };

export const publishedOperations: PublishedOperations = { runOperation };
