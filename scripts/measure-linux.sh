#!/usr/bin/env bash
# Resource and latency measurement of Pane's whole process tree on X11 (#4).
# One fixed workload, documented in docs/research/resource-measurements.md:
# a cold start (a fresh data folder, nothing installed), warm restarts, an
# idle core, Pane hidden with its default extensions (#189), four
# installed-but-unused extensions, a root answer run repeatedly, the
# Watching continuing service and the Counting scheduled work running, and
# repeated reload and disable of one package. Pane's whole process tree —
# Pane and any helper it started, and every thread of it — is sampled from
# /proc at a fixed cadence (scripts/proc_tree.py watch) while the workload
# runs, and the record is written beside the workload's screenshots:
# samples.jsonl (one JSON line per sample), events.jsonl, summary.json and
# record.json in the output folder. The summary is checked against
# scripts/resource-targets.json (the proposed targets; a null ceiling is
# pending, not checked).
#
# Requires what scripts/smoke-linux.sh requires: Xvfb, xdotool, Pillow (the
# one screenshot check), a Vulkan driver, and for Settings, where the
# lifecycle's reloads and disables happen (#168), dbus-launch (dbus-x11),
# at-spi2-core, python3-gi and gir1.2-atspi-2.0; plus the guests built
# (`cargo xtask ci` or `cargo xtask guests`) for the workload's sample
# packages, and the default extensions' repositories, which the hidden
# phase clones at the commits the committed pins name and serves on
# 127.0.0.1. The
# hidden phase sets them up from those repositories on 127.0.0.1 and
# wires no application update source, so the measured Pane is offered no
# update of its own. The binary measured
# is the one given (target/debug/pane by default, the smoke's; record.json
# records the profile: a release build's first setup fetches its default
# extensions from the repositories its committed pins name, so the
# release profile's workload waits for a network the runner may not
# give).
# Set PANE_XVFB / PANE_XDOTOOL to use binaries outside PATH.
#
# The workload's fixed durations can be shortened for a quick run; the
# documented record is the defaults: PANE_MEASURE_IDLE_SECONDS (60),
# PANE_MEASURE_HIDDEN_SECONDS (60), PANE_MEASURE_SETTLE_SECONDS (30),
# PANE_MEASURE_UNUSED_SECONDS (60), PANE_MEASURE_SERVICE_SECONDS (90),
# PANE_MEASURE_CALCULATOR_RUNS (10), PANE_MEASURE_WARM_RESTARTS (3),
# PANE_MEASURE_RELOADS (6), PANE_MEASURE_DISABLES (5),
# PANE_MEASURE_SAMPLE_SECONDS (1).
# Usage: scripts/measure-linux.sh <output-dir> [pane-binary]
set -euo pipefail
out=${1:-measure}
pane=${2:-target/debug/pane}
here=$(cd "$(dirname "$0")" && pwd)
sampler=$here/proc_tree.py
xvfb=${PANE_XVFB:-Xvfb}
xdotool=${PANE_XDOTOOL:-xdotool}
idle_seconds=${PANE_MEASURE_IDLE_SECONDS:-60}
hidden_seconds=${PANE_MEASURE_HIDDEN_SECONDS:-60}
settle_seconds=${PANE_MEASURE_SETTLE_SECONDS:-30}
unused_seconds=${PANE_MEASURE_UNUSED_SECONDS:-60}
service_seconds=${PANE_MEASURE_SERVICE_SECONDS:-90}
calculator_runs=${PANE_MEASURE_CALCULATOR_RUNS:-10}
warm_restarts=${PANE_MEASURE_WARM_RESTARTS:-3}
reloads=${PANE_MEASURE_RELOADS:-6}
disables=${PANE_MEASURE_DISABLES:-5}
sample_seconds=${PANE_MEASURE_SAMPLE_SECONDS:-1}
# The four packages installed but never invoked: the applications sample
# (whose host-side scan of desktop entries runs, as it does for a user's
# Pane) and the three language samples.
unused_packages=(sample-applications-js sample-rust sample-js sample-ts)
# Pane's default extensions (#60), set up at the hidden phase's first
# setup from the repositories the workload clones at the commits the
# committed pins name and serves on 127.0.0.1.
default_extensions=(calculator applications quicklinks files clipboard-history)
# A development build takes its default extensions' pins from PANE_DEFAULTS;
# the hidden phase names its own pins, and every other phase names none:
# first setup adds nothing to their measurements. The pins file is the
# first thing written into the output folder, which is made for it.
mkdir -p "$out"
no_default_pins=$out/no-default-pins.json
printf '[]\n' >"$no_default_pins"
export PANE_DEFAULTS=$no_default_pins

