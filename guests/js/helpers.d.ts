// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Declarations for `pane:extension/helpers` in wit/helpers.wit: running a
// native helper, a prebuilt program the command's own package ships for
// each system (under `helpers` in its pane.json), through Pane.

/** `pane:extension/helpers@0.1.0`. */
declare module "pane:extension/helpers@0.1.0" {
  /**
   * Why a helper run did not produce the helper's output.
   *
   * - `not-found`: the package's pane.json declares no helper by that name.
   * - `unavailable`: it cannot run on this system (no file for this system
   *   and processor, a missing file, a program for another system, or the
   *   system would not start it).
   * - `failed`: it exited unsuccessfully (the message holds its exit status
   *   and the end of its standard error), or its output is not UTF-8 text.
   * - `refused`: Pane refused or stopped the run (the command's code was
   *   stopped, Pane is quitting, or the input, arguments or output are over
   *   Pane's limits).
   */
  export type HelperErrorKind = "not-found" | "unavailable" | "failed" | "refused";

  /** `message` explains the failure for people. */
  export interface HelperError {
    kind: HelperErrorKind;
    message: string;
  }

  /**
   * Runs the package's helper `helper` with `args`, writes `input` to its
   * standard input, and resolves with its standard output once it exits
   * successfully. Pane ends the helper's process when the Pane call that
   * started it returns and when the package's code stops.
   *
   * On failure the promise rejects with an object whose `payload` is the
   * {@link HelperError}.
   */
  export function run(helper: string, args: string[], input: string): Promise<string>;
}
