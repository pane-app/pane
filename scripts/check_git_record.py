"""Asserts that Pane's installed.json records exactly one package from Git,
at the commit given, from the reference named and pinned as named: the
smokes' Git phase, reading the record as JSON rather than matching its
text. The defaults are the phase's install of the tag v0.1.0 (pinned);
`--ref refs/heads/release --unpinned` checks a tracked branch instead, as
the automatic-update phase installs.

With `--defaults <pins-file>`, checks every default extension the pins
file names instead: its record keeps its default identity, and the Git
source it was fetched from — the repository, the pin's release tag, its
commit and that it is pinned — as the smokes' first-setup phases install
them.

Usage: python3 scripts/check_git_record.py [--ref <gitRef>] [--unpinned]
       <installed.json> <commit id>
       python3 scripts/check_git_record.py --defaults <pins-file>
       <installed.json>
"""
import argparse
import json
import sys


def check_git(record: str, ref: str, pinned: bool, commit: str) -> None:
    with open(record, encoding="utf-8") as f:
        packages = json.load(f)["packages"]
    from_git = [package for package in packages if "git" in package]
    if len(from_git) != 1:
        sys.exit(f"{record}: {len(from_git)} packages from Git, not one: {from_git}")
    package = from_git[0]
    # `pinned` is written only when it is true (serde skips false), so a
    # missing key means false, as the record's reader takes it.
    package.setdefault("pinned", False)
    expected = {"gitRef": ref, "gitCommit": commit, "pinned": pinned}
    wrong = {key: (package.get(key), value) for key, value in expected.items() if package.get(key) != value}
    if wrong:
        sys.exit(f"{record}: expected {expected}, found {wrong} in {package}")


def check_defaults(pins_file: str, record: str) -> None:
    with open(pins_file, encoding="utf-8") as f:
        pins = {pin["id"]: pin for pin in json.load(f)}
    with open(record, encoding="utf-8") as f:
        packages = json.load(f)["packages"]
    defaults = {package["default"]: package for package in packages if "default" in package}
    for id, pin in pins.items():
        package = defaults.get(id)
        if package is None:
            sys.exit(f"{record}: no record of the default {id}")
        expected = {
            "gitUrl": pin["repository"],
            "gitRef": "refs/tags/" + pin["tag"],
            "gitCommit": pin["commit"],
            "pinned": True,
        }
        wrong = {
            key: (package.get(key), value)
            for key, value in expected.items()
            if package.get(key) != value
        }
        if wrong:
            sys.exit(f"{record}: the record of {id} is wrong: {wrong} in {package}")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--ref", default="refs/tags/v0.1.0")
    parser.add_argument("--unpinned", action="store_true")
    parser.add_argument("--defaults", metavar="PINS-FILE")
    parser.add_argument("record")
    parser.add_argument("commit", nargs="?")
    args = parser.parse_args()
    if args.defaults:
        if args.commit:
            sys.exit("with --defaults, name the installed.json alone")
        check_defaults(args.defaults, args.record)
    else:
        if not args.commit:
            sys.exit("a commit id is needed without --defaults")
        check_git(args.record, args.ref, not args.unpinned, args.commit)


if __name__ == "__main__":
    main()
