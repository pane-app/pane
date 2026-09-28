// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Declarations for `pane:extension/operations` in wit/operations.wit:
// calling an operation another installed package publishes, through Pane.

/** `pane:extension/operations@0.1.0`. */
declare module "pane:extension/operations@0.1.0" {
  /**
   * Why a call did not produce the operation's result: a lifecycle failure,
   * or `failed`, the operation's own error.
   *
   * - `not-found`: no installed package has that source, or it does not
   *   publish that operation.
   * - `disabled`: the package is disabled; Pane does not enable it.
   * - `incompatible`: it publishes the operation at another version, or Pane
   *   cannot run its component.
   * - `unavailable`: it cannot run here (another system, or no runtime).
   * - `failed`: the operation ran and reported this error.
   * - `crashed`: it crashed serving the call; the next call starts it afresh.
   * - `refused`: the call would reach a package already serving a call in
   *   the same chain, the chain is too deep, or the input or result is not
   *   JSON within Pane's limits.
   */
  export type CallErrorKind =
    | "not-found"
    | "disabled"
    | "incompatible"
    | "unavailable"
    | "failed"
    | "crashed"
    | "refused";

  /** `message` explains the failure for people. */
  export interface CallError {
    kind: CallErrorKind;
    message: string;
  }

  /**
   * Calls `operation` at `version` of the installed package with source
   * `source`, and resolves with its result. The package is started for the
   * call if it is not running. `source` is `local:` followed by the package
   * folder's path: absolute, or relative to the calling package's own source
   * folder, such as `local:../other`. `input` and the result are JSON text.
   *
   * On failure the promise rejects with an object whose `payload` is the
   * {@link CallError}.
   */
  export function call(
    source: string,
    operation: string,
    version: number,
    input: string,
  ): Promise<string>;
}
