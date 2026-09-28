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

# Starts Xvfb on a display no other server uses, and uses it only once it
# is up: its socket appeared after it started and it is still running. A
# display another server has (the developer's own session, a parallel
# smoke) is never used: Xvfb exits there, and another number is tried.
xvfb_pid=
pane_pid=
cleanup() {
  [ -n "$pane_pid" ] && kill "$pane_pid" 2>/dev/null || true
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
# whose command (Call from Rust, selected once installed) opens its form,
# takes the JavaScript package's identity (local: and the folder's resolved
# path) and a name, and calls that package's greet operation: "Hello, Rust,
# from JavaScript" comes from the other package's guest, started for the call.
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
"$xdotool" key Return; sleep 2   # "Greet through another extension": its form
"$xdotool" type --delay 20 "local:$(realpath target/guests/packages/sample-operations-js)"
"$xdotool" key Tab; "$xdotool" type --delay 50 Rust
"$xdotool" key Return; sleep 5   # Greet
capture 32-operation-answer.png
check 32-operation-answer.png 9fd8a8   # the JavaScript guest's answer
python3 "$(dirname "$0")/check_screenshot.py" --distinct "$out"/{31-operations-target,32-operation-answer}.png
stop_pane

# Reload a development package while Pane stays open. Its command starts as
# the Rust sample; a new build of it is the JavaScript sample. Root lists the
# three samples, Rust sample, Greeting, Calculator, Call from JavaScript, Call
# from Rust, Dev sample (the ninth row), the install row, then Manage
# extensions… last; the extension list holds the six packages (Dev is the
# sixth), then their six Reload rows (Reload Dev is the twelfth).
mkdir -p "$out/dev"
cp target/guests/sample_rust.wasm "$out/dev/command.wasm"
cat >"$out/dev/pane.json" <<'JSON'
{
  "manifestVersion": 1,
  "title": "Dev",
  "apiVersion": "0.1",
  "commands": [{ "id": "sample", "title": "Dev sample", "component": "command.wasm" }]
}
JSON
start_pane --install "$out/dev"
"$xdotool" windowfocus --sync "$window"
"$xdotool" key Return; sleep 2   # Install; Dev sample is selected
"$xdotool" key Return; sleep 3
"$xdotool" key Return; sleep 2   # "Say hello"
capture 33-dev-before.png
check 33-dev-before.png 9fd8a8   # "Hello from the Rust guest"
"$xdotool" key Escape; sleep 1
cp target/guests/sample_js.wasm "$out/dev/command.wasm"
for ((i = 0; i < 12; i++)); do "$xdotool" key Down; done   # the last row
"$xdotool" key Return; sleep 1
for ((i = 0; i < 11; i++)); do "$xdotool" key Down; done   # Reload Dev
"$xdotool" key Return; sleep 3
capture 34-reloaded.png
check 34-reloaded.png 9fd8a8   # "Reloaded Dev"
"$xdotool" key Escape; sleep 1
for ((i = 0; i < 8; i++)); do "$xdotool" key Down; done   # Dev sample
"$xdotool" key Return; sleep 3
"$xdotool" key Return; sleep 2   # "Say hello"
capture 35-dev-after.png
check 35-dev-after.png 9fd8a8   # "Hello from the JavaScript guest"
python3 "$(dirname "$0")/check_screenshot.py" --distinct "$out/33-dev-before.png" "$out/35-dev-after.png"
"$xdotool" key Escape; sleep 1

# A build that fails the install checks (here its component is missing) is
# not reloaded: the working code keeps running, exactly as before.
rm "$out/dev/command.wasm"
for ((i = 0; i < 12; i++)); do "$xdotool" key Down; done
"$xdotool" key Return; sleep 1
for ((i = 0; i < 11; i++)); do "$xdotool" key Down; done
"$xdotool" key Return; sleep 2
capture 36-not-reloaded.png
check 36-not-reloaded.png f08c8c   # "Dev was not reloaded: ..."
"$xdotool" key Escape; sleep 1
for ((i = 0; i < 8; i++)); do "$xdotool" key Down; done
"$xdotool" key Return; sleep 3
"$xdotool" key Return; sleep 2
capture 37-still-running.png
python3 "$(dirname "$0")/check_screenshot.py" --same "$out/35-dev-after.png" "$out/37-still-running.png"
"$xdotool" key Escape; sleep 1

# A build whose start fails is reported with Retry, after Reload Dev; this
# one saves a setting and fails its first start only, so Retry starts it.
cp target/guests/failing_start.wasm "$out/dev/command.wasm"
for ((i = 0; i < 12; i++)); do "$xdotool" key Down; done
"$xdotool" key Return; sleep 1
for ((i = 0; i < 11; i++)); do "$xdotool" key Down; done
"$xdotool" key Return; sleep 3
capture 38-start-failed.png
check 38-start-failed.png f08c8c   # "Reloaded Dev, but it failed to start; ..."
"$xdotool" key Down key Return; sleep 3   # Retry starting Dev
capture 39-retried.png
check 39-retried.png 9fd8a8   # "Started Dev"
stop_pane
grep -q '"start-attempted": "yes"' "$out/data/extensions/settings.json" || { echo "the failed start's setting was not kept"; exit 1; }

# The settings sample keeps one value of each kind of data: its formal style
# (settings) and "Good day to you" (cache) are saved above; its fourth and
# fifth items save a note (content) and sign in (a local credential), and its
# sixth shows all four.
start_pane
"$xdotool" windowfocus --sync "$window"
for ((i = 0; i < 4; i++)); do "$xdotool" key Down; done   # Greeting
"$xdotool" key Return; sleep 3
for ((i = 0; i < 3; i++)); do "$xdotool" key Down; done
"$xdotool" key Return; sleep 2   # "Save a note"
"$xdotool" key Down key Return; sleep 2   # "Sign in"
"$xdotool" key Down key Return; sleep 2   # "Show what Pane keeps"
capture 40-kept.png
check 40-kept.png 9fd8a8   # every value, the cached greeting included
"$xdotool" key Escape; sleep 1
stop_pane
grep -q '"note": "Water the plants"' "$out/data/extensions/content.json" || { echo "note not saved"; exit 1; }
grep -q '"token": "sample-token"' "$out/data/extensions/credentials.json" || { echo "credential not saved"; exit 1; }
grep -q '"last-greeting": "Good day to you"' "$out/data/extensions/cache.json" || { echo "greeting not cached"; exit 1; }

# Clear the settings sample's cache in Manage extensions: its row follows the
# six package rows, their six Reload rows and "Clear cache of Rust sample". Pane asks first, then deletes only the cached
# greeting, without running the extension.
start_pane
"$xdotool" windowfocus --sync "$window"
for ((i = 0; i < 12; i++)); do "$xdotool" key Down; done   # the last row
"$xdotool" key Return; sleep 1
for ((i = 0; i < 13; i++)); do "$xdotool" key Down; done
"$xdotool" key Return; sleep 1   # "Clear cache of Settings sample"
capture 41-confirm-clear-cache.png
check 41-confirm-clear-cache.png aab4c0   # what is deleted and what is kept
"$xdotool" key Return; sleep 2   # "Clear cache"
capture 42-cache-cleared.png
check 42-cache-cleared.png 9fd8a8   # "Cleared the cache of Settings sample"
"$xdotool" key Escape; sleep 1
for ((i = 0; i < 4; i++)); do "$xdotool" key Down; done   # Greeting
"$xdotool" key Return; sleep 3
for ((i = 0; i < 5; i++)); do "$xdotool" key Down; done
"$xdotool" key Return; sleep 2   # "Show what Pane keeps"
capture 43-kept-after-clear.png
check 43-kept-after-clear.png 9fd8a8   # "... Cached greeting: none"
python3 "$(dirname "$0")/check_screenshot.py" --distinct "$out/40-kept.png" "$out/43-kept-after-clear.png"
"$xdotool" key Escape; sleep 1
stop_pane
if grep -q 'Good day to you' "$out/data/extensions/cache.json"; then echo "cache not cleared"; exit 1; fi
grep -q '"greeting-style": "formal"' "$out/data/extensions/settings.json" || { echo "setting lost"; exit 1; }
grep -q '"note": "Water the plants"' "$out/data/extensions/content.json" || { echo "note lost"; exit 1; }
grep -q '"token": "sample-token"' "$out/data/extensions/credentials.json" || { echo "credential lost"; exit 1; }

# Applications, a default extension: an installed application is found by
# name in root search and Enter opens it. The application is a desktop entry
# the smoke adds in an XDG_DATA_HOME of its own (for Pane only), whose
# program writes a marker file, so nothing else is started; Pane still
# searches the system's applications too.
apps=$(cd "$out" && pwd)/apps
rm -rf "$apps"
mkdir -p "$apps/data/applications"
cat >"$apps/data/applications/pane-smoke-app.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Pane Smoke App
Exec=sh -c "echo launched > '$apps/launched'"
EOF
XDG_DATA_HOME=$apps/data start_pane --install target/guests/packages/applications
"$xdotool" windowfocus --sync "$window"
"$xdotool" key Return; sleep 2   # Install
"$xdotool" type --delay 50 'pane smoke'; sleep 3
capture 44-application.png
check 44-application.png 364355 3000   # the selected application row
"$xdotool" key Return; sleep 3
capture 45-opened.png
check 45-opened.png 9fd8a8   # "Opened Pane Smoke App"
for _ in $(seq 50); do [ -f "$apps/launched" ] && break; sleep 0.2; done
[ -f "$apps/launched" ] || { echo "the application did not run"; exit 1; }
python3 "$(dirname "$0")/check_screenshot.py" --distinct "$out"/{44-application,45-opened}.png
stop_pane

# Quicklinks, a default extension: installed, its command's form saves a
# quicklink (Quicklinks is selected once installed, and "Create quicklink" is
# its first item). After a restart, typing part of its name lists it,
# selected, and Enter opens its address with the system's link handler:
# xdg-open, with no desktop session and a script that records the address,
# instead of starting a browser, as the only handler for web links.
start_pane --install target/guests/packages/quicklinks
"$xdotool" windowfocus --sync "$window"
"$xdotool" key Return; sleep 2   # Install
"$xdotool" key Return; sleep 3   # open Quicklinks
"$xdotool" key Return; sleep 1   # Create quicklink
"$xdotool" type --delay 50 'Pane issues'
"$xdotool" key Tab
"$xdotool" type --delay 50 'https://example.com/pane-issues'
"$xdotool" key Return; sleep 2
capture 46-quicklink-saved.png
check 46-quicklink-saved.png 9fd8a8   # "Saved quicklink “Pane issues”"
"$xdotool" key Escape key Escape; sleep 1
stop_pane
printf '#!/bin/sh\necho "$1" >"%s/opened-link.txt"\n' "$out" >"$out/browser.sh"
chmod +x "$out/browser.sh"
rm -f "$out/opened-link.txt"
# Only the recording script may open the link. No desktop session may choose
# a browser: xdg-open would ask it (gio, kde-open, ...) for the user's.
# XDG_CONFIG_HOME and XDG_DATA_HOME of the smoke's own, replacing the user's,
# make the script the default for web links, and every way xdg-open finds a
# default checks those before the system's; BROWSER, its last resort, is the
# script too. XDG_DATA_DIRS and XDG_CONFIG_DIRS keep the system's folders:
# Pane's Vulkan driver is found there (/usr/share/vulkan/icd.d), and without
# one Pane has no window on CI.
unset XDG_CURRENT_DESKTOP XDG_SESSION_DESKTOP DESKTOP_SESSION GDMSESSION DBUS_SESSION_BUS_ADDRESS \
  GNOME_DESKTOP_SESSION_ID KDE_FULL_SESSION KDE_SESSION_VERSION MATE_DESKTOP_SESSION_ID
xdg=$(cd "$out" && pwd)/xdg
rm -rf "$xdg"
mkdir -p "$xdg/applications"
cat >"$xdg/applications/pane-smoke-browser.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Pane Smoke Browser
Exec=$(cd "$out" && pwd)/browser.sh %u
MimeType=x-scheme-handler/http;x-scheme-handler/https;
NoDisplay=true
EOF
printf '[Default Applications]\nx-scheme-handler/http=pane-smoke-browser.desktop\nx-scheme-handler/https=pane-smoke-browser.desktop\n' \
  >"$xdg/mimeapps.list"
cp "$xdg/mimeapps.list" "$xdg/applications/mimeapps.list"
export BROWSER="$(cd "$out" && pwd)/browser.sh" XDG_CONFIG_HOME="$xdg" XDG_DATA_HOME="$xdg"
if command -v xdg-mime >/dev/null; then
  handler=$(xdg-mime query default x-scheme-handler/https)
  [ "$handler" = pane-smoke-browser.desktop ] || { echo "web links would open with $handler, not the smoke's script"; exit 1; }
fi
start_pane
"$xdotool" windowfocus --sync "$window"
"$xdotool" type --delay 50 'pane iss'; sleep 2
capture 47-quicklink-found.png
check 47-quicklink-found.png 364355 3000   # the selected quicklink row
"$xdotool" key Return; sleep 3
capture 48-quicklink-opened.png
check 48-quicklink-opened.png 9fd8a8   # "Opened https://example.com/pane-issues"
[ "$(cat "$out/opened-link.txt")" = https://example.com/pane-issues ] || { echo "the link handler was not asked to open the quicklink"; exit 1; }
python3 "$(dirname "$0")/check_screenshot.py" --distinct "$out"/{46-quicklink-saved,47-quicklink-found,48-quicklink-opened}.png
stop_pane

# Uninstall the settings sample, keeping its saved data: its row follows the
# eight Clear cache rows. Pane asks first, showing its saved data, and the first
# choice keeps its settings and content while its copy and credential go.
# Installing the same folder again finds its formal style and note, signed out.
start_pane
"$xdotool" windowfocus --sync "$window"
for ((i = 0; i < 20; i++)); do "$xdotool" key Down; done   # the last row
"$xdotool" key Return; sleep 1
for ((i = 0; i < 25; i++)); do "$xdotool" key Down; done
"$xdotool" key Return; sleep 1   # "Uninstall Settings sample"
capture 49-confirm-uninstall.png
check 49-confirm-uninstall.png aab4c0   # what is removed and the saved data
"$xdotool" key Return; sleep 2   # "Uninstall and keep saved data"
capture 50-uninstalled.png
check 50-uninstalled.png 9fd8a8   # "Uninstalled Settings sample; its settings and content are kept"
stop_pane
grep -q '"retained"' "$out/data/extensions/installed.json" || { echo "kept data not recorded"; exit 1; }
if grep -q 'sample-token' "$out/data/extensions/credentials.json"; then echo "credential not removed"; exit 1; fi
grep -q '"greeting-style": "formal"' "$out/data/extensions/settings.json" || { echo "setting not kept"; exit 1; }
grep -q '"note": "Water the plants"' "$out/data/extensions/content.json" || { echo "note not kept"; exit 1; }
start_pane --install target/guests/packages/sample-settings
"$xdotool" windowfocus --sync "$window"
"$xdotool" key Return; sleep 2   # Install; Greeting is selected
"$xdotool" key Return; sleep 3   # open Greeting
for ((i = 0; i < 5; i++)); do "$xdotool" key Down; done
"$xdotool" key Return; sleep 2   # "Show what Pane keeps"
capture 51-reinstalled.png
check 51-reinstalled.png 9fd8a8   # "Style: formal · Note: Water the plants · Signed in: no ..."
python3 "$(dirname "$0")/check_screenshot.py" --distinct "$out/43-kept-after-clear.png" "$out/51-reinstalled.png"
"$xdotool" key Escape; sleep 1
stop_pane
if grep -q '"retained"' "$out/data/extensions/installed.json"; then echo "retained record not dropped"; exit 1; fi

# Global hotkeys: in Manage extensions, the settings sample's command,
# Greeting, is given Ctrl+Alt+G by pressing it on its hotkey screen (its row
# follows the package's state, Reload, Clear cache and Uninstall rows).
# With Pane no longer focused, pressing the hotkey opens Greeting in Pane's
# window, also after a restart; once the extension is disabled, pressing it does nothing.
# A data folder of its own keeps the rows in a known order. Only the Xvfb
# display is touched: Pane's key grab is on DISPLAY, and WAYLAND_DISPLAY is
# unset for the whole smoke.
unfocus_pane() {   # focus the root window: no Pane window has focus
  "$xdotool" windowfocus "$("$xdotool" search --maxdepth 0 '.*' 2>/dev/null | head -1)"; sleep 1
  [ "$("$xdotool" getwindowfocus 2>/dev/null)" != "$window" ] || { echo "Pane still has focus"; exit 1; }
}
press_hotkey() { "$xdotool" key ctrl+alt+g; sleep 3; }
export PANE_DATA_DIR=$out/hotkeys-data
rm -rf "$PANE_DATA_DIR"
start_pane --install target/guests/packages/sample-settings
"$xdotool" windowfocus --sync "$window"
"$xdotool" key Return; sleep 2   # Install; Greeting is selected
for ((i = 0; i < 10; i++)); do "$xdotool" key Down; done   # Manage extensions…
"$xdotool" key Return; sleep 1
"$xdotool" key Down Down Down Down Return; sleep 1   # "Hotkey for Greeting"
capture 52-hotkey-screen.png
check 52-hotkey-screen.png aab4c0   # "Press the keys that should open Greeting ..."
"$xdotool" key ctrl+alt+g; sleep 2
capture 53-hotkey-assigned.png
check 53-hotkey-assigned.png 9fd8a8   # "Ctrl+Alt+G now opens Greeting"
"$xdotool" key Escape; sleep 1   # root search
unfocus_pane
capture 54-unfocused.png
press_hotkey
capture 55-hotkey-opened.png
check 55-hotkey-opened.png 364355 3000   # Greeting's first item, selected
python3 "$(dirname "$0")/check_screenshot.py" --distinct "$out"/{53-hotkey-assigned,55-hotkey-opened}.png
stop_pane
grep -q '"ctrl+alt+g"' "$PANE_DATA_DIR/extensions/hotkeys.json" || { echo "hotkey not recorded"; exit 1; }
start_pane
unfocus_pane
press_hotkey
capture 56-hotkey-after-restart.png
check 56-hotkey-after-restart.png 364355 3000   # Greeting's first item, selected
python3 "$(dirname "$0")/check_screenshot.py" --distinct "$out"/{54-unfocused,56-hotkey-after-restart}.png
"$xdotool" windowfocus --sync "$window"   # no window manager: Pane is focused here
"$xdotool" key Escape; sleep 1
for ((i = 0; i < 10; i++)); do "$xdotool" key Down; done   # Manage extensions…
"$xdotool" key Return; sleep 1
"$xdotool" key Return; sleep 2   # disable Settings sample
"$xdotool" key Escape; sleep 1
capture 57-disabled.png   # root search
unfocus_pane
press_hotkey
"$xdotool" windowfocus --sync "$window"; sleep 1
capture 58-disabled-pressed.png   # still root search: nothing opened
python3 "$(dirname "$0")/check_screenshot.py" --same "$out"/{57-disabled,58-disabled-pressed}.png
stop_pane
echo "screenshots in $out"
