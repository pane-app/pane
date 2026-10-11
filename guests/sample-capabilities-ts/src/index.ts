// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Pane's capabilities sample in TypeScript: a package that uses a
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
// and the JavaScript one.
//
// Each capability's `greet`/`farewell` operation takes `{"name": "<name>"}`
// and answers `{"greeting": "..."}`. Before calling, "Who provides it"
// asks for the providers that can serve the capability now, with their
// titles.
import type { Command } from "@pane-app/extension";
import { available, call, callEvery, providers } from "@pane-app/extension/capabilities";
import type { CallError, Provider, ProviderAnswer } from "pane:extension/operations@0.1.0";
import { showToast } from "@pane-app/extension/feedback";

/** The capabilities this package uses, as its pane.json declares them. */
const GREET = "pane-samples:greet@1";
const FAREWELL = "pane-samples:farewell@1";

/** `greet` and `farewell`'s input and result. */
interface NameInput {
  name: string;
}
interface GreetingResult {
  greeting: string;
}

/**
 * Calls `operation` of `capability` for "Pane", and returns what its
 * provider answers. A failed call throws "<kind>: <message>".
 */
async function callName(capability: string, operation: string): Promise<string> {
  let result: string;
  try {
    const input: NameInput = { name: "Pane" };
    result = await call(capability, operation, JSON.stringify(input));
  } catch (error) {
    const { kind, message } = (error as { payload: CallError }).payload;
    throw new Error(`${kind}: ${message}`);
  }
  const { greeting } = JSON.parse(result) as GreetingResult;
  if (typeof greeting !== "string") {
    throw new Error("the answer has no greeting");
  }
  return greeting;
}

/** What the action of the item `itemId` answers, or throws why not. */
async function outcome(itemId: string): Promise<string> {
  switch (itemId) {
    // A required use that no installed package provides is the user's to
    // fix; Pane's reason is the answer.
    case "greet":
      return callName(GREET, "greet");
    // A use of every provider: each provider's answer, labelled with its
    // title, and an empty list when none can serve.
    case "every": {
      let answers: ProviderAnswer[];
      try {
        const input: NameInput = { name: "Pane" };
        answers = await callEvery(GREET, "greet", JSON.stringify(input));
      } catch (error) {
        const { kind, message } = (error as { payload: CallError }).payload;
        throw new Error(`${kind}: ${message}`);
      }
      const said: string[] = answers.map((answer: ProviderAnswer) => {
        if (answer.answer.tag === "ok") {
          const { greeting } = JSON.parse(answer.answer.val) as GreetingResult;
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
        if ((error as Error).message.startsWith("not-found:")) {
          return `No installed extension provides ${FAREWELL}; install one to use it`;
        }
        throw error;
      }
    case "who": {
      const who: string[] = providers(GREET).map((provider: Provider) => provider.title);
      return who.length === 0
        ? `No installed extension provides ${GREET}`
        : `${GREET} is provided by ${who.join(", ")}`;
    }
    case "available": {
      const provider: Provider | undefined = available(FAREWELL);
      return provider === undefined
        ? `${FAREWELL} has no provider now`
        : `${FAREWELL} has a provider: ${provider.title}`;
    }
    default:
      throw new Error(`unknown item: ${itemId}`);
  }
}

/** The items the command lists, each with its id, title and subtitle. */
interface SampleItem {
  id: string;
  title: string;
  subtitle: string;
}

const ITEMS: SampleItem[] = [
  {
    id: "greet",
    title: "Greet through a capability",
    subtitle: "Calls pane-samples:greet@1, whichever extension provides it",
  },
  {
    id: "every",
    title: "Greet every provider",
    subtitle: "Calls pane-samples:greet@1 on every extension that provides it",
  },
  {
    id: "farewell",
    title: "Say farewell",
    subtitle: "Calls the optional pane-samples:farewell@1, which no sample provides",
  },
  {
    id: "who",
    title: "Who provides the greeting",
    subtitle: "Asks which extensions can serve pane-samples:greet@1 now",
  },
  {
    id: "available",
    title: "Is the farewell capability available",
    subtitle: "Asks whether an optional capability has a provider",
  },
];

export const command: Command = {
  async render() {
    return {
      title: "Greet from TypeScript",
      items: ITEMS.map((item) => ({
        ...item,
        onAction: async () => {
          showToast({ title: await outcome(item.id) });
        },
      })),
    };
  },

  async openView(itemId: string) {
    throw new Error(`unknown view: ${itemId}`);
  },
};
