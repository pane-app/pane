// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Pane's capabilities sample in JavaScript: a package that uses a
// capability by its name, under `uses` in its pane.json, instead of naming
// the package that serves it (ADR 0041). It uses `pane-samples:greet@1`,
// which the greet provider samples provide in Rust, JavaScript and
// TypeScript (guests/sample-greet*), each answering with its own language's
// name: whoever is installed serves the call, and the code names none of
// them. Its use of it says `"use": "all"`, so beside the call that reaches
// one provider, an item fans a call out to every provider, answering each
// one's greeting labelled with its title. It also uses
// `pane-samples:farewell@1` optionally, which no sample provides: a call
// to it answers `not-found`, and nothing is gated on it. Items, titles,
// results and errors match the Rust sample (guests/sample-capabilities)
// and the TypeScript one.
//
// Each capability's `greet`/`farewell` operation takes `{"name": "<name>"}`
// and answers `{"greeting": "..."}`. Before calling, "Who provides it"
// asks for the providers that can serve the capability now, with their
// titles.
// @ts-check
import { available, call, callEvery, providers } from "@pane-app/extension/capabilities";
import { showToast } from "@pane-app/extension/feedback";

/** The capabilities this package uses, as its pane.json declares them. */
const GREET = "pane-samples:greet@1";
const FAREWELL = "pane-samples:farewell@1";

/**
 * Calls `operation` of `capability` for "Pane", and returns what its
 * provider answers. A failed call throws "<kind>: <message>".
 * @param {string} capability
 * @param {string} operation
 * @returns {Promise<string>}
 */
async function callName(capability, operation) {
  let result;
  try {
    result = await call(capability, operation, JSON.stringify({ name: "Pane" }));
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

/**
 * What the action of the item `itemId` says: the greeting a capability
 * answers, every provider's greeting, or why there is none. A failed call
 * throws "<kind>: <message>".
 * @param {string} itemId
 * @returns {Promise<string>}
 */
async function outcome(itemId) {
  switch (itemId) {
    // A required use that no installed package provides is the user's to
    // fix; Pane's reason is the answer.
    case "greet":
      return callName(GREET, "greet");
    // A use of every provider: each provider's answer, labelled with its
    // title, and an empty list when none can serve.
    case "every": {
      let answers;
      try {
        answers = await callEvery(GREET, "greet", JSON.stringify({ name: "Pane" }));
      } catch (error) {
        /** @type {import("pane:extension/operations@0.1.0").CallError} */
        const { kind, message } = /** @type {any} */ (error).payload;
        throw new Error(`${kind}: ${message}`);
      }
      const said = answers.map((answer) => {
        if (answer.answer.tag === "ok") {
          const { greeting } = JSON.parse(answer.answer.val);
          if (typeof greeting !== "string") {
            throw new Error("the answer has no greeting");
          }
          return `${answer.title}: ${greeting}`;
        }
        return `${answer.title}: ${answer.answer.val.kind}: ${answer.answer.val.message}`;
      });
      return said.length === 0
        ? `No installed extension provides ${GREET}`
        : said.join("; ");
    }
    case "farewell":
      try {
        return await callName(FAREWELL, "farewell");
      } catch (error) {
        // An optional use with no provider degrades gracefully: say so
        // rather than fail.
        if (error instanceof Error && error.message.startsWith("not-found:")) {
          return `No installed extension provides ${FAREWELL}; install one to use it`;
        }
        throw error;
      }
    case "who": {
      const who = providers(GREET).map((provider) => provider.title);
      return who.length === 0
        ? `No installed extension provides ${GREET}`
        : `${GREET} is provided by ${who.join(", ")}`;
    }
    case "available": {
      const provider = available(FAREWELL);
      return provider === undefined
        ? `${FAREWELL} has no provider now`
        : `${FAREWELL} has a provider: ${provider.title}`;
    }
    default:
      throw new Error(`unknown item: ${itemId}`);
  }
}

/** @type {import("@pane-app/extension").Command} */
export const command = {
  async render() {
    return {
      title: "Greet from JavaScript",
      items: [
        {
          id: "greet",
          title: "Greet through a capability",
          subtitle: "Calls pane-samples:greet@1, whichever extension provides it",
          onAction: async () => {
            showToast({ title: await outcome("greet") });
          },
        },
        {
          id: "every",
          title: "Greet every provider",
          subtitle: "Calls pane-samples:greet@1 on every extension that provides it",
          onAction: async () => {
            showToast({ title: await outcome("every") });
          },
        },
        {
          id: "farewell",
          title: "Say farewell",
          subtitle: "Calls the optional pane-samples:farewell@1, which no sample provides",
          onAction: async () => {
            showToast({ title: await outcome("farewell") });
          },
        },
        {
          id: "who",
          title: "Who provides the greeting",
          subtitle: "Asks which extensions can serve pane-samples:greet@1 now",
          onAction: async () => {
            showToast({ title: await outcome("who") });
          },
        },
        {
          id: "available",
          title: "Is the farewell capability available",
          subtitle: "Asks whether an optional capability has a provider",
          onAction: async () => {
            showToast({ title: await outcome("available") });
          },
        },
      ],
    };
  },

  async openView(itemId) {
    throw new Error(`unknown view: ${itemId}`);
  },
};
