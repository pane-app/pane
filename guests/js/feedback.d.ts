// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Declarations for `@pane/extension/feedback` (feedback.js): what a
// command does after it acts, through `pane:extension/feedback@0.1.0` and
// `pane:extension/window@0.1.0` (feedback-host.d.ts, wit/feedback.wit).
// Pane shows nothing of what an action, a run or a search result resolves
// with: the command tells the user what happened with a toast or a HUD, or
// closes the window.

import type { Shortcut } from "./pane";

/**
 * How a toast or a HUD is drawn: `"animated"` is work in progress, with a
 * spinner, and stays until it is updated or hidden; `"success"` and
 * `"failure"` hide after 3 seconds (a HUD after 1.2, or 3 for a failure).
 */
export type ToastStyle = "animated" | "success" | "failure";

/**
 * One of a toast's actions. Choosing it (with the pointer, from the
 * keyboard, or with its shortcut while the toast shows) runs `onAction`, as
 * an item's action does; throwing shows the error as a failure toast.
 */
export interface ToastAction {
  title: string;
  onAction: () => Promise<void> | void;
  /** Its keys while the toast shows; Pane never binds one of its own keys. */
  shortcut?: Shortcut | null;
}

/** A toast's style, title, message and actions. */
export interface ToastOptions {
  /** `"success"` when omitted. */
  style?: ToastStyle;
  title: string;
  /** More text under the title. */
  message?: string | null;
  /** The first action the keyboard reaches. */
  primaryAction?: ToastAction | null;
  secondaryAction?: ToastAction | null;
}

/** A toast the command showed. */
export interface Toast {
  /** Pane's id for it. */
  readonly id: bigint;
  /**
   * Changes its style, title, message and actions; it is shown again if it
   * had left. Does nothing once another toast replaced it or it was hidden.
   */
  update(options: ToastOptions): void;
  /** Hides it. Does nothing once another toast replaced it. */
  hide(): void;
}

/**
 * Shows a toast in the launcher's footer, replacing the one shown: toasts
 * do not queue. While the launcher is hidden or collapsed to its search
 * field, it is shown as a HUD instead.
 */
export function showToast(options: ToastOptions): Toast;

/**
 * Closes the launcher, then shows `title` in a small window of its own
 * over other applications, which never takes the focus.
 */
export function showHUD(title: string, style?: ToastStyle): void;

/**
 * What the launcher shows the next time it is shown, once
 * {@link closeMainWindow} hid it: `"default"` follows the user's Launcher
 * setting, `"immediate"` returns to root search now, `"suspended"` keeps
 * the screen left on display.
 */
export type PopToRootType = "default" | "immediate" | "suspended";

/**
 * Hides the launcher; with `clearRootSearch`, root search's query is empty
 * the next time it shows. Answers whether a window was shown for the call:
 * in a background launch, a schedule or a service it does nothing and
 * answers false.
 */
export function closeMainWindow(options?: {
  clearRootSearch?: boolean;
  popToRootType?: PopToRootType;
}): boolean;

/**
 * Returns to root search with the window open, emptying its search field
 * with `clearSearchBar`. Answers whether a window was shown for the call.
 */
export function popToRoot(options?: { clearSearchBar?: boolean }): boolean;

/** Empties the search field on screen. Answers whether a window was shown. */
export function clearSearchBar(): boolean;

/**
 * Replaces the subtitle the command's row shows in root search (and
 * matches), such as "3 unread", until it is set again; `null` gives back
 * the one its `pane.json` entry declares. Pane keeps it across restarts
 * and updates. A refusal (a call Pane does not know the command of) throws
 * an object whose `payload` is the reason.
 */
export function setSubtitle(subtitle: string | null): void;
