import { readFile } from 'node:fs/promises';
import { resolve, join, dirname } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
const here=dirname(fileURLToPath(import.meta.url));
const meta=JSON.parse(await readFile(resolve(here,'../p3-native-toolchain-local.json'),'utf8'));
const js=JSON.parse(await readFile(resolve(here,'../wasi03-js-local.json'),'utf8'));
const old=JSON.parse(await readFile(resolve(here,'../wasm-primary-local.json'),'utf8'));
const {build}=await import(pathToFileURL(join(old.root,'node_modules/esbuild/lib/main.js')));
await build({entryPoints:[join(here,'capabilities.ts')],outfile:join(meta.root,'capabilities.mjs'),
  nodePaths:[join(js.root,'node_modules')],bundle:true,format:'esm',platform:'neutral',
  target:'es2020',external:['wasi:*']});
