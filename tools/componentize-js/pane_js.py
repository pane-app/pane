#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0 OR MIT
"""Build the wasm parts and the per-platform componentizer of Pane's JS/TS builds.

Usage:
  pane_js.py wasm-parts <dir>     build runtime.wasm and copy wasi-sdk's P3 libc.so
                                   into <dir>, what built them and their digests
                                   in wasm-parts.json
  pane_js.py componentizer <dir>  build this host's componentizer into <dir>
                                   against the wasm parts there
  pane_js.py check-parts <dir>    verify the committed wasm parts against a
                                   fresh build in <dir>

The componentizer is the vendored componentize-qjs
(tools/componentize-js/componentize-qjs) at the commit pins.json names, with
the patch queue in patches/ applied in its source: nothing is downloaded or
patched here. Its `crates/core` is a member of the repository's workspace,
built with the repository's stable Rust; `componentizer` builds its
`p3_build` example, the standalone componentizer of the `p3_build <wit>
<world> <js> <runtime.wasm> <out.wasm>` contract that a development build
spawns when it does not link the componentizer in-process (#218).

Only `wasm-parts` needs the pinned nightly Rust and wasi-sdk: the runtime
crate of the vendored tree builds for wasm32-wasip3 against the SDK's P3
libc. The wasm parts are host-independent, built once (on Linux, by
componentizer.yml), recorded in wasm-parts.json, and committed
(tools/componentize-js/wasm-parts): the repository's JS/TS build embeds
those committed files, so nothing a package builds needs the nightly or the
SDK (#218). `componentizer.yml` runs `check-parts` to keep the committed
files from drifting from the source.

Building a JS/TS package is no longer this script's work: pane-build
(crates/pane-build) does it, with Node.js and npm alone, and `cargo xtask
js-guests` rebuilds the committed samples with it. When #219 retires
componentizer.yml, this script goes with it.

Prerequisites: Python 3.12+ and rustup, and for `wasm-parts` the wasi-sdk
download (recorded in pins.json). Node.js 22+ with npm is needed to build
packages, not to run these.
"""
from __future__ import annotations

import hashlib
import json
import os
import platform
import shlex
import shutil
import subprocess
import sys
import tarfile
import urllib.request
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
PINS = json.loads((HERE / "pins.json").read_text(encoding="utf-8"))
# The vendored componentize-qjs, patches applied in its source.
VENDORED = HERE / "componentize-qjs"
WASM_PARTS = HERE / "wasm-parts"
EXE = ".exe" if os.name == "nt" else ""


def cache_root() -> Path:
    configured = os.environ.get("PANE_JS_TOOLCHAIN_DIR")
    if configured:
        return Path(configured).resolve()
    if sys.platform == "win32":
        base = Path(os.environ.get("LOCALAPPDATA", Path.home() / "AppData" / "Local"))
    elif sys.platform == "darwin":
        base = Path.home() / "Library" / "Caches"
    else:
        base = Path(os.environ.get("XDG_CACHE_HOME", Path.home() / ".cache"))
    return base / "pane" / "componentize-js"


CACHE = cache_root()


def log(message: str) -> None:
    print(f"pane-js: {message}", flush=True)


def tool(name: str) -> str:
    found = shutil.which(name)
    if not found:
        raise SystemExit(f"pane-js: `{name}` was not found on PATH")
    return found


def run(cmd, *, cwd=None, env=None, capture=False) -> str:
    cmd = [str(part) for part in cmd]
    result = subprocess.run(cmd, cwd=cwd, env=env, text=True,
                            stdout=subprocess.PIPE if capture else None)
    if result.returncode:
        raise SystemExit(f"pane-js: {' '.join(cmd)} failed with exit code {result.returncode}")
    return result.stdout or ""