[ -x "$pane" ] || { echo "no pane binary at $pane (cargo build -p pane)"; exit 1; }
# The hidden phase needs the guests' assembled sample packages and the
# default extensions' repositories, cloned at the commits the committed
# pins name; the rest needs the guests too.
skipped=
if [ ! -f target/guests/packages/sample-rust/pane.json ]; then
  echo "SKIPPED the hidden phase (#189): the guests are not built (cargo xtask guests)" >&2
  skipped=hidden-idle
fi
for package in "${unused_packages[@]}" sample-service sample-schedule; do
  [ -f "target/guests/packages/$package/pane.json" ] || { echo "target/guests/packages/$package is missing (cargo xtask guests)"; exit 1; }
done
for component in target/guests/sample_rust.wasm target/guests/sample_js.wasm; do
  [ -f "$component" ] || { echo "$component is missing (cargo xtask guests)"; exit 1; }
done
# The sampler's logic is checked without a display before it is trusted.
python3 "$sampler" selfcheck

rm -rf "$out/data" "$out/hidden-data" "$out/hidden-home" "$out/lifecycle-data" "$out/lifecycle-package"
export PANE_DATA_DIR=$out/data
{ grep PRETTY_NAME /etc/os-release; uname -srm; } >"$out/system.txt"   # the measured OS and architecture

# The machine-readable environment record: build settings included.
profile=release; case "$pane" in *debug*) profile=debug;; esac
python3 - "$out/record.json" "$pane" "$profile" "$idle_seconds" "$unused_seconds" \
  "$service_seconds" "$calculator_runs" "$warm_restarts" "$reloads" "$disables" \
  "$sample_seconds" "$hidden_seconds" "$settle_seconds" "$skipped" <<'PY'
import json, os, subprocess, sys
(out, pane, profile, idle, unused, service, calculator, warm, reloads, disables, cadence,
 hidden, settle, skipped) = sys.argv[1:]
record = {
    # Phases the workload skipped, their inputs absent: `proc_tree.py check`
    # reports them as skipped rather than as not measured.
    "skipped": skipped.split() if skipped else [],
    "format": 1,
    "date": subprocess.run(["date", "-Is"], capture_output=True, text=True).stdout.strip(),
    "os": open("/etc/os-release", encoding="utf-8").read(),
    "kernel": subprocess.run(["uname", "-srm"], capture_output=True, text=True).stdout.strip(),
    "pane": {
        "path": os.path.abspath(pane),
        "bytes": os.path.getsize(pane),
        "profile": profile,   # debug by default: a release build's first setup fetches its default extensions from the repositories its committed pins name
    },
    "commit": "",
    "rustc": "",
    "workload": {
        "idleSeconds": int(idle), "unusedSeconds": int(unused), "serviceSeconds": int(service),
        "calculatorRuns": int(calculator), "warmRestarts": int(warm), "reloads": int(reloads),
        "disables": int(disables), "sampleSeconds": float(cadence),
        "hiddenSeconds": int(hidden), "settleSeconds": int(settle),
        "unusedPackages": ["sample-applications-js", "sample-rust", "sample-js", "sample-ts"],
        "defaultExtensions": ["calculator", "applications", "quicklinks", "files", "clipboard-history"],
    },
}
for key, command in (("commit", ["git", "rev-parse", "HEAD"]), ("rustc", ["rustc", "--version"])):
    try:
        record[key] = subprocess.run(command, capture_output=True, text=True, timeout=30).stdout.strip()
    except (OSError, subprocess.SubprocessError):
        pass   # git or rustc may be absent; what ran is recorded without them
with open(out, "w", encoding="utf-8") as handle:
    json.dump(record, handle, indent=2, sort_keys=True)
PY

