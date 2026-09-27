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
ROOT = Path(json.loads((HERE.parent / 'p3-native-toolchain-local.json').read_text())['root'])

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


import statistics
reports = []
for label, wasm in [('stock-mixed', Path(META['root'])/'search.wasm'), ('p3-port', ROOT/'qjs-p3-search.wasm')]:
    cache = ROOT/(label+'.cwasm')
    compilation = run([OLD['exe'], 'compile', '-W', 'component-model-async=y', '-o', cache, wasm])
    cached = []
    for _ in range(3):
        result = run([OLD['exe'], 'run', '-W', 'component-model-async=y', '-S', 'p3=y', '--allow-precompiled', '--invoke', 'query("calclator")', cache])
        value = json.loads(json.loads(result['stdout']))
        assert value['items'][0]['id'] == 'calculator' and value['invalidDataRejected']
        assert int(value['elapsedNs']) >= 10_000_000
        cached.append(result)
    reports.append(dict(label=label, component_bytes=wasm.stat().st_size, cache_bytes=cache.stat().st_size,
        compilation=compilation, cached=cached,
        median_peak_mib=statistics.median(r['peak_working_set_bytes'] for r in cached)/1048576,
        median_elapsed_ms=statistics.median(r['elapsed_ms'] for r in cached)))
report=dict(scope='Windows fresh release CLI processes; same Fuse/Zod/10ms-clock workload; 3 cached samples per variant. Different runtime/libc build optimizations; not intrinsic P3 overhead or persistent host budget.', variants=reports)
(HERE/'measurements.json').write_text(json.dumps(report,indent=2))
for r in reports:
    print(json.dumps({k:v for k,v in r.items() if k not in ['compilation','cached']}))
