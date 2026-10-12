// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// The JSX runtime's types (jsx-runtime.js): `jsxImportSource:
// "@pane-app/extension"` in a tsconfig.json makes the TypeScript and
// esbuild transforms of `.tsx` import from here.

import type { Element } from "./view.js";

/** One JSX element: its type, its properties and its key. */
export declare function jsx(type: unknown, props: unknown, key?: string): Element;

/** One JSX element with several children, as `jsx` (the transform picks). */
export declare const jsxs: typeof jsx;

/** A fragment: its children are drawn where it sits, unwrapped. */
export { Fragment } from "./view.js";

/** What the transform checks JSX against (view.js's elements). */
export namespace JSX {
  export type Element = import("./view.js").Element;
}
