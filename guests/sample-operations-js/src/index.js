// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Pane's operations sample in JavaScript. Its package publishes the operation
// `greet` (under `operations` in its pane.json), served by
// `publishedOperations` (built with it through package.json's `"pane"`), and
// its command calls the `greet` operation of another package with
// `pane:extension/operations`: a form asks for that package's source (its
// identity, as Pane shows it: `local:` and the folder it was installed
// from), a name, and whether to ask once or twice at once. Items, titles,
// results and errors match the Rust sample (guests/sample-operations) and
// the TypeScript one.
//
// `greet` version 1 takes `{"name": "<name>"}` and answers
// `{"greeting": "Hello, <name>, from JavaScript"}`, or the error "a name is
// needed".
//
// `wait` version 1 shows a call Pane stops: it saves `waiting` as "started"
// in its settings, waits ten seconds, saves "finished" and answers
// `{"waited": true}`. Disabling or reloading either package meanwhile stops
// it; the "wait" item calls it.
// @ts-check
import { call } from "pane:extension/operations@0.1.0";
import { set } from "pane:extension/settings@0.1.0";
import { waitFor } from "wasi:clocks/monotonic-clock@0.3.0";

/**
 * Calls `greet` version 1 of the package with `source` for `name`, and
 * returns its greeting. A failed call throws "<kind>: <message>".
 * @param {string} source
 * @param {string} name
 * @returns {Promise<string>}
 */
async function greet(source, name) {
  let result;
  try {
    result = await call(source, "greet", 1, JSON.stringify({ name }));
  } catch (error) {
    /** @type {import("pane:extension/operations@0.1.0").CallError} */
    const { kind, message } = /** @type {any} */ (error).payload;
    throw new Error(`${kind}: ${message}`);
  }
  const { greeting } = JSON.parse(result);
  if (typeof greeting !== "string") {
    throw new Error("the answer has no greeting");
  }
  return greeting;
}

/** @type {import("@pane/extension").Form} */
const greetForm = {
  title: "Greet through another extension",
  fields: [
    {
      id: "source",
      label: "Package source",
      kind: { tag: "text", val: { placeholder: "local:/path/to/sample-operations" } },
    },
    { id: "name", label: "Name", kind: { tag: "text", val: { placeholder: "JavaScript" } } },
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

/** @type {import("@pane/extension").Form} */
const waitForm = {
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

/** @type {import("@pane/extension").Command} */
export const command = {
  async render() {
    return {
      title: "Call from JavaScript",
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
  },

  async submitForm(itemId, values) {
    if (itemId !== "greet" && itemId !== "wait") {
      throw { message: `unknown form: ${itemId}` };
    }
    /** @param {string} id */
    const value = (id) => values.find((field) => field.id === id)?.value ?? "";
    const source = value("source").trim();
    if (source === "") {
      throw { field: "source", message: "Enter the package's source" };
    }
    if (itemId === "wait") {
      try {
        await call(source, "wait", 1, "{}");
      } catch (error) {
        const { kind, message } = /** @type {any} */ (error).payload;
        throw { message: `${kind}: ${message}` };
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
      throw { message: /** @type {Error} */ (error).message };
    }
  },

  async openView(itemId) {
    throw new Error(`unknown view: ${itemId}`);
  },
};

/** @type {import("@pane/extension").PublishedOperations} */
export const publishedOperations = {
  async runOperation(operation, input) {
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
    const { name } = JSON.parse(input);
    if (typeof name !== "string" || name === "") {
      throw new Error("a name is needed");
    }
    return JSON.stringify({ greeting: `Hello, ${name}, from JavaScript` });
  },
};
