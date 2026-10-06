#!/usr/bin/env bash
# Resource and latency measurement of Pane's whole process tree on X11 (#4).
# One fixed workload, documented in docs/research/resource-measurements.md:
# a cold start (a fresh data folder, nothing installed), warm restarts, an
# idle core, seven installed-but-unused extensions, the calculator run
# repeatedly, the Watching continuing service and the Counting scheduled
# work running, and repeated reload and disable of one package. Pane's whole
# process tree — Pane and any helper it started — is sampled from /proc at a
# fixed cadence (scripts/proc_tree.py watch) while the workload runs, and the
# record is written beside the workload's screenshots: samples.jsonl (one
# JSON line per sample), events.jsonl, summary.json and record.json in the
# output folder. The summary is checked against scripts/resource-targets.json
# (the proposed targets; a null ceiling is pending, not checked).
#
# Requires what scripts/smoke-linux.sh requires: Xvfb, xdotool, Pillow (the
# one screenshot check) and a Vulkan driver, plus the guests built
# (`cargo xtask ci` or `cargo xtask guests`). The binary measured is the one
# given (target/debug/pane by default, the smoke's; record.json records the
# profile: a release build reaches for the not-yet-deployed artifact source
# at start, so the release profile's workload waits for that source).
# Set PANE_XVFB / PANE_XDOTOOL to use binaries outside PATH.
#
# The workload's fixed durations can be shortened for a quick run; the
# documented record is the defaults: PANE_MEASURE_IDLE_SECONDS (60),
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
unused_seconds=${PANE_MEASURE_UNUSED_SECONDS:-60}
service_seconds=${PANE_MEASURE_SERVICE_SECONDS:-90}
calculator_runs=${PANE_MEASURE_CALCULATOR_RUNS:-10}
warm_restarts=${PANE_MEASURE_WARM_RESTARTS:-3}
reloads=${PANE_MEASURE_RELOADS:-6}
disables=${PANE_MEASURE_DISABLES:-5}
sample_seconds=${PANE_MEASURE_SAMPLE_SECONDS:-1}
# The seven packages installed but never invoked: the four default
# extensions and the three language samples.
unused_packages=(calculator quicklinks applications files sample-rust sample-js sample-ts)

[ -x "$pane" ] || { echo "no pane binary at $pane (cargo build -p pane)"; exit 1; }
for package in "${unused_packages[@]}" sample-service sample-schedule; do
  [ -f "target/guests/packages/$package/pane.json" ] || { echo "target/guests/packages/$package is missing (cargo xtask guests)"; exit 1; }
done
for component in target/guests/sample_rust.wasm target/guests/sample_js.wasm; do
  [ -f "$component" ] || { echo "$component is missing (cargo xtask guests)"; exit 1; }
done
# The sampler's logic is checked without a display before it is trusted.
python3 "$sampler" selfcheck

mkdir -p "$out"
rm -rf "$out/data" "$out/lifecycle-data" "$out/lifecycle-package"
export PANE_DATA_DIR=$out/data
{ grep PRETTY_NAME /etc/os-release; uname -srm; } >"$out/system.txt"   # the measured OS and architecture

# The machine-readable environment record: build settings included.
profile=release; case "$pane" in *debug*) profile=debug;; esac
python3 - "$out/record.json" "$pane" "$profile" "$idle_seconds" "$unused_seconds" \
  "$service_seconds" "$calculator_runs" "$warm_restarts" "$reloads" "$disables" \
  "$sample_seconds" <<'PY'
