#!/usr/bin/env bash
# Installs Pane for one user: the Pane.app bundle in ~/Applications, built
# around the pane program and this package's Info.plist. The default is the
# user's own Applications folder, so no administrator rights are needed;
# name another folder with --app-dir.
#
# Run from the folder the package unpacked (pane/), or from anywhere: the
# folder is the one this script is in, or the one given as an argument. The
# install copies the program and the bundle's Info.plist into
# <app-dir>/Pane.app/Contents and runs `pane --version` to check what it
# installed — the one check macOS has to offer, since a missing library the
# program needs only shows when it runs.
#
# The package holds none of Pane's default extensions: Pane downloads them
# itself at first setup (see the README beside this script).
#
# Nothing is signed (no Apple Developer credentials exist). macOS checks
# Gatekeeper only on files that carry its quarantine mark, which a web
# browser or mail program sets when it downloads the package: the first
# Pane.app this script installs from such a package is blocked as an app
# macOS cannot check until the user allows it in System Settings (Privacy
# & Security). A package built on this machine carries no mark, and the
# script's `pane --version` check runs the program directly, which Gatekeeper
# does not stop.
set -euo pipefail

usage() {
  cat <<'EOF'
usage: install.sh [--app-dir DIR] [PACKAGE-FOLDER]
  --app-dir DIR  install Pane.app into DIR instead of ~/Applications (default)
  PACKAGE-FOLDER  the folder the package unpacked, when not running in it
EOF
}

app_dir=${HOME:-$PWD}/Applications
folder=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
arguments=()
while [ $# -gt 0 ]; do
  case $1 in
    --app-dir)
      [ $# -ge 2 ] || { echo "install.sh: --app-dir needs a directory"; exit 1; }
      app_dir=$2
      shift 2
      ;;
    -h | --help)
      usage
      exit 0
      ;;
    *)
      arguments+=("$1")
      shift
      ;;
  esac
done
[ ${#arguments[@]} -le 1 ] || { usage; exit 1; }
[ ${#arguments[@]} -eq 1 ] && folder=${arguments[0]}

program=$folder/pane
plist=$folder/Info.plist
for file in "$program" "$plist" "$folder/README.txt"; do
  [ -f "$file" ] || { echo "install.sh: $file is missing; run this in the folder the package unpacked"; exit 1; }
done

bundle=$app_dir/Pane.app
mkdir -p "$bundle/Contents/MacOS"
install -m 0755 "$program" "$bundle/Contents/MacOS/pane"
install -m 0644 "$plist" "$bundle/Contents/Info.plist"

echo "Installed Pane to $bundle (remove it to uninstall; Pane keeps its"
echo "own data in ~/Library/Application Support/Pane)."
"$bundle/Contents/MacOS/pane" --version