# Starts Xvfb on a display no other server uses, and uses it only once it
# is up (the smoke's dance; a display another server has is never used).
xvfb_pid=
pane_pid=
sampler_pid=
repository_server_pid=
a11y_bus_pid=
dbus_pid=
cleanup() {
  [ -n "$pane_pid" ] && kill "$pane_pid" 2>/dev/null || true
  [ -n "$sampler_pid" ] && kill "$sampler_pid" 2>/dev/null || true
  [ -n "$repository_server_pid" ] && kill "$repository_server_pid" 2>/dev/null || true
  [ -n "$a11y_bus_pid" ] && kill "$a11y_bus_pid" 2>/dev/null || true
  [ -n "$dbus_pid" ] && kill "$dbus_pid" 2>/dev/null || true
  [ -n "$xvfb_pid" ] && kill "$xvfb_pid" 2>/dev/null || true
}
trap cleanup EXIT
display=
for _ in $(seq 10); do
  number=$((90 + RANDOM % 100))
  socket=/tmp/.X11-unix/X$number
  [ -e "$socket" ] || [ -e "/tmp/.X$number-lock" ] && continue
  "$xvfb" ":$number" -screen 0 1280x800x24 -nolisten tcp 2>"$out/xvfb.log" &
  xvfb_pid=$!
  for _ in $(seq 50); do
    kill -0 "$xvfb_pid" 2>/dev/null || break
    [ -S "$socket" ] && break
    sleep 0.1
  done
  if kill -0 "$xvfb_pid" 2>/dev/null && [ -S "$socket" ]; then
    display=:$number
    break
  fi
  kill "$xvfb_pid" 2>/dev/null || true
  xvfb_pid=
done
[ -n "$display" ] || { echo "Xvfb did not start (see $out/xvfb.log)"; exit 1; }
export DISPLAY=$display
unset WAYLAND_DISPLAY
if command -v xdpyinfo >/dev/null; then
  xdpyinfo >/dev/null || { echo "Xvfb on $display does not answer"; exit 1; }
fi

# Settings (#168) is driven through its accessibility tree, as the smoke
# drives it (see scripts/smoke-linux.sh): a D-Bus session bus of the
# workload's own, the accessibility bus on it, reported enabled so that
# Pane registers its windows there. PANE_A11Y_PYTHON names another
# interpreter than the system's python3.
a11y_python=${PANE_A11Y_PYTHON:-/usr/bin/python3}
"$a11y_python" -c 'import gi; gi.require_version("Atspi", "2.0"); from gi.repository import Atspi' 2>/dev/null \
  || { echo "Settings is driven through AT-SPI: $a11y_python needs python3-gi and gir1.2-atspi-2.0"; exit 1; }
eval "$(dbus-launch --sh-syntax)" || { echo "no D-Bus session bus for AT-SPI (dbus-launch, from dbus-x11)"; exit 1; }
dbus_pid=$DBUS_SESSION_BUS_PID
# Not `ls a b | head`: ls fails for the path that is missing, and pipefail
# ends the script there, silently.
bus_launcher=
for candidate in /usr/libexec/at-spi-bus-launcher /usr/lib/at-spi2-core/at-spi-bus-launcher; do
  [ -x "$candidate" ] && { bus_launcher=$candidate; break; }
done
[ -n "$bus_launcher" ] || { echo "no at-spi-bus-launcher (at-spi2-core)"; exit 1; }
"$bus_launcher" --launch-immediately 2>>"$out/at-spi.log" &
a11y_bus_pid=$!
sleep 1
dbus-send --session --print-reply --dest=org.a11y.Bus /org/a11y/bus \
  org.freedesktop.DBus.Properties.Set string:org.a11y.Status string:IsEnabled variant:boolean:true >/dev/null \
  || { echo "the accessibility bus did not start (see $out/at-spi.log)"; exit 1; }

capture() {
  if command -v import >/dev/null; then
    import -window root "$out/$1"
  else
    python3 -c 'import sys; from PIL import ImageGrab; ImageGrab.grab(xdisplay=sys.argv[1]).save(sys.argv[2])' \
      "$display" "$out/$1"
  fi
}
check() { python3 "$here/check_screenshot.py" "$out/$1" "$2" ${3:+"$3"}; }

