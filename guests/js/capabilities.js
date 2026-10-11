// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Calling a capability by name (`@pane-app/extension/capabilities`, ADR
// 0041): a named, versioned set of operations, written
// `<namespace>:<name>@<major>` such as "acme:translate@1", that any
// installed package may provide and this one calls without naming the
// package. The package's pane.json declares each capability it uses, with
// the operations it calls, under `uses`; a call to one it does not declare
// is refused. Bundled into the command that imports it, like any npm
// module. The underlying functions are also importable directly from
// "pane:extension/operations@0.1.0" as `callCapability`, `callEvery` and
// `providers`.

import { providers } from "pane:extension/operations@0.1.0";

export { callCapability as call, callEvery, providers } from "pane:extension/operations@0.1.0";

/**
 * The provider of `capability` that a call reaches, with its `source` and
 * `title`, or undefined when no provider can serve it now: whether an
 * optional capability is worth showing, answered in one call.
 * @param {string} capability
 * @returns {import("pane:extension/operations@0.1.0").Provider | undefined}
 */
export function available(capability) {
  return providers(capability)[0];
}
