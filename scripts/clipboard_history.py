#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0 OR MIT
"""Reads and changes Pane's clipboard history file for the native smokes
(#36), as a downtime would leave it: Pane is stopped while the smoke writes.

    clipboard_history.py seed <extensions-dir> <text>=<days-ago> ...
        Writes the history of the only installed package whose folder ends
        with "clipboard-history" (its identity read from installed.json):
        history on, one item per text, copied that many days ago, newest
        first. Replaces the file.
    clipboard_history.py backdate <extensions-dir> <days-ago> [<text> ...]
        Makes the kept items with these texts (every kept item of every
        package when none is named) that many days old; days may be a
        fraction, such as 0.1 for 2.4 hours.
    clipboard_history.py texts <extensions-dir>
        Prints the kept texts, newest first, joined by commas (nothing when
        none are kept or there is no file).
    clipboard_history.py field <extensions-dir> <name>
        Prints the field <name> (such as "capture") of the only package's
        history, or nothing when it is not set.
    clipboard_history.py files <extensions-dir>
        Prints the paths of the kept files items, newest first, joined by
        commas.

On Windows Pane keeps each item's text and files encrypted with DPAPI for
the current user (#130): this script, run as the same user, decrypts them
with the same entropy as Pane. A file it seeds is version 1, as an earlier
Pane wrote it, which Pane converts when it starts.

Only the smokes' own data folders are touched, never the user's.
"""
import base64
import json
import os
import sys
import time

FILE = "clipboard-history.json"
DAY_MS = 86_400_000
# Pane's DPAPI entropy (`ENTROPY` in crates/pane-core/src/protection.rs).
ENTROPY = b"Pane extension data, protected for this user (#130)"


def unprotect(encoded):
    """The bytes Pane protected with DPAPI as `encoded` (base64)."""
    import ctypes
    from ctypes import wintypes

    class Blob(ctypes.Structure):
        _fields_ = [("cbData", wintypes.DWORD), ("pbData", ctypes.POINTER(ctypes.c_char))]

    def blob(data):
        buffer = ctypes.create_string_buffer(data, len(data))
        return Blob(len(data), ctypes.cast(buffer, ctypes.POINTER(ctypes.c_char))), buffer

    data, _data = blob(base64.b64decode(encoded))
    entropy, _entropy = blob(ENTROPY)
    out = Blob()
    CRYPTPROTECT_UI_FORBIDDEN = 1
    if not ctypes.windll.crypt32.CryptUnprotectData(
        ctypes.byref(data), None, ctypes.byref(entropy), None, None,
        CRYPTPROTECT_UI_FORBIDDEN, ctypes.byref(out),
    ):
        raise ctypes.WinError()
    try:
        return ctypes.string_at(out.pbData, out.cbData)
    finally:
        ctypes.windll.kernel32.LocalFree(out.pbData)


def copy(item):
    """What `item` holds: its text and files, decrypted if protected."""
    protected = item.get("protected")
    if protected is None:
        return {"text": item["text"], "files": item.get("files", [])}
    if "dpapi" in protected:
        held = json.loads(unprotect(protected["dpapi"]).decode("utf-8"))
    else:
        held = json.loads(protected["plain"])
    return {"text": held["text"], "files": held.get("files", [])}


def path(extensions):
    return os.path.join(extensions, FILE)


def read(extensions):
    try:
        with open(path(extensions), encoding="utf-8") as f:
            return json.load(f)
    except FileNotFoundError:
        return {"version": 1, "packages": {}}


def write(extensions, history):
    with open(path(extensions), "w", encoding="utf-8") as f:
        json.dump(history, f, indent=2)


def now_ms():
    return int(time.time() * 1000)


def seed(extensions, pairs):
    with open(os.path.join(extensions, "installed.json"), encoding="utf-8") as f:
        installed = json.load(f)
    keys = [
        "local:" + package["local"]
        for package in installed["packages"]
        if package.get("local", "").replace("\\", "/").endswith("clipboard-history")
    ]
    if len(keys) != 1:
        sys.exit(f"expected one installed clipboard-history package, found {len(keys)}")
    items = []
    for number, pair in enumerate(pairs):
        text, days = pair.rsplit("=", 1)
        items.append({"id": number, "text": text, "copiedAt": now_ms() - int(float(days) * DAY_MS)})
    items.sort(key=lambda item: -item["copiedAt"])
    history = {
        "version": 1,
        "packages": {keys[0]: {"capture": "on", "items": items, "nextId": len(items)}},
    }
    write(extensions, history)


def backdate(extensions, days, only):
    history = read(extensions)
    for package in history["packages"].values():
        for item in package.get("items", []):
            if not only or copy(item)["text"] in only:
                item["copiedAt"] = now_ms() - int(float(days) * DAY_MS)
    write(extensions, history)


def texts(extensions):
    history = read(extensions)
    return [
        copy(item)["text"]
        for package in history["packages"].values()
        for item in package.get("items", [])
    ]


def files(extensions):
    history = read(extensions)
    return [
        path
        for package in history["packages"].values()
        for item in package.get("items", [])
        for path in copy(item)["files"]
    ]


def field(extensions, name):
    packages = list(read(extensions)["packages"].values())
    if len(packages) > 1:
        sys.exit(f"expected at most one package's history, found {len(packages)}")
    value = packages[0].get(name) if packages else None
    return "" if value is None else str(value)


def main():
    command, extensions, *rest = sys.argv[1:]
    if command == "seed":
        seed(extensions, rest)
    elif command == "backdate":
        backdate(extensions, rest[0], rest[1:])
    elif command == "texts":
        print(",".join(texts(extensions)))
    elif command == "field":
        print(field(extensions, rest[0]))
    elif command == "files":
        print(",".join(files(extensions)))
    else:
        sys.exit(f"unknown command {command}")


if __name__ == "__main__":
    main()
