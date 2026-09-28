// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Declarations for `pane:extension/applications` in wit/applications.wit:
// the applications installed on the system, which Pane finds and opens for
// the extension, since a WASI guest cannot.

/** `pane:extension/applications@0.1.0`. */
declare module "pane:extension/applications@0.1.0" {
  /** An installed application. */
  export interface Application {
    /** Identifies it to `open`: the shortcut, bundle or desktop entry Pane found it by. */
    id: string;
    /** Its name, as the system shows it. */
    name: string;
    /** Where it was found, for people. */
    location: string;
  }

  /**
   * The installed applications, in no particular order: Start menu
   * shortcuts and packaged apps on Windows, application bundles on macOS,
   * desktop entries on Linux. On failure it throws an object whose
   * `payload` is the reason.
   */
  export function installed(): Application[];

  /**
   * Opens (launches) the application with `id`, as `installed` gave it. On
   * failure it throws an object whose `payload` explains why the system did
   * not open it.
   */
  export function open(id: string): void;
}
