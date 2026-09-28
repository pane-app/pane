#!/usr/bin/env bash
# Native GUI smoke on X11: starts a virtual X server (Xvfb), launches Pane,
# drives it with real key events and captures screenshots.
#
# Requires Xvfb, xdotool, Python 3 with Pillow (screenshot checks and, without
# ImageMagick's `import`, capture), plus a Vulkan driver (Mesa's lavapipe works without
# a GPU). Set PANE_XVFB / PANE_XDOTOOL to use binaries outside PATH. Pane keeps
# installed packages in <output-dir>/data, not the user's data folder.
# Usage: scripts/smoke-linux.sh <output-dir> [pane-binary]
set -euo pipefail
out=${1:-smoke}
pane=${2:-target/debug/pane}
xvfb=${PANE_XVFB:-Xvfb}
xdotool=${PANE_XDOTOOL:-xdotool}
mkdir -p "$out"
rm -rf "$out/data"
export PANE_DATA_DIR=$out/data
{ grep PRETTY_NAME /etc/os-release; uname -srm; } >"$out/system.txt"   # the tested OS and architecture

display=:$((90 + RANDOM % 100))
"$xvfb" "$display" -screen 0 1280x800x24 -nolisten tcp 2>"$out/xvfb.log" &
xvfb_pid=$!
pane_pid=
cleanup() {
  [ -n "$pane_pid" ] && kill "$pane_pid" 2>/dev/null || true
  kill "$xvfb_pid" 2>/dev/null || true
}
trap cleanup EXIT
export DISPLAY=$display
unset WAYLAND_DISPLAY
sleep 1

capture() {
  if command -v import >/dev/null; then
    import -window root "$out/$1"
  else
    python3 -c 'import sys; from PIL import ImageGrab; ImageGrab.grab(xdisplay=sys.argv[1]).save(sys.argv[2])' \
      "$display" "$out/$1"
  fi
}
check() { python3 "$(dirname "$0")/check_screenshot.py" "$out/$1" "$2" ${3:+"$3"}; }
# Prints "x y": where the screenshot shows the given color.
locate() { python3 "$(dirname "$0")/check_screenshot.py" --locate "$out/$1" "$2"; }
# Clicks the primary button at screen position x y (screenshot pixels: the
# screenshot is of the whole X screen).
click_at() { "$xdotool" mousemove "$1" "$2" click 1; }

# Starts Pane with the given arguments and focuses its window.
start_pane() {
  "$pane" "$@" 2>>"$out/stderr.log" &
  pane_pid=$!
  window=
  for _ in $(seq 100); do
    window=$("$xdotool" search --onlyvisible --pid "$pane_pid" 2>/dev/null | head -1) && [ -n "$window" ] && break
    sleep 0.2
  done
  [ -n "$window" ] || { echo "Pane window did not appear"; exit 1; }
  sleep 2
}

stop_pane() {
  kill -0 "$pane_pid" || { echo "Pane exited during the smoke"; exit 1; }
  kill "$pane_pid"
  wait "$pane_pid" 2>/dev/null || true
  pane_pid=
}

start_pane
capture 1-root.png
check 1-root.png 8a96a3   # the hint line: text renders
"$xdotool" windowfocus --sync "$window"

# Open each sample command (Rust, JavaScript, TypeScript) and run an item.
for index in 0 1 2; do
  for ((i = 0; i < index; i++)); do "$xdotool" key Down; done
  "$xdotool" key Return; sleep 3
  capture "$((index + 2))-command-$index.png"
  "$xdotool" key Down key Return; sleep 2
  capture "$((index + 2))-result-$index.png"
  check "$((index + 2))-result-$index.png" 9fd8a8   # the guest's answer
  "$xdotool" key Escape; sleep 1
done
capture 5-back-to-root.png
# Each command must have answered from its own guest, not the same view twice.
python3 "$(dirname "$0")/check_screenshot.py" --distinct "$out"/{2,3,4}-result-*.png

# The Rust command's form (its fifth item): submitting it empty is rejected
# and focus returns to the name, so typing there and choosing a greeting with
# Tab and Down makes the guest answer.
"$xdotool" key Return; sleep 3
for _ in 1 2 3 4; do "$xdotool" key Down; done
"$xdotool" key Return; sleep 1
capture 6-form.png
"$xdotool" key Return; sleep 2
capture 7-form-error.png
check 7-form-error.png f08c8c   # the rejected field's message
"$xdotool" type --delay 50 Ada
"$xdotool" key Tab key Down key Return; sleep 2
capture 8-form-result.png
check 8-form-result.png 9fd8a8   # the guest's answer
"$xdotool" key Escape key Escape; sleep 1
stop_pane

# Install the assembled Rust sample package (the folder the picker would
# return), then run its command. Root lists the three samples, the installed
# command, then the install and Manage extensions… rows.
start_pane --install target/guests/packages/sample-rust
"$xdotool" windowfocus --sync "$window"
capture 9-package.png
check 9-package.png aab4c0   # the package's identity and compatibility lines
"$xdotool" key Return; sleep 2
capture 10-installed.png
check 10-installed.png 9fd8a8   # "Installed Rust sample"
"$xdotool" key Return; sleep 3
"$xdotool" key Return; sleep 2
capture 11-installed-result.png
check 11-installed-result.png 9fd8a8   # the installed guest's answer
stop_pane

