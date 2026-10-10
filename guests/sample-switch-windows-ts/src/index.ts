// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Pane's Switch Windows sample in TypeScript: a command that lists the
// open windows through `pane:extension/windows` — the ones Alt+Tab would
// show, each with its title, its application's name and icon, and
// whether it is minimized, maximized, on another desktop or elevated —
// one item for each, which switches to it and says what it answered in
// a toast: "Switched to notes.txt - Notepad", or why it could not.
// Typing in the command's search field filters the windows by their
// titles and their applications' names. Answers and errors match the
// Rust sample (guests/sample-switch-windows) and the JavaScript one.
import type { Command, CommandSearch, List, SearchResult } from "@pane-app/extension";
import { showToast } from "@pane-app/extension/feedback";
import { activate, listWindows } from "pane:extension/windows@0.1.0";
import type { Window } from "pane:extension/windows@0.1.0";

/** The command's id in `pane.json`. */
const SWITCH = "switch-windows";

/** Calls a host function, turning its error into a plain message. */
function host<T>(call: () => T): T {
  try {
    return call();
  } catch (error) {
    throw new Error(message(error));
  }
}

/** What `error` says, as a plain message: a host function's error (an
 * object whose payload carries why) or an Error's own message. */
function message(error: unknown): string {
  const payload = (error as { payload?: unknown } | null)?.payload;
  if (payload !== null && typeof payload === "object" && "tag" in payload) {
    return String((payload as { val?: unknown }).val);
  }
  return error instanceof Error ? error.message : String(error);
}

/** The second line of a listed window: its application's name, saying
 * "on another desktop" of a window on one. */
const subtitle = (window: Window): string =>
  window.elsewhere
    ? `${window.applicationName} — on another desktop`
    : window.applicationName;

/** One listed window as an item: its title, its application's name and
 * icon. Running it switches to the window and says what that answered. */
const item = (window: Window) => ({
  id: window.id,
  title: window.title,
  subtitle: subtitle(window),
  icon: window.icon === null ? null : { file: window.icon },
  onAction: async (): Promise<void> => {
    try {
      activate(window.id);
      showToast({ title: `Switched to ${window.title}` });
    } catch (error) {
      showToast({
        style: "failure" as const,
        title: `Switch to ${window.title}: ${message(error)}`,
      });
    }
  },
});

export const command: Command = {
  async render(): Promise<List> {
    const listed = host(() => listWindows());
    return {
      title: "Switch Windows",
      items:
        listed.length === 0
          ? [
              {
                id: "none",
                title: "No windows are open",
                subtitle: "Nothing Alt+Tab would show is listed",
              },
            ]
          : listed.map(item),
    };
  },

  /** Runs the search result the user chose: the window its id names,
   * switched to. */
  async runSearchResult(id: string): Promise<void> {
    // The list is asked for again, to say which window was switched to;
    // one that is gone says so.
    const title = host(() => listWindows()).find((window) => window.id === id)?.title;
    const gone = "a window that is gone";
    try {
      activate(id);
      showToast({ title: `Switched to ${title ?? gone}` });
    } catch (error) {
      showToast({
        style: "failure" as const,
        title: `Switch to ${title ?? gone}: ${message(error)}`,
      });
    }
  },
};

export const commandSearch: CommandSearch = {
  /** The windows whose title or application's name matches what is
   * typed, in the order they were listed. */
  async search(command: string, query: string): Promise<SearchResult[]> {
    if (command !== SWITCH) {
      throw new Error(`unknown command: ${command}`);
    }
    const typed = query.trim().toLowerCase();
    return host(() => listWindows())
      .filter(
        (window) =>
          window.title.toLowerCase().includes(typed) ||
          window.applicationName.toLowerCase().includes(typed),
      )
      .map((window) => ({
        id: window.id,
        title: window.title,
        subtitle: subtitle(window),
      }));
  },
};
