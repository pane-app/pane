// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Declarations for `pane:extension/settings` in wit/settings.wit: string
// values an installed package's commands save under keys of their choosing.
// Pane keeps them for the package's source identity while it is disabled,
// updated or Pane restarts.

/** `pane:extension/settings@0.1.0`. */
declare module "pane:extension/settings@0.1.0" {
  /**
   * The value saved under `key`, or `null` if there is none. Throws an
   * `Error` with the reason if Pane cannot read the settings, or the command
   * is built into Pane rather than installed.
   */
  export function get(key: string): string | null;
  /**
   * Saves `value` under `key`, replacing any earlier value. Throws an
   * `Error` with the reason if it cannot be saved, for example while the
   * package is disabled.
   */
  export function set(key: string, value: string): void;
}
