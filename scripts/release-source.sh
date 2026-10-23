#!/usr/bin/env bash
# Writes the source archive a release of Pane carries beside its packages,
# so the release itself offers the complete source its programs are built
# from (GPL-3.0-or-later, docs/licensing.md):
# <out-dir>/pane-<version>-source.tar.gz and its .sha256. The archive
# holds, under pane-<version>/:
#
# - the repository's files at the commit checked out (`git archive`: the
#   tag's tree, for a tagged release);
# - vendor/: the source of every crate the workspace's Cargo.lock pins,
#   from crates.io and the git revisions it names (`cargo vendor --locked`),
#   with vendor/config.toml naming them, so that
#   `cargo build --release --offline --config vendor/config.toml -p pane`
#   builds the program without reaching the network.
#
# The archive is checked as its user gets it: unpacked, its workspace
# resolves every crate from vendor/ alone, offline.
#
# Run from the repository's root.
# Usage: scripts/release-source.sh <version> <out-dir>
set -euo pipefail
version=$1
out=$2
name=pane-$version-source.tar.gz
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT

mkdir -p "$out"
git archive --format=tar --prefix="pane-$version/" -o "$stage/source.tar" HEAD
# The vendored crates, named by a config whose directory is relative to the
# archive's top folder (the folder `cargo build` runs in).
mkdir -p "$stage/pane-$version"
# Its progress, a line per crate, goes to a log shown only on failure.
cargo vendor --locked --versioned-dirs "$stage/pane-$version/vendor" \
  >"$stage/vendored.toml" 2>"$stage/vendor.log" \
  || { tail -20 "$stage/vendor.log"; echo "cargo vendor failed"; exit 1; }
sed 's|^directory = .*|directory = "vendor"|' "$stage/vendored.toml" | tr -d '\r' \
  >"$stage/config.toml"
grep -q '^directory = "vendor"$' "$stage/config.toml" \
  || { echo "cargo vendor wrote no source replacement"; exit 1; }
mv "$stage/config.toml" "$stage/pane-$version/vendor/config.toml"
tar -rf "$stage/source.tar" -C "$stage" "pane-$version/vendor"

mkdir "$stage/check"
tar -xf "$stage/source.tar" -C "$stage/check"
(cd "$stage/check/pane-$version" \
  && cargo metadata --format-version 1 --offline --locked --config vendor/config.toml >/dev/null) \
  || { echo "the source archive's workspace does not resolve from its vendored crates"; exit 1; }

gzip -9 -n -c "$stage/source.tar" >"$out/$name"
(cd "$out" && sha256sum "$name" >"$name.sha256")
echo "source archive written: $out/$name ($(du -h "$out/$name" | cut -f1))"
