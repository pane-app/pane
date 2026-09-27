// Temporary author tools; no launcher SDK or production host is implemented here.
import { readFile, writeFile } from "node:fs/promises";
import { resolve, join, dirname } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
const here = dirname(fileURLToPath(import.meta.url));
const meta = JSON.parse(await readFile(resolve(here, "../wasi03-js-local.json"), "utf8"));
const old = JSON.parse(await readFile(resolve(here, "../wasm-primary-local.json"), "utf8"));
const { build } = await import(pathToFileURL(join(old.root, "node_modules/esbuild/lib/main.js")));
const { componentize } = await import(pathToFileURL(join(old.root, "node_modules/componentize-qjs/index.js")));
const jsPath = join(meta.root, "search.mjs");
await build({
  entryPoints: [join(here, "search.ts")], outfile: jsPath,
  nodePaths: [join(meta.root, "node_modules")],
  bundle: true, format: "esm", platform: "neutral", target: "es2020",
  external: ["wasi:*"],
});
const start = performance.now();
const result = await componentize({
  witPath: join(here, "wit"), world: "search-extension", jsPath,
  jsSource: await readFile(jsPath, "utf8"), optSize: true, sync: false,
});
const out = join(meta.root, "search.wasm");
await writeFile(out, result.component);
const report = { componentBytes: result.component.length, componentizeMs: performance.now()-start, path: out, sync: false };
await writeFile(join(here, "build-result.json"), JSON.stringify(report, null, 2));
console.log(JSON.stringify(report));
