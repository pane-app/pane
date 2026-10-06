// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Pane's schedule sample in TypeScript: a command whose `pane.json` entry
// declares a `schedule`, so Pane runs its "Run now" item every interval
// while the package is enabled, without the user asking. Each run adds one
// to a count kept in its content and answers the new count. Items, titles,
// results and errors match the Rust schedule sample (guests/sample-schedule)
// and the JavaScript one. "Run slowly" notes in its settings that it
// started, waits ten seconds, then notes that it finished: disabling or
// reloading the package meanwhile stops the call, so it never finishes.
// "Answer an error" throws, which is an error the extension answers with,
// never a pause. "Crash" crashes on purpose: three crashes within five
// minutes pause the package until retried. "Stop responding" computes
// without waiting for up to a minute, so Pane stops it after five seconds
// of its own computing and counts that as a crash too.
import type { Command, CustomView, Item, List } from "@pane/extension";
import { get, set } from "pane:extension/settings@0.1.0";
import * as content from "pane:extension/content@0.1.0";
import { waitFor } from "wasi:clocks/monotonic-clock@0.3.0";

/** The content key holding how many runs the command counted. */
const COUNT = "count";
/** The settings key where "Run slowly" notes how far it got. */
const SLOW = "slow";
/** How long "Run slowly" waits, in nanoseconds. */
const SLOW_WAIT = 10_000_000_000;
/** How long "Stop responding" computes at most, in milliseconds: bounded, so that even without Pane stopping it, it ends. */
const BUSY_FOR = 60_000;

/** The count kept in the command's content, zero before the first run. */
function count(): number {
  const saved = content.get(COUNT);
  const counted = saved === null ? 0 : Number.parseInt(saved, 10);
  if (Number.isNaN(counted)) {
    throw new Error("the count is not a number");
  }
  return counted;
}

/** Runs the action of the item `itemId`. */
async function act(itemId: string): Promise<string> {
  switch (itemId) {
    case "count":
    case "slow": {
      // One more run: counted before anything else, so a run Pane stops
      // on the way still counts as having begun.
      const runs = count() + 1;
      content.set(COUNT, String(runs));
      if (itemId === "slow") {
        set(SLOW, "started");
        // The command suspends here; if Pane stops the call meanwhile,
        // nothing after this line runs.
        await waitFor(SLOW_WAIT);
        set(SLOW, "finished");
        return `Ran ${runs} times, after waiting 10 seconds`;
      }
      return `Ran ${runs} times`;
    }
    case "refuse":
      // Throwing is an error the extension answers with, never a crash.
      throw new Error("The schedule sample refuses, to show how an error looks");
    case "crash":
      // Resolving with something other than a string is a crash, unlike
      // throwing, which is an error the extension answers with. The run
      // is counted, as the Rust sample's is.
      content.set(COUNT, String(count() + 1));
      return undefined as unknown as string;
    case "busy": {
      // The run is counted, then it computes without awaiting anything:
      // the guest never yields to Pane by itself, so Pane stops it after
      // its computing limit and counts it towards pausing the package,
      // as a crash.
      const runs = count() + 1;
      content.set(COUNT, String(runs));
      const end = Date.now() + BUSY_FOR;
      while (Date.now() < end) {
        // busy
      }
      return `Ran ${runs} times`;
    }
    default:
      throw new Error(`unknown item: ${itemId}`);
  }
}

/** An item whose action is `act` with its id. */
const item = (id: string, title: string, subtitle: string): Item => ({
  id,
  title,
  subtitle,
  onAction: () => act(id),
});

export const command: Command = {
  async render(): Promise<List> {
    return {
      title: `Ran ${count()} times`,
      items: [
        item("count", "Run now", "What the schedule runs; it adds one to the count and answers it"),
        item("slow", "Run slowly", "Waits 10 seconds, then answers; disabling or reloading stops it"),
        item("refuse", "Answer an error", "An error the extension answers with never pauses it"),
        item("crash", "Crash", "Crashes on purpose; three crashes within five minutes pause the extension"),
        item("busy", "Stop responding", "Computes without waiting for up to a minute; Pane stops it after 5 seconds, counted as a crash"),
      ],
    };
  },

  async submitForm(itemId: string): Promise<string> {
    // A rejected submitForm is a message about the whole form.
    throw new Error(`The schedule sample has no forms: ${itemId}`);
  },

  async openView(itemId: string): Promise<CustomView> {
    throw new Error(`The schedule sample has no custom views: ${itemId}`);
  },
};
