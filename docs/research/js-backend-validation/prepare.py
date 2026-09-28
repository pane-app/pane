"""Create the patched scratch checkout from the verified upstream tarball.

Applies, in order: runtime-port.patch (the saved Windows prototype's P3 port) and
random-reseed.patch (fresh-instance randomness fix; skipped with --no-reseed to
reproduce the defect). `patch` ships with Linux/macOS and with Git for Windows.
"""
import shutil
import sys

from common import HERE, QJS_PORT, QJS_SOURCE, run

patches = ["runtime-port.patch"]
if "--no-reseed" not in sys.argv:
    patches.append("random-reseed.patch")
if QJS_PORT.exists():
    shutil.rmtree(QJS_PORT)
shutil.copytree(QJS_SOURCE, QJS_PORT)
for name in patches:
    run(["patch", "-p1", "--forward", "-i", HERE / name], cwd=QJS_PORT)
    print("applied", name)
examples = QJS_PORT / "crates/core/examples"
examples.mkdir(exist_ok=True)
shutil.copyfile(HERE / "p3_build.rs", examples / "p3_build.rs")
print(QJS_PORT)
