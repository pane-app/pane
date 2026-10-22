"""Inspect the final ABI and exercise successful and denied std I/O."""
import hashlib
import json
from pathlib import Path
import subprocess
import time

HERE = Path(__file__).resolve().parent
ROOT = Path(json.loads((HERE.parent / "p3-native-toolchain-local.json").read_text())["root"])
TOOLS = json.loads((HERE.parent / "wasi03-js-local.json").read_text())["wasm_tools"]
WASMTIME = json.loads((HERE.parent / "wasm-spike-local.json").read_text())["exe"]
WASM = ROOT / "std-nightly-target/wasm32-wasip3/release/p3-std-spike.wasm"
subprocess.run([TOOLS, "validate", "--features", "all", str(WASM)], check=True)
wit = subprocess.check_output([TOOLS, "component", "wit", str(WASM)], text=True)
(HERE / "component.wit").write_text(wit, encoding="utf-8")
world = wit.split("world root {", 1)[1].split("\n}", 1)[0]
imports = [line.strip() for line in world.splitlines() if line.strip().startswith("import ")]
assert imports and all(line.endswith("@0.3.0;") for line in imports), imports
work = ROOT / "std-fixture"
work.mkdir(exist_ok=True)
(work / "fixture.txt").write_text("p3 standard library fixture\n", newline="\n")
cases = []
for mode in ["success", "denied"]:
    command = [WASMTIME, "run", "-S", "p3=y"]
    if mode == "success":
        command += ["--dir", str(work) + "::.", "--env", "P3_PROBE=runtime-value"]
    command += [str(WASM), mode]
    started = time.perf_counter()
    p = subprocess.run(command, capture_output=True, text=True, timeout=60)
    result = {"mode": mode, "exit_code": p.returncode, "stdout": p.stdout,
              "stderr": p.stderr, "elapsed_ms": (time.perf_counter()-started)*1000}
    assert p.returncode == 0, result
    if mode == "success":
        assert json.loads(p.stdout)["ok"] is True
        assert "p3 diagnostics work" in p.stderr
        assert not (work / "roundtrip.txt").exists()
    else:
        assert p.stdout.strip() == "denied-ok"
    cases.append(result)
report = {"pass": True, "component_bytes": WASM.stat().st_size,
          "sha256": hashlib.sha256(WASM.read_bytes()).hexdigest(), "imports": imports,
          "p3_only_imports": True, "execution_host": "Wasmtime CLI 49.0.1 (supports P2 and P3)",
          "cases": cases}
(HERE / "result.json").write_text(json.dumps(report, indent=2) + "\n")
print(json.dumps(report, indent=2))
