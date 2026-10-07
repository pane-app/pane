// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Pane's settings sample in TypeScript: a command whose chosen greeting style
// Pane keeps between runs, saved with `pane:extension/settings`, and one value
// of each other kind of data: a note (content), the last greeting (cache) and
// a sign-in token (credentials). Items, titles, toasts and errors match the
// Rust settings sample (guests/sample-settings) and the JavaScript one.
// "Save after waiting" notes in its settings that it started, waits ten
// seconds, then notes that it finished: disabling or reloading the package
// meanwhile stops the call, so it never finishes. "Crash" crashes on purpose:
// three crashes within five minutes pause the package until retried.
// "Stop responding" notes in its settings that it started, then computes
// without waiting for anything for up to a minute before noting that it
// finished: Pane stops a call that computes for 5 seconds without waiting,
// so it never finishes, and it counts towards pausing the package as a
// crash does.
import type { Command, CustomView, Item, List } from "@pane/extension";
import { showToast } from "@pane/extension/feedback";
import { get, set } from "pane:extension/settings@0.1.0";
import * as cache from "pane:extension/cache@0.1.0";
import * as content from "pane:extension/content@0.1.0";
import * as credentials from "pane:extension/credentials@0.1.0";
import { waitFor } from "wasi:clocks/monotonic-clock@0.3.0";

/** The settings key holding the chosen greeting style. */
const STYLE = "greeting-style";
/** The content key holding the user's note. */
const NOTE = "note";
/** The cache key holding the last greeting, which "Greet me" can make again. */
const LAST_GREETING = "last-greeting";
/** The credentials key holding the sign-in token. */
const TOKEN = "token";
/** The settings key where "Save after waiting" notes how far it got. */
const SLOW_SAVE = "slow-save";
/** How long "Save after waiting" waits, in nanoseconds. */
const SLOW_WAIT = 10_000_000_000;
/** The settings key where "Stop responding" notes how far it got. */
const BUSY = "busy";
/** How long "Stop responding" computes at most, in milliseconds: bounded, so that even without Pane stopping it, it ends. */
const BUSY_FOR = 60_000;

/** An item whose action is `act` with its id. */
const item = (id: string, title: string, subtitle: string): Item => ({
  id,
  title,
  subtitle,
  onAction: () => act(id),
});

/** The greeting in the saved `style`; throws if no style is saved. */
function greetingIn(style: string | null): string {
  switch (style) {
    case "formal":
      return "Good day to you";
    case "casual":
      return "Hi there";
    default:
      throw new Error("No greeting style is saved yet; choose one first");
  }
}

async function render(): Promise<List> {
  // A settings error (get throws) is shown to the user as the command's error.
  const style = get(STYLE);
  return {
    title: style === null ? "Greeting" : `Greeting: ${style}`,
    items: [
      item("formal", "Use a formal greeting", "Saved in Pane's settings"),
      item("casual", "Use a casual greeting", "Saved in Pane's settings"),
      item("greet", "Greet me", "Answer in the saved style"),
      item("note", "Save a note", "Kept in Pane as the extension's content"),
      item("sign-in", "Sign in", "Keeps a token as a local credential"),
      item("kept", "Show what Pane keeps", "Settings, content, cache and credential"),
      item("slow", "Save after waiting", "Waits 10 seconds, then saves; disabling or reloading stops it"),
      item("crash", "Crash", "Crashes on purpose; three crashes within five minutes pause the extension"),
      item("busy", "Stop responding", "Computes without waiting for up to a minute; Pane stops it after 5 seconds"),
    ],
  };
}

/** Runs the action of the item `itemId`, showing a toast with what it did. */
async function act(itemId: string): Promise<void> {
  const done = await outcome(itemId);
  // Resolving with a value, where an action resolves with nothing, is a
  // crash, unlike throwing, which is an error the extension answers with.
  if (done === null) return null as unknown as void;
  showToast({ title: done });
}

/**
 * Does what the item `itemId`'s action does; the text its toast shows, or
 * `null` for "Crash", which crashes.
 */
async function outcome(itemId: string): Promise<string | null> {
  switch (itemId) {
    case "formal":
    case "casual":
      set(STYLE, itemId);
      return `Saved the ${itemId} greeting`;
    case "greet": {
      const greeting = greetingIn(get(STYLE));
      cache.set(LAST_GREETING, greeting);
      return greeting;
    }
    case "note":
      content.set(NOTE, "Water the plants");
      return "Saved a note";
    case "sign-in":
      credentials.set(TOKEN, "sample-token");
      return "Signed in on this computer";
    case "kept":
      return [
        `Style: ${get(STYLE) ?? "none"}`,
        `Note: ${content.get(NOTE) ?? "none"}`,
        `Signed in: ${credentials.get(TOKEN) === null ? "no" : "yes"}`,
        `Cached greeting: ${cache.get(LAST_GREETING) ?? "none"}`,
      ].join(" · ");
    case "slow":
      set(SLOW_SAVE, "started");
      // The command suspends here; if Pane stops the call meanwhile, nothing
      // after this line runs.
      await waitFor(SLOW_WAIT);
      set(SLOW_SAVE, "finished");
      return "Saved after waiting 10 seconds";
    case "busy": {
      set(BUSY, "started");
      // Computes without awaiting anything: the guest never yields to
      // Pane by itself.
      const end = Date.now() + BUSY_FOR;
      while (Date.now() < end) {
        // busy
      }
      set(BUSY, "finished");
      return "Finished computing after a minute";
    }
    case "crash":
      return null;
    default:
      throw new Error(`unknown item: ${itemId}`);
  }
}

async function submitForm(itemId: string): Promise<string> {
  // Any Error thrown from submitForm is a message about the whole form.
  throw new Error(`unknown form: ${itemId}`);
}

async function openView(itemId: string): Promise<CustomView> {
  throw new Error(`unknown view: ${itemId}`);
}

export const command: Command = { render, submitForm, openView };
