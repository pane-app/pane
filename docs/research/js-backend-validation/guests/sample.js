// Pane's sample command in plain JavaScript (no bundler needed): the same
// contract and behaviour as guests/sample-rust, against wit/extension.wit.
import { waitFor } from "wasi:clocks/monotonic-clock@0.3.0";

export const command = {
  // WIT `result<view, string>` at the top level: return the view, or throw.
  async getView() {
    return {
      title: "JavaScript sample",
      items: [
        { id: "greet", title: "Say hello", subtitle: "Answer from the JavaScript guest" },
        { id: "wait", title: "Wait briefly", subtitle: "Await a WASI 0.3 clock, then answer" },
        { id: "random", title: "Roll a number", subtitle: "Math.random from a fresh instance" },
      ],
    };
  },

  async runAction(itemId) {
    switch (itemId) {
      case "greet":
        return "Hello from the JavaScript guest";
      case "wait":
        // A native component-model async import; the guest suspends here.
        await waitFor(50_000_000);
        return "Waited 50 ms inside the JavaScript guest";
      case "random":
        return String(Math.random());
      default:
        throw `unknown item: ${itemId}`;
    }
  },
};
