"""Shared, OS-portable paths and pinned inputs for the JS/TS WASI 0.3 backend check.

Every download, toolchain and build directory lives under the scratch root
(`PANE_SCRATCH`, default: a `pane-scratch` directory beside the repository).
Nothing here edits global state other than installing the pinned rustup
toolchain in the user's existing rustup home.
"""
from __future__ import annotations

import hashlib
import json
import os
import platform
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[2]
SCRATCH = Path(os.environ.get("PANE_SCRATCH", REPO.parent / "pane-scratch")).resolve()
EXE = ".exe" if os.name == "nt" else ""

NIGHTLY = "nightly-2026-09-27"
QJS_REPO = "andreiltd/componentize-qjs"
QJS_COMMIT = "e563c6d6ae50b087980414015663ca9c948c09bb"
WASI_SDK_RELEASE = "wasi-sdk-34"
WASI_SDK_VERSION = "34.0"
WASMTIME_VERSION = "49.0.1"
# Recorded (not trusted-on-first-use) digests of downloaded archives. The Windows
# SDK digest comes from the saved Windows prototype metadata.
SHA256 = {
    "qjs-e563c6d.tar.gz": "80effd71d2c96750f015c9d7773a1b789027d4b15d6184600eebaae470a1e233",
    "wasi-sdk-34.0-x86_64-linux.tar.gz": "b761e3a0721dbae9c09a0059e5fdb2bf917d1b4a8a7b430fb3b5aafb0984b2c4",
    "wasi-sdk-34.0-x86_64-windows.tar.gz": "cccb5c323a9b34f0349a9b09e8804a0a7632c68c3310f4b5f437ed57d7e71d8f",
    "wasmtime-v49.0.1-x86_64-linux.tar.xz": "c71f7e0d30a92e418f0d17db7c6d8f6664c1ad764340a1278678f4209deab534",
}


def host_triple() -> tuple[str, str]:
    """(arch, os) in the spelling wasi-sdk and Wasmtime release assets use."""
    machine = platform.machine().lower()
    arch = {"amd64": "x86_64", "x86_64": "x86_64", "arm64": "aarch64", "aarch64": "aarch64"}[machine]
    system = {"linux": "linux", "darwin": "macos", "win32": "windows"}[sys.platform]
    return arch, system


def sdk_dir() -> Path:
    arch, system = host_triple()
    sdk_arch = "arm64" if arch == "aarch64" else arch
    return SCRATCH / f"wasi-sdk-{WASI_SDK_VERSION}-{sdk_arch}-{system}"


def wasmtime_cli() -> Path:
    arch, system = host_triple()
    return SCRATCH / f"wasmtime-v{WASMTIME_VERSION}-{arch}-{system}" / f"wasmtime{EXE}"


QJS_SOURCE = SCRATCH / "qjs-upstream"
QJS_PORT = SCRATCH / "qjs-port"
RUNTIME_TARGET = SCRATCH / "qjs-runtime-target"
RUNTIME_WASM = RUNTIME_TARGET / "wasm32-wasip3/release/componentize_qjs_runtime.wasm"
COMPONENTIZER_TARGET = SCRATCH / "componentizer-target"
COMPONENTIZER = COMPONENTIZER_TARGET / "release/examples" / f"p3_build{EXE}"
HOST_TARGET = SCRATCH / "host-target"
HOST = HOST_TARGET / "release" / f"p3-only-host{EXE}"
NODE_DIR = SCRATCH / "node"
OUT = SCRATCH / "out"
FIXTURE = SCRATCH / "fixture"


def libc_so() -> Path:
    return sdk_dir() / "share/wasi-sysroot/lib/wasm32-wasip3/libc.so"


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def run(cmd, *, env=None, cwd=None, log: Path | None = None, check=True, timeout=3600):
    cmd = [str(c) for c in cmd]
    if log:
        with log.open("w") as f:
            p = subprocess.run(cmd, env=env, cwd=cwd, stdout=f, stderr=subprocess.STDOUT, timeout=timeout)
        if check and p.returncode:
            print(log.read_text()[-8000:])
    else:
        p = subprocess.run(cmd, env=env, cwd=cwd, capture_output=True, text=True, timeout=timeout)
    if check and p.returncode:
        raise SystemExit(f"{cmd} failed with {p.returncode}\n{getattr(p, 'stderr', '') or ''}")
    return p


def write_json(name: str, value) -> Path:
    path = HERE / name
    path.write_text(json.dumps(value, indent=2) + "\n")
    return path
