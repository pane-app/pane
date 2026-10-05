"""Whether a workbench binary was built from the sources in the tree (#103:
an old binary must not pass unnoticed).

Cargo decides freshness when the workbench builds, but `-SkipBuild` (and a
helper run on its own) reuses whatever binary is in `target/`. So after
every build the workbench records, beside the binary, the SHA-256 of the
binary and a digest of the build's inputs (every tracked or new file under
`crates/`, plus the workspace's `Cargo.toml`, `Cargo.lock` and toolchain
file). A later run that skips the build verifies both: a binary someone
rebuilt by hand, or sources edited or checked out since, refuse the run
until it builds again. The digest covers more than the one binary needs
(tests, other binaries), which can only make a skipped build refuse; a
build always records afresh.

    python freshness.py record --binary target/debug/pane-visual-fixture.exe --repo .
    python freshness.py verify --binary target/debug/pane-visual-fixture.exe --repo .
"""
import argparse
import datetime
import hashlib
import json
import subprocess
import sys
from pathlib import Path

INPUTS = ["crates", "Cargo.toml", "Cargo.lock", "rust-toolchain.toml", "rust-toolchain"]


def sha256_file(path):
    digest = hashlib.sha256()
    with open(path, "rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest().upper()


def digest_files(root, relative_paths):
    """One digest over the files' paths and contents, in path order; a
    listed file that no longer exists counts as deleted."""
    digest = hashlib.sha256()
    for relative in sorted(set(p.replace("\\", "/") for p in relative_paths)):
        path = Path(root, relative)
        content = sha256_file(path) if path.is_file() else "deleted"
        digest.update(f"{relative}\0{content}\n".encode("utf-8"))
    return digest.hexdigest().upper()


def tracked_inputs(repo):
    """Tracked and new (not ignored) files under the build's inputs."""
    listed = subprocess.run(["git", "ls-files", "-co", "--exclude-standard", "--", *INPUTS], cwd=repo,
                            capture_output=True, text=True, check=True).stdout
    return [line for line in listed.splitlines() if line.strip()]


def sidecar_for(binary):
    binary = Path(binary)
    return binary.with_name(binary.stem + ".inputs.json")


def verdict(record, binary_sha256, inputs_sha256):
    """None when the record vouches for this binary and these inputs, or
    the reason it doesn't."""
    if record is None:
        return "no build record beside the binary: it was not built by the workbench"
    if record.get("binarySha256") != binary_sha256:
        return (f"the binary is {binary_sha256[:12]}..., but the workbench built "
                f"{str(record.get('binarySha256'))[:12]}...: it was rebuilt or replaced outside the workbench")
    if record.get("inputsSha256") != inputs_sha256:
        return "the sources changed since the binary was built: it is older than the tree"
    return None


def record(binary, repo):
    entry = {
        "binary": str(Path(binary).resolve()),
        "binarySha256": sha256_file(binary),
        "inputsSha256": digest_files(repo, tracked_inputs(repo)),
        "recordedUtc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
    }
    sidecar_for(binary).write_text(json.dumps(entry, indent=2), encoding="utf-8")
    return entry


def verify(binary, repo):
    sidecar = sidecar_for(binary)
    saved = json.loads(sidecar.read_text(encoding="utf-8")) if sidecar.exists() else None
    binary_sha = sha256_file(binary)
    inputs_sha = digest_files(repo, tracked_inputs(repo))
    reason = verdict(saved, binary_sha, inputs_sha)
    return {"fresh": reason is None, "reason": reason, "binarySha256": binary_sha, "inputsSha256": inputs_sha,
            "recordedUtc": saved.get("recordedUtc") if saved else None}


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("action", choices=["record", "verify"])
    parser.add_argument("--binary", required=True)
    parser.add_argument("--repo", required=True)
    args = parser.parse_args()
    if not Path(args.binary).is_file():
        print(json.dumps({"fresh": False, "reason": f"no binary at {args.binary}"}))
        return 2
    if args.action == "record":
        entry = record(args.binary, args.repo)
        print(json.dumps({"fresh": True, "reason": None, **entry}))
        return 0
    result = verify(args.binary, args.repo)
    print(json.dumps(result))
    return 0 if result["fresh"] else 2


if __name__ == "__main__":
    sys.exit(main())
