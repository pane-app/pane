import { readFile, writeFile } from 'node:fs/promises';
import { resolve, join, dirname } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
const here = dirname(fileURLToPath(import.meta.url));
const meta = JSON.parse(await readFile(resolve(here, '../p3-native-toolchain-local.json'), 'utf8'));
const jsMeta = JSON.parse(await readFile(resolve(here, '../wasi03-js-local.json'), 'utf8'));
const old = JSON.parse(await readFile(resolve(here, '../wasm-primary-local.json'), 'utf8'));
const { componentize } = await import(pathToFileURL(join(old.root, 'node_modules/componentize-qjs/index.js')));
const jsPath = join(jsMeta.root, 'search.mjs');
const runtime = join(meta.root, 'qjs-runtime-target/wasm32-wasip3/release/componentize_qjs_runtime.wasm');
const start = performance.now();
try {
  const result = await componentize({
    witPath: resolve(here, '../wasi03-js-spike/wit'), world: 'search-extension',
    jsPath, jsSource: await readFile(jsPath, 'utf8'), runtime, sync: false,
  });
  const output = join(meta.root, 'qjs-p3-search.wasm');
  await writeFile(output, result.component);
  const report = { pass: true, milliseconds: performance.now()-start,
    componentBytes: result.component.length, output };
  await writeFile(join(here, 'componentize-result.json'), JSON.stringify(report, null, 2));
  console.log(JSON.stringify(report));
} catch (error) {
  const report = { pass: false, milliseconds: performance.now()-start, error: String(error) };
  await writeFile(join(here, 'componentize-result.json'), JSON.stringify(report, null, 2));
  console.error(report);
  process.exitCode = 1;
}
