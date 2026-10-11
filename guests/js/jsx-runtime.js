// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// The JSX runtime: `jsxImportSource: "@pane-app/extension"` in a
// tsconfig.json makes the TypeScript and esbuild transforms of `.tsx`
// import `jsx` and `jsxs` from here, so an author writes the tree of a
// designed view (view.js) as JSX with every property type-checked. The
// elements are plain objects the tree runtime reads; React is not used.

export { Fragment } from "./view.js";

/** One JSX element: its type, its properties and its key. */
export function jsx(type, props, key) {
  return { type, props: props ?? {}, key };
}

/** One JSX element with several children, as `jsx` (the transform picks). */
export const jsxs = jsx;
