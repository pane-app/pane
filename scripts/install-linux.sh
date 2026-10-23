#!/usr/bin/env bash
# Installs Pane for one user: the pane program in <prefix>/bin, its
# desktop entry in <prefix>/share/applications and the entry's icon, in
# each size, in <prefix>/share/icons/hicolor. The default prefix is
# $HOME/.local, so no root is needed; name another with --prefix.
#
# Run from the folder the package unpacked (pane/), or from anywhere: the
# folder is the one this script is in, or the one given as an argument.
# The install checks that the program's libraries are there, copies the
# files and runs `pane --version` to check what it installed.
#
# The package holds none of Pane's default extensions: Pane downloads them
# itself at first setup (see the README beside this script).
set -euo pipefail

usage() {
  cat <<'EOF'
usage: install.sh [--prefix DIR] [PACKAGE-FOLDER]
  --prefix DIR  install there instead of ~/.local (default)
  PACKAGE-FOLDER  the folder the package unpacked, when not running in it
EOF
}

prefix=${HOME:-$PWD}/.local
folder=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
arguments=()
while [ $# -gt 0 ]; do
  case $1 in
    --prefix)
      [ $# -ge 2 ] || { echo "install.sh: --prefix needs a directory"; exit 1; }
      prefix=$2
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
entry=$folder/pane.desktop
icons=$folder/icons/hicolor
for file in "$program" "$entry" "$folder/README.txt" "$icons/256x256/apps/pane.png"; do
  [ -f "$file" ] || { echo "install.sh: $file is missing; run this in the folder the package unpacked"; exit 1; }
done

# The libraries the program needs. A missing one is named, with the
# Ubuntu package that provides it, rather than installed: an installer
# for one user installs nothing of the system.
missing=
if command -v ldd >/dev/null 2>&1; then
  while read -r line; do
    case $line in
      *"not found"*)
        library=${line%% =>*}
        missing="$missing ${library#*}"
        ;;
    esac
  done < <(ldd "$program")
fi
if [ -n "$missing" ]; then
  echo "install.sh: these libraries are missing, and Pane cannot run without them:"
  for library in $missing; do
    echo "  $library"
  done
  cat <<'EOF'
On Ubuntu 24.04 they come from these packages (see the README):
  libxkbcommon0 libxkbcommon-x11-0 libwayland-client0 libxcb1 \
libfontconfig1 libfreetype6 libvulkan1 mesa-vulkan-drivers
EOF
  exit 1
fi

mkdir -p "$prefix/bin" "$prefix/share/applications"
install -m 0755 "$program" "$prefix/bin/pane"
install -m 0644 "$entry" "$prefix/share/applications/pane.desktop"
# The icon the entry names (Icon=pane), in every size the package holds.
for icon in "$icons"/*/apps/pane.png; do
  size=${icon#"$icons"/}
  size=${size%%/*}
  mkdir -p "$prefix/share/icons/hicolor/$size/apps"
  install -m 0644 "$icon" "$prefix/share/icons/hicolor/$size/apps/pane.png"
done

echo "Installed Pane to $prefix (remove $prefix/bin/pane,"
echo "$prefix/share/applications/pane.desktop and"
echo "$prefix/share/icons/hicolor/*/apps/pane.png to uninstall)."
"$prefix/bin/pane" --version
