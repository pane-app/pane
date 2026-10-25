"""Recompute guests/prebuilt/manifest.json's inputs_sha256 the way
xtask/src/js_guests.rs's component_inputs does, without rebuilding.

Usage: python -I jsdigest.py <repo root> check|write
"""
import hashlib
import json
import os
import sys

TOOL_INPUTS = [
    "tools/componentize-js/pins.json",
    "tools/componentize-js/package.json",
    "tools/componentize-js/package-lock.json",
    "tools/componentize-js/wasm-parts",
    "tools/componentize-js/componentize-qjs/crates/core",
    "crates/pane-build/src",
    "xtask/src/js_guests.rs",
]
PANE_WIT = [
    "extension.wit", "commands.wit", "feedback.wit", "system.wit", "data.wit",
    "preferences.wit", "root-results.wit", "operations.wit", "applications.wit",
    "helpers.wit", "files.wit", "clipboard.wit", "service.wit", "programs.wit",
    "file-index.wit", "registrations.wit", "run.wit", "system-commands.wit",
    "typed-folder.wit", "windows.wit", "view.wit",
]
SKIP = {"node_modules", ".git"}


def tree_files(path):
    if os.path.isfile(path):
        return [path]
    out = []
    stack = [path]
    while stack:
        folder = stack.pop()
        try:
            entries = os.listdir(folder)
        except OSError:
            continue
        for name in entries:
            p = os.path.join(folder, name)
            if os.path.isdir(p):
                if name not in SKIP:
                    stack.append(p)
            else:
                out.append(p)
    return out


def parts(root, path):
    rel = os.path.relpath(path, "/")
    return [x for x in os.path.normpath(path).replace("\\", "/").split("/") if x not in ("", ".")]


def digest(root, paths):
    files = []
    for p in paths:
        for f in tree_files(p):
            if f.lower().endswith(".md"):
                continue
            files.append((parts(root, f), f))
    files.sort()
    h = hashlib.sha256()
    for _, f in files:
        rel = os.path.relpath(f, root).replace("\\", "/")
        h.update(rel.encode())
        h.update(b"\0")
        with open(f, "rb") as fh:
            h.update(fh.read().replace(b"\r\n", b"\n"))
        h.update(b"\0")
    return h.hexdigest()


def component_inputs(root, source):
    paths = [os.path.join(root, i) for i in TOOL_INPUTS]
    paths += [os.path.join(root, "wit", n) for n in PANE_WIT]
    deps = os.path.join(root, "wit", "deps")
    paths += sorted(os.path.join(deps, n) for n in os.listdir(deps))
    paths.append(os.path.join(root, "guests/js"))
    paths.append(os.path.join(root, source))
    return digest(root, paths)


def main():
    root, mode = os.path.abspath(sys.argv[1]), sys.argv[2]
    path = os.path.join(root, "guests/prebuilt/manifest.json")
    with open(path, "rb") as fh:
        raw = fh.read()
    manifest = json.loads(raw)
    changed = 0
    for name, entry in manifest["components"].items():
        new = component_inputs(root, entry["source"])
        if new != entry["inputs_sha256"]:
            changed += 1
            if mode == "write":
                raw = raw.replace(entry["inputs_sha256"].encode(), new.encode())
    print(f"{changed} of {len(manifest['components'])} differ")
    if mode == "write":
        with open(path, "wb") as fh:
            fh.write(raw)


main()
