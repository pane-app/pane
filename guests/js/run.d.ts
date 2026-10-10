// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Declarations for `pane:extension/run` in wit/run.wit: what Windows'
// Run dialog (Win+R) runs — a command line naming a program by its name
// or path with arguments, a Control Panel applet, a management console, a
// document, a folder, a network path, a `shell:` or `ms-settings:`
// address or another registered scheme, with environment variables
// expanded — run for the command, through Windows' own elevation prompt
// when asked, together with the Run dialog's own history, which the
// command and Windows' Run dialog share in both directions, and the
// completions for the text typed in Run's field: history entries,
// programs, applets, consoles, registered schemes and environment
// variables, each matching the typed text. Windows only. Only a command
// whose package.json sets `"pane": { "run": true }` imports it.

/** `pane:extension/run@0.1.0`. */
declare module "pane:extension/run@0.1.0" {
  /**
   * Why a run or a history call did not answer, as the call throws it
   * (an object whose `payload` is one of these): `tag` tells the cases
   * apart.
   *
   * - `not-available`: running what the Run dialog runs is Windows-only,
   *   and this Pane may reach no system at all; nothing went wrong, and
   *   the command can do something else;
   * - `failed`: it went wrong. A run that ran but whose history could not
   *   be written says that, saying it ran;
   * - `declined`: the user declined Windows' elevation prompt: nothing
   *   ran, and nothing was recorded.
   *
   * `val` says why for people.
   */
  export type RunError =
    | { tag: "not-available"; val: string }
    | { tag: "failed"; val: string }
    | { tag: "declined"; val: string };

  /**
   * Runs the command line `line` as the Run dialog reads it, through
   * Windows' own elevation prompt when `elevated` (which the user may
   * decline). A run that answered is recorded in the Run dialog's
   * history, the trimmed line; a failed run, or a declined elevation,
   * records nothing. On failure it throws an object whose `payload` is
   * the {@link RunError}.
   */
  export function run(line: string, elevated: boolean): void;

  /**
   * The Run dialog's history, newest first, as the user typed it: what
   * ran in Windows' Run dialog and in Pane alike. On failure it throws an
   * object whose `payload` is the {@link RunError}.
   */
  export function history(): string[];

  /**
   * Removes `line` from the Run dialog's history, matched ignoring case,
   * so it is gone from Windows' Run dialog too; a line that is not in the
   * history throws an object whose `payload` is the {@link RunError}.
   */
  export function deleteFromHistory(line: string): void;

  /**
   * Which source a completion came from, as the WIT's `completion-source`
   * enum: the Run dialog's history (`"history"`), a program App Paths
   * registered (`"app-path"`), a program on the registry search path
   * (`"search-path"`), a Control Panel applet (`"applet"`), a management
   * console (`"console"`), a registered scheme (`"scheme"`), or an
   * environment variable (`"variable"`).
   */
  export type CompletionSource =
    | "history"
    | "app-path"
    | "search-path"
    | "applet"
    | "console"
    | "scheme"
    | "variable";

  /**
   * One completion for the text typed in Run's field, as the WIT's
   * `completion` record: the command line the text completes to, `line`,
   * which the Run dialog runs as it is, and which source it came from.
   */
  export interface Completion {
    line: string;
    source: CompletionSource;
  }

  /**
   * The completions for `text`, the text typed so far, in the order they
   * are offered: the Run dialog's history first, then programs from App
   * Paths and the registry search path, Control Panel applets,
   * management consoles, registered schemes and environment variables —
   * each a line that starts with the typed text, ignoring case, at most
   * 100. On failure it throws an object whose `payload` is the
   * {@link RunError}.
   */
  export function completions(text: string): Completion[];

  /**
   * Windows Terminal, when it is installed: `wt.exe`'s absolute path, as
   * App Paths registered it and then the registry search path spell it;
   * `null` when it is not installed, so the caller runs the command line
   * another way. On failure it throws an object whose `payload` is the
   * {@link RunError}.
   */
  export function terminal(): string | null;
}
