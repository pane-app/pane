"""Throwaway execution/import check. A mixed p2/p3 guest is explicitly not a p3-only pass."""
import ctypes
from ctypes import wintypes
import json
from pathlib import Path
import subprocess
import time

HERE = Path(__file__).resolve().parent
META = json.loads((HERE.parent / 'wasi03-js-local.json').read_text())
OLD = json.loads((HERE.parent / 'wasm-spike-local.json').read_text())
ROOT = Path(META['root'])
WASM = ROOT / 'search.wasm'

class Counters(ctypes.Structure):
    _fields_ = [('cb', wintypes.DWORD), ('PageFaultCount', wintypes.DWORD)] + [
        (name, ctypes.c_size_t) for name in [
            'PeakWorkingSetSize', 'WorkingSetSize', 'QuotaPeakPagedPoolUsage',
            'QuotaPagedPoolUsage', 'QuotaPeakNonPagedPoolUsage', 'QuotaNonPagedPoolUsage',
            'PagefileUsage', 'PeakPagefileUsage', 'PrivateUsage',
        ]
    ]

get_memory = ctypes.WinDLL('psapi', use_last_error=True).GetProcessMemoryInfo
get_memory.argtypes = [wintypes.HANDLE, ctypes.POINTER(Counters), wintypes.DWORD]
get_memory.restype = wintypes.BOOL

def run(args):
    start = time.perf_counter()
    p = subprocess.Popen([str(a) for a in args], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    try:
        out, err = p.communicate(timeout=60)
    except subprocess.TimeoutExpired:
        p.kill()
        p.communicate()
        raise
    c = Counters()
    c.cb = ctypes.sizeof(c)
    if not get_memory(wintypes.HANDLE(int(p._handle)), ctypes.byref(c), c.cb):
        raise ctypes.WinError(ctypes.get_last_error())
    result = dict(exit_code=p.returncode, stdout=out.decode('utf8'), stderr=err.decode('utf8'),
                  elapsed_ms=(time.perf_counter()-start)*1000, peak_working_set_bytes=c.PeakWorkingSetSize)
    assert p.returncode == 0, result
    return result

wit = subprocess.check_output([META['wasm_tools'], 'component', 'wit', str(WASM)], text=True)
(HERE / 'component-interface.wit').write_text(wit, encoding='utf8')
world = wit.split('world root {', 1)[1].split('\n}', 1)[0]
imports = [s.strip().removeprefix('import ').removesuffix(';') for s in world.splitlines() if s.strip().startswith('import ')]
p2 = [s for s in imports if s.startswith('wasi:') and '@0.2.' in s]
assert 'wasi:clocks/monotonic-clock@0.3.0' in imports
assert 'export query: async func' in world
cases = []
for query, ids in [('calclator', ['calculator']), ('', ['calculator', 'applications', 'quicklinks']), ('zzzzzz', [])]:
    r = run([OLD['exe'], 'run', '-W', 'component-model-async=y', '-S', 'p3=y', '--invoke', f'query({json.dumps(query)})', WASM])
    value = json.loads(json.loads(r['stdout']))
    assert [x['id'] for x in value['items']] == ids, value
    assert value['invalidDataRejected'] is True
    assert int(value['elapsedNs']) >= 10_000_000
    cases.append(dict(query=query, value=value, execution=r))
(HERE / 'view.json').write_text(json.dumps(cases[0]['value'], indent=2), encoding='utf8')
cache = ROOT / 'search.cwasm'
compilation = run([OLD['exe'], 'compile', '-W', 'component-model-async=y', '-o', cache, WASM])
cached = []
for _ in range(3):
    r = run([OLD['exe'], 'run', '-W', 'component-model-async=y', '-S', 'p3=y', '--allow-precompiled', '--invoke', 'query("calclator")', cache])
    value = json.loads(json.loads(r['stdout']))
    assert value['items'][0]['id'] == 'calculator'
    assert int(value['elapsedNs']) >= 10_000_000 and value['invalidDataRejected']
    cached.append(r)
report = dict(
    scope='Windows fresh CLI processes, real p3 clock + Fuse.js/Zod; not persistent host or pure-p3 validation',
    versions={'wasmtime': '49.0.1', 'wasm_tools': '1.259.0', 'componentize_qjs': '0.4.5', 'fuse_js': '7.5.0', 'zod': '4.6.5'},
    imports=imports, p2_imports=p2, satisfies_p3_only=(not p2),
    component_bytes=WASM.stat().st_size, compiled_bytes=cache.stat().st_size,
    cases=cases, compilation=compilation, cached=cached,
)
(HERE / 'results.json').write_text(json.dumps(report, indent=2), encoding='utf8')
print(json.dumps({'functional_cases_passed':len(cases), 'cached_runs_passed':len(cached),
                  'satisfies_p3_only':report['satisfies_p3_only'], 'p2_import_count':len(p2),
                  'component_bytes':report['component_bytes'], 'cached_peak_bytes':[r['peak_working_set_bytes'] for r in cached]}))
