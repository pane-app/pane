"""Asserts that Pane's installed.json records exactly one package from Git,
at the commit given, from the reference named and pinned as named: the
smokes' Git phase, reading the record as JSON rather than matching its
text. The defaults are the phase's install of the tag v0.1.0 (pinned);
`--ref refs/heads/release --unpinned` checks a tracked branch instead, as
the automatic-update phase installs.

Usage: python3 scripts/check_git_record.py [--ref <gitRef>] [--unpinned]
       <installed.json> <commit id>
"""
import argparse
import json
import sys


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--ref", default="refs/tags/v0.1.0")
    parser.add_argument("--unpinned", action="store_true")
    parser.add_argument("record")
    parser.add_argument("commit")
    args = parser.parse_args()
    with open(args.record, encoding="utf-8") as f:
        packages = json.load(f)["packages"]
    from_git = [package for package in packages if "git" in package]
    if len(from_git) != 1:
        sys.exit(f"{args.record}: {len(from_git)} packages from Git, not one: {from_git}")
    package = from_git[0]
    expected = {"gitRef": args.ref, "gitCommit": args.commit, "pinned": not args.unpinned}
    wrong = {key: package.get(key) for key, value in expected.items() if package.get(key) != value}
    if wrong:
        sys.exit(f"{args.record}: expected {expected}, found {wrong} in {package}")


if __name__ == "__main__":
    main()