# The installed command is still listed after a restart.
start_pane
capture 12-restarted.png
check 12-restarted.png 8a96a3
[ -f "$out/data/extensions/installed.json" ] || { echo "no install record"; exit 1; }
"$xdotool" windowfocus --sync "$window"

# The Rust command's seventh item is declared for Windows only, its eighth
# for macOS and Linux only. Here the first is explained without running and
# the second runs.
"$xdotool" key Return; sleep 3
for _ in 1 2 3 4 5 6; do "$xdotool" key Down; done
"$xdotool" key Return; sleep 2
capture 13-windows-only.png
check 13-windows-only.png d6a36a   # the row's reason
check 13-windows-only.png f08c8c   # Linux: the reason as the error
"$xdotool" key Down key Return; sleep 2
capture 14-not-windows.png
check 14-not-windows.png 9fd8a8    # Linux: the guest's answer
"$xdotool" key Escape; sleep 1
stop_pane

# A package that supports only the other two systems has nothing for this
# one: it is explained instead of offered for installation.
mkdir -p "$out/elsewhere"
cp target/guests/sample_rust.wasm "$out/elsewhere/"
cat >"$out/elsewhere/pane.json" <<'JSON'
{
  "manifestVersion": 1,
  "title": "Elsewhere",
  "apiVersion": "0.1",
  "platforms": ["windows", "macos"],
  "commands": [{ "id": "sample", "title": "Elsewhere sample", "component": "sample_rust.wasm" }]
}
JSON
start_pane --install "$out/elsewhere"
capture 15-no-compatible-package.png
check 15-no-compatible-package.png f08c8c   # "Not available on Linux: ..."
stop_pane

# Install the settings sample, save a choice with it, then disable it in
# Manage extensions. Root lists the three samples, Rust sample, Greeting, the
# install row, then Manage extensions… last; the extension list holds Rust
# sample, then Settings sample.
start_pane --install target/guests/packages/sample-settings
"$xdotool" windowfocus --sync "$window"
"$xdotool" key Return; sleep 2   # Install; Greeting is selected
"$xdotool" key Return; sleep 3   # open Greeting
"$xdotool" key Return; sleep 2   # "Use a formal greeting"
capture 16-setting-saved.png
check 16-setting-saved.png 9fd8a8   # "Saved the formal greeting"
"$xdotool" key Escape; sleep 1
for ((i = 0; i < 10; i++)); do "$xdotool" key Down; done   # the last row
"$xdotool" key Return; sleep 1
"$xdotool" key Down key Return; sleep 2
capture 17-disabled.png
check 17-disabled.png 9fd8a8   # "Disabled Settings sample"
stop_pane
grep -q '"disabled": true' "$out/data/extensions/installed.json" || { echo "disabled state not recorded"; exit 1; }
grep -q '"greeting-style": "formal"' "$out/data/extensions/settings.json" || { echo "setting not saved"; exit 1; }

# After a restart Greeting is no longer in root search: root looks exactly as
# it did before the settings sample was installed. Enabling the package again
# brings it back with its setting: "Greet me" answers in the saved formal
# style, where without a saved style it reports an error.
start_pane
"$xdotool" windowfocus --sync "$window"
capture 18-restarted-disabled.png
check 18-restarted-disabled.png 8a96a3
python3 "$(dirname "$0")/check_screenshot.py" --same "$out/12-restarted.png" "$out/18-restarted-disabled.png"
for ((i = 0; i < 10; i++)); do "$xdotool" key Down; done
"$xdotool" key Return; sleep 1
"$xdotool" key Down key Return; sleep 2
capture 19-enabled.png
check 19-enabled.png 9fd8a8   # "Enabled Settings sample"
"$xdotool" key Escape; sleep 1
for ((i = 0; i < 4; i++)); do "$xdotool" key Down; done   # Greeting
"$xdotool" key Return; sleep 3
"$xdotool" key Down key Down key Return; sleep 2   # "Greet me"
capture 20-greeted.png
check 20-greeted.png 9fd8a8   # "Good day to you"
stop_pane

# Restarted, root lists Greeting again, after Rust sample.
start_pane
"$xdotool" windowfocus --sync "$window"

# The Rust command's color picker (its sixth item), which the guest draws:
# Right chooses purple, and a click on the dark green swatch chooses it. The
# chosen color fills its swatch and the preview, far more pixels than any
# other swatch covers.
"$xdotool" key Return; sleep 3
for _ in 1 2 3 4 5; do "$xdotool" key Down; done
"$xdotool" key Return; sleep 2
capture 21-color.png
check 21-color.png 1e88e5 3000   # blue, chosen when the view opens
"$xdotool" key Right; sleep 1
capture 22-color-key.png
check 22-color-key.png 8e24aa 3000   # purple
read -r x y < <(locate 22-color-key.png 1b5e20)
click_at "$x" "$y"; sleep 1
capture 23-color-click.png
check 23-color-click.png 1b5e20 3000   # dark green
"$xdotool" key Escape key Escape; sleep 1
stop_pane
echo "screenshots in $out"
