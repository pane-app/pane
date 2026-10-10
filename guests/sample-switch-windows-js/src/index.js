// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Pane's Switch Windows sample in JavaScript: a command that lists the
// open windows through `pane:extension/windows` — the ones Alt+Tab would
// show, each with its title, its application's name and icon, and
// whether it is minimized, maximized, on another desktop or elevated —
// one item for each, which switches to it and says what it answered in
// a toast: "Switched to notes.txt - Notepad", or why it could not.
// Typing in the command's search field filters the windows by their
// titles and their applications' names. Answers and errors match the
// Rust sample (guests/sample-switch-windows) and the TypeScript one.
// @ts-check
import { showToast } from "@pane-app/extension/feedback";
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
 * One listed window as an item: its title, its application's name and
 * icon. Running it switches to the window and says what that answered.
 * @param {import("pane:extension/windows@0.1.0").Window} window
 */
const item = (window) => ({
  id: window.id,
  title: window.title,
  subtitle: subtitle(window),
  icon: window.icon === null ? null : { file: window.icon },
  onAction: async () => {
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
});

/** @type {import("@pane-app/extension").Command} */
export const command = {
  async render() {
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
  async runSearchResult(id) {
    // The list is asked for again, to say which window was switched to;
    // one that is gone says so.
    const title = host(() => listWindows()).find(
      (window) => window.id === id,
    )?.title;
    try {
      activate(id);
      showToast({ title: `Switched to ${title ?? "a window that is gone"}` });
    } catch (error) {
      showToast({
        style: "failure",
        title: `Switch to ${title ?? "a window that is gone"}: ${message(error)}`,
      });
    }
  },
};

/** @type {import("@pane-app/extension").CommandSearch} */
export const commandSearch = {
  /** The windows whose title or application's name matches what is
   * typed, in the order they were listed. */
  async search(command, query) {
    if (command !== "switch-windows") {
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
