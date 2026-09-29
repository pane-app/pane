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

Only the smokes' own data folders are touched, never the user's.
"""
import json
import os
import sys
import time

FILE = "clipboard-history.json"
DAY_MS = 86_400_000


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
            if not only or item["text"] in only:
                item["copiedAt"] = now_ms() - int(float(days) * DAY_MS)
    write(extensions, history)


def texts(extensions):
    history = read(extensions)
    return [
        item["text"]
        for package in history["packages"].values()
        for item in package.get("items", [])
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
    else:
        sys.exit(f"unknown command {command}")


if __name__ == "__main__":
    main()