click_at() { "$xdotool" mousemove "$1" "$2" click 1; }
# Focusing a window that is not viewable is an X error (BadMatch) that ends
# xdotool and the script, as the smoke's run 37698693722 did after
# Settings closed: X window $1 takes the keyboard once it is viewable, and
# the launcher is summoned with the Open Pane hotkey (Ctrl+Alt+Space) where
# it is not shown (see scripts/smoke-linux.sh).
viewable() { "$xdotool" search --onlyvisible --pid "$pane_pid" 2>/dev/null | grep -qx "$1"; }
focus_window() {
  local _
  for _ in $(seq 50); do
    if viewable "$1" && "$xdotool" windowfocus --sync "$1" 2>/dev/null; then return 0; fi
    sleep 0.1
  done
  return 1
}
focus_launcher() {
  focus_window "$window" && return 0
  "$xdotool" key ctrl+alt+space; sleep 1
  focus_window "$window" || { echo "the launcher window is not shown, even after the Open Pane hotkey"; exit 1; }
}
# Back to a blank root search, with the return to root key (Shift+Escape).
to_root() { "$xdotool" key shift+Escape; sleep 1; }
# Hides the launcher as a user does, with Escape at a blank root search
# (where a start leaves it), until X window $window is no longer viewable.
# Never focus_launcher after it: the Open Pane hotkey would show it again.
hide_launcher() {
  local _
  for _ in 1 2 3; do
    viewable "$window" || return 0
    focus_window "$window" && "$xdotool" key Escape
    sleep 1
  done
  viewable "$window" && { echo "the launcher did not hide on Escape"; exit 1; }
  return 0
}
# Extensions are managed in Settings (#168), one page per extension, whose
# switch and Actions menu answer the pointer: they are found by their
# accessible names and invoked, as the smoke does (scripts/smoke-linux.sh
# documents the names). a11y <verb> <name> [prefix]: "press", "toggle"
# (the switch of that name), "shown" or "absent", in the Settings window.
a11y() {
  local at
  at=$("$a11y_python" - "$pane_pid" "$@" <<'PY'
import sys
import time

import gi

gi.require_version("Atspi", "2.0")
from gi.repository import Atspi

Atspi.init()
pid, verb, name = int(sys.argv[1]), sys.argv[2], sys.argv[3]
prefix = sys.argv[4:5] == ["prefix"]
# What a switch is to AT-SPI: AccessKit gives a switch and a toggle button
# TOGGLE_BUTTON (a checkbox CHECK_BOX), never the role of the sidebar entry,
# the group page's item or the page's heading of the same name.
SWITCHES = {getattr(Atspi.Role, role) for role in ("TOGGLE_BUTTON", "CHECK_BOX", "SWITCH")
            if hasattr(Atspi.Role, role)}


def settings():
    desktop = Atspi.get_desktop(0)
    for i in range(desktop.get_child_count()):
        app = desktop.get_child_at_index(i)
        if app is None or app.get_process_id() != pid:
            continue
        for j in range(app.get_child_count()):
            window = app.get_child_at_index(j)
            if window is not None and window.get_name() == "Settings":
                return window
    return None


def walk(node):
    yield node
    for i in range(node.get_child_count()):
        child = node.get_child_at_index(i)
        if child is not None:
            yield from walk(child)


def find():
    window = settings()
    if window is None:
        return None
    window.clear_cache()   # what the window shows now, not what was read before
    for node in walk(window):
        label = node.get_name() or ""
        if not (label.startswith(name) if prefix else label == name):
            continue
        if verb == "toggle" and node.get_role() not in SWITCHES:
            continue
        return node
    return None


def found():
    try:
        return find()
    except Exception:   # a node that went away while the tree was read
        return None


if verb == "absent":
    time.sleep(1)
    sys.exit(1 if found() else 0)
for _ in range(600):
    node = found()
    if node is not None:
        break
    time.sleep(0.1)
else:
    sys.exit(f"Settings shows nothing named {name}")
if verb in ("press", "toggle"):
    action = node.get_action_iface()
    if action is not None and action.get_n_actions() > 0:
        action.do_action(0)
    else:
        box = node.get_extents(Atspi.CoordType.SCREEN)
        print(box.x + box.width // 2, box.y + box.height // 2)
PY
  ) || { [ "$1" = absent ] && echo "Settings still shows $2"; exit 1; }
  if [ -n "$at" ]; then click_at $at; fi
  case $1 in press|toggle) sleep 1;; esac
}
# The Settings window's X11 id, if it is open.
settings_window() { "$xdotool" search --all --pid "$pane_pid" --name '^Settings$' 2>/dev/null | head -1; }
# Opens the Settings page of the extension titled $1 from root search:
# "manage" finds the Manage Extensions command, which opens Settings at
# its Extensions group, and its sidebar entry opens the page.
open_extension() {
  focus_launcher
  to_root
  "$xdotool" key ctrl+a
  "$xdotool" type --delay 50 manage; sleep 1
  "$xdotool" key Return; sleep 2
  a11y press "$1"
  a11y shown "Actions for $1"
}
# Closes Settings (Ctrl+W) and goes back to a blank root search.
close_settings() {
  local settings
  settings=$(settings_window)
  if [ -n "$settings" ]; then
    focus_window "$settings" || { echo "Settings cannot take the keyboard"; exit 1; }; sleep 0.5
    "$xdotool" key ctrl+w; sleep 1
  fi
  focus_launcher
  to_root
}

