"""Asserts that Pane's installed.json records exactly one package from Git,
installed from the tag v0.1.0 (pinned) at the commit given: the smokes' Git
phase, reading the record as JSON rather than matching its text.

Usage: python3 scripts/check_git_record.py <installed.json> <commit id>
"""
import json
import sys


def main(record: str, commit: str) -> None:
    with open(record, encoding="utf-8") as f:
        packages = json.load(f)["packages"]
    from_git = [package for package in packages if "git" in package]
    if len(from_git) != 1:
        sys.exit(f"{record}: {len(from_git)} packages from Git, not one: {from_git}")
    package = from_git[0]
    expected = {"gitRef": "refs/tags/v0.1.0", "gitCommit": commit, "pinned": True}
    wrong = {key: package.get(key) for key, value in expected.items() if package.get(key) != value}
    if wrong:
        sys.exit(f"{record}: expected {expected}, found {wrong} in {package}")


if __name__ == "__main__":
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    main(sys.argv[1], sys.argv[2])
