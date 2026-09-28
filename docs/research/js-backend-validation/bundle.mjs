// Bundle one TS/JS entry into a single ES module for componentize-qjs.
// Usage: node bundle.mjs <node_modules dir> <entry> <out.mjs>
import { createRequire } from "node:module";
import { join } from "node:path";

const [nodeModules, entry, outfile] = process.argv.slice(2);
const require = createRequire(join(nodeModules, "noop.js"));
const { build } = require("esbuild");
await build({
  entryPoints: [entry], outfile, nodePaths: [nodeModules],
  bundle: true, format: "esm", platform: "neutral", target: "es2020",
  external: ["wasi:*"], logLevel: "warning",
});
