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
  DesignedView,
  LaunchRecord,
  List,
  PublishedOperations,
} from "@pane-app/extension";
import { launch } from "pane:extension/commands@0.1.0";
import { call, type CallError } from "pane:extension/operations@0.1.0";
import { set } from "pane:extension/settings@0.1.0";
import { Column, Form, Text, createView, useState } from "@pane-app/extension/view";
import type { Element, FormSubmittedValues } from "@pane-app/extension/view";
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

async function render(): Promise<List> {
  return {
    title: "Call from TypeScript",
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
}

async function openView(
  commandId: string,
  _launch: LaunchRecord,
): Promise<DesignedView> {
  if (commandId !== "greet" && commandId !== "wait") {
    throw new Error("this command opens no designed view");
  }
  return createView(CallForm, { waiting: commandId === "wait" });
}

/** Opens the form command `which`, as the user would. */
async function openForm(which: string): Promise<void> {
  launch({ command: which }, "user-initiated", [], null);
}

/**
 * The form command's view (#241): the fields the operation's arguments
 * ask for, whose submission calls it. The call runs as the listener
 * itself — an async listener is awaited — and its answer, or why there
 * is none, draws over the form.
 */
function CallForm({ waiting = false }: { waiting?: boolean }): Element {
  const [answer, setAnswer] = useState("");
  const [error, setError] = useState<string | null>(null);
  const submit = async (submitted: FormSubmittedValues) => {
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
      } catch (problem) {
        const payload = (problem as { payload?: { kind: string; message: string } }).payload;
        setAnswer(payload ? `${payload.kind}: ${payload.message}` : String(problem));
      }
      return;
    }
    const name = String(submitted.name ?? "");
    const times = submitted.times === "twice" ? 2 : 1;
    try {
      const result = await call(source, "greet", 1, JSON.stringify({ name }));
      const greeting = JSON.parse(result).greeting as string;
      setAnswer(times === 2 ? `${greeting} (twice at once)` : greeting);
    } catch (problem) {
      const payload = (problem as { payload?: { kind: string; message: string } }).payload;
      setAnswer(payload ? `${payload.kind}: ${payload.message}` : String(problem));
    }
  };
  const form = (
    <Form key="form" submitTitle="Call" onSubmit={submit}>
      <Form.TextField
        key="source"
        id="source"
        title="Package source"
        placeholder="local:/path/to/sample-operations"
        autoFocus
        error={error ?? undefined}
      />
      {!waiting && (
        <Form.TextField key="name" id="name" title="Name" placeholder="TypeScript" />
      )}
      {!waiting && (
        <Form.Dropdown
          key="times"
          id="times"
          title="Ask"
          options={[
            { value: "once", label: "Once" },
            { value: "twice", label: "Twice at once" },
          ]}
          defaultValue="once"
        />
      )}
    </Form>
  );
  if (answer === "") {
    return form;
  }
  return (
    <Column gap="m">
      <Text style="title">{answer}</Text>
      {form}
    </Column>
  );
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

export const command: Command = { render, openView };

export const publishedOperations: PublishedOperations = { runOperation };
