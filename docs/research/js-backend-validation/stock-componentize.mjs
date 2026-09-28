// Negative control: stock componentize-qjs 0.4.5 from npm, its own bundled runtime.
// Usage: node stock-componentize.mjs <node_modules dir> <wit dir> <world> <in.mjs> <out.wasm>
import { readFile, writeFile } from "node:fs/promises";
import { createRequire } from "node:module";
import { join } from "node:path";

const [nodeModules, witPath, world, jsPath, out] = process.argv.slice(2);
const require = createRequire(join(nodeModules, "noop.js"));
const { componentize } = require("componentize-qjs");
const start = performance.now();
const result = await componentize({
  witPath, world, jsPath, jsSource: await readFile(jsPath, "utf8"), sync: false,
});
await writeFile(out, result.component);
console.log(JSON.stringify({ component_bytes: result.component.length, componentize_ms: Math.round(performance.now() - start) }));
