// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Declarations for `pane:extension/applications` in wit/applications.wit:
// the applications installed on the system, which Pane finds and opens for
// the extension, since a WASI guest cannot.

/** `pane:extension/applications@0.1.0`. */
declare module "pane:extension/applications@0.1.0" {
  /** An installed application. */
  export interface Application {
    /**
     * Identifies it to `open`: an opaque id, stable across the
     * application's updates and Pane's restarts, so it can be kept in the
     * extension's data and opened later. Several shortcuts to one program
     * are one application with one id. Do not parse it.
     */
    id: string;
    /** Its name, as the system shows it in the user's language. */
    name: string;
    /** Where it was found, for people. */
    location: string;
    /**
     * The other names that find it, each different from `name`: its
     * untranslated name, another shortcut's name, and its program's name
     * (`code` for `Code.exe`) unless that name is generic, shared with
     * another application, or the shortcut passes the program arguments
     * saying what it opens. Give them as an indexed result's
     * `alternateTitles`.
     */
    alternateTitles: string[];
    /**
     * Words that find it besides its names (a Linux desktop entry's
     * `Keywords`). Give them as an indexed result's `keywords`.
     */
    keywords: string[];
    /**
     * When another installed application has the same `name`, what tells
     * this one apart (its program's name, else its folder's, else its full
     * path), to show as a subtitle; `undefined` or `null` when it is alone
     * with its name.
     */
    distinction?: string | null;
    /**
     * Its own icon, for an item to show: `icon: { application: app.icon }`.
     * Pane extracts it in the background and keeps it; the icon's fallback
     * (or a neutral placeholder) shows until it is there. An opaque
     * reference, like `id`.
     */
    icon: string;
  }

  /**
   * The installed applications, in no particular order: Start menu
   * shortcuts and packaged apps on Windows, application bundles on macOS,
   * desktop entries on Linux. On failure it throws an object whose
   * `payload` is the reason.
   */
  export function installed(): Application[];

  /**
   * Opens (launches) the application with `id`, as `installed` gave it (an
   * id an earlier Pane gave, the path of its shortcut, bundle or desktop
   * entry, still opens it). On failure it throws an object whose `payload`
   * explains why the system did not open it.
   */
  export function open(id: string): void;
}
