// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Pane's Switch Windows sample in JavaScript: a command that lists the
// open windows through `pane:extension/windows` — the ones Alt+Tab would
// show, each with its title, its application's name and icon, and
// whether it is minimized, maximized, on another desktop or elevated —
// one item for each, which switches to it and says what it answered in
// a toast: "Switched to notes.txt - Notepad", or why it could not.
// Typing in the command's search field filters the windows by their
// titles and their applications' names, as the designed List's host
// filtering does (#240, each item's keywords naming its application).
// Answers and errors match the Rust sample
// (guests/sample-switch-windows) and the TypeScript one.
// @ts-check
import { showToast } from "@pane-app/extension/feedback";
import { EmptyState, List, createView } from "@pane-app/extension/view";
import { jsxs } from "@pane-app/extension/jsx-runtime";
import { activate, listWindows } from "pane:extension/windows@0.1.0";

/**
 * Calls a host function, turning its error into a plain message.
 * @template T
 * @param {() => T} call
 * @returns {T}
 */
function host(call) {
  try {
    return call();
  } catch (error) {
    throw new Error(message(error));
  }
}

/**
 * What `error` says, as a plain message: a host function's error (an
 * object whose payload carries why) or an Error's own message.
 * @param {unknown} error
 * @returns {string}
 */
function message(error) {
  const payload = /** @type {{ payload?: unknown } | null} */ (error)?.payload;
  if (payload !== null && typeof payload === "object" && "tag" in payload) {
    return String(/** @type {{ val?: unknown }} */ (payload).val);
  }
  return error instanceof Error ? error.message : String(error);
}

/**
 * The second line of a listed window: its application's name, saying
 * "on another desktop" of a window on one.
 * @param {import("pane:extension/windows@0.1.0").Window} window
 * @returns {string}
 */
const subtitle = (window) =>
  window.elsewhere
    ? `${window.applicationName} — on another desktop`
    : window.applicationName;

/**
 * One listed window as an item: its title, its application's name (as
 * the keywords the List's host filtering matches besides the title) and
 * icon. Choosing it switches to the window and says what that answered.
 * @param {import("pane:extension/windows@0.1.0").Window} window
 */
const item = (window) =>
  jsxs(List.Item, {
    key: window.id,
    title: window.title,
    subtitle: subtitle(window),
    icon: window.icon === null ? null : { file: window.icon },
    keywords: [window.applicationName],
    onClick: async () => {
      try {
        activate(window.id);
        showToast({ title: `Switched to ${window.title}` });
      } catch (error) {
        showToast({
          style: "failure",
          title: `Switch to ${window.title}: ${message(error)}`,
        });
      }
    },
    children: [window.title],
  });

/**
 * The view the command opens (#240): a List of the open windows, which
 * Pane filters by the text typed in its search field with the
 * root-search matcher, each item's keywords naming its application, so
 * typing finds a window by its title or its application's name.
 */
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

/** @type {import("@pane-app/extension").Command} */
export const command = {
  async openView(command) {
    if (command !== "switch-windows") {
      throw new Error(`unknown command: ${command}`);
    }
    return createView(SwitchWindows);
  },
};
