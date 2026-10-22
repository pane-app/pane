"""Type-check, bundle and componentize the JS/TS guests; build the stock negative controls.

Usage: python componentize.py [--variant NAME]   (default variant: "fixed")
Artifacts go to $PANE_SCRATCH/out/<variant>/. A small summary is written to
componentize-<variant>.json next to this script.
"""
import json
import os
import shutil
import sys

from common import COMPONENTIZER, HERE, NODE_DIR, OUT, REPO, RUNTIME_WASM, SCRATCH, libc_so, run, sha256, write_json

variant = sys.argv[sys.argv.index("--variant") + 1] if "--variant" in sys.argv else "fixed"
out = OUT / variant
out.mkdir(parents=True, exist_ok=True)
node = shutil.which("node") or "node"
modules = NODE_DIR / "node_modules"
guests = HERE / "guests"
report = {"variant": variant, "runtime_sha256": sha256(RUNTIME_WASM), "artifacts": {}}

# Type-check the TS Pane sample against hand-written WASI declarations.
run([node, modules / "typescript/bin/tsc", "--noEmit", "--strict", "--target", "es2022", "--module", "esnext",
     "--moduleResolution", "bundler", guests / "sample.ts", guests / "wasi.d.ts"])

bundles = {"capabilities": guests / "capabilities.ts", "sample-ts": guests / "sample.ts",
           "search": HERE.parent / "wasi03-js-spike/search.ts"}
for name, entry in bundles.items():
    run([node, HERE / "bundle.mjs", modules, entry, out / f"{name}.mjs"])

# Author-side Pane WIT: our world plus an unmodified copy of the product contract.
pane_wit = out / "pane-wit"
shutil.rmtree(pane_wit, ignore_errors=True)
(pane_wit / "deps/pane-extension").mkdir(parents=True)
shutil.copyfile(guests / "pane-world.wit", pane_wit / "world.wit")
shutil.copyfile(REPO / "wit/extension.wit", pane_wit / "deps/pane-extension/extension.wit")
shutil.copytree(HERE.parent / "qjs-p3-port-spike/wit/deps/wasi-clocks-0.3.0", pane_wit / "deps/wasi-clocks-0.3.0")

capability_wit = HERE.parent / "qjs-p3-port-spike/wit"
p3_jobs = [
    ("capabilities", capability_wit, "search-extension", out / "capabilities.mjs"),
    # Same source and WIT as stock-search, for a like-for-like size/cost comparison.
    ("search", HERE.parent / "wasi03-js-spike/wit", "search-extension", out / "search.mjs"),
    ("sample-js", pane_wit, "js-extension", guests / "sample.js"),
    ("sample-ts", pane_wit, "js-extension", out / "sample-ts.mjs"),
]
env = {**os.environ, "QJS_P3_LIBC": str(libc_so())}
for name, wit, world, js in p3_jobs:
    wasm = out / f"p3-{name}.wasm"
    p = run([COMPONENTIZER, wit, world, js, RUNTIME_WASM, wasm], env=env)
    report["artifacts"][f"p3-{name}"] = {**json.loads(p.stdout.strip().splitlines()[-1]), "sha256": sha256(wasm)}

stock_jobs = [
    ("search", HERE.parent / "wasi03-js-spike/wit", "search-extension", out / "search.mjs"),
    ("sample-ts", pane_wit, "js-extension", out / "sample-ts.mjs"),
]
for name, wit, world, js in stock_jobs:
    wasm = out / f"stock-{name}.wasm"
    p = run([node, HERE / "stock-componentize.mjs", modules, wit, world, js, wasm])
    report["artifacts"][f"stock-{name}"] = {**json.loads(p.stdout.strip().splitlines()[-1]), "sha256": sha256(wasm)}

text = json.dumps(report).replace(str(SCRATCH), "$PANE_SCRATCH")
write_json(f"componentize-{variant}.json", json.loads(text))
print(json.dumps(report, indent=1))
