// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// What a JS/TS command does after it acts (`@pane/extension/feedback`):
// tell the user what happened with a toast or a HUD, close Pane's window,
// pop back to root search, clear the search field, and set its row's
// subtitle, through `pane:extension/feedback@0.1.0`,
// `pane:extension/window@0.1.0` and `pane:extension/commands@0.1.0`
// (wit/feedback.wit, wit/commands.wit). Bundled into the command that
// imports it, like any npm module.
//
// A toast's actions are functions: the toast names each by a callback id
// (`toast:<n>:primary`, `toast:<n>:secondary`), which Pane hands back to
// the command's `handle-event` when the user chooses it, and the SDK's
// adapter (adapt.js) runs the function. The two share the table of the
// newest toast's actions through `globalThis`, so it is one table however
// the bundler places the two modules.

import { hideToast, showHud, showToast as show, updateToast } from "pane:extension/feedback@0.1.0";
import { clearSearch, close, popToRoot as pop } from "pane:extension/window@0.1.0";
import { setSubtitle as set } from "pane:extension/commands@0.1.0";

/** Where the newest toast's actions are kept, by callback id. */
const ACTIONS = Symbol.for("pane.extension.toastActions");

/** The newest toast's actions, by callback id. */
function actions() {
  if (!(globalThis[ACTIONS] instanceof Map)) globalThis[ACTIONS] = new Map();
  return globalThis[ACTIONS];
}

/** The number the last toast shown got. */
let last = 0;

/** `options` as the WIT carries a toast; its actions go into the table. */
function wire(number, options) {
  const kept = actions();
  kept.clear();
  const action = (slot, given) => {
    if (given == null) return null;
    const callback = `toast:${number}:${slot}`;
    if (typeof given.onAction === "function") kept.set(callback, given.onAction);
    return {
      title: String(given.title),
      callback,
      shortcut: given.shortcut == null ? null : JSON.stringify(given.shortcut),
    };
  };
  return {
    style: options?.style ?? "success",
    title: String(options?.title ?? ""),
    message: options?.message == null ? null : String(options.message),
    primary: action("primary", options?.primaryAction),
    secondary: action("secondary", options?.secondaryAction),
  };
}

/**
 * Shows a toast in the launcher's footer, replacing the one shown (as a HUD
 * while the launcher is hidden or collapsed to its search field), and
 * returns it, to update or hide.
 */
export function showToast(options) {
  last += 1;
  const number = last;
  const id = show(wire(number, options));
  return {
    id,
    update(changed) {
      updateToast(id, wire(number, changed));
    },
    hide() {
      hideToast(id);
      const prefix = `toast:${number}:`;
      for (const callback of [...actions().keys()]) {
        if (callback.startsWith(prefix)) actions().delete(callback);
      }
    },
  };
}

/** Closes the launcher, then shows `title` over other applications. */
export function showHUD(title, style = "success") {
  showHud(String(title), style);
}

/**
 * Hides the launcher. Answers whether a window was shown for the call
 * (false in a background launch, a schedule or a service).
 */
export function closeMainWindow(options = {}) {
  return close(Boolean(options?.clearRootSearch), options?.popToRootType ?? "default");
}

/** Returns to root search with the window open; whether a window was shown. */
export function popToRoot(options = {}) {
  return pop(Boolean(options?.clearSearchBar));
}

/** Empties the search field on screen; whether a window was shown. */
export function clearSearchBar() {
  return clearSearch();
}

/**
 * Replaces the subtitle the command's row shows in root search; `null`
 * gives back its `pane.json` one. A refusal throws an object whose
 * `payload` is the reason.
 */
export function setSubtitle(subtitle) {
  set(subtitle == null ? null : String(subtitle));
}

/** The action of the newest toast named `callback`, if any (adapt.js). */
export function toastAction(callback) {
  return actions().get(callback);
}
