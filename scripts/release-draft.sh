#!/usr/bin/env bash
# Assembles a draft release of Pane from the packages the Package jobs
# built (.github/workflows/release.yml): the package assets a release
# carries (each system's package and its .sha256, named as Pane's updater
# looks for them, ADR 0048 and 0049; and the installers beside them, the
# Windows setup, #463, and the macOS disk image and the Linux AppImage,
# #464, with theirs) and the release notes, into <out-dir>:
#
#   <out-dir>/assets/   the assets, each package checked against its
#                       .sha256 (the release workflow adds the source
#                       archive, scripts/release-source.sh, beside them)
#   <out-dir>/notes.md  .github/release-notes.md with the version filled in
#   <out-dir>/version   the version, as crates/pane/Cargo.toml names it
#
# With a tag (the release workflow passes the pushed tag), the tag must be
# v<version>: a tag that names another version than the packages report
# is refused before anything is drafted. Creates nothing on GitHub.
#
# Usage: scripts/release-draft.sh <packages-dir> <out-dir> [tag]
set -euo pipefail
packages=$1
out=$2
tag=${3:-}

version=$(sed -n 's/^version = "\(.*\)"$/\1/p' crates/pane/Cargo.toml | head -1)
[ -n "$version" ] || { echo "crates/pane/Cargo.toml names no version"; exit 1; }
if [ -n "$tag" ] && [ "$tag" != "v$version" ]; then
  echo "the tag $tag does not name the version the packages report (v$version)"
  exit 1
fi

rm -rf "$out"
mkdir -p "$out/assets"
# The packages a release carries, for the systems CI's runners build, and
# the installers beside them (the Windows setup, #463, which the updater
# never looks for; the disk image and the AppImage, #464).
for package in \
  "pane-$version-windows-x86_64.zip" \
  "Pane-Setup-$version.exe" \
  "pane-$version-macos-aarch64.zip" \
  "Pane-$version.dmg" \
  "pane-$version-linux-x86_64.tar.gz" \
  "Pane-$version-x86_64.AppImage"; do
  for asset in "$package" "$package.sha256"; do
    found=$(find "$packages" -path "*/release/$asset" -type f)
    [ "$(printf '%s\n' "$found" | grep -c .)" = 1 ] \
      || { echo "the packages hold no single $asset (found: ${found:-nothing})"; exit 1; }
    cp "$found" "$out/assets/$asset"
  done
  (cd "$out/assets" && sha256sum -c "$package.sha256") \
    || { echo "$package does not match its .sha256"; exit 1; }
done

sed "s/{version}/$version/g" .github/release-notes.md >"$out/notes.md"
printf '%s\n' "$version" >"$out/version"
echo "Pane $version: the draft's assets and notes are in $out"
ls -l "$out/assets"
