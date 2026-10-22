// Developer tooling lives in the temporary directory passed as argv[2].
import { readFile, writeFile } from "node:fs/promises";
import { join, dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { performance } from "node:perf_hooks";

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(process.argv[2]);
const backend = process.argv[3];
const esbuild = await import(pathToFileURL(join(root, "node_modules/esbuild/lib/main.js")));
const sourcePath = join(root, "search.mjs");
await esbuild.build({
  entryPoints: [join(here, "search.ts")], outfile: sourcePath,
  bundle: true, format: "esm", platform: "neutral", target: "es2020",
});
const witPath = resolve(here, "../wasm-spike/wit");
const start = performance.now();
let result;
if (backend === "qjs") {
  const api = await import(pathToFileURL(join(root, "node_modules/componentize-qjs/index.js")));
  result = await api.componentize({
    witPath, world: "search-extension", jsPath: sourcePath,
    jsSource: await readFile(sourcePath, "utf8"), optSize: true, sync: true,
  });
} else if (backend === "spidermonkey") {
  const api = await import(pathToFileURL(join(root, "node_modules/@bytecodealliance/componentize-js/src/componentize.js")));
  result = await api.componentize({
    sourcePath, witPath, worldName: "search-extension",
    disableFeatures: ["stdio", "random", "clocks", "http", "fetch-event"],
  });
} else {
  throw new Error("Specify qjs or spidermonkey");
}
const elapsedMs = performance.now() - start;
await writeFile(join(root, `${backend}.wasm`), result.component);
const report = { backend, componentBytes: result.component.byteLength, componentizeMs: elapsedMs };
await writeFile(join(root, `${backend}-build.json`), JSON.stringify(report, null, 2));
console.log(JSON.stringify(report));
