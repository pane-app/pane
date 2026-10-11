// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Declarations for `pane:extension/operations` in wit/operations.wit:
// calling an operation another installed package publishes, and calling a
// capability by its name (ADR 0041), through Pane.

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

  /**
   * An installed package that provides a capability, as `providers`
   * answers it: the package's `source`, as `call` names it by, and its
   * title.
   */
  export interface Provider {
    source: string;
    title: string;
  }

  /**
   * One provider's answer to `callEvery`: the provider's `source`, as
   * `call` names it by, its title, and what serving the call answered.
   */
  export interface ProviderAnswer {
    provider: string;
    title: string;
    answer: { tag: "ok"; val: string } | { tag: "err"; val: CallError };
  }

  /**
   * Calls `operation` of the capability `capability`, such as
   * "acme:translate@1", and resolves with its result. Any installed
   * package may provide the capability: Pane routes the call to the
   * provider that can serve it — the first one installed, until the user
   * chooses one in Settings — and starts it if it is not running, never the
   * calling package itself. The caller's pane.json must declare the
   * capability and the operation under `uses`; a call to one it does not
   * declare is `refused`, with the message saying to declare it. The
   * provider serves the call through the entry point that serves its
   * published operations, with the operation qualified by its capability
   * ("acme:translate@1/translate"). `input` and the result are JSON text,
   * and the call is as a call by identity is.
   *
   * On failure the promise rejects with an object whose `payload` is the
   * {@link CallError}; each message names the capability.
   */
  export function callCapability(
    capability: string,
    operation: string,
    input: string,
  ): Promise<string>;

  /**
   * Calls `operation` of the capability `capability` on every provider
   * that can serve it now — enabled, not paused, not waiting for what it
   * needs, and built for this system — and resolves with each one's answer,
   * with its source, its title and its result or error, so the caller can
   * merge them. Each provider is its own call in the chain, with the same
   * input; the providers that cannot serve are skipped, and with none the
   * answer is an empty list, not an error: a use of every provider never
   * makes a command wait for it.
   *
   * The caller's pane.json must declare the capability under `uses` with
   * "use": "all": fanning out a "use": "one" capability is `refused`, as
   * is one the caller does not declare. On failure the promise rejects
   * with an object whose `payload` is the {@link CallError}.
   * @param {string} capability
   * @param {string} operation
   * @param {string} input
   * @returns {Promise<ProviderAnswer[]>}
   */
  export function callEvery(
    capability: string,
    operation: string,
    input: string,
  ): Promise<ProviderAnswer[]>;

  /**
   * The installed packages that provide the capability `capability` and
   * can serve a call to it now — enabled, not paused, not waiting for what
   * they need, and built for this system — in the order Pane calls them.
   * The calling package is never among them. With no such provider the
   * list is empty.
   */
  export function providers(capability: string): Provider[];
}
