#!/usr/bin/env bash
# Checks the installer `cargo xtask package-<system>` built (#464) the way
# a user meets it, and that Pane starts from it: run by the Package job
# (.github/workflows/package.yml) right after the build, on the runner
# that built it.
#
#   macos  Pane-<version>.dmg: its digest; mounted read-only, it holds
#          Pane.app (program, Info.plist naming the version, icon) and a
#          link to /Applications; Pane.app copied into an Applications
#          folder of its own, as a drag in Finder copies it, answers
#          `--version` with the version.
#   linux  Pane-<version>-<arch>.AppImage: its digest and the AppImage
#          mark; it answers `--version` extracted (the runner may have no
#          FUSE) and, where FUSE is there, mounted as a user's desktop runs
#          it; and it holds AppRun, the desktop entry and its icons.
#
# Usage: scripts/check-installer.sh macos|linux [release-folder]
# The release folder defaults to target/dist/release.
set -euo pipefail
system=$1
release=${2:-target/dist/release}

version=$(sed -n 's/^version = "\(.*\)"$/\1/p' crates/pane/Cargo.toml | head -1)
[ -n "$version" ] || { echo "crates/pane/Cargo.toml names no version"; exit 1; }
expected="Pane $version"
scratch=$(mktemp -d)

case $system in
  macos)
    name=Pane-$version.dmg
    (cd "$release" && shasum -a 256 -c "$name.sha256")
    volume=$scratch/volume
    mkdir -p "$volume"
    hdiutil attach -nobrowse -readonly -noautoopen -mountpoint "$volume" "$release/$name" >/dev/null
    trap 'hdiutil detach "$volume" -quiet || hdiutil detach "$volume" -force -quiet' EXIT
    [ "$(readlink "$volume/Applications")" = /Applications ] \
      || { echo "the disk image holds no link to /Applications"; exit 1; }
    app=$volume/Pane.app/Contents
    for file in MacOS/pane Info.plist Resources/pane.icns Resources/LICENSE.txt Resources/THIRD-PARTY-NOTICES.txt; do
      [ -f "$app/$file" ] || { echo "the disk image's Pane.app holds no $file"; exit 1; }
    done
    [ -x "$app/MacOS/pane" ] || { echo "the disk image's Pane.app program is not executable"; exit 1; }
    plutil -lint "$app/Info.plist"
    for key in CFBundleShortVersionString CFBundleVersion; do
      found=$(/usr/libexec/PlistBuddy -c "Print :$key" "$app/Info.plist")
      [ "$found" = "$version" ] || { echo "Pane.app's $key is $found, not $version"; exit 1; }
    done
    [ "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$app/Info.plist")" = pane ] \
      || { echo "Pane.app's Info.plist names another program"; exit 1; }
    # As a drag onto Applications copies it: ditto keeps the bundle whole.
    applications=$scratch/Applications
    mkdir -p "$applications"
    ditto "$volume/Pane.app" "$applications/Pane.app"
    answer=$("$applications/Pane.app/Contents/MacOS/pane" --version)
    [ "$answer" = "$expected" ] || { echo "the copied Pane.app answered '$answer', not '$expected'"; exit 1; }
    echo "$name: mounted, Pane.app copied to $applications, and it answers '$answer'"
    ;;
  linux)
    arch=$(uname -m)
    name=Pane-$version-$arch.AppImage
    (cd "$release" && sha256sum -c "$name.sha256")
    appimage=$(realpath "$release/$name")
    chmod +x "$appimage"
    # The AppImage type 2 mark, at byte 8 of the ELF header.
    [ "$(head -c 11 "$appimage" | tail -c 3 | od -An -c | tr -d ' ')" = 'AI002' ] \
      || { echo "$name carries no AppImage type 2 mark"; exit 1; }
    answer=$("$appimage" --appimage-extract-and-run --version)
    [ "$answer" = "$expected" ] || { echo "the AppImage answered '$answer', not '$expected'"; exit 1; }
    if [ -e /dev/fuse ] && { command -v fusermount3 || command -v fusermount; } >/dev/null; then
      answer=$("$appimage" --version)
      [ "$answer" = "$expected" ] || { echo "the mounted AppImage answered '$answer', not '$expected'"; exit 1; }
      echo "$name answers '$answer' mounted through FUSE"
    else
      echo "no FUSE on this runner: $name ran extracted only"
    fi
    (cd "$scratch" && "$appimage" --appimage-extract >/dev/null)
    root=$scratch/squashfs-root
    [ "$(readlink "$root/AppRun")" = usr/bin/pane ] || { echo "the AppImage's AppRun is not its program"; exit 1; }
    [ -x "$root/usr/bin/pane" ] || { echo "the AppImage holds no runnable program"; exit 1; }
    for file in pane.desktop pane.png .DirIcon usr/share/applications/pane.desktop \
      usr/share/doc/pane/LICENSE.txt usr/share/doc/pane/THIRD-PARTY-NOTICES.txt; do
      [ -e "$root/$file" ] || { echo "the AppImage holds no $file"; exit 1; }
    done
    for size in 16 24 32 48 64 128 256 512; do
      [ -f "$root/usr/share/icons/hicolor/${size}x${size}/apps/pane.png" ] \
        || { echo "the AppImage holds no ${size}x${size} icon"; exit 1; }
    done
    for line in Exec=pane Icon=pane StartupWMClass=pane; do
      grep -qx "$line" "$root/pane.desktop" || { echo "the AppImage's desktop entry has no $line"; exit 1; }
    done
    if command -v desktop-file-validate >/dev/null; then
      desktop-file-validate "$root/pane.desktop"
    fi
    echo "$name: runs and answers '$expected', and holds its desktop entry and icons"
    ;;
  *)
    echo "usage: scripts/check-installer.sh macos|linux [release-folder]"
    exit 1
    ;;
esac
