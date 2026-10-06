// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Declarations for `pane:extension/commands` in wit/commands.wit: the
// launch record every command receives, and launching another command.

/** `pane:extension/commands@0.1.0`. */
declare module "pane:extension/commands@0.1.0" {
  /**
   * Whether the user launched the command, or Pane did in the background (a
   * schedule, or another command's background launch), without a window.
   */
  export type LaunchType = "user-initiated" | "background";

  /** Where a launch came from. */
  export type LaunchSource =
    | "root-search"
    | "alias"
    | "fallback"
    | "hotkey"
    | "quick-slot"
    | "command"
    | "schedule";

  /** The value of one of the command's arguments, by its name. */
  export interface ArgumentValue {
    name: string;
    value: string;
  }

  /** What a command receives on every way in: how it was launched, and with what. */
  export interface LaunchRecord {
    launchType: LaunchType;
    source: LaunchSource;
    /**
     * Its arguments' values by name (`"arguments"` in its `pane.json`
     * entry), in the order it declares them; an optional argument left
     * empty is absent.
     */
    arguments: ArgumentValue[];
    /**
     * The text sent through the command's alias or to it as a fallback,
     * trimmed and never empty; absent otherwise.
     */
    fallbackText?: string | null;
    /** The JSON value, as text, another command passed when it launched this one. */
    context?: string | null;
  }

  /**
   * A command to launch by its id in its package's `pane.json`: the
   * caller's own package's when `source` is omitted, else the installed
   * package whose identity `source` is (`local:` and the absolute folder it
   * was installed from, `npm:` and its npm package name, or `git:` and its
   * repository).
   */
  export interface CommandRef {
    source?: string | null;
    command: string;
  }

  /**
   * Launches `target`, passing `context` (JSON text) in its launch record
   * and asking nothing, with `arguments_` the values of its arguments by
   * name. `"user-initiated"` opens it as if the user had invoked it (Pane's
   * argument form asks for a required argument left without a value);
   * `"background"` runs a no-view command without a window and is refused
   * for a view command or a required argument without a value. Returns
   * once the launch has started, not when the target has run. A refusal
   * (the target is not installed, has no such command, is disabled, paused
   * or unavailable here, an argument is not its own or a dropdown's value
   * not among its options, or the context is not JSON) throws an object
   * whose `payload` is the reason.
   */
  export function launch(
    target: CommandRef,
    launchType: LaunchType,
    arguments_: ArgumentValue[],
    context?: string | null,
  ): void;

  /**
   * Replaces the subtitle the calling command's row shows in root search
   * (and matches), until it is set again; `null` gives back the one its
   * `pane.json` entry declares. Pane keeps it across restarts and updates,
   * and forgets it on uninstall. A refusal (a call Pane does not know the
   * command of, such as an operation's) throws an object whose `payload`
   * is the reason. `@pane/extension/feedback`'s `setSubtitle` calls it.
   */
  export function setSubtitle(subtitle?: string | null): void;
}
