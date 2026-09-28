// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Bundles one JS/TS entry and its npm dependencies into a single ES module
// for componentize-qjs. `wasi:` imports stay external: the component's world
// provides them.
//
// Usage: node bundle.mjs <tool node_modules> <entry> <out.mjs>
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
  external: ["wasi:*"],
  mainFields: ["module", "main"],
  logLevel: "warning",
});
