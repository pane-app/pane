"""Download and verify the pinned archives into the scratch root, then install the pinned nightly."""
import shutil
import tarfile
import urllib.request

from common import (HERE, NODE_DIR, NIGHTLY, QJS_COMMIT, QJS_REPO, QJS_SOURCE, SCRATCH, SHA256, WASI_SDK_RELEASE,
                    WASI_SDK_VERSION, WASMTIME_VERSION, host_triple, run, sdk_dir, sha256, wasmtime_cli)

SCRATCH.mkdir(parents=True, exist_ok=True)
arch, system = host_triple()
sdk_arch = "arm64" if arch == "aarch64" else arch
sdk_name = f"wasi-sdk-{WASI_SDK_VERSION}-{sdk_arch}-{system}.tar.gz"
wt_ext = "zip" if system == "windows" else "tar.xz"
wt_name = f"wasmtime-v{WASMTIME_VERSION}-{arch}-{system}.{wt_ext}"
downloads = {
    "qjs-e563c6d.tar.gz": f"https://codeload.github.com/{QJS_REPO}/tar.gz/{QJS_COMMIT}",
    sdk_name: f"https://github.com/WebAssembly/wasi-sdk/releases/download/{WASI_SDK_RELEASE}/{sdk_name}",
    wt_name: f"https://github.com/bytecodealliance/wasmtime/releases/download/v{WASMTIME_VERSION}/{wt_name}",
}
for name, url in downloads.items():
    path = SCRATCH / name
    if not path.exists():
        print("downloading", url)
        with urllib.request.urlopen(url) as r, path.open("wb") as f:
            shutil.copyfileobj(r, f)
    digest = sha256(path)
    if name in SHA256:
        assert digest == SHA256[name], (name, digest)
    else:
        print(f"UNRECORDED digest {name} {digest}; add it to common.SHA256 after review")

if not sdk_dir().exists():
    with tarfile.open(SCRATCH / sdk_name) as t:
        t.extractall(SCRATCH, filter="tar")
if not wasmtime_cli().exists():
    if wt_ext == "zip":
        shutil.unpack_archive(SCRATCH / wt_name, SCRATCH)
    else:
        with tarfile.open(SCRATCH / wt_name) as t:
            t.extractall(SCRATCH, filter="tar")
if not QJS_SOURCE.exists():
    with tarfile.open(SCRATCH / "qjs-e563c6d.tar.gz") as t:
        prefix = t.getmembers()[0].name.split("/")[0]
        t.extractall(SCRATCH, filter="tar")
    (SCRATCH / prefix).rename(QJS_SOURCE)

# Author-side npm tools (esbuild, TypeScript, stock componentize-qjs for the negative control).
NODE_DIR.mkdir(exist_ok=True)
for name in ["package.json", "package-lock.json"]:
    shutil.copyfile(HERE / name, NODE_DIR / name)
run([shutil.which("npm") or "npm", "ci", "--no-audit", "--no-fund"], cwd=NODE_DIR)

run(["rustup", "toolchain", "install", NIGHTLY, "--profile", "minimal", "--component", "rust-src"])
print(run(["rustup", "run", NIGHTLY, "rustc", "-vV"]).stdout)
