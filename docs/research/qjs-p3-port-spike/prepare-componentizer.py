"""Apply only the recorded scratch source edits; upstream checkout is preserved."""
from pathlib import Path
import json, shutil

HERE=Path(__file__).resolve().parent
meta=json.loads((HERE.parent/'p3-native-toolchain-local.json').read_text())
root=Path(meta['root']); repo=root/'qjs-port'
p=repo/'crates/core/src/lib.rs'; s=p.read_text()
needle='    linker.library("wit-dylib.wasm", &wit_dylib, false)?;'
replacement=needle+'''
    // Probe: SDK 34's runtime depends on its actual P3 libc shared module.
    let libc = std::fs::read(std::env::var("QJS_P3_LIBC")?)?;
    linker.library("libc.so", &libc, false)?;'''
if 'QJS_P3_LIBC' not in s:
    assert needle in s
    s=s.replace(needle,replacement)
    s=s.replace('use wasi_preview1_component_adapter_provider::WASI_SNAPSHOT_PREVIEW1_REACTOR_ADAPTER;\n','')
    s=s.replace('''    linker.encoder().adapter(
        "wasi_snapshot_preview1",
        WASI_SNAPSHOT_PREVIEW1_REACTOR_ADAPTER,
    )?;
''','')
    s=s.replace('    wasmtime_wasi::p2::add_to_linker_async(&mut linker)?;\n','')
    p.write_text(s)
prebuilt=repo/'crates/core/prebuilt';prebuilt.mkdir(exist_ok=True)
runtime=root/'qjs-runtime-target/wasm32-wasip3/release/componentize_qjs_runtime.wasm'
# Build script requires all variants. This probe only uses Runtime::Custom;
# these copies satisfy the build script, not a claim of distinct optimized builds.
for name in ['runtime.wasm','runtime-opt-size.wasm','runtime-sync.wasm','runtime-opt-size-sync.wasm']:
    shutil.copyfile(runtime,prebuilt/name)
examples=repo/'crates/core/examples';examples.mkdir(exist_ok=True)
shutil.copyfile(HERE/'p3_build.rs',examples/'p3_build.rs')
print(repo)