def clean_env(**extra: str) -> dict[str, str]:
    """The caller's environment without Cargo/rustup settings inherited from `cargo xtask`."""
    env = {key: value for key, value in os.environ.items()
           if not (key.startswith("CARGO_") and key != "CARGO_HOME")
           and key not in {"CARGO", "RUSTUP_TOOLCHAIN", "RUSTC", "RUSTDOC", "RUSTFLAGS"}}
    env.update(extra)
    return env


def sha256_file(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def host_platform() -> tuple[str, str]:
    """(arch, os) as wasi-sdk release assets spell them."""
    machine = platform.machine().lower()
    arch = {"amd64": "x86_64", "x86_64": "x86_64", "arm64": "arm64", "aarch64": "arm64"}.get(machine)
    system = {"linux": "linux", "darwin": "macos", "win32": "windows"}.get(sys.platform)
    if arch is None or system is None:
        raise SystemExit(f"pane-js: no wasi-sdk build for {sys.platform}/{machine}; "
                         "supported hosts are x86_64 and arm64 Linux, macOS and Windows")
    return arch, system


def download(url: str, path: Path, expected: str) -> None:
    if not path.exists() or sha256_file(path) != expected:
        log(f"downloading {url}")
        path.parent.mkdir(parents=True, exist_ok=True)
        partial = path.with_suffix(path.suffix + ".part")
        with urllib.request.urlopen(url) as response, partial.open("wb") as out:
            shutil.copyfileobj(response, out)
        partial.replace(path)
    actual = sha256_file(path)
    if actual != expected:
        raise SystemExit(f"pane-js: {path.name} has sha256 {actual}, expected {expected}")


def extract(archive: Path, into: Path) -> Path:
    """Extracts a tarball with one top-level directory into `into`; returns that directory."""
    into.mkdir(parents=True, exist_ok=True)
    with tarfile.open(archive) as tar:
        top = tar.getmembers()[0].name.split("/")[0]
        tar.extractall(into, filter="tar")
    return into / top


def rust_stable() -> str:
    """The repository's pinned stable toolchain, which builds the componentizer."""
    text = (REPO / "rust-toolchain.toml").read_text(encoding="utf-8")
    return text.split('channel = "', 1)[1].split('"', 1)[0]


def patch_digests() -> dict[str, str]:
    return {name: sha256_file(HERE / "patches" / name) for name in PINS["patches"]}


class Toolchain:
    def __init__(self) -> None:
        arch, system = host_platform()
        sdk = PINS["wasi_sdk"]
        self.sdk_name = f"wasi-sdk-{sdk['version']}-{arch}-{system}"
        self.sdk_digest = sdk["archive_sha256"][f"{arch}-{system}"]
        self.sdk = CACHE / self.sdk_name
        self.sdk_libc = self.sdk / "share" / "wasi-sysroot" / "lib" / "wasm32-wasip3" / "libc.so"

    def ensure_sdk(self) -> None:
        """wasi-sdk: its compiler builds the runtime; its P3 libc is linked into every component."""
        if self.sdk_libc.exists():
            return
        sdk = PINS["wasi_sdk"]
        archive = CACHE / "downloads" / f"{self.sdk_name}.tar.gz"
        download(f"https://github.com/WebAssembly/wasi-sdk/releases/download/{sdk['release']}/{self.sdk_name}.tar.gz",
                 archive, self.sdk_digest)
        extract(archive, CACHE)

    def build_runtime(self, out: Path) -> None:
        """The QuickJS runtime, for wasm32-wasip3 against the SDK's P3 libc,
        with the pinned nightly: the vendored runtime crate, copied to the
        cache so its lockfile and build artifacts stay out of the repository
        (it is excluded from the repository's workspace, so `--locked` has
        no lockfile to hold it to; what the runtime is built from is
        recorded by wasm-parts.json's digests instead)."""
        nightly = PINS["rust_nightly"]
        rustup = tool("rustup")
        run([rustup, "toolchain", "install", nightly, "--profile", "minimal", "--component", "rust-src"])
        scratch = CACHE / "src" / "runtime"
        shutil.rmtree(scratch, ignore_errors=True)
        shutil.copytree(VENDORED / "crates" / "runtime", scratch)
        clang = str(self.sdk / "bin" / f"clang{EXE}")
        sysroot_lib = self.sdk_libc.parent
        runtime_target = CACHE / "runtime-target"
        env = clean_env(
            CARGO_TARGET_DIR=str(runtime_target),
            PATH=str(self.sdk / "bin") + os.pathsep + os.environ["PATH"],
            CARGO_TARGET_WASM32_WASIP3_LINKER=clang,
            # Encoded (0x1f-separated) so a cache path containing spaces stays
            # one argument. With --target, these reach only the Wasm target.
            CARGO_ENCODED_RUSTFLAGS="\x1f".join([
                "-Crelocation-model=pic", "-Clink-arg=--target=wasm32-wasip3",
                "-Clink-arg=-shared", "-Clink-arg=-Wl,--no-entry", "-Clink-arg=-Wl,--allow-undefined",
                "-Clink-arg=-Wl,--export=__wasm_library_tls_info", "-L", f"native={sysroot_lib}"]),
            WASI_SDK=str(self.sdk), WASI_SDK_PATH=str(self.sdk),
            # rquickjs' bindgen loads the SDK's libclang (bin/ on Windows, lib/ elsewhere).
            LIBCLANG_PATH=str(self.sdk / ("bin" if os.name == "nt" else "lib")),
            CC_wasm32_wasip3=clang,
            AR_wasm32_wasip3=str(self.sdk / "bin" / f"llvm-ar{EXE}"),
            # rquickjs-sys would pass the sysroot through CFLAGS, which cc splits
            # on whitespace, so a cache path with spaces breaks it. The SDK's
            # clang finds its own sysroot; bindgen gets it shell-quoted instead.
            RQUICKJS_SYS_NO_WASI_SDK="1",
            BINDGEN_EXTRA_CLANG_ARGS_wasm32_wasip3=shlex.quote(f"--sysroot={self.sdk / 'share' / 'wasi-sysroot'}"),
            CFLAGS_wasm32_wasip3="--target=wasm32-wasip3 -fPIC -Oz",
        )
        run([rustup, "run", nightly, "cargo", "build", "--release", "--target", "wasm32-wasip3",
             "-Zbuild-std=std,panic_abort", "--manifest-path", scratch / "Cargo.toml"],
            env=env)
        out.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(runtime_target / "wasm32-wasip3" / "release" / "componentize_qjs_runtime.wasm", out)

    def build_componentizer(self, runtime: Path, out: Path) -> None:
        """This host's componentizer, with the stable Rust: the `p3_build`
        example of the vendored crate, a member of the repository's
        workspace, built in the repository (so the repository's lockfile and
        target folder apply)."""
        stable = rust_stable()
        rustup = tool("rustup")
        run([rustup, "toolchain", "install", stable, "--profile", "minimal"])
        run([rustup, "run", stable, "cargo", "build", "--release", "--locked",
             "--manifest-path", REPO / "Cargo.toml", "-p", "componentize-qjs",
             "--example", "p3_build"], cwd=REPO, env=clean_env())
        target = os.environ.get("CARGO_TARGET_DIR") or os.environ.get("CARGO_BUILD_TARGET_DIR") or "target"
        built = REPO / target / "release" / "examples" / f"p3_build{EXE}"
        out.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(built, out)
        out.chmod(0o755)

    def wasm_versions(self, runtime: Path, libc: Path) -> dict:
        """What the wasm parts were built from, and their digests."""
        qjs = PINS["componentize_qjs"]
        rustup = tool("rustup")
        return {
            "componentize_qjs": {k: qjs[k] for k in ["repository", "version", "commit"]},
            "patches": patch_digests(),
            "runtime_rustc": run([rustup, "run", PINS["rust_nightly"], "rustc", "-V"], capture=True).strip(),
            "wasi_sdk": PINS["wasi_sdk"]["version"],
            "runtime_sha256": sha256_file(runtime),
            "libc_sha256": sha256_file(libc),
        }

    def componentizer_versions(self) -> dict:
        """What this host's componentizer was built with, and where."""
        rustup = tool("rustup")
        return {
            "componentizer_rustc": run([rustup, "run", rust_stable(), "rustc", "-V"], capture=True).strip(),
            "built_on": "-".join(host_platform()),
        }


def wasm_parts(into: Path) -> None:
    """Builds the runtime and copies the SDK's P3 libc into `into`, with
    what they were built from and their digests in `wasm-parts.json`."""
    toolchain = Toolchain()
    toolchain.ensure_sdk()
    runtime, libc = into / "runtime.wasm", into / "libc.so"
    toolchain.build_runtime(runtime)
    shutil.copyfile(toolchain.sdk_libc, libc)
    record = toolchain.wasm_versions(runtime, libc)
    (into / "wasm-parts.json").write_text(json.dumps(record, indent=2) + "\n", encoding="utf-8")
    log(f"runtime.wasm {record['runtime_sha256']}, libc.so {record['libc_sha256']}")


def componentizer(into: Path) -> None:
    """Builds this host's componentizer into `into` against the wasm parts
    `wasm_parts` left there, and records the whole toolchain in
    `toolchain.json`, so that `into` can be a PANE_COMPONENTIZER folder."""
    record = json.loads((into / "wasm-parts.json").read_text(encoding="utf-8"))
    runtime, libc = into / "runtime.wasm", into / "libc.so"
    for path, key in [(runtime, "runtime_sha256"), (libc, "libc_sha256")]:
        if sha256_file(path) != record[key]:
            raise SystemExit(f"pane-js: {path} does not match its digest in wasm-parts.json")
    if (record["componentize_qjs"]["commit"] != PINS["componentize_qjs"]["commit"]
            or record["patches"] != patch_digests()):
        raise SystemExit("pane-js: the wasm parts were built from other pins or patches")
    toolchain = Toolchain()
    out = into / f"componentize-qjs-p3{EXE}"
    toolchain.build_componentizer(runtime, out)
    record.update(toolchain.componentizer_versions())
    (into / "toolchain.json").write_text(json.dumps(record, indent=2) + "\n", encoding="utf-8")
    log(f"built {out} ({sha256_file(out)})")


def check_parts(fresh: Path) -> None:
    """Verifies the committed wasm parts (tools/componentize-js/wasm-parts)
    against the fresh build in `fresh`: their bytes and the record of what
    built them, so the committed files cannot drift from the vendored source
    they should have been built from."""
    problems = [name for name in ["runtime.wasm", "libc.so", "wasm-parts.json"]
                if sha256_file(fresh / name) != sha256_file(WASM_PARTS / name)]
    if problems:
        raise SystemExit("pane-js: the committed wasm parts (tools/componentize-js/wasm-parts) "
                         f"do not match the fresh build: {', '.join(problems)} differ. Rebuild them "
                         "with `pane_js.py wasm-parts tools/componentize-js/wasm-parts`, commit "
                         "them, and rebuild the samples with `cargo xtask js-guests`.")
    log("the committed wasm parts match the fresh build")


def main(argv: list[str]) -> None:
    match argv:
        case ["wasm-parts", into]:
            wasm_parts(Path(into).resolve())
        case ["componentizer", into]:
            componentizer(Path(into).resolve())
        case ["check-parts", fresh]:
            check_parts(Path(fresh).resolve())
        case _:
            raise SystemExit(__doc__)


if __name__ == "__main__":
    try:
        main(sys.argv[1:])
    except SystemExit as failure:
        # A failure is one line starting "pane-js: error:" on standard error.
        message = failure.code
        if isinstance(message, str) and message.startswith("pane-js: "):
            print("pane-js: error: " + message.removeprefix("pane-js: "), file=sys.stderr, flush=True)
            raise SystemExit(1) from None
        raise
