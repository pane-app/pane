"""Functional, import-graph and fresh-instance regression checks for one build variant.

Usage: python validate.py [--variant fixed|unfixed]
Writes validation-<variant>.json. Exits non-zero if any check fails, so the
fresh-instance checks are a regression test: they fail on the `unfixed`
variant (runtime-port.patch only) and pass on `fixed` (+ random-reseed.patch).
Debug-grade timings here are not benchmarks; see measure.py.
"""
import json
import subprocess
import sys
import time

from common import EXE, FIXTURE, HOST, OUT, REPO, SCRATCH, run, write_json

variant = sys.argv[sys.argv.index("--variant") + 1] if "--variant" in sys.argv else "fixed"
out = OUT / variant
RUN_GUEST = REPO / "target/release/examples" / f"run_guest{EXE}"
FIXTURE.mkdir(parents=True, exist_ok=True)
(FIXTURE / "fixture.txt").write_bytes(b"p3 standard library fixture\n")
checks = []


def check(name, fn):
    try:
        detail = fn()
        checks.append({"check": name, "pass": True, "detail": detail})
    except AssertionError as error:
        checks.append({"check": name, "pass": False, "detail": str(error)[:3000]})
    print(("PASS " if checks[-1]["pass"] else "FAIL ") + name)


def proc(cmd):
    p = subprocess.run([str(c) for c in cmd], capture_output=True, text=True, timeout=300)
    return p.returncode, p.stdout, p.stderr


def world_lines(wasm):
    wit = run(["wasm-tools", "component", "wit", wasm]).stdout
    world = wit.split("world root {", 1)[1].split("\n}", 1)[0]
    lines = [line.strip() for line in world.splitlines() if line.strip()]
    return ([l for l in lines if l.startswith("import ")], [l for l in lines if l.startswith("export ")], wit)


def p3_imports(name, exports_expected):
    def fn():
        wasm = out / f"{name}.wasm"
        assert proc(["wasm-tools", "validate", "--features", "all", wasm])[0] == 0, "wasm-tools validate failed"
        imports, exports, _ = world_lines(wasm)
        assert imports and all(l.endswith("@0.3.0;") for l in imports), imports
        for expected in exports_expected:
            assert any(expected in l for l in exports), (expected, exports)
        return {"imports": imports, "exports": exports}
    return fn


def stock_imports(name):
    def fn():
        imports, _, _ = world_lines(out / f"{name}.wasm")
        p2 = [l for l in imports if "@0.2." in l]
        assert p2, imports
        return {"p2_imports": len(p2), "p3_imports": len([l for l in imports if "@0.3." in l])}
    return fn


def capability_value(value, i=None):
    assert value["invalidDataRejected"] is True, value
    assert int(value["elapsedNs"]) >= 10_000_000, value
    assert [x["id"] for x in value["items"]] == ["calculator"], value
    assert 0 <= value["random"] < 1, value
    assert abs(value["dateMs"] - time.time() * 1000) < 60_000, value
    if i is not None:
        assert value["invocations"] == i + 1, value


def host(*args):
    code, stdout, stderr = proc([HOST, out / "p3-capabilities.wasm", *args])
    assert code == 0, (code, stderr[-2000:])
    return [json.loads(line) for line in stdout.splitlines()]


def twenty_calls():
    values = host("queries", "io", FIXTURE, "20")
    assert len(values) == 20, len(values)
    for i, v in enumerate(values):
        capability_value(v, i)
        assert v["fileText"] == "p3 standard library fixture\n" and v["fileError"] is None, v
        assert v["completion"] == {"tag": "ok"}, v
    assert values[-1]["dateMs"] > values[0]["dateMs"]
    randoms = [v["random"] for v in values]
    assert len(set(randoms)) == 20, randoms
    return {"invocations": [v["invocations"] for v in values], "distinct_random": len(set(randoms)),
            "date_advance_ms": values[-1]["dateMs"] - values[0]["dateMs"]}


def missing_file():
    [v] = host("query", "missing", FIXTURE)
    capability_value(v, 0)
    assert v["fileErrorPayload"] == {"tag": "no-entry"}, v
    return {"fileError": v["fileError"], "fileErrorPayload": v["fileErrorPayload"]}


def no_preopen():
    [v] = host("query", "io")
    capability_value(v, 0)
    assert v["fileError"] == "Error: no-preopens", v
    return {"fileError": v["fileError"]}


FRESH = 5
fresh_values = []


def fresh_random():
    fresh_values[:] = [host("query", "io", FIXTURE)[0] for _ in range(FRESH)]
    firsts = [v["random"] for v in fresh_values]
    detail = {"first_random_per_instance": firsts,
              "build_time_random": sorted({v["buildRandom"] for v in fresh_values})}
    assert len(set(firsts)) == FRESH, f"fresh instances repeat Math.random: {detail}"
    return detail


