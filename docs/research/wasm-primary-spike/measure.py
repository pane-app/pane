"""Windows fresh-process comparison; no persistent runtime/GUI claim."""
import ctypes
from ctypes import wintypes
import gzip
import json
from pathlib import Path
import random
import statistics
import subprocess
import time

HERE = Path(__file__).resolve().parent
OLD = json.loads((HERE.parent / "wasm-spike-local.json").read_text())
META = json.loads((HERE.parent / "wasm-primary-local.json").read_text())
ROOT = Path(META["root"])
EXE = OLD["exe"]

class Counters(ctypes.Structure):
    _fields_ = [("cb", wintypes.DWORD), ("PageFaultCount", wintypes.DWORD)] + [
        (name, ctypes.c_size_t) for name in [
            "PeakWorkingSetSize", "WorkingSetSize", "QuotaPeakPagedPoolUsage",
            "QuotaPagedPoolUsage", "QuotaPeakNonPagedPoolUsage",
            "QuotaNonPagedPoolUsage", "PagefileUsage", "PeakPagefileUsage", "PrivateUsage"
        ]
    ]

get_memory = ctypes.WinDLL("psapi", use_last_error=True).GetProcessMemoryInfo
get_memory.argtypes = [wintypes.HANDLE, ctypes.POINTER(Counters), wintypes.DWORD]
get_memory.restype = wintypes.BOOL

def execute(args, expected=None):
    start = time.perf_counter()
    p = subprocess.Popen(args, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    try:
        stdout, stderr = p.communicate(timeout=120)
    except subprocess.TimeoutExpired:
        p.kill()
        p.communicate()
        raise
    elapsed_ms = (time.perf_counter() - start) * 1000
    counters = Counters()
    counters.cb = ctypes.sizeof(counters)
    if not get_memory(wintypes.HANDLE(int(p._handle)), ctypes.byref(counters), counters.cb):
        raise ctypes.WinError(ctypes.get_last_error())
    output = stdout.decode("utf8", errors="replace").strip()
    if p.returncode:
        raise RuntimeError(stderr.decode("utf8", errors="replace"))
    if expected is not None and output != expected:
        raise AssertionError(output)
    return {"elapsed_ms": elapsed_ms, "peak_working_set_bytes": counters.PeakWorkingSetSize,
            "stdout": output}

components = {
    "rust": Path(OLD["root"]) / "target/wasm32-wasip2/release/launcher_wit_smoke.wasm",
    "qjs": ROOT / "qjs.wasm", "spidermonkey": ROOT / "spidermonkey.wasm",
}
expected = '[{id: "calculator", title: "Calculator"}]'
compilation = {}
commands = {}
sizes = {}
for name, path in components.items():
    precompiled = ROOT / f"{name}.cwasm"
    compilation[name] = execute([EXE, "compile", "-o", str(precompiled), str(path)])
    sizes[name] = {"wasm_bytes": path.stat().st_size,
                   "gzip_bytes": len(gzip.compress(path.read_bytes(), mtime=0)),
                   "precompiled_bytes": precompiled.stat().st_size}
    commands[name + "_compile_each_process"] = [EXE, "run", "-C", "cache=n", "--invoke", 'query("calc")', str(path)]
    commands[name + "_precompiled_process"] = [EXE, "run", "--allow-precompiled", "--invoke", 'query("calc")', str(precompiled)]
commands["node_process"] = ["node", str(HERE.parent / "wasm-spike/baseline.cjs")]
samples = {name: [] for name in commands}
schedule = [name for name in commands for _ in range(5 if "compile_each" in name else 15)]
random.Random(73).shuffle(schedule)
for name in schedule:
    output = '[{"id":"calculator","title":"Calculator"}]' if name == "node_process" else expected
    samples[name].append(execute(commands[name], output))

report = {
    "scope": "Windows x64; sequential interleaved fresh CLI processes; potentially warm filesystem caches; no embedded host, GPUI, custom host calls, hot reload, or steady-state workload",
    "versions": {"wasmtime": execute([EXE, "--version"])["stdout"],
                 "node": execute(["node", "--version"])["stdout"], "build_packages": META["packages"]},
    "sizes": sizes, "compilation": compilation,
    "component_builds": {name: json.loads((ROOT / f"{name}-build.json").read_text()) for name in ["qjs", "spidermonkey"]},
    "summary": {name: {"n": len(values),
                       "median_ms": statistics.median(x["elapsed_ms"] for x in values),
                       "median_peak_working_set_bytes": statistics.median(x["peak_working_set_bytes"] for x in values)}
                for name, values in samples.items()},
    "samples": samples,
}
(HERE / "results.json").write_text(json.dumps(report, indent=2), encoding="utf8")
print(json.dumps({k: v for k, v in report.items() if k != "samples"}, indent=2))
