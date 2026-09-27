"""Build the bounded std probe using isolated nightly + SDK 34; no global env edits."""
import json
import os
from pathlib import Path
import subprocess
import time

HERE = Path(__file__).resolve().parent
META = json.loads((HERE.parent / "p3-native-toolchain-local.json").read_text())
ROOT, SDK = Path(META["root"]), Path(META["sdk"])
TOOLCHAIN = "nightly-2026-09-27"
env = {
    **os.environ,
    "RUSTUP_HOME": str(ROOT / "rustup"),
    "CARGO_TARGET_DIR": str(ROOT / "std-nightly-target"),
    "PATH": str(SDK / "bin") + os.pathsep + os.environ["PATH"],
    "CARGO_TARGET_WASM32_WASIP3_LINKER": str(SDK / "bin/wasm-component-ld.exe"),
    "RUSTFLAGS": " ".join([
        "-L", "native=" + str(SDK / "share/wasi-sysroot/lib/wasm32-wasip3"),
        "-C", "link-arg=--wasm-ld-path=" + str(SDK / "bin/wasm-ld.exe"),
        "-C", "link-arg=--cooperative-threading",
    ]),
}
command = ["rustup", "run", TOOLCHAIN, "cargo", "build", "--release", "--target",
           "wasm32-wasip3", "-Zbuild-std=std,panic_abort", "--manifest-path", str(HERE / "Cargo.toml")]
start = time.monotonic()
with (HERE / "build-nightly.log").open("w") as log:
    p = subprocess.run(command, env=env, stdout=log, stderr=subprocess.STDOUT)
result = {"exit_code": p.returncode, "seconds": time.monotonic()-start, "command": command,
          "toolchain": TOOLCHAIN, "target_dir": env["CARGO_TARGET_DIR"]}
(HERE / "build-result.json").write_text(json.dumps(result, indent=2) + "\n")
print(json.dumps(result))
print((HERE / "build-nightly.log").read_text()[-7000:])
raise SystemExit(p.returncode)