def fresh_performance():
    values = fresh_values or [host("query", "io", FIXTURE)[0] for _ in range(FRESH)]
    detail = [{"perfNow": v["perfNow"], "timeOrigin": v["timeOrigin"], "dateMs": v["dateMs"]} for v in values]
    for v in values:
        # The query awaits a 10 ms clock before reading performance.now().
        assert 10 <= v["perfNow"] < 60_000, f"performance.now() not relative to this instance: {detail}"
        assert abs(v["dateMs"] - v["perfNow"] - v["timeOrigin"]) < 1_000, f"timeOrigin not this instance's: {detail}"
    return detail


def host_rejects_stock():
    code, _, stderr = proc([HOST, out / "stock-search.wasm", "query", "calclator"])
    assert code != 0 and "@0.2" in stderr, (code, stderr[-1500:])
    return {"exit_code": code, "error": next((l for l in stderr.splitlines() if "@0.2" in l), "")}


def pane(name, lang):
    def fn():
        code, stdout, stderr = proc([RUN_GUEST, out / f"{name}.wasm", "view", "action:greet", "action:wait",
                                     "action:random", "action:nope"])
        lines = [line.split("\t", 2) for line in stdout.splitlines()]
        assert len(lines) == 5 and code == 1, (code, stdout, stderr[-1500:])
        (s0, _, view), (s1, _, greet), (s2, wait_ms, wait), (s3, _, rnd), (s4, _, nope) = lines
        assert s0 == "ok" and f'title: "{lang} sample"' in view and '"greet"' in view and '"wait"' in view, view
        assert (s1, greet) == ("ok", f"Hello from the {lang} guest"), greet
        assert s2 == "ok" and wait == f"Waited 50 ms inside the {lang} guest" and float(wait_ms) >= 50, (wait_ms, wait)
        assert s3 == "ok" and 0 <= float(rnd) < 1, rnd
        assert s4 == "err" and nope == "The extension reported an error: unknown item: nope", nope
        return {"lines": stdout.splitlines()}
    return fn


def pane_fresh_random(name):
    def fn():
        values = []
        for _ in range(3):
            code, stdout, stderr = proc([RUN_GUEST, out / f"{name}.wasm", "action:random"])
            assert code == 0, stderr
            values.append(stdout.split("\t", 2)[2].strip())
        assert len(set(values)) == 3, f"fresh Pane runtimes repeat Math.random: {values}"
        return values
    return fn


def pane_rejects_stock():
    code, stdout, _ = proc([RUN_GUEST, out / "stock-sample-ts.wasm", "view"])
    assert code == 1 and "Incompatible extension: Pane supports only WASI 0.3" in stdout, stdout
    return stdout.split("\t", 2)[2][:300]


check("p3-capabilities imports only @0.3.0", p3_imports("p3-capabilities", ["export query: async func"]))
for name in ["p3-sample-js", "p3-sample-ts"]:
    check(f"{name} imports only @0.3.0 and exports pane:extension/command", p3_imports(name, ["pane:extension/command@0.1.0"]))
for name in ["stock-search", "stock-sample-ts"]:
    check(f"{name} (stock control) imports WASI 0.2", stock_imports(name))
check("host: 20 sequential calls, one instance", twenty_calls)
check("host: missing file, fresh instance", missing_file)
check("host: no filesystem grant, fresh instance", no_preopen)
check("host: rejects stock mixed P2/P3 component", host_rejects_stock)
check(f"REGRESSION host: {FRESH} fresh instances get distinct first Math.random", fresh_random)
check("REGRESSION host: performance.now()/timeOrigin belong to the fresh instance", fresh_performance)
check("pane-core: JS sample get-view/run-action", pane("p3-sample-js", "JavaScript"))
check("pane-core: TS sample get-view/run-action", pane("p3-sample-ts", "TypeScript"))
check("REGRESSION pane-core: fresh runtimes get distinct Math.random (TS sample)", pane_fresh_random("p3-sample-ts"))
check("pane-core: rejects stock TS sample as incompatible", pane_rejects_stock)

report = {"variant": variant, "pass": all(c["pass"] for c in checks), "checks": checks}
text = json.dumps(report).replace(str(SCRATCH), "$PANE_SCRATCH").replace(str(REPO), "$REPO")
path = write_json(f"validation-{variant}.json", json.loads(text))
print(json.dumps({"variant": variant, "pass": report["pass"],
                  "failed": [c["check"] for c in checks if not c["pass"]], "report": str(path)}))
raise SystemExit(0 if report["pass"] else 1)
