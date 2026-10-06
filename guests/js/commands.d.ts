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
    /** Its arguments' values by name; commands declare none yet, so it is empty. */
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
   * and asking nothing. `"user-initiated"` opens it as if the user had
   * invoked it; `"background"` runs a no-view command without a window and
   * is refused for a view command. Returns once the launch has started, not
   * when the target has run. A refusal (the target is not installed, has no
   * such command, is disabled, paused or unavailable here, or the context
   * is not JSON) throws an object whose `payload` is the reason.
   */
  export function launch(
    target: CommandRef,
    launchType: LaunchType,
    arguments_: ArgumentValue[],
    context?: string | null,
  ): void;
}
