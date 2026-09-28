"""Size and initialization-cost observations for the `fixed` variant on this machine.

Linux only as written (per-process peak RSS via GNU time's %M). Writes measurements-<os>.json.
Scope: release builds, fresh processes, small sample counts. Not launcher idle
memory, not steady per-extension cost, not an installer-size prediction.
"""
import gzip
import json
import os
import platform
import statistics
import subprocess
import time

from common import EXE, FIXTURE, HOST, OUT, REPO, SCRATCH, sha256, wasmtime_cli, write_json

out = OUT / "fixed"
RUN_GUEST = REPO / "target/release/examples" / f"run_guest{EXE}"
RUST_SAMPLE = REPO / "target/guests/sample_rust.wasm"


TIME = "/usr/bin/time"  # GNU time (Linux). See run_rss for why wait4 alone is not used.


def run_rss(cmd):
    """Run one process; return stdout, wall ms and that process's own peak RSS in MiB.

    Peak RSS comes from GNU time's %M. Reading ru_maxrss of a child spawned
    directly from Python is wrong on Linux: exec carries over the parent's
    high-water mark, so every result was floored at this script's own RSS.
    Wall time includes the GNU time wrapper (well under 1 ms here).
    """
    stats = SCRATCH / "measure.time"
    start = time.perf_counter()
    p = subprocess.run([TIME, "-f", "%M", "-o", str(stats), *map(str, cmd)], capture_output=True, text=True, timeout=300)
    elapsed = (time.perf_counter() - start) * 1000
    assert p.returncode == 0, (cmd, p.stderr[-2000:])
    return p.stdout, elapsed, int(stats.read_text().split()[-1]) / 1024


def med(values):
    return round(statistics.median(values), 3)


artifacts = {name: out / f"{name}.wasm" for name in
             ["p3-capabilities", "p3-search", "p3-sample-js", "p3-sample-ts", "stock-search", "stock-sample-ts"]}
artifacts["rust-sample (target/guests)"] = RUST_SAMPLE
sizes = {}
for name, wasm in artifacts.items():
    data = wasm.read_bytes()
    sizes[name] = {"bytes": len(data), "mib": round(len(data) / 1048576, 2),
                   "gzip9_bytes": len(gzip.compress(data, 9)), "sha256": sha256(wasm)}

# In-process Wasmtime 49.0.1 (P3-only host): compile, then deserialize + 20 fresh instantiations.
host_costs = {}
for name in ["p3-capabilities", "p3-search", "p3-sample-js", "p3-sample-ts", "rust-sample (target/guests)"]:
    cwasm = SCRATCH / "measure" / (name.split(" ")[0] + ".cwasm")
    cwasm.parent.mkdir(exist_ok=True)
    stdout, _, compile_rss = run_rss([HOST, artifacts[name], "precompile", cwasm])
    compile_ms = json.loads(stdout)["compile_ms"]
    runs = []
    for _ in range(3):
        stdout, elapsed, rss = run_rss([HOST, cwasm, "instantiate", "20"])
        value = json.loads(stdout)
        runs.append({"load_ms": value["load_ms"], "instantiate_ms": [float(x) for x in value["instantiate_ms"]],
                     "process_ms": elapsed, "peak_rss_mib": rss})
    first = [r["instantiate_ms"][0] for r in runs]
    rest = [x for r in runs for x in r["instantiate_ms"][1:]]
    host_costs[name] = {
        "cranelift_compile_ms": round(compile_ms, 1), "compile_peak_rss_mib": round(compile_rss, 1),
        "cwasm_bytes": cwasm.stat().st_size,
        "deserialize_ms_median": med([r["load_ms"] for r in runs]),
        "first_instantiate_ms_median": med(first),
        "later_instantiate_ms_median": med(rest),
        "later_instantiate_ms_max": round(max(rest), 3),
        "instantiate20_process_peak_rss_mib_median": med([r["peak_rss_mib"] for r in runs]),
    }

# Same Fuse/Zod/10 ms workload, stock vs patched, Wasmtime CLI 49.0.1, precompiled, fresh processes.
cli = wasmtime_cli()
cli_costs = {}
for name in ["stock-search", "p3-search"]:
    cwasm = SCRATCH / "measure" / f"cli-{name}.cwasm"
    _, compile_ms, compile_rss = run_rss([cli, "compile", "-W", "component-model-async=y", "-o", cwasm, artifacts[name]])
    samples = []
    for _ in range(5):
        stdout, elapsed, rss = run_rss([cli, "run", "-W", "component-model-async=y", "-S", "p3=y", "--allow-precompiled",
                                        "--invoke", 'query("calclator")', cwasm])
        value = json.loads(json.loads(stdout))
        assert value["items"][0]["id"] == "calculator" and int(value["elapsedNs"]) >= 10_000_000
        samples.append((elapsed, rss))
    cli_costs[name] = {"compile_process_ms": round(compile_ms, 1), "compile_peak_rss_mib": round(compile_rss, 1),
                       "cwasm_bytes": cwasm.stat().st_size,
                       "run_process_ms_median": med([s[0] for s in samples]),
                       "run_peak_rss_mib_median": med([s[1] for s in samples])}

# Pane's real runtime: first call includes reading + Cranelift-compiling the component (no cache yet).
pane_costs = {}
for name in ["p3-sample-js", "p3-sample-ts", "rust-sample (target/guests)"]:
    samples = []
    for _ in range(3):
        stdout, elapsed, rss = run_rss([RUN_GUEST, artifacts[name], "view", "action:greet", "action:greet"])
        ms = [float(line.split("\t")[1]) for line in stdout.splitlines()]
        samples.append((ms, elapsed, rss))
    pane_costs[name] = {"first_get_view_ms_median": med([s[0][0] for s in samples]),
                        "later_action_ms_median": med([x for s in samples for x in s[0][1:]]),
                        "process_peak_rss_mib_median": med([s[2] for s in samples])}

report = {
    "scope": ("Fresh release processes on one machine; 3-5 samples. Host = docs/research/p3-only-host "
              "(Wasmtime 49.0.1, P3 only). Peak RSS is the whole process, including Wasmtime/Cranelift. "
              "Not launcher idle memory, steady per-extension cost, or installer size."),
    "machine": {"os": platform.platform(), "cpu_count": os.cpu_count(), "python": platform.python_version()},
    "sizes": sizes, "host_in_process": host_costs, "cli_same_workload": cli_costs, "pane_core_run_guest": pane_costs,
}
name = f"measurements-{platform.system().lower()}.json"
write_json(name, json.loads(json.dumps(report).replace(str(SCRATCH), "$PANE_SCRATCH")))
print(json.dumps(report, indent=1))