# The sampler watches the whole tree of the pid the control file names, in
# the phase the control file names, until it reads "stop".
control=$out/control
samples=$out/samples.jsonl
events=$out/events.jsonl
: >"$control"
: >"$samples"
: >"$events"
current_phase=setup
set_phase() {   # the workload is in this phase from now on
  current_phase=$1
  printf '%s %s\n' "$1" "${pane_pid:-0}" >>"$control"
}
event() {   # event <name> <milliseconds>: a latency the workload measured
  printf '{"phase": "%s", "event": "%s", "latency_ms": %s}\n' "$current_phase" "$1" "$2" >>"$events"
}
note() {   # note <key> <number>: a fact about the phase (counts, sizes)
  printf '{"phase": "%s", "%s": %s}\n' "$current_phase" "$1" "$2" >>"$events"
}
python3 "$sampler" watch "$sample_seconds" "$control" "$samples" &
sampler_pid=$!

# Starts Pane with the given arguments, focuses its window and measures how
# long the window took to appear; the sampler is pointed at the new pid.
start_pane() {
  local started
  started=$(date +%s%N)
  "$pane" "$@" 2>>"$out/stderr.log" &
  pane_pid=$!
  printf '%s %s\n' "$current_phase" "$pane_pid" >>"$control"   # the sampler sees the tree from its first moments
  window=
  for _ in $(seq 100); do
    window=$("$xdotool" search --onlyvisible --pid "$pane_pid" 2>/dev/null | head -1) && [ -n "$window" ] && break
    sleep 0.2
  done
  [ -n "$window" ] || { echo "Pane window did not appear"; exit 1; }
  event window $(( ($(date +%s%N) - started) / 1000000 ))
  sleep 2
}

stop_pane() {
  kill -0 "$pane_pid" || { echo "Pane exited during the workload"; exit 1; }
  kill "$pane_pid"
  wait "$pane_pid" 2>/dev/null || true
  pane_pid=
}

# Waits until file $1 holds text $2 ("present") or no longer does ("absent"),
# trying $4 times, a tenth of a second apart (300 by default).
wait_for() {
  for _ in $(seq "${4:-300}"); do
    if grep -q "$2" "$1" 2>/dev/null; then [ "$3" = present ] && return; else [ "$3" = absent ] && return; fi
    sleep 0.1
  done
  echo "$1: $2 is not $3"; exit 1
}

# Installs a package the way the smoke does: one start, Enter on Install,
# then the record in installed.json before Pane stops.
install_package() {
  start_pane --install "target/guests/packages/$1"
  focus_launcher
  "$xdotool" key Return
  wait_for "$PANE_DATA_DIR/extensions/installed.json" "packages/$1" present; sleep 1
  stop_pane
}

# 1. Cold start: a fresh data folder, nothing installed.
set_phase cold-start
start_pane
capture 1-cold-root.png
check 1-cold-root.png hint   # the hint line: text renders
stop_pane

# 2. Warm restarts: the same data folder, still nothing installed.
set_phase warm-start
for run in $(seq "$warm_restarts"); do
  start_pane
  [ "$run" -lt "$warm_restarts" ] && stop_pane   # the last one stays for the idle phase
done

# 3. Idle core: nothing installed, nothing invoked, Pane untouched.
set_phase idle-core
sleep "$idle_seconds"
capture 2-idle-root.png
stop_pane

