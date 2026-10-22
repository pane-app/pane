// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// `console`, which Pane's SDK gives every JS/TS command (guests/js/console.js):
// what it logs goes to the package's extension log. The `es2022` library
// commands type-check against declares none.

interface PaneConsole {
  /** Logs at debug level. */
  debug(...data: unknown[]): void;
  /** Logs at info level. */
  log(...data: unknown[]): void;
  /** Logs at info level. */
  info(...data: unknown[]): void;
  /** Logs a warning. */
  warn(...data: unknown[]): void;
  /** Logs an error; an `Error` with its stack. */
  error(...data: unknown[]): void;
  /** Logs the message with the current stack, at debug level. */
  trace(...data: unknown[]): void;
  /** Logs an error when `condition` is false. */
  assert(condition?: boolean, ...data: unknown[]): void;
}

declare var console: PaneConsole;
