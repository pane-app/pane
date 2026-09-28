#!/usr/bin/env bash
# Native GUI smoke on macOS: launches Pane, drives it with real key events
# (System Events via osascript, which needs the Accessibility permission the
# GitHub macOS runners grant) and captures and checks the screen.
# Requires Python 3 with Pillow for the screenshot checks.
# Usage: scripts/smoke-macos.sh <output-dir> [pane-binary]
set -euo pipefail
out=${1:-smoke}
pane=${2:-target/debug/pane}
mkdir -p "$out"
{ sw_vers; uname -m; } >"$out/system.txt"   # the tested OS version and architecture

"$pane" 2>"$out/stderr.log" &
pid=$!
trap 'kill "$pid" 2>/dev/null || true' EXIT
sleep 8

capture() { screencapture -x "$out/$1"; }
check() { python3 "$(dirname "$0")/check_screenshot.py" "$out/$1" "$2"; }
key() {  # macOS virtual key codes: 36 Return, 125 Down, 53 Escape
  osascript -e "tell application \"System Events\" to key code $1"
}
osascript -e "tell application \"System Events\" to set frontmost of (first process whose unix id is $pid) to true"
sleep 1

capture 1-root.png
check 1-root.png 8a96a3   # the hint line: text renders

# Open each sample command (Rust, JavaScript, TypeScript) and run an item.
for index in 0 1 2; do
  for ((i = 0; i < index; i++)); do key 125; done   # not seq: BSD "seq 0" prints 1 0
  key 36; sleep 3
  capture "$((index + 2))-command-$index.png"
  key 125; key 36; sleep 2
  capture "$((index + 2))-result-$index.png"
  check "$((index + 2))-result-$index.png" 9fd8a8   # the guest's answer
  key 53; sleep 1
done
capture 5-back-to-root.png
# Each command must have answered from its own guest, not the same view twice.
python3 "$(dirname "$0")/check_screenshot.py" --distinct "$out"/{2,3,4}-result-*.png

kill -0 "$pid" || { echo "Pane exited during the smoke"; exit 1; }
