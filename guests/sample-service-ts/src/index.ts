// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Pane's service sample in TypeScript: a command whose `pane.json` entry
// declares a continuing service, so Pane runs its cycles while the package
// is enabled, without the user asking and at no interval the manifest
// declares: each cycle answers the status to show and asks Pane when to
// run the next. The service watches a count of events kept in its content
// — its "Add an event" item adds one — and counts its cycles, in its
// content for all time and in this module for this run: the second count
// is the state of the task the service manages, which lives as long as
// the code's generation and is dropped with it, so a disable, a reload or
// a pause ends it and enabling or retrying starts a fresh one. Items,
// titles, results and errors match the Rust service sample
// (guests/sample-service) and the JavaScript one. "Wait on the next
// cycle" makes the next cycle note in its settings that it started, wait
// ten seconds, then note that it finished: disabling or reloading the
// package meanwhile stops the cycle, so it never finishes. "Fail the next
// cycle" throws, which is an error the extension answers with, never a
// pause. "Crash the next cycle" resolves with a wrong value on purpose:
// three crashes within five minutes pause the package until retried.
// "Stop responding on the next cycle" computes without waiting for up to
// a minute, so Pane stops it after five seconds of its own computing and
// counts that as a crash too. "Ask for a 0-second cadence" and "Ask for a
// 31-day cadence" make the next cycle answer cadences beyond Pane's
// bounds, which it clamps to its 1-second minimum and 30-day maximum.
import { get, set } from "pane:extension/settings@0.1.0";
import * as content from "pane:extension/content@0.1.0";
import { waitFor } from "wasi:clocks/monotonic-clock@0.3.0";
import type { Cycle, Item, List, Service } from "@pane/extension";
import { showToast } from "@pane/extension/feedback";

/** The content key holding how many cycles the service has run, ever. */
const CYCLES = "cycles";
/** The content key holding how many events the service is watching. */
const EVENTS = "events";
/** The settings key holding what the next service cycle does: "" (nothing), "slow", "fail", "crash", "busy", "fast" or "far". */
const MODE = "mode";
/** The settings key where a waiting cycle notes how far it got. */
const SLOW = "slow";
/** How long a waiting cycle waits, in nanoseconds. */
const SLOW_WAIT = 10_000_000_000;
/** How long a computing cycle computes at most, in milliseconds: bounded, so that even without Pane stopping it, it ends. */
const BUSY_FOR = 60_000;
/** The cadence the service asks Pane for, in seconds: how it paces itself. */
const EVERY = 1;
/** The cadence a "0-second" cycle answers: none at all, which Pane clamps to its 1-second minimum, so a service can never busy-loop itself. */
const AT_ONCE = 0;
/** The cadence a "31-day" cycle answers: beyond the 30-day maximum Pane runs a service at, which it clamps to, so a service always runs again. */
const TOO_FAR = 31 * 86_400;

/**
 * Cycles this instance of the service has run: the state of the task it
 * manages, living in this module's memory, so it begins again whenever
 * Pane starts the code afresh (a new generation) and carries on between
 * cycles while the generation lasts.
 */
let thisRun = 0;

/** The count kept in the command's content, zero before the first. */
function counted(key: string): number {
  const saved = content.get(key);
  const count = saved === null ? 0 : Number.parseInt(saved, 10);
  if (Number.isNaN(count)) {
    throw new Error("the count is not a number");
  }
  return count;
}

/** Runs the action of the item `itemId`, showing a toast with what it did. */
async function act(itemId: string): Promise<void> {
  showToast({ title: await outcome(itemId) });
}

