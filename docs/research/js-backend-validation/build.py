"""Build the P3 QuickJS runtime, the scratch componentizer, the P3-only probe host and Pane's run_guest example.

Usage: python build.py [runtime] [componentizer] [host] [pane]   (default: all four)
Environment is passed per process; nothing global is changed. Logs go to the
scratch root; a small timing summary is written to build-result.json.
"""
import json
import os
import shutil
import sys
import time

from common import (COMPONENTIZER_TARGET, HERE, HOST_TARGET, NIGHTLY, QJS_PORT, RUNTIME_TARGET,
                    RUNTIME_WASM, SCRATCH, EXE, run, sdk_dir, sha256, write_json)

steps = sys.argv[1:] or ["runtime", "componentizer", "host", "pane"]
sdk = sdk_dir()
sysroot_lib = sdk / "share/wasi-sysroot/lib/wasm32-wasip3"
report = json.loads((HERE / "build-result.json").read_text()) if (HERE / "build-result.json").exists() else {}


def timed(name, cmd, env):
    start = time.monotonic()
    run(cmd, env=env, log=SCRATCH / f"build-{name}.log")
    report[name] = {"seconds": round(time.monotonic() - start, 1),
                    "command": [str(c) for c in cmd]}


if "runtime" in steps:
    clang = str(sdk / "bin" / f"clang{EXE}")
    env = {**os.environ,
           "CARGO_TARGET_DIR": str(RUNTIME_TARGET),
           "PATH": str(sdk / "bin") + os.pathsep + os.environ["PATH"],
           "CARGO_TARGET_WASM32_WASIP3_LINKER": clang,
           "CARGO_TARGET_WASM32_WASIP3_RUSTFLAGS": " ".join([
               "-Crelocation-model=pic", "-Clink-arg=--target=wasm32-wasip3",
               "-Clink-arg=-shared", "-Clink-arg=-Wl,--no-entry", "-Clink-arg=-Wl,--allow-undefined",
               "-Clink-arg=-Wl,--export=__wasm_library_tls_info",
               "-L", "native=" + str(sysroot_lib)]),
           "WASI_SDK": str(sdk), "WASI_SDK_PATH": str(sdk),
           # rquickjs' bindgen feature loads the SDK's libclang (bin/ on Windows, lib/ elsewhere).
           "LIBCLANG_PATH": str(sdk / ("bin" if os.name == "nt" else "lib")),
           "CC_wasm32_wasip3": clang,
           "CFLAGS_wasm32_wasip3": "--target=wasm32-wasip3 -fPIC -Oz"}
    timed("runtime", ["rustup", "run", NIGHTLY, "cargo", "build", "--release", "--target", "wasm32-wasip3",
                      "-Zbuild-std=std,panic_abort", "--manifest-path", QJS_PORT / "Cargo.toml",
                      "-p", "componentize-qjs-runtime"], env)
    report["runtime"].update(bytes=RUNTIME_WASM.stat().st_size, sha256=sha256(RUNTIME_WASM))

if "componentizer" in steps:
    # The core crate's build script insists on four prebuilt runtimes. The probe
    # always passes Runtime::Custom, so these copies only satisfy the build
    # script; they are not distinct size/sync variants.
    prebuilt = QJS_PORT / "crates/core/prebuilt"
    prebuilt.mkdir(exist_ok=True)
    for name in ["runtime.wasm", "runtime-opt-size.wasm", "runtime-sync.wasm", "runtime-opt-size-sync.wasm"]:
        shutil.copyfile(RUNTIME_WASM, prebuilt / name)
    env = {**os.environ, "CARGO_TARGET_DIR": str(COMPONENTIZER_TARGET)}
    timed("componentizer", ["cargo", "build", "--release", "--locked", "--manifest-path", QJS_PORT / "Cargo.toml",
                            "-p", "componentize-qjs", "--example", "p3_build"], env)

if "host" in steps:
    env = {**os.environ, "CARGO_TARGET_DIR": str(HOST_TARGET)}
    timed("host", ["cargo", "build", "--release", "--manifest-path",
                   HERE.parent / "p3-only-host/Cargo.toml"], env)

if "pane" in steps:
    # Pane's real runtime (pane-core) driven by its run_guest example; repo toolchain and target dir.
    timed("pane", ["cargo", "build", "--release", "--locked", "-p", "pane-core", "--example", "run_guest"],
          {**os.environ})
    # The Rust sample is the size/cost baseline in measure.py.
    run(["cargo", "xtask", "guests"], cwd=HERE.parents[2], log=SCRATCH / "build-xtask-guests.log")

report["nightly"] = run(["rustup", "run", NIGHTLY, "rustc", "-V"]).stdout.strip()
report["stable"] = run(["cargo", "-V"]).stdout.strip()
# Scratch-absolute paths are machine-specific; keep them relative to the scratch root.
text = json.dumps(report).replace(str(SCRATCH), "$PANE_SCRATCH").replace(str(HERE.parents[2]), "$REPO")
write_json("build-result.json", json.loads(text))
print(json.dumps({k: v.get("seconds") if isinstance(v, dict) else v for k, v in report.items()}))
