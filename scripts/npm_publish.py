"""Publish a newer version of an npm tarball a smoke already serves: reads
the tarball `src` and writes a newer version of it beside it, its
`package.json` and `pane.json` at the newer version and everything else —
the built component — as it was. The local registry (`npm_registry.py`)
reads its folder on request, so the newer version is served from the moment
it is written, tagged `latest`.

Usage: npm_publish.py <source tarball> <to version>
The source's file name ends in its version; the tarball written beside it
differs in that part. Nothing reaches the network.
"""
import io
import json
import os
import sys
import tarfile

src, to_version = sys.argv[1:3]
folder, file = os.path.split(src)
from_version = file[: -len(".tgz")].rsplit("-", 1)[1]
dst = os.path.join(folder, file.replace(f"-{from_version}.tgz", f"-{to_version}.tgz"))

with tarfile.open(src, "r:gz") as tar:
    files = [
        (member.name, tar.extractfile(member).read())
        for member in tar.getmembers()
        if member.isfile()
    ]

out = tarfile.open(dst, "w:gz")
for path, data in files:
    # The package's own version is in its `package.json` (which names the
    # package, as Pane checks) and in its `pane.json`.
    if path in ("package/package.json", "package/pane.json"):
        text = json.loads(data)
        text["version"] = to_version
        data = json.dumps(text, indent=2).encode() + b"\n"
    info = tarfile.TarInfo(path)
    info.size = len(data)
    info.mode = 0o644
    out.addfile(info, io.BytesIO(data))
out.close()
print(f"published {dst} ({to_version}) from {src} ({from_version})")
