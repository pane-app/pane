"""Compile the temporary QuickJS runtime port; requires the recorded scratch source."""
from pathlib import Path
import subprocess, os, json, time

HERE = Path(__file__).resolve().parent
meta = json.loads((HERE.parent / 'p3-native-toolchain-local.json').read_text())
sdk, root = Path(meta['sdk']), Path(meta['root'])
env = {**os.environ,
    'RUSTUP_HOME': str(root/'rustup'),
    'CARGO_TARGET_DIR': str(root/'qjs-runtime-target'),
    'PATH': str(sdk/'bin')+os.pathsep+os.environ['PATH'],
    'CARGO_TARGET_WASM32_WASIP3_LINKER': str(sdk/'bin/clang.exe'),
    'CARGO_TARGET_WASM32_WASIP3_RUSTFLAGS': ' '.join([
        '-Crelocation-model=pic', '-Clink-arg=--target=wasm32-wasip3',
        '-Clink-arg=-shared', '-Clink-arg=-Wl,--no-entry', '-Clink-arg=-Wl,--allow-undefined',
        '-Clink-arg=-Wl,--export=__wasm_library_tls_info',
        '-L', 'native='+str(sdk/'share/wasi-sysroot/lib/wasm32-wasip3')]),
    'WASI_SDK': str(sdk), 'WASI_SDK_PATH': str(sdk), 'LIBCLANG_PATH': str(sdk/'bin'),
    'CC_wasm32_wasip3': str(sdk/'bin/clang.exe'),
    'CFLAGS_wasm32_wasip3': '--target=wasm32-wasip3 -fPIC -Oz',
}
cmd = ['rustup','run','nightly-2026-09-27','cargo','build','--release','--target','wasm32-wasip3',
       '-Zbuild-std=std,panic_abort','--manifest-path',str(root/'qjs-port/Cargo.toml'),
       '-p','componentize-qjs-runtime']
start=time.monotonic()
with (HERE/'build-runtime.log').open('w') as log:
    p=subprocess.run(cmd,env=env,stdout=log,stderr=subprocess.STDOUT)
report={'exit_code':p.returncode,'seconds':time.monotonic()-start,'command':cmd}
(HERE/'build-runtime-result.json').write_text(json.dumps(report,indent=2))
print(json.dumps(report)); print((HERE/'build-runtime.log').read_text()[-8000:])
raise SystemExit(p.returncode)
