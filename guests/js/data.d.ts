// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Declarations for the interfaces in wit/data.wit: `settings`, `content`,
// `cache` and `credentials`, each string values an installed package's
// commands save under keys of their choosing. Pane keeps them for the
// package's source identity while it is disabled, updated or Pane restarts;
// the kind decides what clearing the extension's cache removes (only `cache`).

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

/**
 * `pane:extension/content@0.1.0`: the extension's own durable records, such
 * as notes or history, kept like its settings and not removed when its cache
 * is cleared. `get` and `set` behave as in the settings module.
 */
declare module "pane:extension/content@0.1.0" {
  export function get(key: string): string | null;
  export function set(key: string, value: string): void;
}

/**
 * `pane:extension/cache@0.1.0`: disposable values the extension can compute
 * or download again. The user can clear the cache in Manage extensions at any
 * time without the extension running, so any value may be `null` next time.
 * `get` and `set` behave as in the settings module.
 */
declare module "pane:extension/cache@0.1.0" {
  export function get(key: string): string | null;
  export function set(key: string, value: string): void;
}

/**
 * `pane:extension/credentials@0.1.0`: secrets the extension keeps on this
 * computer, such as a sign-in token. Clearing the cache keeps them. Pane
 * stores them as plain text in its data folder, not in the system's keychain.
 * `get` and `set` behave as in the settings module.
 */
declare module "pane:extension/credentials@0.1.0" {
  export function get(key: string): string | null;
  export function set(key: string, value: string): void;
}

/**
 * `pane:extension/preferences@0.1.0` (wit/preferences.wit): the effective
 * values of the preferences the command's package declares in `pane.json`,
 * as JSON text. Use `getPreferenceValues` from `@pane/extension/preferences`,
 * which parses it.
 */
declare module "pane:extension/preferences@0.1.0" {
  /**
   * The effective preference values of `command` (its id in `pane.json`) of
   * the caller's own package as a JSON object's text; of the command Pane is
   * running when `command` is `null` or absent. A refusal (a command the
   * package does not have, code that is stopped) throws an object whose
   * `payload` is the reason.
   */
  export function values(command?: string | null): string;
}
