// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Declarations for `@pane-app/extension/state` (state.js): the state
// handoff's serialization helpers (ADR 0041). A command that opts in with
// `"pane": { "snapshot": true }` in its package.json exports
// `lifecycle = { activate, snapshot, restore }`; `save` writes the value
// `snapshot` answers and `load` reads what `restore` receives. The bytes
// are the author's to version: JSON here, so a `restore` that cannot read
// what an older release wrote should throw, and the extension starts
// fresh.

/**
 * `value` as the bytes `lifecycle.snapshot` answers: JSON as UTF-8.
 * @param {unknown} value
 * @returns {Uint8Array}
 */
export function save(value: unknown): Uint8Array;

/**
 * The bytes `lifecycle.restore` receives, as the value `save` wrote for
 * it: JSON parsed. Throws when the bytes are not what this release wrote,
 * which discards the state and starts the extension fresh.
 * @param {Uint8Array} state
 * @returns {unknown}
 */
export function load(state: Uint8Array): unknown;

/**
 * The lifecycle entry points of a command that opts in to the state
 * handoff (ADR 0041): `activate`, beside which the package.json's
 * `"pane": { "snapshot": true }` links `snapshot` and `restore`. An
 * `activate` that does nothing and a `restore` that throws suit a command
 * that hands nothing over.
 */
export interface Lifecycle {
  /** The activation entry point, called only when the package's
   * `pane.json` declares `"activate"` for this component. */
  activate(): Promise<void>;
  /** The state this instance hands to its replacement: `undefined` for
   * nothing. */
  snapshot(): Promise<Uint8Array | undefined>;
  /** Restores the state a replaced instance handed over. Throwing
   * discards the state and starts fresh, which is not a failure. */
  restore(state: Uint8Array): Promise<void>;
}