/** Does what the item `itemId`'s action does; the text its toast shows. */
async function outcome(itemId: string): Promise<string> {
  switch (itemId) {
    case "add": {
      const events = counted(EVENTS) + 1;
      content.set(EVENTS, String(events));
      return `Added event ${events}; the next cycle reports it`;
    }
    case "slow":
      set(MODE, "slow");
      return "The next cycle will wait 10 seconds";
    case "fail":
      set(MODE, "fail");
      return "The next cycle will answer an error";
    case "crash":
      set(MODE, "crash");
      return "The next cycle will crash";
    case "busy":
      set(MODE, "busy");
      return "The next cycle will stop responding";
    case "fast":
      set(MODE, "fast");
      return "The next cycle will answer 0 seconds";
    case "far":
      set(MODE, "far");
      return "The next cycle will answer 31 days";
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

export const command = {
  async render(): Promise<List> {
    return {
      title: `Watching: ${counted(EVENTS)} events (${counted(CYCLES)} cycles)`,
      items: [
        item("add", "Add an event", "What the service watches; the next cycle reports it"),
        item("slow", "Wait on the next cycle", "The next cycle waits 10 seconds; disabling or reloading stops it"),
        item("fail", "Fail the next cycle", "The next cycle answers an error, which never pauses the extension"),
        item("crash", "Crash the next cycle", "The next cycle crashes on purpose; three crashes within five minutes pause the extension"),
        item("busy", "Stop responding on the next cycle", "The next cycle computes without waiting; Pane stops it after 5 seconds, counted as a crash"),
        item("fast", "Ask for a 0-second cadence", "The next cycle answers 0; Pane runs it no sooner than its 1-second minimum"),
        item("far", "Ask for a 31-day cadence", "The next cycle answers 31 days; Pane clamps it to its 30-day maximum"),
      ],
    };
  },

  async submitForm(itemId: string): Promise<string> {
    // A rejected submitForm is a message about the whole form.
    throw new Error(`The service sample has no forms: ${itemId}`);
  },

  async openView(itemId: string): Promise<never> {
    throw new Error(`The service sample has no custom views: ${itemId}`);
  },
};

export const service: Service = {
  // One cycle of the service: it counts itself (in its content for all
  // time, in this module for this run) and answers the status to show and
  // when to run the next. The mode an item armed makes this cycle wait,
  // answer an error, crash or stop responding instead.
  async runCycle(command: string): Promise<Cycle> {
    if (command !== "watching") {
      throw new Error(`unknown command: ${command}`);
    }
    // One more cycle: counted before anything else, so a cycle Pane stops
    // on the way still counts as having begun.
    const cycles = counted(CYCLES) + 1;
    content.set(CYCLES, String(cycles));
    thisRun += 1;
    const mode = get(MODE) ?? "";
    if (mode !== "") {
      set(MODE, "");
    }
    /** The cycle's answer, after `waited` says whether it waited and `next` is the cadence it asks for. */
    const answer = (waited: boolean, next: number): Cycle => ({
      status:
        `Watching: ${counted(EVENTS)} events (cycle ${cycles}, ${thisRun} this run)` +
        (waited ? ", after waiting 10 seconds" : ""),
      nextSeconds: next,
    });
    switch (mode) {
      case "slow":
        set(SLOW, "started");
        // The command suspends here; if Pane stops the cycle meanwhile,
        // nothing after this line runs.
        await waitFor(SLOW_WAIT);
        set(SLOW, "finished");
        return answer(true, EVERY);
      case "fail":
        // Throwing is an error the extension answers with, never a crash.
        throw new Error("The service sample refuses, to show how an error looks");
      case "crash":
        // Resolving with something other than a cycle is a crash, unlike
        // throwing, which is an error the extension answers with. The
        // cycle is counted, as the Rust sample's is.
        return undefined as unknown as Cycle;
      case "busy": {
        // The cycle is counted, then it computes without awaiting
        // anything: the guest never yields to Pane by itself, so Pane
        // stops it after its computing limit and counts it towards
        // pausing the package, as a crash.
        const end = Date.now() + BUSY_FOR;
        while (Date.now() < end) {
          // busy
        }
        return answer(false, EVERY);
      }
      // Cadences beyond Pane's bounds, clamped by it: a service that asks
      // for no wait at all still runs no sooner than every second, and one
      // that asks for more than 30 days still runs again.
      case "fast":
        return answer(false, AT_ONCE);
      case "far":
        return answer(false, TOO_FAR);
      default:
        return answer(false, EVERY);
    }
  },
};