# 4. Hidden idle (#189): Pane with its default extensions and nothing
# else, hidden after the one show of its start and left alone, as it sits
# in the background most of a user's day. A data folder of its own; the
# defaults are set up at its first setup from their repositories, cloned
# at the commits the committed pins name (the workload's own setup on the
# runner; the Pane under test fetches only from 127.0.0.1) and served as
# the smoke serves them, so the phase measures what a release installs.
# Files' index covers an empty folder of the workload's
# (PANE_TEST_FILE_INDEX_HOME), not the runner's home, so the phase
# measures Pane idling, not a first walk of a home folder. The launcher is
# hidden while the setup runs; the phase starts once the five defaults this
# system's pins install are recorded (the Windows-only three are pinned
# "windows" and never fetched here) and Pane has settled for
# PANE_MEASURE_SETTLE_SECONDS. Whatever
# still runs then (the applications' icons, say) is part of the phase, and
# the per-thread wake-ups and CPU in summary.json say whose it is. No
# application update source is named, so no update of Pane's own is
# offered (the smoke leaves a newer Pane package in the artifacts it
# serves): an update offered would be no part of idling. Skipped
# without the guests (see above); its lines are left unindented.
if [ -z "$skipped" ]; then
set_phase hidden-setup
main_data=$PANE_DATA_DIR
export PANE_DATA_DIR=$out/hidden-data
mkdir -p "$out/hidden-home"
file_index_home=$(cd "$out/hidden-home" && pwd)
export PANE_TEST_FILE_INDEX_HOME=$file_index_home
hidden_repositories=$out/hidden-repositories
rm -rf "$hidden_repositories"
mkdir -p "$hidden_repositories"
rm -f "$out/hidden-repository-server.port"
python3 "$here/repository_server.py" serve "$hidden_repositories" \
  "$out/hidden-repository-server.port" 2>>"$out/hidden-repository-server.log" &
repository_server_pid=$!
for _ in $(seq 600); do [ -s "$out/hidden-repository-server.port" ] && break; kill -0 "$repository_server_pid" 2>/dev/null || break; sleep 0.1; done
[ -s "$out/hidden-repository-server.port" ] \
  || { echo "the default extensions' repository server did not start (see $out/hidden-repository-server.log)"; exit 1; }
# The clones reach the real repositories, the workload's own setup on this
# computer, as the smoke's; a failure ends the workload, as a failure to
# make them from the guests' packages did.
python3 "$here/repository_server.py" clone-defaults crates/pane/defaults.json \
  "$hidden_repositories" "$out/hidden-pins.json" \
  "http://127.0.0.1:$(cat "$out/hidden-repository-server.port")/"
export PANE_DEFAULTS=$out/hidden-pins.json
start_pane
hide_launcher
# Generous, as the smoke's: a slow runner may take a while to check every
# revision's components.
for default_ in "${default_extensions[@]}"; do
  wait_for "$PANE_DATA_DIR/extensions/installed.json" "\"default\": \"$default_\"" present 6000
done
sleep "$settle_seconds"
set_phase hidden-idle
note default_extensions "${#default_extensions[@]}"
sleep "$hidden_seconds"
if viewable "$window"; then echo "the launcher showed itself during the hidden phase"; exit 1; fi
stop_pane
# The server may have ended already: under `set -e` a failed kill must not
# end the workload.
kill "$repository_server_pid" 2>/dev/null || true
wait "$repository_server_pid" 2>/dev/null || true
repository_server_pid=
unset PANE_DEFAULTS PANE_TEST_FILE_INDEX_HOME
export PANE_DEFAULTS=$no_default_pins
export PANE_DATA_DIR=$main_data
fi   # the hidden phase

# 5. Installed but unused: the four packages, none of them invoked. The
# applications sample's host-side scan of desktop entries runs, as it
# does for a user's Pane; no command is opened and nothing is typed.
set_phase install-unused-packages
for package in "${unused_packages[@]}"; do
  install_package "$package"
done
start_pane
set_phase installed-unused
note unused_packages "${#unused_packages[@]}"
sleep "$unused_seconds"
capture 3-installed-root.png
stop_pane

# 6. A root answer, run repeatedly: a query typed into root search, Enter
# copies the answer, Escape clears it. The Rust sample computes the
# answers ("reverse <text>"), as the calculator does its arithmetic for a
# user's Pane; the first answer is checked to be sure the workload runs
# it.
start_pane
set_phase calculator
focus_launcher
"$xdotool" type --delay 50 'reverse 42'; sleep 2
capture 4-calculator-answer.png
check 4-calculator-answer.png answer   # the selected answer card
"$xdotool" key Return; sleep 1
"$xdotool" key Escape; sleep 1
for run in $(seq $((calculator_runs - 1))); do
  "$xdotool" type --delay 50 "reverse $((run * 11))"; sleep 1
  "$xdotool" key Return; sleep 1
  "$xdotool" key Escape; sleep 1
