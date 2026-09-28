// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Pane's sample command in TypeScript. Items, titles and results match the
// Rust sample (guests/sample-rust) and the JavaScript sample.
import type { Command, View } from "@pane/extension";
import { waitFor } from "wasi:clocks/monotonic-clock@0.3.0";
import * as z from "zod/mini";

// Module top-level code runs once, when the component is built, and its state
// is part of the snapshot every instance starts from. Pure setup such as this
// schema belongs here; secrets, IDs, timestamps and random values do not.
const PORT_RANGE = "port must be between 1 and 65535";
const Settings = z.object({
  name: z.string().check(z.minLength(1, "name must not be empty")),
  port: z.int().check(z.minimum(1, PORT_RANGE), z.maximum(65535, PORT_RANGE)),
});

async function getView(): Promise<View> {
  return {
    title: "TypeScript sample",
    items: [
      { id: "greet", title: "Say hello", subtitle: "Answer from the TypeScript guest" },
      { id: "wait", title: "Wait briefly", subtitle: "Await a WASI 0.3 clock, then answer" },
      { id: "validate", title: "Validate settings", subtitle: "Reject settings with an out-of-range port" },
      { id: "random", title: "Roll a number", subtitle: "A random number from this instance" },
    ],
  };
}

async function runAction(itemId: string): Promise<string> {
  switch (itemId) {
    case "greet":
      return "Hello from the TypeScript guest";
    case "wait":
      // A native component-model async import; the command suspends here.
      await waitFor(50_000_000);
      return "Waited 50 ms inside the TypeScript guest";
    case "validate": {
      const parsed = z.safeParse(Settings, { name: "Pane", port: 70000 });
      if (!parsed.success) {
        // The thrown error's message is shown to the user as the command's error.
        throw new Error(`Invalid settings: ${parsed.error.issues[0].message}`);
      }
      return `Settings are valid: ${parsed.data.name} on port ${parsed.data.port}`;
    }
    case "random":
      return String(Math.random());
    default:
      throw new Error(`unknown item: ${itemId}`);
  }
}

export const command: Command = { getView, runAction };
