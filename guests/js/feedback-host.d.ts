// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Declarations for `pane:extension/window` and `pane:extension/feedback` in
// wit/feedback.wit: closing Pane's window, and toasts and HUDs. Most
// commands use them through `@pane/extension/feedback` (feedback.d.ts),
// whose toast actions are functions.

/** `pane:extension/window@0.1.0`. */
declare module "pane:extension/window@0.1.0" {
  export type PopToRootType = "default" | "immediate" | "suspended";
  /** Hides the launcher; whether a window was shown for the call. */
  export function close(clearRootSearch: boolean, pop: PopToRootType): boolean;
  /** Returns to root search with the window open; whether a window was shown. */
  export function popToRoot(clearSearch: boolean): boolean;
  /** Empties the search field on screen; whether a window was shown. */
  export function clearSearch(): boolean;
}

/** `pane:extension/feedback@0.1.0`. */
declare module "pane:extension/feedback@0.1.0" {
  export type ToastStyle = "animated" | "success" | "failure";
  export interface ToastAction {
    title: string;
    /** Handed to the command's `handle-event` when the user chooses it. */
    callback: string;
    /** Its shortcut as JSON text, written as an item's action's is. */
    shortcut?: string | null;
  }
  export interface Toast {
    style: ToastStyle;
    title: string;
    message?: string | null;
    primary?: ToastAction | null;
    secondary?: ToastAction | null;
  }
  export function showToast(toast: Toast): bigint;
  export function updateToast(id: bigint, toast: Toast): void;
  export function hideToast(id: bigint): void;
  export function showHud(title: string, style: ToastStyle): void;
  export interface Confirmation {
    title: string;
    message?: string | null;
    /** The primary button's label: Enter chooses it. */
    primary: string;
    destructive: boolean;
    /** The dismiss button's label ("Cancel" when none): Escape chooses it. */
    dismiss?: string | null;
    /** Offers "Don't ask again", remembering a confirmed answer (never a dismissal) under this key. */
    remember?: string | null;
  }
  /**
   * Whether the user confirmed. Rejects with an object whose `payload` is
   * why Pane asked nothing.
   */
  export function confirm(confirmation: Confirmation): Promise<boolean>;
}
