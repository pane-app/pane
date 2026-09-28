// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Pane's operations sample in TypeScript. Its package publishes the operation
// `greet` (under `operations` in its pane.json), served by
// `publishedOperations` (built with it through package.json's `"pane"`), and
// its command calls the `greet` operation the Rust operations sample
// publishes, with `pane:extension/operations`. Items, titles, results and
// errors match the Rust sample (guests/sample-operations) and the JavaScript
// one.
//
// `greet` version 1 takes `{"name": "<name>"}` and answers
// `{"greeting": "Hello, <name>, from TypeScript"}`, or the error "a name is
// needed".
import type { Command, CustomView, Item, PublishedOperations, View } from "@pane/extension";
import { call, type CallError } from "pane:extension/operations@0.1.0";

/**
 * The packages this command calls, by source: relative to this package's own
 * source folder, so the samples work side by side wherever they are.
 */
const RUST = "local:../sample-operations";
const MISSING = "local:../no-such-extension";

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

const item = (id: string, title: string): Item => ({ id, title });

async function getView(): Promise<View> {
  return {
    title: "Call from TypeScript",
    items: [
      item("ask-rust", "Ask Rust to greet"),
      item("ask-empty", "Ask Rust with no name"),
      item("ask-missing", "Ask an extension that is not installed"),
    ],
  };
}

async function runAction(itemId: string): Promise<string> {
  switch (itemId) {
    case "ask-rust":
      return `Rust answered: ${await greet(RUST, "TypeScript")}`;
    case "ask-empty":
      return `Rust answered: ${await greet(RUST, "")}`;
    case "ask-missing":
      return `Nobody answered: ${await greet(MISSING, "TypeScript")}`;
    default:
      throw new Error(`unknown item: ${itemId}`);
  }
}

async function submitForm(itemId: string): Promise<string> {
  throw { message: `unknown form: ${itemId}` };
}

async function openView(itemId: string): Promise<CustomView> {
  throw new Error(`unknown view: ${itemId}`);
}

async function runOperation(operation: string, input: string): Promise<string> {
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

export const command: Command = { getView, runAction, submitForm, openView };

export const publishedOperations: PublishedOperations = { runOperation };
