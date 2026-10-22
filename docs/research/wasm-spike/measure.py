"""Windows-only, fresh-process smoke measurements. Not an embedded-host benchmark."""
import ctypes
from ctypes import wintypes
import json
from pathlib import Path
import random
import statistics
import subprocess
import time

HERE = Path(__file__).resolve().parent
metadata = json.loads((HERE.parent / "wasm-spike-local.json").read_text())
ROOT = Path(metadata["root"])
EXE = metadata["exe"]
COMPONENT = ROOT / "target/wasm32-wasip2/release/launcher_wit_smoke.wasm"
PRECOMPILED = ROOT / "launcher-smoke.cwasm"

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

def execute(args):
    started = time.perf_counter()
    p = subprocess.Popen(args, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    stdout, stderr = p.communicate(timeout=30)
    elapsed_ms = (time.perf_counter() - started) * 1000
    counters = Counters()
    counters.cb = ctypes.sizeof(counters)
    if not get_memory(wintypes.HANDLE(int(p._handle)), ctypes.byref(counters), counters.cb):
        raise ctypes.WinError(ctypes.get_last_error())
    if p.returncode:
        raise RuntimeError(stderr.decode("utf-8", errors="replace"))
    return {"elapsed_ms": elapsed_ms, "peak_working_set_bytes": counters.PeakWorkingSetSize,
            "stdout": stdout.decode("utf-8").strip()}

compile_result = execute([EXE, "compile", "-o", str(PRECOMPILED), str(COMPONENT)])
commands = {
    "wasm_compile_each_process": [EXE, "run", "-C", "cache=n", "--invoke", 'query("calc")', str(COMPONENT)],
    "wasm_locally_precompiled": [EXE, "run", "--allow-precompiled", "--invoke", 'query("calc")', str(PRECOMPILED)],
    "node_process": ["node", str(HERE / "baseline.cjs")],
}
expected = {
    "wasm_compile_each_process": '[{id: "calculator", title: "Calculator"}]',
    "wasm_locally_precompiled": '[{id: "calculator", title: "Calculator"}]',
    "node_process": '[{"id":"calculator","title":"Calculator"}]',
}
first = {name: execute(args) for name, args in commands.items()}
samples = {name: [] for name in commands}
schedule = list(commands) * 15
random.Random(42).shuffle(schedule)
for name in schedule:
    result = execute(commands[name])
    assert result["stdout"] == expected[name], result
    samples[name].append(result)

report = {
    "scope": "Windows x64; fresh CLI processes; filesystem/OS caches may be warm; no UI, SDK host calls, reload or JS/Python-in-Wasm measured",
    "versions": {tool: subprocess.check_output(args).decode().strip() for tool,args in {
        "wasmtime": [EXE,"--version"], "rust": ["rustc","--version"], "node": ["node","--version"]}.items()},
    "component_bytes": COMPONENT.stat().st_size,
    "precompiled_bytes": PRECOMPILED.stat().st_size,
    "runtime_exe_bytes": Path(EXE).stat().st_size,
    "compile_once": compile_result,
    "first_runs": first,
    "summary": {name: {
        "n": len(values),
        "median_ms": statistics.median(v["elapsed_ms"] for v in values),
        "min_ms": min(v["elapsed_ms"] for v in values),
        "max_ms": max(v["elapsed_ms"] for v in values),
        "median_peak_working_set_bytes": statistics.median(v["peak_working_set_bytes"] for v in values)
    } for name,values in samples.items()},
    "samples": samples,
}
(HERE / "results.json").write_text(json.dumps(report,indent=2),encoding="utf-8")
print(json.dumps({k:v for k,v in report.items() if k != "samples"},indent=2))