import json, os, subprocess, sys
(out, pane, profile, idle, unused, service, calculator, warm, reloads, disables, cadence) = sys.argv[1:]
record = {
    "format": 1,
    "date": subprocess.run(["date", "-Is"], capture_output=True, text=True).stdout.strip(),
    "os": open("/etc/os-release", encoding="utf-8").read(),
    "kernel": subprocess.run(["uname", "-srm"], capture_output=True, text=True).stdout.strip(),
    "pane": {
        "path": os.path.abspath(pane),
        "bytes": os.path.getsize(pane),
        "profile": profile,   # debug by default: a release build's first setup reaches for the artifact source
    },
    "commit": "",
    "rustc": "",
    "workload": {
        "idleSeconds": int(idle), "unusedSeconds": int(unused), "serviceSeconds": int(service),
        "calculatorRuns": int(calculator), "warmRestarts": int(warm), "reloads": int(reloads),
        "disables": int(disables), "sampleSeconds": float(cadence),
        "unusedPackages": ["calculator", "quicklinks", "applications", "files", "sample-rust", "sample-js", "sample-ts"],
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
cleanup() {
  [ -n "$pane_pid" ] && kill "$pane_pid" 2>/dev/null || true
  [ -n "$sampler_pid" ] && kill "$sampler_pid" 2>/dev/null || true
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

capture() {
  if command -v import >/dev/null; then
    import -window root "$out/$1"
  else
    python3 -c 'import sys; from PIL import ImageGrab; ImageGrab.grab(xdisplay=sys.argv[1]).save(sys.argv[2])' \
      "$display" "$out/$1"
  fi
}
check() { python3 "$here/check_screenshot.py" "$out/$1" "$2" ${3:+"$3"}; }

# Opens Manage extensions from root search. A blind run of Downs to root's
# end was the way in until #72's Settings… root result made itself last of
# all (it is listed whatever is installed, so this root ends with it too):
# the run now opens the Settings window instead, and the lifecycle's
# reloads and disables would never happen. Searching for the row by its
# title is order-proof: "manage" matches only the Manage extensions… row,
# which is selected when the list narrows to it, and Return opens it.
manage_extensions() {
  "$xdotool" key ctrl+a
  "$xdotool" type --delay 50 manage; sleep 1
  "$xdotool" key Return; sleep 1
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

# Waits until file $1 holds text $2 ("present") or no longer does ("absent").
wait_for() {
  for _ in $(seq 300); do
    if grep -q "$2" "$1" 2>/dev/null; then [ "$3" = present ] && return; else [ "$3" = absent ] && return; fi
    sleep 0.1
  done
  echo "$1: $2 is not $3"; exit 1
}

# Installs a package the way the smoke does: one start, Enter on Install,
# then the record in installed.json before Pane stops.
install_package() {
  start_pane --install "target/guests/packages/$1"
  "$xdotool" windowfocus --sync "$window"
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

# 4. Installed but unused: the seven packages, none of them invoked. The
# applications extension's host-side scan of desktop entries runs, as it
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

# 5. The calculator, a default extension, run repeatedly: an expression
# typed into root search, Enter copies the answer, Escape clears it. The
# first answer is checked to be sure the workload runs the calculator.
start_pane
set_phase calculator
"$xdotool" windowfocus --sync "$window"
"$xdotool" type --delay 50 '6*7'; sleep 2
capture 4-calculator-answer.png
check 4-calculator-answer.png answer   # the selected answer card
"$xdotool" key Return; sleep 1
"$xdotool" key Escape; sleep 1
for run in $(seq $((calculator_runs - 1))); do
  "$xdotool" type --delay 50 "$((run * 11))+$run"; sleep 1
  "$xdotool" key Return; sleep 1
  "$xdotool" key Escape; sleep 1
done
stop_pane

# 6. Continuing work: the Watching service (a cycle a second) and the
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
"$xdotool" windowfocus --sync "$window"
"$xdotool" type --delay 50 watch; sleep 2
"$xdotool" key Return; sleep 2
capture 5-watching.png
"$xdotool" key Escape; sleep 1
"$xdotool" type --delay 50 count; sleep 2
"$xdotool" key Return; sleep 2
capture 6-counting.png
"$xdotool" key Escape; sleep 1
stop_pane

# 7. Repeated lifecycle: one package of its own in a fresh data folder, so
# the extension list's rows are known (the package's row first, its Reload
# row second). Each reload swaps the component between the Rust and the
# JavaScript sample and waits until Pane replaced the managed copy; each
# disable cycle disables and enables the package again.
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
"$xdotool" windowfocus --sync "$window"
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
for run in $(seq "$reloads"); do
  if [ $((run % 2)) = 0 ]; then cp target/guests/sample_rust.wasm "$package/command.wasm";
  else cp target/guests/sample_js.wasm "$package/command.wasm"; fi
  manage_extensions
  "$xdotool" key Down; sleep 0.5   # Reload Measure
  "$xdotool" key Return; sleep 2
  wait_reloaded
  "$xdotool" key Escape; sleep 1
done
set_phase disable
for run in $(seq "$disables"); do
  manage_extensions
  "$xdotool" key Return; sleep 1   # the package's row: disable
  wait_for "$PANE_DATA_DIR/extensions/installed.json" '"disabled": true' present
  "$xdotool" key Return; sleep 1   # the same row: enable again
  wait_for "$PANE_DATA_DIR/extensions/installed.json" '"disabled": true' absent
  "$xdotool" key Escape; sleep 1
done
capture 7-lifecycle-root.png
stop_pane

# The record, the summary and the check against the proposed targets.
printf 'stop\n' >>"$control"
wait "$sampler_pid"
python3 "$sampler" summary "$samples" "$events" "$out/summary.json" "$out/record.json"
python3 "$sampler" check "$out/summary.json" "$here/resource-targets.json"
echo "record in $out"
