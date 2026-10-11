// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Declarations for `@pane-app/extension/capabilities` (capabilities.js):
// calling a capability by name (ADR 0041), fanning a call out to every
// provider of a use that says "use": "all", and asking which providers
// can serve one now.

import type { Provider, ProviderAnswer } from "pane:extension/operations@0.1.0";

export type { Provider, ProviderAnswer };

export {
  /**
   * Calls `operation` of the capability `capability`, such as
   * "acme:translate@1", and resolves with its result. Any installed
   * package may provide the capability: Pane routes the call to the
   * provider that can serve it — the first one installed, until the user
   * chooses one in Settings — and starts it if it is not running, never the
   * calling package itself. The caller's pane.json must declare the
   * capability and the operation under `uses`; a call to one it does not
   * declare is `refused`, with the message saying to declare it. `input`
   * and the result are JSON text, and the call is as a call by identity is.
   *
   * On failure the promise rejects with an object whose `payload` is the
   * `CallError`; each message names the capability.
   * @param {string} capability
   * @param {string} operation
   * @param {string} input
   * @returns {Promise<string>}
   */
  callCapability as call,
  /**
   * Calls `operation` of the capability `capability` on every provider that
   * can serve it now, and resolves with each one's answer, with its source,
   * its title and its result or error, so the caller can merge them. The
   * caller's pane.json must declare the capability under `uses` with
   * "use": "all"; fanning out a "use": "one" capability is `refused`. With
   * no provider that can serve, the answer is an empty list.
   * @param {string} capability
   * @param {string} operation
   * @param {string} input
   * @returns {Promise<import("pane:extension/operations@0.1.0").ProviderAnswer[]>}
   */
  callEvery,
  /**
   * The installed packages that provide the capability `capability` and
   * can serve a call to it now, in the order Pane calls them; the calling
   * package is never among them. With no such provider the list is empty.
   * @param {string} capability
   * @returns {Provider[]}
   */
  providers,
} from "pane:extension/operations@0.1.0";

/**
 * The provider of `capability` that a call reaches, with its `source` and
 * `title`, or undefined when no provider can serve it now: whether an
 * optional capability is worth showing, answered in one call.
 * @param {string} capability
 * @returns {Provider | undefined}
 */
export function available(capability: string): Provider | undefined;
