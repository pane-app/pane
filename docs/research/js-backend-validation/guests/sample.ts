// Pane's sample command in TypeScript: the same contract and behaviour as
// guests/sample-rust, against wit/extension.wit (pane:extension/command).
import { waitFor } from "wasi:clocks/monotonic-clock@0.3.0";

interface Item {
  id: string;
  title: string;
  subtitle: string | null;
}

interface View {
  title: string;
  items: Item[];
}

// WIT `result<T, string>` at the top level: return T, or throw the string.
async function getView(): Promise<View> {
  return {
    title: "TypeScript sample",
    items: [
      { id: "greet", title: "Say hello", subtitle: "Answer from the TypeScript guest" },
      { id: "wait", title: "Wait briefly", subtitle: "Await a WASI 0.3 clock, then answer" },
      { id: "random", title: "Roll a number", subtitle: "Math.random from a fresh instance" },
    ],
  };
}

async function runAction(itemId: string): Promise<string> {
  switch (itemId) {
    case "greet":
      return "Hello from the TypeScript guest";
    case "wait":
      // A native component-model async import; the guest suspends here.
      await waitFor(50_000_000);
      return "Waited 50 ms inside the TypeScript guest";
    case "random":
      return String(Math.random());
    default:
      throw `unknown item: ${itemId}`;
  }
}

export const command = { getView, runAction };
