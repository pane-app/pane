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

# Root search: typing narrows root to the matching commands and Enter opens
# the best match. "typescr" matches only TypeScript sample, whose "Wait
# briefly" answers exactly as in step 4. A query that matches nothing shows
# no results, and Enter then opens nothing.
start_pane
"$xdotool" windowfocus --sync "$window"
"$xdotool" type --delay 50 typescr; sleep 1
capture 24-search.png
"$xdotool" key Return; sleep 3
"$xdotool" key Down key Return; sleep 2
capture 25-search-result.png
check 25-search-result.png 9fd8a8   # the TypeScript guest's answer
python3 "$(dirname "$0")/check_screenshot.py" --same "$out/4-result-2.png" "$out/25-search-result.png"
"$xdotool" key Escape; sleep 1
"$xdotool" type --delay 50 zzz; sleep 1
"$xdotool" key Return; sleep 1
capture 26-no-results.png
python3 "$(dirname "$0")/check_screenshot.py" --distinct "$out"/{1-root,24-search,25-search-result,26-no-results}.png
stop_pane

# The calculator, a default extension: an expression typed into root search
# lists its answer first, selected, and Enter copies it. Pasting the copy
# over the query and typing on shows exactly the screen typing the whole
# expression shows, so the clipboard held the answer.
start_pane --install target/guests/packages/calculator
"$xdotool" windowfocus --sync "$window"
"$xdotool" key Return; sleep 2   # Install
"$xdotool" type --delay 50 '6*7'; sleep 2
capture 27-answer.png
check 27-answer.png 364355 3000   # the selected answer row
"$xdotool" key Return; sleep 1
capture 28-copied.png   # "Copied 42 to the clipboard"
"$xdotool" key ctrl+a; "$xdotool" type --delay 50 '42+1'; sleep 2
capture 29-typed.png
"$xdotool" key ctrl+a ctrl+v; "$xdotool" type --delay 50 '+1'; sleep 2
capture 30-pasted.png
python3 "$(dirname "$0")/check_screenshot.py" --distinct "$out"/{27-answer,28-copied,29-typed}.png
python3 "$(dirname "$0")/check_screenshot.py" --same "$out/29-typed.png" "$out/30-pasted.png"
stop_pane

# Operations: install the JavaScript operations sample, then the Rust one,
# whose command (Call from Rust, selected once installed) asks the JavaScript
# package's greet operation: "JavaScript answered: Hello, Rust, from
# JavaScript" comes from the other package's guest, started for the call.
start_pane --install target/guests/packages/sample-operations-js
"$xdotool" windowfocus --sync "$window"
"$xdotool" key Return; sleep 2   # Install
capture 31-operations-target.png
check 31-operations-target.png 9fd8a8   # "Installed JavaScript operations sample"
stop_pane
start_pane --install target/guests/packages/sample-operations
"$xdotool" windowfocus --sync "$window"
"$xdotool" key Return; sleep 2   # Install; Call from Rust is selected
"$xdotool" key Return; sleep 3   # open Call from Rust
"$xdotool" key Return; sleep 5   # "Ask JavaScript to greet"
capture 32-operation-answer.png
check 32-operation-answer.png 9fd8a8   # the JavaScript guest's answer
python3 "$(dirname "$0")/check_screenshot.py" --distinct "$out"/{31-operations-target,32-operation-answer}.png
stop_pane
# Quicklinks, a default extension: installed, its command's form saves a
# quicklink (Quicklinks is selected once installed, and "Create quicklink" is
# its first item). After a restart, typing part of its name lists it,
# selected, and Enter opens its address with the system's link handler. The
# handler is xdg-open outside any desktop session, with BROWSER set to a
# script that records the address instead of starting a browser.
start_pane --install target/guests/packages/quicklinks
"$xdotool" windowfocus --sync "$window"
"$xdotool" key Return; sleep 2   # Install
"$xdotool" key Return; sleep 3   # open Quicklinks
"$xdotool" key Return; sleep 1   # Create quicklink
"$xdotool" type --delay 50 'Pane issues'
"$xdotool" key Tab
"$xdotool" type --delay 50 'https://example.com/pane-issues'
"$xdotool" key Return; sleep 2
capture 33-quicklink-saved.png
check 33-quicklink-saved.png 9fd8a8   # "Saved quicklink “Pane issues”"
"$xdotool" key Escape key Escape; sleep 1
stop_pane
mkdir -p "$out/xdg"
printf '#!/bin/sh\necho "$1" >"%s/opened-link.txt"\n' "$out" >"$out/browser.sh"
chmod +x "$out/browser.sh"
rm -f "$out/opened-link.txt"
unset XDG_CURRENT_DESKTOP DESKTOP_SESSION DBUS_SESSION_BUS_ADDRESS GNOME_DESKTOP_SESSION_ID KDE_FULL_SESSION
# No desktop settings of the user choose the browser: only BROWSER.
export BROWSER="$(cd "$out" && pwd)/browser.sh" XDG_CONFIG_HOME="$out/xdg" XDG_CONFIG_DIRS="$out/xdg" \
  XDG_DATA_HOME="$out/xdg" XDG_DATA_DIRS="$out/xdg"
start_pane
"$xdotool" windowfocus --sync "$window"
"$xdotool" type --delay 50 'pane iss'; sleep 2
capture 34-quicklink-found.png
check 34-quicklink-found.png 364355 3000   # the selected quicklink row
"$xdotool" key Return; sleep 3
capture 35-quicklink-opened.png
check 35-quicklink-opened.png 9fd8a8   # "Opened https://example.com/pane-issues"
[ "$(cat "$out/opened-link.txt")" = https://example.com/pane-issues ] || { echo "the link handler was not asked to open the quicklink"; exit 1; }
python3 "$(dirname "$0")/check_screenshot.py" --distinct "$out"/{33-quicklink-saved,34-quicklink-found,35-quicklink-opened}.png
stop_pane
echo "screenshots in $out"
