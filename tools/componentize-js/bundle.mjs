// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Bundles one JS/TS entry and its npm dependencies into a single ES module
// for componentize-qjs. `wasi:` and `pane:` imports stay external: the
// component's world provides them.
//
// Usage: node bundle.mjs <tool node_modules> <entry> <out.mjs>
//
// A source map is written beside the bundle (`<out.mjs>.map`): Pane's
// development builds keep it beside the component, and the extension log's
// stack traces map back to the sources with it (#214). It holds positions
// only — esbuild's default sources content is left out, so the map stays
// small — and `pane_js.py build` copies it next to the component it hands
// to the componentizer.
import { createRequire } from "node:module";
import { join } from "node:path";

const [toolModules, entry, outfile] = process.argv.slice(2);
const require = createRequire(join(toolModules, "noop.js"));
const { build } = require("esbuild");
await build({
  entryPoints: [entry],
  outfile,
  bundle: true,
  format: "esm",
  platform: "neutral",
  target: "es2020",
  external: ["wasi:*", "pane:*"],
  mainFields: ["module", "main"],
  logLevel: "warning",
  sourcemap: "external",
  sourcesContent: false,
});
