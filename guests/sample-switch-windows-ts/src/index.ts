// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Pane's Switch Windows sample in TypeScript: a command that lists the
// open windows through `pane:extension/windows` — the ones Alt+Tab would
// show, each with its title, its application's name and icon, and
// whether it is minimized, maximized, on another desktop or elevated —
// one item for each, which switches to it and says what it answered in
// a toast: "Switched to notes.txt - Notepad", or why it could not.
// Typing in the command's search field filters the windows by their
// titles and their applications' names, as the designed List's host
// filtering does (#240, each item's keywords naming its application).
// Answers and errors match the Rust sample
// (guests/sample-switch-windows) and the JavaScript one.
import type { Command } from "@pane-app/extension";
import { showToast } from "@pane-app/extension/feedback";
import { EmptyState, List, createView } from "@pane-app/extension/view";
import { jsxs } from "@pane-app/extension/jsx-runtime";
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

/** One listed window as an item: its title, its application's name (as
 * the keywords the List's host filtering matches besides the title) and
 * icon. Choosing it switches to the window and says what that answered. */
const item = (window: Window) =>
  jsxs(List.Item, {
    key: window.id,
    title: window.title,
    subtitle: subtitle(window),
    icon: window.icon === null ? null : { file: window.icon },
    keywords: [window.applicationName],
    onClick: async (): Promise<void> => {
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
    children: [window.title],
  });

/** The view the command opens (#240): a List of the open windows, which
 * Pane filters by the text typed in its search field with the
 * root-search matcher, each item's keywords naming its application, so
 * typing finds a window by its title or its application's name. */
function SwitchWindows() {
  const listed = host(() => listWindows());
  return jsxs(List, {
    navigationTitle: "Switch Windows",
    searchPlaceholder: "Filter the windows…",
    children:
      listed.length === 0
        ? [
            jsxs(EmptyState, {
              title: "No windows are open",
              description: "Nothing Alt+Tab would show is listed",
              children: [],
            }),
          ]
        : listed.map(item),
  });
}

export const command: Command = {
  async openView(command: string) {
    if (command !== SWITCH) {
      throw new Error(`unknown command: ${command}`);
    }
    return createView(SwitchWindows);
  },
};
