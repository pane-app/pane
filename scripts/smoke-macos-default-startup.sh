#!/usr/bin/env bash
# Bounded native regression for default (glass) startup, separate from the
# opaque behavior smoke. No clicks, keystrokes, refocusing or blur claims.
# Usage: bash scripts/smoke-macos-default-startup.sh <output-dir> [pane-binary]
set -euo pipefail
out=${1:-smoke/default-startup}
pane=${2:-target/debug/pane}
mkdir -p "$out"
out=$(cd "$out" && pwd)
pane=$(cd "$(dirname "$pane")" && pwd)/$(basename "$pane")
smoke_home=$(mktemp -d "$out/home.XXXXXX")
pid=
cleanup() {
  if [ -n "$pid" ]; then
    kill "$pid" 2>/dev/null || true
    wait "$pid" 2>/dev/null || true
  fi
}
trap cleanup EXIT
{ sw_vers; uname -m; printf '%s\n' 'theme/material unset: default startup (not blur evidence)'; } >"$out/system.txt"

# A development build takes its default extensions' pins from PANE_DEFAULTS;
# without it, the committed pins point at the real repositories, which no
# check may reach. This smoke checks native startup, not first setup, so
# the file names none: first setup adds nothing, and Pane reaches no
# address.
no_default_pins=$out/no-default-pins.json
printf '[]\n' >"$no_default_pins"

# env -i is deliberate: appearance defaults must work without the behavior
# smoke's PANE_MATERIAL=opaque hiding a crash in the blurred-view callback.
env -i HOME="$smoke_home" PATH=/usr/bin:/bin PANE_DATA_DIR="$smoke_home/data" \
  PANE_DEFAULTS="$no_default_pins" XDG_CACHE_HOME="$smoke_home/cache" \
  "$pane" >"$out/stdout.log" 2>"$out/stderr.log" &
pid=$!
assert_alive() {
  if ! kill -0 "$pid" 2>/dev/null; then
    cat "$out/stderr.log" >&2
    echo 'Pane exited during default macOS startup' >&2
    exit 1
  fi
}
windows=0
for _ in $(seq 30); do
  assert_alive
  windows=$(osascript -e "tell application \"System Events\" to count windows of (first process whose unix id is $pid)" 2>>"$out/window-probe.log") || windows=0
  [ "$windows" -gt 0 ] && break
  sleep 1
done
[ "$windows" -gt 0 ] || { echo 'Pane did not expose a native window' >&2; exit 1; }
# Keep pumping native layer updates after the window appears. The original
# setBackgroundColor: ABI panic aborted here, before the first smoke focus.
for _ in $(seq 10); do sleep 1; assert_alive; done
printf 'pid=%s windows=%s survived native startup and layer updates\n' "$pid" "$windows" >"$out/result.txt"