done
stop_pane

# 7. Continuing work: the Watching service (a cycle a second) and the
# Counting schedule (a run a minute) keep running while Pane sits at root
# search. Their counts in the packages' content are the proof the work ran.
set_phase install-continuing-work
install_package sample-service
install_package sample-schedule
start_pane
set_phase continuing-work
sleep "$service_seconds"
counts=$(python3 - "$PANE_DATA_DIR/extensions/content.json" <<'PY'
import json, sys
packages = json.load(open(sys.argv[1], encoding="utf-8"))["packages"]
cycles = next((int(v["cycles"]) for v in packages.values() if "cycles" in v), 0)
runs = next((int(v["count"]) for v in packages.values() if "count" in v), 0)
print(cycles, runs)
PY
)
read -r service_cycles schedule_runs <<<"$counts"
note service_cycles "$service_cycles"
note schedule_runs "$schedule_runs"
[ "$service_cycles" -ge 10 ] || { echo "the service ran only $service_cycles cycles"; exit 1; }
[ "$schedule_runs" -ge 1 ] || { echo "the schedule ran no work"; exit 1; }
# Both commands opened once, so the record holds what their screens show.
focus_launcher
"$xdotool" type --delay 50 watch; sleep 2
"$xdotool" key Return; sleep 2
capture 5-watching.png
"$xdotool" key Escape; sleep 1
"$xdotool" type --delay 50 count; sleep 2
"$xdotool" key Return; sleep 2
capture 6-counting.png
"$xdotool" key Escape; sleep 1
stop_pane

# 8. Repeated lifecycle: one package of its own in a fresh data folder,
# managed on its page in Settings (#168), which stays open through both
# phases. Each reload (Reload, an item of the page's Actions menu) swaps
# the component between the Rust and the JavaScript sample and waits until
# Pane replaced the managed copy; each disable cycle disables and enables
# the package again with the page's switch.
export PANE_DATA_DIR=$out/lifecycle-data
rm -rf "$PANE_DATA_DIR"
package=$out/lifecycle-package
mkdir -p "$package"
cp target/guests/sample_rust.wasm "$package/command.wasm"
cat >"$package/pane.json" <<'JSON'
{
  "manifestVersion": 1,
  "title": "Measure",
  "apiVersion": "0.1",
  "commands": [{ "id": "sample", "title": "Measure sample", "component": "command.wasm" }]
}
JSON
set_phase install-lifecycle-package
start_pane --install "$package"
focus_launcher
"$xdotool" key Return
wait_for "$PANE_DATA_DIR/extensions/installed.json" lifecycle-package present; sleep 1
set_phase reload
# Waits until Pane reloaded the new component: the managed copy is it.
wait_reloaded() {
  for _ in $(seq 300); do
    managed=$(find "$PANE_DATA_DIR/extensions/packages" -name command.wasm | head -1)
    if [ -n "$managed" ] && cmp -s "$package/command.wasm" "$managed"; then
      sleep 2; return
    fi
    sleep 0.5
  done
  echo "Pane did not reload the package"; exit 1
}
open_extension Measure
for run in $(seq "$reloads"); do
  if [ $((run % 2)) = 0 ]; then cp target/guests/sample_rust.wasm "$package/command.wasm";
  else cp target/guests/sample_js.wasm "$package/command.wasm"; fi
  a11y press "Actions for Measure"
  a11y press Reload   # Reload Measure
  wait_reloaded
done
set_phase disable
for run in $(seq "$disables"); do
  a11y toggle Measure   # the page's switch: disable
  wait_for "$PANE_DATA_DIR/extensions/installed.json" '"disabled": true' present
  a11y toggle Measure   # the same switch: enable again
  wait_for "$PANE_DATA_DIR/extensions/installed.json" '"disabled": true' absent
done
close_settings
capture 7-lifecycle-root.png

stop_pane

# The record, the summary and the check against the proposed targets.
printf 'stop\n' >>"$control"
wait "$sampler_pid"
python3 "$sampler" summary "$samples" "$events" "$out/summary.json" "$out/record.json"
python3 "$sampler" check "$out/summary.json" "$here/resource-targets.json"
echo "record in $out"
