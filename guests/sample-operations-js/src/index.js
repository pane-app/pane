// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Pane's operations sample in JavaScript. Its package publishes the operation
// `greet` (under `operations` in its pane.json), served by
// `publishedOperations` (built with it through package.json's `"pane"`), and
// its commands call the `greet` operation of another package with
// `pane:extension/operations`: a form on the designed tree (#241) asks for
// that package's source (its identity, as Pane shows it: `local:` and the
// folder it was installed from), a name, and whether to ask once or twice
// at once. Items, titles, results and errors match the Rust sample
// (guests/sample-operations) and the TypeScript one.
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
import { launch } from "pane:extension/commands@0.1.0";
import { set } from "pane:extension/settings@0.1.0";
import { jsxs } from "@pane-app/extension/jsx-runtime";
import { Column, Form, Text, createView, useState } from "@pane-app/extension/view";
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

/** @type {import("@pane-app/extension").Command} */
export const command = {
  async render() {
    return {
      title: "Call from JavaScript",
      items: [
        {
          id: "greet",
          title: "Greet through another extension",
          subtitle: "Calls its greet operation through Pane",
          onAction: () => openForm("greet"),
        },
        {
          id: "wait",
          title: "Wait in another extension",
          subtitle: "Calls its wait operation, which takes ten seconds",
          onAction: () => openForm("wait"),
        },
      ],
    };
  },

  async openView(commandId) {
    if (commandId !== "greet" && commandId !== "wait") {
      throw new Error("this command opens no designed view");
    }
    return createView(CallForm, { waiting: commandId === "wait" });
  },
};

/**
 * Opens the form command `which`, as the user would.
 * @param {string} which
 * @returns {Promise<void>}
 */
async function openForm(which) {
  launch({ command: which }, "user-initiated", [], null);
}

/**
 * The form command's view (#241): the fields the operation's arguments
 * ask for, whose submission calls it. The call runs as the listener
 * itself — an async listener is awaited — and its answer, or why there
 * is none, draws over the form.
 * @param {{ waiting?: boolean }} props
 * @returns {import("@pane-app/extension/view").Element}
 */
function CallForm({ waiting = false }) {
  const [answer, setAnswer] = useState("");
  const [error, setError] = useState(/** @type {string | null} */ (null));
  /** @type {Record<string, string>} */
  const values = { source: "", name: "", times: "once" };
  const submit = async (/** @type {import("@pane-app/extension/view").FormSubmittedValues} */ submitted) => {
    const source = String(submitted.source ?? "").trim();
    if (source === "") {
      setError("Enter the package's source");
      return;
    }
    setError(null);
    if (waiting) {
      try {
        await call(source, "wait", 1, "{}");
        setAnswer("Waited in the other extension");
      } catch (/** @type {any} */ problem) {
        const { kind, message } = problem.payload ?? {};
        setAnswer(kind && message ? `${kind}: ${message}` : String(problem));
      }
      return;
    }
    const name = String(submitted.name ?? "");
    const times = submitted.times === "twice" ? 2 : 1;
    try {
      const result = [];
      for (let at = 0; at < times; at += 1) {
        result.push(await call(source, "greet", 1, JSON.stringify({ name })));
      }
      const greeting = JSON.parse(result[0]).greeting;
      setAnswer(times === 2 ? `${greeting} (twice at once)` : greeting);
    } catch (/** @type {any} */ problem) {
      const { kind, message } = problem.payload ?? {};
      setAnswer(kind && message ? `${kind}: ${message}` : String(problem));
    }
  };
  const form = jsxs(Form, {
    key: "form",
    submitTitle: "Call",
    onSubmit: submit,
    children: [
      jsxs(Form.TextField, {
        key: "source",
        id: "source",
        title: "Package source",
        placeholder: "local:/path/to/sample-operations",
        autoFocus: true,
        error: error ?? undefined,
      }),
      ...(!waiting
        ? [
            jsxs(Form.TextField, { key: "name", id: "name", title: "Name", placeholder: "JavaScript" }),
            jsxs(Form.Dropdown, {
              key: "times",
              id: "times",
              title: "Ask",
              options: [
                { value: "once", label: "Once" },
                { value: "twice", label: "Twice at once" },
              ],
              defaultValue: "once",
            }),
          ]
        : []),
    ],
  });
  if (answer === "") {
    return form;
  }
  return jsxs(Column, {
    gap: "m",
    children: [jsxs(Text, { style: "title", children: [answer] }), form],
  });
}

/** @type {import("@pane-app/extension").PublishedOperations} */
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
