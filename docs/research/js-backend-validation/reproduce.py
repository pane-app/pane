"""End-to-end reproduction: fetch, build, componentize, regression-check and measure.

Usage: PANE_SCRATCH=<dir> python reproduce.py [--clean]
1. Builds the `unfixed` variant (runtime-port.patch only) and runs validate.py,
   which is expected to FAIL exactly the fresh-instance regression checks.
2. Builds the `fixed` variant (+ random-reseed.patch), which must pass all checks.
3. Measures the fixed variant (POSIX only).
"""
import platform
import shutil
import subprocess
import sys

from common import COMPONENTIZER_TARGET, HERE, HOST_TARGET, OUT, QJS_PORT, RUNTIME_TARGET, RUNTIME_WASM


def step(*args, expect_fail=False):
    code = subprocess.run([sys.executable, *map(str, args)], cwd=HERE).returncode
    print(f"== {' '.join(map(str, args))}: exit {code}", flush=True)
    if (code != 0) != expect_fail:
        raise SystemExit(f"unexpected exit {code} from {args}")


if "--clean" in sys.argv:
    for path in [QJS_PORT, RUNTIME_TARGET, COMPONENTIZER_TARGET, HOST_TARGET, OUT]:
        shutil.rmtree(path, ignore_errors=True)
step("fetch.py")
step("prepare.py", "--no-reseed")
step("build.py", "runtime", "componentizer", "host", "pane")
step("componentize.py", "--variant", "unfixed")
shutil.copyfile(RUNTIME_WASM, OUT / "unfixed/runtime.wasm")
step("validate.py", "--variant", "unfixed", expect_fail=True)
step("prepare.py")
step("build.py", "runtime")
step("componentize.py", "--variant", "fixed")
step("validate.py", "--variant", "fixed")
if platform.system() != "Windows":
    step("measure.py")
