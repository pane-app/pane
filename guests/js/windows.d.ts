// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Declarations for `pane:extension/windows` in wit/windows.wit: the
// open windows Windows' Alt+Tab would show — each with its title, its
// application's name and icon, whether it is minimized, maximized, on
// another virtual desktop or elevated, in z-order with the front
// application's window first — and bringing one of them to the front,
// restoring it first if it is minimized. Windows only. Only a command
// whose package.json sets `"pane": { "windows": true }` imports it.

/** `pane:extension/windows@0.1.0`. */
declare module "pane:extension/windows@0.1.0" {
  /**
   * One window Alt+Tab would show, as it is listed.
   */
  export interface Window {
    /**
     * Identifies the window for this session alone: opaque (a command
     * must not parse it), valid while the session runs, and names the
     * window to `activate`. A window that closed is gone, and another
     * may have taken its place.
     */
    id: string;
    /** The window's title, as the system shows it ("notes.txt - Notepad"). */
    title: string;
    /**
     * The name of the application the window belongs to, as the system
     * shows it ("Notepad", "Google Chrome"), best-effort: the
     * application installed on the system the window resolves to, or one
     * named for the window itself where none matches.
     */
    applicationName: string;
    /**
     * What the application's icon is the system's icon of: its program's
     * path, or a Windows `shell:AppsFolder` name, as a file icon names
     * one. `null` when nothing of the application is known.
     */
    icon: string | null;
    /** Whether the window is minimized. */
    minimized: boolean;
    /** Whether the window is maximized. */
    maximized: boolean;
    /**
     * Whether the window is on another virtual desktop than the one
     * shown: it was kept only because the user's Alt+Tab shows all
     * desktops, and activating it is as Windows activates it from
     * Alt+Tab.
     */
    elsewhere: boolean;
    /**
     * Whether the window's process is running as administrator, as far
     * as Pane can tell: bringing it to the front may still fail, and
     * window actions on it (a later slice) will say they cannot reach
     * it.
     */
    elevated: boolean;
  }

  /**
   * Why a call did not answer, as the call throws it (an object whose
   * `payload` is one of these): `tag` tells the cases apart.
   *
   * - `not-available`: the open windows are Windows-only, and this Pane
   *   may reach no system at all; nothing went wrong, and the command
   *   can do something else;
   * - `failed`: it went wrong — a window gone, or one that did not come
   *   to the front.
   *
   * `val` says why for people.
   */
  export type WindowsError =
    | { tag: "not-available"; val: string }
    | { tag: "failed"; val: string };

  /**
   * The windows Alt+Tab would show, in z-order with the front
   * application's window first: the application the user was in before
   * Pane, then the rest by how recently they were in front. On failure
   * it throws an object whose `payload` is the {@link WindowsError}.
   */
  export function listWindows(): Window[];

  /**
   * Brings the window `id` names to the front, restoring it first if it
   * is minimized. An id no window of this session names, or one whose
   * window closed since it was listed, throws an object whose `payload`
   * is the {@link WindowsError}; nothing else changed.
   */
  export function activate(id: string): void;
}
