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
# Behavior captures use a fixed palette without desktop-dependent glass.
export PANE_THEME=dark PANE_MATERIAL=opaque
out=${1:-smoke}
pane=${2:-target/debug/pane}
xvfb=${PANE_XVFB:-Xvfb}
xdotool=${PANE_XDOTOOL:-xdotool}
mkdir -p "$out"
rm -rf "$out/data"
export PANE_DATA_DIR=$out/data
{ grep PRETTY_NAME /etc/os-release; uname -srm; } >"$out/system.txt"   # the tested OS and architecture

printf "%s\n" "theme=dark material=opaque (behavior smoke; not blur evidence)" >>"$out/system.txt"

# Starts Xvfb on a display no other server uses, and uses it only once it
# is up: its socket appeared after it started and it is still running. A
# display another server has (the developer's own session, a parallel
# smoke) is never used: Xvfb exits there, and another number is tried.
xvfb_pid=
pane_pid=
npm_registry_pid=
repository_server_pid=
artifact_server_pid=
cleanup() {
  [ -n "$pane_pid" ] && kill "$pane_pid" 2>/dev/null || true
  [ -n "$npm_registry_pid" ] && kill "$npm_registry_pid" 2>/dev/null || true
  [ -n "$repository_server_pid" ] && kill "$repository_server_pid" 2>/dev/null || true
  [ -n "$artifact_server_pid" ] && kill "$artifact_server_pid" 2>/dev/null || true
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
# Captures screenshot $1 until it shows text in color $2, for at most $3
# seconds, then checks it: for a view that appears once work in the
# background ends, whenever that is.
capture_until() {
  local deadline=$((SECONDS + $3))
  while :; do
    capture "$1"
    check "$1" "$2" >/dev/null 2>&1 && return 0
    [ "$SECONDS" -lt "$deadline" ] || { check "$1" "$2"; return 1; }
    sleep 0.5
  done
}
# Prints "x y": where the screenshot shows the given color.
locate() { python3 "$(dirname "$0")/check_screenshot.py" --locate "$out/$1" "$2"; }
# Clicks the primary button at screen position x y (screenshot pixels: the
# screenshot is of the whole X screen).
click_at() { "$xdotool" mousemove "$1" "$2" click 1; }

# Opens Manage extensions from root search. A blind run of Downs to root's
# end was the way in until #72's Settings… root result made itself last of
# all (it is listed whatever is installed, so every phase's root ends with
# it): the run now opens the Settings window instead. Searching for the row
# by its title is order-proof: "manage" matches only the Manage extensions…
# row, which is selected when the list narrows to it, and Return opens it.
# Ctrl+A first, so a query an earlier step left in the field is replaced,
# not extended.
manage_extensions() {
  "$xdotool" key ctrl+a
  "$xdotool" type --delay 50 manage; sleep 1
  "$xdotool" key Return; sleep 1
}

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
  # A --install preview arrives once its check finishes; until then the
  # screen is root search. Wait for preview metadata below its heading, then
  # send the phase's first Enter to the preview:
  # run 36796103906's frame 31 lost that race (the check outlasted the
  # wait and the Enter opened root search's own first row instead).
  if [ "${1:-}" = "--install" ]; then
    for _ in $(seq 60); do
      capture preview-wait.png
      if python3 "$(dirname "$0")/check_screenshot.py" --preview "$out/preview-wait.png"; then
        break
      fi
      sleep 0.5
    done
    capture preview-wait.png
    python3 "$(dirname "$0")/check_screenshot.py" --preview "$out/preview-wait.png" \
      || { echo "the --install preview did not appear (still root search)"; exit 1; }
  fi
}

stop_pane() {
  kill -0 "$pane_pid" || { echo "Pane exited during the smoke"; exit 1; }
  kill "$pane_pid"
  wait "$pane_pid" 2>/dev/null || true
  pane_pid=
}

start_pane
capture 1-root.png
check 1-root.png hint   # the hint line: text renders
"$xdotool" windowfocus --sync "$window"

# Open each sample command (Rust, JavaScript, TypeScript) and run an item.
for index in 0 1 2; do
  for ((i = 0; i < index; i++)); do "$xdotool" key Down; done
  "$xdotool" key Return; sleep 3
  capture "$((index + 2))-command-$index.png"
  "$xdotool" key Down key Return; sleep 2
  capture "$((index + 2))-result-$index.png"
  check "$((index + 2))-result-$index.png" success   # the guest's answer
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
check 7-form-error.png error   # the rejected field's message
"$xdotool" type --delay 50 Ada
"$xdotool" key Tab key Down key Return; sleep 2
capture 8-form-result.png
check 8-form-result.png success   # the guest's answer
"$xdotool" key Escape key Escape; sleep 1
stop_pane

# Install the assembled Rust sample package (the folder the picker would
# return), then run its command. Root lists the three samples, the installed
# command, then the install and Manage extensions… rows.
start_pane --install target/guests/packages/sample-rust
"$xdotool" windowfocus --sync "$window"
capture 9-package.png
check 9-package.png details   # the package's identity and compatibility lines
"$xdotool" key Return; sleep 2
capture 10-installed.png
check 10-installed.png success   # "Installed Rust sample"
"$xdotool" key Return; sleep 3
"$xdotool" key Return; sleep 2
capture 11-installed-result.png
check 11-installed-result.png success   # the installed guest's answer
stop_pane

# The installed command is still listed after a restart.
start_pane
capture 12-restarted.png
check 12-restarted.png hint
[ -f "$out/data/extensions/installed.json" ] || { echo "no install record"; exit 1; }
"$xdotool" windowfocus --sync "$window"

# The Rust command's seventh item is declared for Windows only, its eighth
# for macOS and Linux only. Here the first is explained without running and
# the second runs.
"$xdotool" key Return; sleep 3
for _ in 1 2 3 4 5 6; do "$xdotool" key Down; done
"$xdotool" key Return; sleep 2
capture 13-windows-only.png
check 13-windows-only.png warning   # the row's reason
check 13-windows-only.png error   # Linux: the reason as the error
"$xdotool" key Down key Return; sleep 2
capture 14-not-windows.png
check 14-not-windows.png success    # Linux: the guest's answer
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
check 15-no-compatible-package.png error   # "Not available on Linux: ..."
stop_pane

# Install the settings sample, save a choice with it, then disable it in
# Manage extensions. Root lists the three samples, Rust sample, Greeting, the
# install rows, then Manage extensions… and Settings… last; the extension
# list holds Rust sample, then Settings sample.
start_pane --install target/guests/packages/sample-settings
"$xdotool" windowfocus --sync "$window"
"$xdotool" key Return; sleep 2   # Install; Greeting is selected
"$xdotool" key Return; sleep 3   # open Greeting
"$xdotool" key Return; sleep 2   # "Use a formal greeting"
capture 16-setting-saved.png
check 16-setting-saved.png success   # "Saved the formal greeting"
"$xdotool" key Escape; sleep 1
manage_extensions
"$xdotool" key Down key Return; sleep 2
capture 17-disabled.png
check 17-disabled.png success   # "Disabled Settings sample"
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
check 18-restarted-disabled.png hint
python3 "$(dirname "$0")/check_screenshot.py" --same "$out/12-restarted.png" "$out/18-restarted-disabled.png"
manage_extensions
"$xdotool" key Down key Return; sleep 2
capture 19-enabled.png
check 19-enabled.png success   # "Enabled Settings sample"
"$xdotool" key Escape; sleep 1
for ((i = 0; i < 4; i++)); do "$xdotool" key Down; done   # Greeting
"$xdotool" key Return; sleep 3
"$xdotool" key Down key Down key Return; sleep 2   # "Greet me"
capture 20-greeted.png
check 20-greeted.png success   # "Good day to you"
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
check 25-search-result.png success   # the TypeScript guest's answer
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
check 27-answer.png answer   # the selected answer card
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
check 31-operations-target.png success   # "Installed JavaScript operations sample"
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
check 32-operation-answer.png success   # the JavaScript guest's answer
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
check 33-dev-before.png success   # "Hello from the Rust guest"
"$xdotool" key Escape; sleep 1
cp target/guests/sample_js.wasm "$out/dev/command.wasm"
manage_extensions
for ((i = 0; i < 11; i++)); do "$xdotool" key Down; done   # Reload Dev
"$xdotool" key Return; sleep 3
capture 34-reloaded.png
check 34-reloaded.png success   # "Reloaded Dev"
"$xdotool" key Escape; sleep 1
for ((i = 0; i < 8; i++)); do "$xdotool" key Down; done   # Dev sample
"$xdotool" key Return; sleep 3
"$xdotool" key Return; sleep 2   # "Say hello"
capture 35-dev-after.png
check 35-dev-after.png success   # "Hello from the JavaScript guest"
python3 "$(dirname "$0")/check_screenshot.py" --distinct "$out/33-dev-before.png" "$out/35-dev-after.png"
"$xdotool" key Escape; sleep 1

# A build that fails the install checks (here its component is missing) is
# not reloaded: the working code keeps running, exactly as before.
rm "$out/dev/command.wasm"
manage_extensions
for ((i = 0; i < 11; i++)); do "$xdotool" key Down; done
"$xdotool" key Return; sleep 2
capture 36-not-reloaded.png
check 36-not-reloaded.png error   # "Dev was not reloaded: ..."
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
manage_extensions
for ((i = 0; i < 11; i++)); do "$xdotool" key Down; done
"$xdotool" key Return; sleep 3
capture 38-start-failed.png
check 38-start-failed.png error   # "Reloaded Dev, but it failed to start; ..."
"$xdotool" key Down key Return; sleep 3   # Retry starting Dev
capture 39-retried.png
check 39-retried.png success   # "Started Dev"
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
check 40-kept.png success   # every value, the cached greeting included
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
manage_extensions
for ((i = 0; i < 13; i++)); do "$xdotool" key Down; done
"$xdotool" key Return; sleep 1   # "Clear cache of Settings sample"
capture 41-confirm-clear-cache.png
check 41-confirm-clear-cache.png details   # what is deleted and what is kept
"$xdotool" key Return; sleep 2   # "Clear cache"
capture 42-cache-cleared.png
check 42-cache-cleared.png success   # "Cleared the cache of Settings sample"
"$xdotool" key Escape; sleep 1
for ((i = 0; i < 4; i++)); do "$xdotool" key Down; done   # Greeting
"$xdotool" key Return; sleep 3
for ((i = 0; i < 5; i++)); do "$xdotool" key Down; done
"$xdotool" key Return; sleep 2   # "Show what Pane keeps"
capture 43-kept-after-clear.png
check 43-kept-after-clear.png success   # "... Cached greeting: none"
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
check 44-application.png selected 3000   # the selected application row
"$xdotool" key Return; sleep 3
capture 45-opened.png
check 45-opened.png success   # "Opened Pane Smoke App"
for _ in $(seq 50); do [ -f "$apps/launched" ] && break; sleep 0.2; done
[ -f "$apps/launched" ] || { echo "the application did not run"; exit 1; }
python3 "$(dirname "$0")/check_screenshot.py" --distinct "$out"/{44-application,45-opened}.png
stop_pane

# Quicklinks, a default extension: installed, its Create Quicklink command,
# found by typing its name, opens its form, which saves a quicklink and
# returns to root search. After a restart, typing part of its name lists it,
# selected, and Enter opens its address with the system's link handler:
# xdg-open, with no desktop session and a script that records the address,
# instead of starting a browser, as the only handler for web links.
start_pane --install target/guests/packages/quicklinks
"$xdotool" windowfocus --sync "$window"
"$xdotool" key Return; sleep 2   # Install
"$xdotool" type --delay 50 'create quicklink'; sleep 2
"$xdotool" key Return; sleep 3   # open Create Quicklink's form
"$xdotool" type --delay 50 'Pane issues'
"$xdotool" key Tab
"$xdotool" type --delay 50 'https://example.com/pane-issues'
"$xdotool" key Return; sleep 2
capture 46-quicklink-saved.png
check 46-quicklink-saved.png success   # "Created “Pane issues”"
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
check 47-quicklink-found.png selected 3000   # the selected quicklink row
"$xdotool" key Return; sleep 3
capture 48-quicklink-opened.png
check 48-quicklink-opened.png success   # "Opened Pane issues"
[ "$(cat "$out/opened-link.txt")" = https://example.com/pane-issues ] || { echo "the link handler was not asked to open the quicklink"; exit 1; }
python3 "$(dirname "$0")/check_screenshot.py" --distinct "$out"/{46-quicklink-saved,47-quicklink-found,48-quicklink-opened}.png
stop_pane

# Uninstall the settings sample, keeping its saved data: its row follows the
# eight Clear cache rows. Pane asks first, showing its saved data, and the first
# choice keeps its settings and content while its copy and credential go.
# Installing the same folder again finds its formal style and note, signed out.
start_pane
"$xdotool" windowfocus --sync "$window"
manage_extensions
for ((i = 0; i < 25; i++)); do "$xdotool" key Down; done
"$xdotool" key Return; sleep 1   # "Uninstall Settings sample"
capture 49-confirm-uninstall.png
check 49-confirm-uninstall.png details   # what is removed and the saved data
"$xdotool" key Return; sleep 2   # "Uninstall and keep saved data"
capture 50-uninstalled.png
check 50-uninstalled.png success   # "Uninstalled Settings sample; its settings and content are kept"
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
check 51-reinstalled.png success   # "Style: formal · Note: Water the plants · Signed in: no ..."
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
manage_extensions
"$xdotool" key Down Down Down Down Return; sleep 1   # "Hotkey for Greeting"
capture 52-hotkey-screen.png
check 52-hotkey-screen.png details   # "Press the keys that should open Greeting ..."
"$xdotool" key ctrl+alt+g; sleep 2
capture 53-hotkey-assigned.png
check 53-hotkey-assigned.png success   # "Ctrl+Alt+G now opens Greeting"
"$xdotool" key Escape; sleep 1   # root search
unfocus_pane
capture 54-unfocused.png
press_hotkey
capture 55-hotkey-opened.png
check 55-hotkey-opened.png selected 3000   # Greeting's first item, selected
python3 "$(dirname "$0")/check_screenshot.py" --distinct "$out"/{53-hotkey-assigned,55-hotkey-opened}.png
stop_pane
grep -q '"ctrl+alt+g"' "$PANE_DATA_DIR/extensions/hotkeys.json" || { echo "hotkey not recorded"; exit 1; }
start_pane
unfocus_pane
press_hotkey
capture 56-hotkey-after-restart.png
check 56-hotkey-after-restart.png selected 3000   # Greeting's first item, selected
python3 "$(dirname "$0")/check_screenshot.py" --distinct "$out"/{54-unfocused,56-hotkey-after-restart}.png
"$xdotool" windowfocus --sync "$window"   # no window manager: Pane is focused here
"$xdotool" key Escape; sleep 1
manage_extensions
"$xdotool" key Return; sleep 2   # disable Settings sample
"$xdotool" key Escape; sleep 1
capture 57-disabled.png   # root search
unfocus_pane
press_hotkey
"$xdotool" windowfocus --sync "$window"; sleep 1
capture 58-disabled-pressed.png   # still root search: nothing opened
python3 "$(dirname "$0")/check_screenshot.py" --same "$out"/{57-disabled,58-disabled-pressed}.png
stop_pane

# Pausing a broken extension: the settings sample's last item, Crash, crashes
# on purpose; the third crash within five minutes pauses the package and
# returns to root search, where Greeting stays listed with why it does not
# run. The pause holds after a restart. In Manage extensions, the package's
# "Why ... is paused" row (after its Reload and Retry rows) shows the
# details, whose only row, Retry, starts it again. A data folder of its own
# keeps the rows in a known order.
export PANE_DATA_DIR=$out/pausing-data
rm -rf "$PANE_DATA_DIR"
start_pane --install target/guests/packages/sample-settings
"$xdotool" windowfocus --sync "$window"
"$xdotool" key Return; sleep 2   # Install; Greeting is selected
"$xdotool" key Return; sleep 2   # open Greeting
for ((i = 0; i < 7; i++)); do "$xdotool" key Down; done   # Crash
for ((i = 0; i < 3; i++)); do "$xdotool" key Return; sleep 2; done
"$xdotool" type --delay 50 greet; sleep 1   # Greeting and its reason at the top on any window height
capture 59-paused.png
check 59-paused.png error   # "Settings sample crashed 3 times within 5 minutes and is paused ..."
check 59-paused.png warning   # Greeting: "Settings sample is paused after an error; ..."
stop_pane
grep -q '"paused"' "$PANE_DATA_DIR/extensions/installed.json" || { echo "pause not recorded"; exit 1; }
start_pane
"$xdotool" windowfocus --sync "$window"
"$xdotool" type --delay 50 greet; sleep 1
capture 60-paused-after-restart.png
check 60-paused-after-restart.png warning   # Greeting is still paused
"$xdotool" key Escape; sleep 1   # clears the query
manage_extensions
"$xdotool" key Down Down Down Return; sleep 1   # "Why Settings sample is paused"
capture 61-pause-details.png
check 61-pause-details.png details   # the details
"$xdotool" key Return; sleep 2   # Retry Settings sample
capture 62-pause-retried.png
check 62-pause-retried.png success   # "Started Settings sample"
python3 "$(dirname "$0")/check_screenshot.py" --distinct "$out"/{61-pause-details,62-pause-retried}.png
stop_pane
if grep -q '"paused"' "$PANE_DATA_DIR/extensions/installed.json"; then echo "pause not cleared"; exit 1; fi

# Delete retained data: with a data folder of its own, the settings sample
# saves a note and is uninstalled keeping it (its Uninstall row follows its
# state, Reload and Clear cache rows); its retained data, the extension
# list's first row with nothing else installed, already selected when the
# list opens, is deleted after confirming (Cancel is selected first, so Down
# then Return), without the extension. Installing the same folder again finds
# nothing. Steps that change Pane's files wait for the change instead of a
# fixed time.
export PANE_DATA_DIR=$out/retained-data
rm -rf "$PANE_DATA_DIR"
# Waits until file $1 contains text $2 ("present") or no longer does ("absent").
# Waits up to a tenth of a second times `tries` (100 by default) for
# `grep -e $2 $1` to be found (present) or gone (absent).
wait_for() {
  local tries=${4:-100}
  for _ in $(seq "$tries"); do
    if grep -q "$2" "$1" 2>/dev/null; then [ "$3" = present ] && return; else [ "$3" = absent ] && return; fi
    sleep 0.1
  done
  echo "$1: $2 is not $3"; exit 1
}
registry=$PANE_DATA_DIR/extensions/installed.json
start_pane --install target/guests/packages/sample-settings
"$xdotool" windowfocus --sync "$window"
"$xdotool" key Return   # Install; Greeting is selected
wait_for "$registry" sample-settings present; sleep 1
"$xdotool" key Return; sleep 3   # open Greeting
for ((i = 0; i < 3; i++)); do "$xdotool" key Down; done
"$xdotool" key Return   # "Save a note"
wait_for "$PANE_DATA_DIR/extensions/content.json" '"note": "Water the plants"' present
"$xdotool" key Escape; sleep 1   # root search
manage_extensions
"$xdotool" key Down Down Down Return; sleep 1   # "Uninstall Settings sample"
"$xdotool" key Return   # "Uninstall and keep saved data"
wait_for "$registry" '"retained"' present; sleep 1
"$xdotool" key Return; sleep 1   # "Delete retained data of Settings sample" (the list's first row, already selected)
capture 63-confirm-delete-retained.png
check 63-confirm-delete-retained.png details   # what is kept and what is not touched
# The confirmation's status line is the idle hint, not a result: the
# extension list also shows hint subtitles, so that color alone let the
# wrong screen pass (#58: Down to the list's end had landed on the
# automatic-update row, whose Enter toggles it and leaves its result on
# screen). No result color on screen says the right screen is up.
python3 "$(dirname "$0")/check_screenshot.py" --absent "$out/63-confirm-delete-retained.png" success
"$xdotool" key Down Return   # "Delete retained data"
wait_for "$registry" '"retained"' absent; sleep 1
capture 64-retained-deleted.png
check 64-retained-deleted.png success   # "Deleted the retained data of Settings sample"
stop_pane
if grep -q 'Water the plants' "$PANE_DATA_DIR/extensions/content.json"; then echo "note not deleted"; exit 1; fi
start_pane --install target/guests/packages/sample-settings
"$xdotool" windowfocus --sync "$window"
"$xdotool" key Return   # Install; Greeting is selected
wait_for "$registry" sample-settings present; sleep 1
"$xdotool" key Return; sleep 3   # open Greeting
for ((i = 0; i < 5; i++)); do "$xdotool" key Down; done
"$xdotool" key Return; sleep 2   # "Show what Pane keeps"
capture 65-reinstalled-empty.png
check 65-reinstalled-empty.png success   # "Style: none · Note: none · Signed in: no ..."
python3 "$(dirname "$0")/check_screenshot.py" --distinct "$out/51-reinstalled.png" "$out/65-reinstalled-empty.png"
"$xdotool" key Escape; sleep 1
stop_pane

# Aliases and fallbacks: in Manage extensions, the query sample's command,
# Echo, is given the alias "ec" (its row follows the package's state, Reload,
# Clear cache, Uninstall and hotkey rows) and made a fallback (the next row).
# In root search, "ec hello" lists the row that sends "hello" to Echo,
# selected, and Enter shows Echo's answer; text nothing matches lists "No
# results" with Echo below it, not selected, until Down selects it and Enter
# sends the text. After a restart with the extension disabled, "ec hello"
# lists nothing: the same screen as a Pane with nothing installed. Data
# folders of their own keep the rows in a known order.
export PANE_DATA_DIR=$out/aliases-data
rm -rf "$PANE_DATA_DIR"
start_pane --install target/guests/packages/sample-query
"$xdotool" windowfocus --sync "$window"
"$xdotool" key Return; sleep 2   # Install; Echo is selected
manage_extensions
for ((i = 0; i < 5; i++)); do "$xdotool" key Down; done   # "Alias for Echo"
"$xdotool" key Return; sleep 1
"$xdotool" type --delay 50 'ec'
"$xdotool" key Return; sleep 2
capture 66-alias-saved.png
check 66-alias-saved.png success   # "Typing “ec” now finds Echo"
"$xdotool" key Down Return; sleep 2   # "Fallback: Echo"
capture 67-fallback-on.png
check 67-fallback-on.png success   # "Echo is now offered for any text typed in root search"
python3 "$(dirname "$0")/check_screenshot.py" --distinct "$out"/{66-alias-saved,67-fallback-on}.png
"$xdotool" key Escape; sleep 1   # root search
"$xdotool" type --delay 50 'ec hello'; sleep 1
capture 68-alias-row.png
check 68-alias-row.png selected 3000   # Echo, sending “hello”, selected
"$xdotool" key Return; sleep 3
capture 69-alias-answer.png
check 69-alias-answer.png success   # "Echo heard “hello”"
"$xdotool" key Escape; sleep 1   # clears the query
"$xdotool" type --delay 50 'zqx'; sleep 1
capture 70-fallback-listed.png   # "No results for “zqx”", then Echo, not selected
"$xdotool" key Down; sleep 1
capture 71-fallback-chosen.png
check 71-fallback-chosen.png selected 3000   # Echo, now selected
python3 "$(dirname "$0")/check_screenshot.py" --distinct "$out"/{70-fallback-listed,71-fallback-chosen}.png
"$xdotool" key Return; sleep 3
capture 72-fallback-answer.png
check 72-fallback-answer.png success   # "Echo heard “zqx”"
python3 "$(dirname "$0")/check_screenshot.py" --distinct "$out"/{69-alias-answer,72-fallback-answer}.png
stop_pane
grep -q '"ec"' "$PANE_DATA_DIR/extensions/aliases.json" || { echo "alias not recorded"; exit 1; }
grep -q '#echo"' "$PANE_DATA_DIR/extensions/aliases.json" || { echo "fallback not recorded"; exit 1; }
start_pane
"$xdotool" windowfocus --sync "$window"
manage_extensions
"$xdotool" key Return; sleep 2   # disable Query sample
"$xdotool" key Escape; sleep 1
"$xdotool" type --delay 50 'ec hello'; sleep 1
capture 73-alias-disabled.png   # "No results for “ec hello”"
stop_pane
grep -q '"disabled": true' "$PANE_DATA_DIR/extensions/installed.json" || { echo "not disabled"; exit 1; }
export PANE_DATA_DIR=$out/aliases-empty-data
rm -rf "$PANE_DATA_DIR"
start_pane
"$xdotool" windowfocus --sync "$window"
"$xdotool" type --delay 50 'ec hello'; sleep 1
capture 74-nothing-installed.png   # "No results for “ec hello”"
python3 "$(dirname "$0")/check_screenshot.py" --same "$out"/{73-alias-disabled,74-nothing-installed}.png
stop_pane

# Dependencies: the dependencies sample requires the JavaScript operations
# sample (from ../sample-operations-js) and can use the Rust one, which is
# optional. Its preview lists both; Install installs it with the JavaScript
# sample only, and its command (selected once installed) calls that
# package's greet operation by its dependency id: "Hello, Pane, from
# JavaScript" comes from the other package's guest. A data folder of its own
# starts with nothing installed.
export PANE_DATA_DIR=$out/dependencies-data
rm -rf "$PANE_DATA_DIR"
start_pane --install target/guests/packages/sample-dependencies
"$xdotool" windowfocus --sync "$window"
capture 75-dependencies-preview.png
check 75-dependencies-preview.png details   # "Requires: JavaScript operations sample, installed with it ..."
"$xdotool" key Return; sleep 3   # Install; Greet through dependencies is selected
capture 76-dependencies-installed.png
check 76-dependencies-installed.png success   # "Installed Dependencies sample with JavaScript operations sample, which it requires"
"$xdotool" key Return; sleep 3   # open Greet through dependencies
"$xdotool" key Return; sleep 5   # Greet through the required greeter
capture 77-dependency-answer.png
check 77-dependency-answer.png success   # the JavaScript guest's answer
python3 "$(dirname "$0")/check_screenshot.py" --distinct "$out"/{75-dependencies-preview,76-dependencies-installed,77-dependency-answer}.png
stop_pane
grep -q '"id": "greeter"' "$PANE_DATA_DIR/extensions/installed.json" || { echo "dependency not recorded"; exit 1; }
[ "$(grep -c '"dir"' "$PANE_DATA_DIR/extensions/installed.json")" = 2 ] || { echo "not exactly two packages installed"; exit 1; }

# Native helpers: the helper sample's command runs pane-echo, the file its
# package ships for this system (built by `cargo xtask guests`). Its first
# item shows the helper's answer, naming the system; its third races the
# helper against a one-second timer and cancels it. Its second has the
# helper wait ten seconds: disabling the package meanwhile (its row is the
# first in Manage extensions) ends the helper's process at once, and the
# note it saved before is kept. A data folder of its own keeps the rows in a
# known order; the helper runs from its managed copy there.
export PANE_DATA_DIR=$out/helper-data
rm -rf "$PANE_DATA_DIR"
# Pane's helper processes: pane-echo run from this data folder.
helpers_running() { pgrep -f "$PANE_DATA_DIR/extensions/packages/.*/pane-echo" >/dev/null; }
start_pane --install target/guests/packages/sample-helper
"$xdotool" windowfocus --sync "$window"
"$xdotool" key Return; sleep 2   # Install; Helper sample is selected
"$xdotool" key Return; sleep 2   # open Helper sample
"$xdotool" key Return   # Echo through the helper
# The helper is a process Pane starts and waits for; a cold spawn on a
# loaded runner can outlast a fixed sleep, so the answer is waited for.
capture_until 90-helper-echoed.png success 15   # 'Echoed "hello from Pane" on Linux x86-64'
"$xdotool" key Down Down Return; sleep 3   # Echo within a second
capture 91-helper-cancelled.png
check 91-helper-cancelled.png success   # "Stopped the helper after one second"
python3 "$(dirname "$0")/check_screenshot.py" --distinct "$out"/{90-helper-echoed,91-helper-cancelled}.png
if helpers_running; then echo "a cancelled helper is still running"; exit 1; fi
"$xdotool" key Up Return; sleep 2   # Echo after waiting
helpers_running || { echo "the waiting helper is not running"; exit 1; }
capture 92-helper-waiting.png
"$xdotool" key Escape; sleep 1   # root search; the helper keeps running
manage_extensions
"$xdotool" key Return; sleep 2   # disable Helper sample
capture 93-helper-disabled.png
check 93-helper-disabled.png success   # "Disabled Helper sample"
if helpers_running; then echo "the helper outlived its disabled package"; exit 1; fi
grep -q '"helper-wait": "started"' "$PANE_DATA_DIR/extensions/settings.json" || { echo "saved note lost"; exit 1; }
if grep -q '"helper-wait": "finished"' "$PANE_DATA_DIR/extensions/settings.json"; then echo "the stopped call finished"; exit 1; fi
stop_pane
if helpers_running; then echo "a helper outlived Pane"; exit 1; fi

# Quitting Pane while a helper runs ends it: with "Echo after waiting"
# running (the helper beats in pane-echo.alive in its folder of the managed
# copy), closing the window the way a window manager asks quits Pane, which
# ends the helper first. A data folder of its own again.
export PANE_DATA_DIR=$out/helper-quit-data
rm -rf "$PANE_DATA_DIR"
start_pane --install target/guests/packages/sample-helper
"$xdotool" windowfocus --sync "$window"
"$xdotool" key Return; sleep 2   # Install; Helper sample is selected
"$xdotool" key Return; sleep 2   # open Helper sample
"$xdotool" key Down Return; sleep 2   # Echo after waiting
helpers_running || { echo "the waiting helper is not running"; exit 1; }
capture 94-helper-before-quit.png
check 94-helper-before-quit.png progress   # "Running…"
alive=$(find "$PANE_DATA_DIR/extensions/packages" -name pane-echo.alive | head -1)
[ -n "$alive" ] || { echo "the waiting helper does not beat"; exit 1; }
python3 "$(dirname "$0")/close_window.py" "$window"
for _ in $(seq 50); do kill -0 "$pane_pid" 2>/dev/null || break; sleep 0.1; done
if kill -0 "$pane_pid" 2>/dev/null; then echo "Pane did not quit when its window closed"; exit 1; fi
wait "$pane_pid" 2>/dev/null || true
pane_pid=
if helpers_running; then echo "a helper outlived Pane quitting"; exit 1; fi
beats=$(stat -c %s "$alive"); sleep 0.5
[ "$(stat -c %s "$alive")" = "$beats" ] || { echo "the helper still beats after Pane quit"; exit 1; }

# Development mode (#12, #13): a copy of each development sample
# (guests/hello-rust, hello-ts, hello-js) is built once, installed and
# developed from Manage extensions ("Develop <title>", the row above the
# list's last: #49's global automatic-update choice is last of all now, and
# the develop row no longer is). Saving
# an edit of its greeting builds it with the documented command and reloads
# it while Pane keeps running; a save that does not build keeps the working
# code and shows the error; two saves in a row (the second while the first
# builds) end with the newer greeting; after "Stop developing", a save builds
# nothing. Each sample has a data folder of its own, so root lists the three
# built-in samples, then its command, the install and Manage extensions…
# rows. The JavaScript and TypeScript samples need the JS toolchain
# (guests/README.md) and are skipped without it.
set_greeting() {   # set_greeting <source file> <line replacing the greeting's>
  python3 - "$1" "$2" <<'PY'
import re, sys
path, line = sys.argv[1], sys.argv[2]
text = open(path, encoding="utf-8").read()
text = re.sub(r"^const GREETING.*$", lambda _: line, text, count=1, flags=re.M)
open(path, "w", encoding="utf-8").write(text)
PY
}
# Waits until Pane has reloaded a new build: the component built in the
# copy differs from $2 (the one before the save) and the managed copy is it.
wait_reloaded() {   # wait_reloaded <built component> <component before the save>
  for _ in $(seq 600); do
    managed=$(find "$PANE_DATA_DIR/extensions/packages" -name "$(basename "$1")" | head -1)
    if [ -n "$managed" ] && ! cmp -s "$1" "$2" && cmp -s "$1" "$managed"; then
      sleep 3; return
    fi
    sleep 0.5
  done
  echo "Pane did not reload $1"; exit 1
}
# Waits until Pane has reported one more build that did not build.
wait_failed() {   # wait_failed <failures before>
  for _ in $(seq 600); do
    [ "$(grep -c 'did not build' "$out/stderr.log")" -gt "$1" ] && { sleep 1; return; }
    sleep 0.5
  done
  echo "Pane did not report the failed build"; exit 1
}
say_hello() {   # from root: open the developed command, the 4th row, and run its item
  "$xdotool" key Down Down Down Return; sleep 3
  "$xdotool" key Return; sleep 2
}
develop_sample() {   # develop_sample <sample> <title> <component> <source> <first frame> <greeting line> <broken line>
  local sample=$1 title=$2 component=$3 source=$4 n=$5 greeting=$6 broken=$7
  export PANE_DATA_DIR=$out/develop-$sample-data
  rm -rf "$PANE_DATA_DIR"
  local copy=$out/develop-$sample
  rm -rf "$copy"
  mkdir -p "$copy"
  (cd "guests/$sample" && tar cf - --exclude=target --exclude=dist --exclude=node_modules .) | (cd "$copy" && tar xf -)
  if [ -f "$copy/Cargo.toml" ]; then
    cp rust-toolchain.toml "$copy/"
    python3 - "$copy/Cargo.toml" "$PWD/guests/pane-guest" <<'PY'
import sys
path, guest = sys.argv[1], sys.argv[2]
text = open(path, encoding="utf-8").read().replace('path = "../pane-guest"', "path = '%s'" % guest)
open(path, "w", encoding="utf-8").write(text)
PY
    (cd "$copy" && cargo build --release --target wasm32-wasip2 --quiet)
  else
    python3 tools/componentize-js/pane_js.py build "$copy" "$copy/$component" >/dev/null
  fi
  local built=$copy/$component before=$out/develop-$sample-before.wasm
  start_pane --install "$copy"
  "$xdotool" windowfocus --sync "$window"
  "$xdotool" key Return; sleep 2   # Install
  manage_extensions
  # "Develop <title>": the row above the list's last, which is the global
  # automatic-update choice since #49 (the develop row was the last row
  # before it, and Down to the end now lands on that instead).
  for ((i = 0; i < 14; i++)); do "$xdotool" key Down; done
  "$xdotool" key Up; sleep 0.12
  "$xdotool" key Return; sleep 2   # Develop <title>
  capture "$n-$sample-develop-started.png"
  check "$n-$sample-develop-started.png" success   # "Developing <title>: each save in ..."
  "$xdotool" key Escape; sleep 1
  say_hello
  capture "$((n + 1))-$sample-greeting-before.png"
  check "$((n + 1))-$sample-greeting-before.png" success   # "Hello from ..."
  "$xdotool" key Escape; sleep 1

  # An edit, saved: built and reloaded.
  cp "$built" "$before"
  set_greeting "$copy/$source" "$(printf "$greeting" "Hello again")"
  wait_reloaded "$built" "$before"
  capture "$((n + 2))-$sample-rebuilt.png"
  check "$((n + 2))-$sample-rebuilt.png" success   # "Reloaded <title>"
  say_hello
  capture "$((n + 3))-$sample-greeting-after.png"
  check "$((n + 3))-$sample-greeting-after.png" success   # "Hello again"
  python3 "$(dirname "$0")/check_screenshot.py" --distinct "$out/$((n + 1))-$sample-greeting-before.png" "$out/$((n + 3))-$sample-greeting-after.png"
  "$xdotool" key Escape; sleep 1

  # A save that does not build: the working code stays.
  local failures
  failures=$(grep -c 'did not build' "$out/stderr.log" || true)
  set_greeting "$copy/$source" "$broken"
  wait_failed "$failures"
  capture "$((n + 4))-$sample-build-failed.png"
  check "$((n + 4))-$sample-build-failed.png" error   # "<title> did not build: ..."
  say_hello
  capture "$((n + 5))-$sample-kept.png"
  check "$((n + 5))-$sample-kept.png" success   # still "Hello again"
  python3 "$(dirname "$0")/check_screenshot.py" --same "$out/$((n + 3))-$sample-greeting-after.png" "$out/$((n + 5))-$sample-kept.png"
  "$xdotool" key Escape; sleep 1

  # Two saves, the second while the first builds: the newer one is reloaded.
  cp "$built" "$before"
  set_greeting "$copy/$source" "$(printf "$greeting" "Hello once more")"
  sleep 0.5
  set_greeting "$copy/$source" "$(printf "$greeting" "Hello at last")"
  wait_reloaded "$built" "$before"
  capture "$((n + 6))-$sample-rebuilt-again.png"
  check "$((n + 6))-$sample-rebuilt-again.png" success   # "Reloaded <title>"
  say_hello
  capture "$((n + 7))-$sample-greeting-fixed.png"
  check "$((n + 7))-$sample-greeting-fixed.png" success   # "Hello at last"
  python3 "$(dirname "$0")/check_screenshot.py" --distinct "$out/$((n + 3))-$sample-greeting-after.png" "$out/$((n + 7))-$sample-greeting-fixed.png"
  "$xdotool" key Escape; sleep 1

  # Stopped: a save builds nothing.
  manage_extensions
  # As above: the row above the list's last.
  for ((i = 0; i < 14; i++)); do "$xdotool" key Down; done
  "$xdotool" key Up; sleep 0.12
  "$xdotool" key Return; sleep 2   # Stop developing <title>
  capture "$((n + 8))-$sample-stopped.png"
  check "$((n + 8))-$sample-stopped.png" success   # "Stopped developing <title>"
  cp "$built" "$before"
  set_greeting "$copy/$source" "$(printf "$greeting" "Hello unseen")"
  sleep 8
  cmp -s "$built" "$before" || { echo "$title was built after development stopped"; exit 1; }
  stop_pane
}
develop_sample hello-rust "Hello Rust" target/wasm32-wasip2/release/hello_rust.wasm src/lib.rs 110 \
  'const GREETING: &str = "%s from Rust";' 'const GREETING: &str = 42;'
js_toolchain=${PANE_JS_TOOLCHAIN_DIR:-${XDG_CACHE_HOME:-$HOME/.cache}/pane/componentize-js}
if compgen -G "$js_toolchain/bin/*/toolchain.json" >/dev/null && command -v node >/dev/null; then
  develop_sample hello-ts "Hello TypeScript" dist/hello_ts.wasm src/index.ts 119 \
    'const GREETING: string = "%s from TypeScript";' 'const GREETING: string = 42;'
  develop_sample hello-js "Hello JavaScript" dist/hello_js.wasm src/index.js 128 \
    'const GREETING = "%s from JavaScript";' 'const GREETING = 42;'
else
  echo "skipped the JavaScript and TypeScript development smoke: no JS toolchain in $js_toolchain"
fi

# Disabling a required dependency: installed with the dependencies sample
# (whose install and data folder are this phase's own), the JavaScript
# operations sample is the first row of Manage extensions. Enter asks first,
# listing the Dependencies sample, which requires it, with Disable all and
# Cancel; Cancel changes nothing, Disable all disables both, and Enter again
# enables the JavaScript operations sample alone: the Dependencies sample
# stays disabled, on record too.
export PANE_DATA_DIR=$out/disable-dependents-data
rm -rf "$PANE_DATA_DIR"
start_pane --install target/guests/packages/sample-dependencies
"$xdotool" windowfocus --sync "$window"
"$xdotool" key Return; sleep 3   # Install
manage_extensions
"$xdotool" key Return; sleep 1   # disable JavaScript operations sample: asks first
capture 140-disable-dependents-asked.png
check 140-disable-dependents-asked.png details   # "Dependencies sample, which requires JavaScript operations sample · …"
"$xdotool" key Down Return; sleep 1   # Cancel
capture 141-disable-dependents-cancelled.png   # both still enabled
"$xdotool" key Return; sleep 1   # asks again
"$xdotool" key Return; sleep 2   # Disable all 2
capture 142-disable-dependents-disabled.png
check 142-disable-dependents-disabled.png success   # "Disabled JavaScript operations sample and Dependencies sample, which requires it"
"$xdotool" key Return; sleep 2   # enable JavaScript operations sample
capture 143-disable-dependents-enabled-alone.png
check 143-disable-dependents-enabled-alone.png success   # "Enabled JavaScript operations sample"; Dependencies sample stays disabled
python3 "$(dirname "$0")/check_screenshot.py" --distinct "$out"/{140-disable-dependents-asked,141-disable-dependents-cancelled,142-disable-dependents-disabled,143-disable-dependents-enabled-alone}.png
stop_pane
[ "$(grep -c '"disabled": true' "$PANE_DATA_DIR/extensions/installed.json")" = 1 ] || { echo "not exactly the dependent left disabled"; exit 1; }

# Recovering from a crash of Pane's extension runtime (#17): the runtime is
# a thread of Pane, so the smoke has it panic on purpose through a fault
# file (PANE_TEST_RUNTIME_FAULTS; nothing else sets it). With the settings
# sample and the helper sample installed and the helper running, a crash
# ends the helper, keeps the saved note and restarts the runtime; Count (the
# settings sample's last item) then saves and loses its answer in a second
# crash, which stops the runtime: the count is not run again. Root search
# explains that nothing runs, Manage extensions shows why (its first rows),
# a disable still works, and Restart runs extensions again, Count only when
# asked. A data folder of its own keeps the rows in a known order.
export PANE_DATA_DIR=$out/runtime-crash-data
rm -rf "$PANE_DATA_DIR"
fault=$out/runtime-fault
rm -f "$fault" "$fault.tmp"
# Asks Pane to inject a fault; it takes the file within 100 ms.
inject() {
  printf %s "$1" >"$fault.tmp"
  mv "$fault.tmp" "$fault"
  for _ in $(seq 50); do [ -e "$fault" ] || break; sleep 0.1; done
  [ ! -e "$fault" ] || { echo "Pane did not take the fault"; exit 1; }
  sleep 2
}
# The count Count keeps in the settings sample's content.
count() {
  python3 - "$PANE_DATA_DIR/extensions/content.json" <<'PY'
import json, sys
packages = json.load(open(sys.argv[1], encoding="utf-8"))["packages"]
print(next((values["count"] for values in packages.values() if "count" in values), "none"))
PY
}
helpers_running() { pgrep -f "$PANE_DATA_DIR/extensions/packages/.*/pane-echo" >/dev/null; }
start_pane --install target/guests/packages/sample-helper
"$xdotool" windowfocus --sync "$window"
"$xdotool" key Return; sleep 2   # Install
stop_pane
export PANE_TEST_RUNTIME_FAULTS=$fault
start_pane --install target/guests/packages/sample-settings
"$xdotool" windowfocus --sync "$window"
"$xdotool" key Return; sleep 2   # Install; Greeting is selected
"$xdotool" key Return; sleep 2   # open Greeting
for ((i = 0; i < 8; i++)); do "$xdotool" key Down; done   # Count
"$xdotool" key Return; sleep 2
capture 200-runtime-counted.png
check 200-runtime-counted.png success   # "Counted 1"
[ "$(count)" = 1 ] || { echo "Count did not count once"; exit 1; }
"$xdotool" key Escape; sleep 1
"$xdotool" type --delay 50 helper; sleep 1
"$xdotool" key Return; sleep 2   # open Helper sample
"$xdotool" key Down Return; sleep 2   # Echo after waiting
helpers_running || { echo "the waiting helper is not running"; exit 1; }
capture 201-runtime-helper-waiting.png
check 201-runtime-helper-waiting.png progress   # "Running…"
alive=$(find "$PANE_DATA_DIR/extensions/packages" -name pane-echo.alive | head -1)
[ -n "$alive" ] || { echo "the waiting helper does not beat"; exit 1; }
inject crash
capture 202-runtime-crashed.png
check 202-runtime-crashed.png error   # "Pane's extension runtime stopped unexpectedly and was started again; ..."
if helpers_running; then echo "the helper outlived the crashed runtime"; exit 1; fi
beats=$(stat -c %s "$alive"); sleep 0.5
[ "$(stat -c %s "$alive")" = "$beats" ] || { echo "the helper still beats after the crash"; exit 1; }
grep -q '"helper-wait": "started"' "$PANE_DATA_DIR/extensions/settings.json" || { echo "saved note lost"; exit 1; }
if grep -q '"helper-wait": "finished"' "$PANE_DATA_DIR/extensions/settings.json"; then echo "the stopped call finished"; exit 1; fi
"$xdotool" key Escape; sleep 1
"$xdotool" type --delay 50 greet; sleep 1
"$xdotool" key Return; sleep 2   # open Greeting in the restarted runtime
for ((i = 0; i < 8; i++)); do "$xdotool" key Down; done   # Count
inject crash-before-answer:count
"$xdotool" key Return; sleep 3   # counts, then the runtime crashes before answering
capture 203-runtime-stopped.png
check 203-runtime-stopped.png error   # the runtime stopped; its answer is lost
[ "$(count)" = 2 ] || { echo "Count did not run once before the crash"; exit 1; }
"$xdotool" key Escape; sleep 1
"$xdotool" type --delay 50 greet; sleep 1
"$xdotool" key Return; sleep 2   # Greeting: nothing runs
capture 204-runtime-refused.png
check 204-runtime-refused.png error   # "Extension runtime unavailable: it stopped after crashing ..."
"$xdotool" key Escape; sleep 1   # clears the query
manage_extensions
capture 205-runtime-manage.png   # Restart the extension runtime, Why the extension runtime stopped
"$xdotool" key Down Return; sleep 1   # Why the extension runtime stopped
capture 206-runtime-details.png
check 206-runtime-details.png details   # the details
"$xdotool" key Escape; sleep 1   # back at its row
"$xdotool" key Down Return; sleep 2   # disable Helper sample, the first package
capture 207-runtime-disabled.png
check 207-runtime-disabled.png success   # "Disabled Helper sample"
"$xdotool" key Up Up Return; sleep 2   # Restart the extension runtime
capture 208-runtime-restarted.png
check 208-runtime-restarted.png success   # "Restarted the extension runtime"
[ "$(count)" = 2 ] || { echo "Count was run again without asking"; exit 1; }
"$xdotool" key Escape; sleep 1
"$xdotool" type --delay 50 greet; sleep 1
"$xdotool" key Return; sleep 2   # open Greeting
for ((i = 0; i < 8; i++)); do "$xdotool" key Down; done   # Count
"$xdotool" key Return; sleep 2
capture 209-runtime-counted-again.png
check 209-runtime-counted-again.png success   # "Counted 3"
[ "$(count)" = 3 ] || { echo "Count did not count once more"; exit 1; }
python3 "$(dirname "$0")/check_screenshot.py" --distinct "$out"/{200-runtime-counted,202-runtime-crashed,203-runtime-stopped,204-runtime-refused,205-runtime-manage,206-runtime-details,207-runtime-disabled,208-runtime-restarted,209-runtime-counted-again}.png
stop_pane
unset PANE_TEST_RUNTIME_FAULTS
if helpers_running; then echo "a helper outlived Pane"; exit 1; fi
grep -q '"disabled": true' "$PANE_DATA_DIR/extensions/installed.json" || { echo "disable not recorded"; exit 1; }
if grep -q '"paused"' "$PANE_DATA_DIR/extensions/installed.json"; then echo "a package was paused for the runtime's crash"; exit 1; fi
# Recovering from an extension that stops responding (#18). The settings
# sample's last item, Stop responding, computes without waiting for up to a
# minute. The phase sets the runtime's limits through the fault file
# (PANE_TEST_RUNTIME_FAULTS): "not responding yet" after 4 seconds without
# progress, given up on after 15, and a guest's own computing at first a
# minute, so that the first Stop responding still computes when frame 240
# is taken (Pane's standard error has stopped no call yet): meanwhile the
# window answers keys, Escape returns to root search and Manage extensions
# opens. The compute limit then becomes 2 seconds, which the running call
# has passed, so Pane stops it at once; each later call is stopped after 2
# seconds of its computing, says why, and the third time pauses the
# package (a failure of its own); Retry starts it again. Then the runtime
# thread itself is made to hang through the fault file: the status line
# says it is not responding yet, then Pane gives up on it, names and
# pauses no extension, and Manage extensions says the runtime stopped
# responding; a fresh thread runs the next call. A data folder of its own keeps the rows in a known order.
export PANE_DATA_DIR=$out/unresponsive-data
rm -rf "$PANE_DATA_DIR"
fault=$out/unresponsive-fault
rm -f "$fault" "$fault.tmp"
# What the settings sample saved under $1, or "none".
saved() {
  python3 - "$PANE_DATA_DIR/extensions/settings.json" "$1" <<'PY'
import json, sys
packages = json.load(open(sys.argv[1], encoding="utf-8"))["packages"]
print(next((values[sys.argv[2]] for values in packages.values() if sys.argv[2] in values), "none"))
PY
}
# How many calls Pane stopped as unresponsive since this phase began, as
# its standard error says.
stderr_before=$(cat "$out/stderr.log" 2>/dev/null | wc -l)
stopped_calls() { tail -n +"$((stderr_before + 1))" "$out/stderr.log" | grep -c "stopped responding" || true; }
export PANE_TEST_RUNTIME_FAULTS=$fault
start_pane --install target/guests/packages/sample-settings
inject limits:60,4,15   # a minute of computing: frame 240 is taken while it computes
"$xdotool" windowfocus --sync "$window"
"$xdotool" key Return; sleep 2   # Install; Greeting is selected
"$xdotool" key Return; sleep 2   # open Greeting
for ((i = 0; i < 9; i++)); do "$xdotool" key Down; done   # Stop responding
"$xdotool" key Return; sleep 1   # it computes
[ "$(saved busy)" = started ] || { echo "Stop responding did not start"; exit 1; }
"$xdotool" key Escape; sleep 1   # root search answers meanwhile
manage_extensions
capture 240-unresponsive-window-answers.png   # the extension list, while the guest computes
check 240-unresponsive-window-answers.png subtitle   # its rows' subtitles
[ "$(stopped_calls)" = 0 ] || { echo "Stop responding was stopped before frame 240"; exit 1; }
inject limits:2,4,15   # it has computed longer: Pane stops it at its next tick
for _ in $(seq 300); do [ "$(stopped_calls)" -ge 1 ] && break; sleep 0.1; done
[ "$(stopped_calls)" -ge 1 ] || { echo "Stop responding was not stopped at the shorter limit"; exit 1; }
"$xdotool" key Escape; sleep 1   # its answer is not shown here
"$xdotool" type --delay 50 greet; sleep 1
"$xdotool" key Return; sleep 2   # open Greeting
for ((i = 0; i < 9; i++)); do "$xdotool" key Down; done   # Stop responding
"$xdotool" key Return; sleep 8
capture 241-unresponsive-stopped.png
check 241-unresponsive-stopped.png error   # "The extension stopped responding: it computed for 2 seconds ..."
"$xdotool" key Return; sleep 8   # the third time
"$xdotool" type --delay 50 greet; sleep 1   # Greeting and its reason at the top
capture 242-unresponsive-paused.png
check 242-unresponsive-paused.png error   # "Settings sample stopped responding 3 times within 5 minutes and is paused ..."
check 242-unresponsive-paused.png warning   # Greeting: "Settings sample is paused after an error; ..."
[ "$(saved busy)" = started ] || { echo "Stop responding finished or was lost"; exit 1; }
"$xdotool" key Escape; sleep 1   # clears the query
manage_extensions
"$xdotool" key Down Down Down Return; sleep 1   # "Why Settings sample is paused"
capture 243-unresponsive-pause-details.png
check 243-unresponsive-pause-details.png details   # the details
"$xdotool" key Return; sleep 2   # Retry Settings sample
capture 244-unresponsive-retried.png
check 244-unresponsive-retried.png success   # "Started Settings sample"
"$xdotool" key Escape; sleep 1
inject hang
"$xdotool" type --delay 50 greet; sleep 1
"$xdotool" key Return; sleep 4   # open Greeting: the stuck runtime is not responding yet
capture 245-unresponsive-not-yet.png
check 245-unresponsive-not-yet.png progress   # "Pane's extension runtime is not responding yet. ..."
sleep 14   # Pane gives up on it
capture 246-unresponsive-runtime.png
check 246-unresponsive-runtime.png error   # the runtime stopped responding and was started again
"$xdotool" key Escape; sleep 1   # clears the query
manage_extensions
"$xdotool" key Return; sleep 1   # Why the extension runtime stopped, its first row
capture 247-unresponsive-runtime-details.png
check 247-unresponsive-runtime-details.png details   # the details
inject release
"$xdotool" key Escape Escape; sleep 1
"$xdotool" type --delay 50 greet; sleep 1
"$xdotool" key Return; sleep 2   # open Greeting on a fresh runtime thread
"$xdotool" key Return; sleep 2   # Use a formal greeting
capture 248-unresponsive-runs-again.png
check 248-unresponsive-runs-again.png success   # "Saved the formal greeting"
python3 "$(dirname "$0")/check_screenshot.py" --distinct "$out"/{240-unresponsive-window-answers,241-unresponsive-stopped,242-unresponsive-paused,243-unresponsive-pause-details,244-unresponsive-retried,245-unresponsive-not-yet,246-unresponsive-runtime,247-unresponsive-runtime-details,248-unresponsive-runs-again}.png
stop_pane
unset PANE_TEST_RUNTIME_FAULTS
[ "$(saved busy)" = started ] || { echo "Stop responding finished after it was stopped"; exit 1; }
[ "$(saved greeting-style)" = formal ] || { echo "the fresh runtime did not save"; exit 1; }
# No package record of installed.json holds a pause (read as JSON, not as text).
paused_packages=$(python3 - "$PANE_DATA_DIR/extensions/installed.json" <<'PY'
import json, sys
record = json.load(open(sys.argv[1], encoding="utf-8"))
print(sum(1 for package in record["packages"] if "paused" in package))
PY
)
[ "$paused_packages" = 0 ] || { echo "a package was paused for the runtime's hang"; exit 1; }

# Uninstalling a required dependency: installed with the dependencies sample
# (whose install and data folder are this phase's own), the JavaScript
# operations sample's Uninstall row is the seventh of Manage extensions.
# Enter asks first, listing the Dependencies sample, which requires it, and
# each one's saved data, with Uninstall all keeping or deleting saved data
# and Cancel; Cancel changes nothing, Uninstall all 2 (keeping) uninstalls
# both, and installing the JavaScript operations sample again installs it
# alone: the Dependencies sample is not restored, on record too.
export PANE_DATA_DIR=$out/uninstall-dependents-data
rm -rf "$PANE_DATA_DIR"
start_pane --install target/guests/packages/sample-dependencies
"$xdotool" windowfocus --sync "$window"
"$xdotool" key Return; sleep 3   # Install
manage_extensions
for ((i = 0; i < 6; i++)); do "$xdotool" key Down; done   # Uninstall JavaScript operations sample
"$xdotool" key Return; sleep 1   # asks first
capture 180-uninstall-dependents-asked.png
check 180-uninstall-dependents-asked.png details   # "Dependencies sample, which requires JavaScript operations sample · …"
"$xdotool" key Down Down Return; sleep 1   # Cancel
capture 181-uninstall-dependents-cancelled.png   # both still installed
"$xdotool" key Return; sleep 1   # asks again
"$xdotool" key Return; sleep 3   # Uninstall all 2 and keep saved data
capture 182-uninstall-dependents-uninstalled.png
check 182-uninstall-dependents-uninstalled.png success   # "Uninstalled JavaScript operations sample and Dependencies sample, which requires it; …"
stop_pane
[ "$(grep -c '"dir"' "$PANE_DATA_DIR/extensions/installed.json")" = 0 ] || { echo "not both uninstalled"; exit 1; }
start_pane --install target/guests/packages/sample-operations-js
"$xdotool" windowfocus --sync "$window"
"$xdotool" key Return; sleep 3   # Install the dependency alone
manage_extensions
capture 183-uninstall-dependents-reinstalled-alone.png   # only the JavaScript operations sample is listed
check 183-uninstall-dependents-reinstalled-alone.png subtitle
python3 "$(dirname "$0")/check_screenshot.py" --distinct "$out"/{180-uninstall-dependents-asked,181-uninstall-dependents-cancelled,182-uninstall-dependents-uninstalled,183-uninstall-dependents-reinstalled-alone}.png
stop_pane
[ "$(grep -c '"dir"' "$PANE_DATA_DIR/extensions/installed.json")" = 1 ] || { echo "not the dependency alone reinstalled"; exit 1; }

# npm packages (#45), from a local registry on 127.0.0.1 serving the npm
# sample `cargo xtask guests` packed (scripts/npm_registry.py; nothing reaches
# the network), with a data folder of its own. Installing the local
# Dependencies from npm sample shows the npm package it requires and
# installs both; its command calls the npm package's greet operation. Then
# "Install extension from npm…" (searched for by title, as manage_extensions
# does: a blind run of Downs to root's end would open #72's Settings… row,
# last of all now) asks for the npm package in a form; naming the installed
# one offers Update, and its command runs: "Hello from the npm package".
export PANE_DATA_DIR=$out/npm-data
rm -rf "$PANE_DATA_DIR"
rm -f "$out/npm-registry.port"
python3 "$(dirname "$0")/npm_registry.py" target/guests/npm "$out/npm-registry.port" 2>>"$out/npm-registry.log" &
npm_registry_pid=$!
# Generous: a slow runner may take seconds to start Python.
for _ in $(seq 600); do [ -s "$out/npm-registry.port" ] && break; kill -0 "$npm_registry_pid" 2>/dev/null || break; sleep 0.1; done
[ -s "$out/npm-registry.port" ] || { echo "the local npm registry did not start (see $out/npm-registry.log)"; exit 1; }
export PANE_NPM_REGISTRY=http://127.0.0.1:$(cat "$out/npm-registry.port")/
start_pane --install target/guests/packages/sample-dependencies-npm
"$xdotool" windowfocus --sync "$window"
capture 260-npm-dependency-preview.png
check 260-npm-dependency-preview.png details   # "Requires: Greeter from npm, installed with it from npm:@pane-samples/greeter"
"$xdotool" key Return; sleep 3   # Install; Greet through an npm dependency is selected
capture 261-npm-dependency-installed.png
check 261-npm-dependency-installed.png success   # "Installed Dependencies from npm sample with Greeter from npm, which it requires"
"$xdotool" key Return; sleep 3   # open it
"$xdotool" key Return; sleep 3   # "Greet through the required greeter"
capture 262-npm-dependency-called.png
check 262-npm-dependency-called.png success   # "Hello, Pane, from the npm package"
"$xdotool" key Escape; sleep 1
"$xdotool" key ctrl+a; "$xdotool" type --delay 50 'install npm'; sleep 1
"$xdotool" key Return; sleep 1   # Install extension from npm…
capture 263-npm-form.png
check 263-npm-form.png hint   # the form's hint line
"$xdotool" type --delay 50 @pane-samples/greeter
"$xdotool" key Return; sleep 3
capture 264-npm-preview.png
check 264-npm-preview.png details   # "Source: npm package @pane-samples/greeter", "npm version: 0.1.0, the latest", …
"$xdotool" key Return; sleep 3   # Update; Greeter from npm is selected
capture 265-npm-updated.png
check 265-npm-updated.png success   # "Updated Greeter from npm to 0.1.0"
"$xdotool" key Return; sleep 3   # open Greeter from npm
"$xdotool" key Return; sleep 2   # "Say hello"
capture 266-npm-command-ran.png
check 266-npm-command-ran.png success   # "Hello from the npm package"

# #49: the update Pane applies by itself. A 0.2.0 of the sample is
# published into the registry this phase serves (it reads its folder on
# request, so publishing is dropping the tarball in), and Pane is stopped
# and started again: the first check, a second after the start, finds the
# newer version and replaces the installed copy — unpinned, and nothing
# of it running, so the safe boundary is at once — saying so in the status
# line. The new copy's command runs as the old one did.
python3 "$(dirname "$0")/npm_publish.py" target/guests/npm/pane-samples-greeter-0.1.0.tgz 0.2.0
stop_pane
start_pane
"$xdotool" windowfocus --sync "$window"
# The check a second after the start, then the download and the apply:
# capture until the status line says the update landed, whenever that is,
# so a slow runner is waited for rather than slept past.
capture_until 267-npm-updated-automatically.png success 60   # "Updated Greeter from npm to 0.2.0"
"$xdotool" key Return; sleep 3   # open Greeter from npm, the new copy
"$xdotool" key Return; sleep 2   # "Say hello"
capture 268-npm-new-copy-ran.png
check 268-npm-new-copy-ran.png success   # "Hello from the npm package"
python3 "$(dirname "$0")/check_screenshot.py" --distinct "$out"/{260-npm-dependency-preview,261-npm-dependency-installed,262-npm-dependency-called,263-npm-form,264-npm-preview,265-npm-updated,266-npm-command-ran,267-npm-updated-automatically,268-npm-new-copy-ran}.png
stop_pane
kill "$npm_registry_pid"; wait "$npm_registry_pid" 2>/dev/null || true; npm_registry_pid=
unset PANE_NPM_REGISTRY
grep -q '"npm": "@pane-samples/greeter"' "$PANE_DATA_DIR/extensions/installed.json" || { echo "npm package not recorded"; exit 1; }
grep -q '"npmVersion": "0.2.0"' "$PANE_DATA_DIR/extensions/installed.json" || { echo "the automatic update was not recorded"; exit 1; }
[ "$(grep -c '"dir"' "$PANE_DATA_DIR/extensions/installed.json")" = 2 ] || { echo "not both installed"; exit 1; }

# Git packages (#46), from a repository the smoke makes with git from the
# Git sample `cargo xtask guests` assembled (target/guests/git/greeter: its
# source on main, its built component on the branch release, tagged v0.1.0),
# served over Git's smart HTTP protocol from 127.0.0.1
# (scripts/repository_server.py; nothing reaches the network), with a data
# folder of its own. `--install git:<address>` names the default branch,
# which holds the source only: explained, nothing offered. Then "Install
# extension from Git…" (searched for by title rather than counted to, as
# manage_extensions does: root's last row is #72's Settings… now, and with
# nothing installed in this data folder there is no Manage extensions… row
# to find either) asks for the repository
# in a form; naming the tag previews the release revision, pinned, and
# installs it, and its command runs: "Hello from the Git repository".
export PANE_DATA_DIR=$out/git-data
rm -rf "$PANE_DATA_DIR" "$out/git-repositories"
python3 "$(dirname "$0")/repository_server.py" make-sample target/guests/git/greeter "$out/git-repositories/greeter"
rm -f "$out/repository-server.port"
python3 "$(dirname "$0")/repository_server.py" serve "$out/git-repositories" "$out/repository-server.port" 2>>"$out/repository-server.log" &
repository_server_pid=$!
for _ in $(seq 600); do [ -s "$out/repository-server.port" ] && break; kill -0 "$repository_server_pid" 2>/dev/null || break; sleep 0.1; done
[ -s "$out/repository-server.port" ] || { echo "the local repository server did not start (see $out/repository-server.log)"; exit 1; }
repository=http://127.0.0.1:$(cat "$out/repository-server.port")/greeter.git
start_pane --install "git:$repository"
"$xdotool" windowfocus --sync "$window"
# The fetch runs after the window shows: capture until its explanation does.
capture_until 300-git-source-only.png error 60   # "The default branch, main (commit …) of the Git repository … holds only the source of …"
"$xdotool" key Escape; sleep 1
"$xdotool" key ctrl+a; "$xdotool" type --delay 50 'install git'; sleep 1
"$xdotool" key Return; sleep 1   # Install extension from Git…
capture 301-git-form.png
check 301-git-form.png hint   # the form's hint line
"$xdotool" type --delay 50 "$repository@v0.1.0"
"$xdotool" key Return; sleep 3
capture 302-git-preview.png
check 302-git-preview.png details   # "Source: Git repository 127.0.0.1:<port>/greeter", "Revision: tag v0.1.0, which you named: …"
"$xdotool" key Return; sleep 3   # Install; Greeter from Git is selected
capture 303-git-installed.png
check 303-git-installed.png success   # "Installed Greeter from Git"
"$xdotool" key Return; sleep 3   # open Greeter from Git
"$xdotool" key Return; sleep 2   # "Say hello"
capture 304-git-command-ran.png
check 304-git-command-ran.png success   # "Hello from the Git repository"
python3 "$(dirname "$0")/check_screenshot.py" --distinct "$out"/{300-git-source-only,301-git-form,302-git-preview,303-git-installed,304-git-command-ran}.png
stop_pane
release=$(python3 "$(dirname "$0")/repository_server.py" commit "$out/git-repositories/greeter" v0.1.0)
python3 "$(dirname "$0")/check_git_record.py" "$PANE_DATA_DIR/extensions/installed.json" "$release" || { echo "Git package not recorded as installed from v0.1.0"; exit 1; }
[ -z "$(ls -A "$PANE_DATA_DIR/extensions/downloads" 2>/dev/null)" ] || { echo "a Git download was left"; exit 1; }

# Git packages update themselves (#50): a second repository of the same
# sample, made in the folder the same server serves (it answers each
# request from the folder as it is), installed in a data folder of its own
# from its tracked release branch -- `--install` naming the branch, so the
# copy is tracked, not pinned -- with its command run; the branch then
# moves to a 0.2.0 (repository_server.py move-sample) while Pane is
# stopped, and the check a second after the restart replaces the installed
# copy by itself, the new code running. Nothing reaches the network.
export PANE_DATA_DIR=$out/git-update-data
rm -rf "$PANE_DATA_DIR"
python3 "$(dirname "$0")/repository_server.py" make-sample target/guests/git/greeter "$out/git-repositories/greeter-tracked"
tracked=http://127.0.0.1:$(cat "$out/repository-server.port")/greeter-tracked.git
start_pane --install "git:$tracked@release"
"$xdotool" windowfocus --sync "$window"
capture_until 305-git-tracked-preview.png details 60   # "Revision: branch release, tracked: an update fetches that branch again"
"$xdotool" key Return; sleep 3   # Install; Greeter from Git is selected
capture 306-git-tracked-installed.png
check 306-git-tracked-installed.png success   # "Installed Greeter from Git"
python3 "$(dirname "$0")/repository_server.py" move-sample "$out/git-repositories/greeter-tracked" 0.2.0
stop_pane
start_pane
"$xdotool" windowfocus --sync "$window"
# The check a second after the start, then the fetch and the apply: capture
# until the status line says the update landed, whenever that is.
capture_until 307-git-updated-automatically.png success 60   # "Updated Greeter from Git to 0.2.0"
"$xdotool" key Return; sleep 3   # open Greeter from Git, the new copy
"$xdotool" key Return; sleep 2   # "Say hello"
capture 308-git-new-copy-ran.png
check 308-git-new-copy-ran.png success   # "Hello from the Git repository"
python3 "$(dirname "$0")/check_screenshot.py" --distinct "$out"/{305-git-tracked-preview,306-git-tracked-installed,307-git-updated-automatically,308-git-new-copy-ran}.png
stop_pane
kill "$repository_server_pid"; wait "$repository_server_pid" 2>/dev/null || true; repository_server_pid=
moved=$(python3 "$(dirname "$0")/repository_server.py" commit "$out/git-repositories/greeter-tracked" release)
python3 "$(dirname "$0")/check_git_record.py" --ref refs/heads/release --unpinned "$PANE_DATA_DIR/extensions/installed.json" "$moved" || { echo "the tracked Git package was not recorded at its moved branch"; exit 1; }
[ -z "$(ls -A "$PANE_DATA_DIR/extensions/downloads" 2>/dev/null)" ] || { echo "a Git download was left"; exit 1; }

# File search (#29): Files, a default extension (its data folder is this
# phase's own; Files is selected once installed, and Pane's own "Choose
# folder…" row is the first of its command). Enter on it would show the
# system's folder picker; the smoke names the folder in
# PANE_TEST_CHOOSE_FOLDER instead (a debug build's hook). The fixture folder's
# path has spaces, and a file in it has non-ASCII letters too; typing "plan"
# lists that file, selected, and Enter opens it with the system's handler for
# files: xdg-open, with no desktop session, whose only handler for plain text
# is a script that records the path, so no program of the user's opens it.
# Each file action closes the window after it acts (#150), so Pane is
# started again (the grant is kept) for the next file. An executable
# script in the folder is found, and Enter reveals it (ADR 0037: file
# search's Enter never runs a program; only its explicit Run does): with no
# file manager on the session bus its folder is handed to xdg-open, never
# the script itself, and the script does not run. The fixture is outside
# the home folder, so no screenshot shows a home path.
export PANE_DATA_DIR=$out/files-data
rm -rf "$PANE_DATA_DIR"
files_fixture=$(mktemp -d /tmp/pane-smoke-files.XXXXXX)
files_folder="$files_fixture/Pane smoke files"
mkdir -p "$files_folder/notes"
printf 'plan\n' >"$files_folder/Résumé plan ü.txt"
printf 'todo\n' >"$files_folder/notes/todo.txt"
printf '#!/bin/sh\ntouch "%s/runner-ran"\n' "$files_fixture" >"$files_folder/notes/runner.sh"
chmod +x "$files_folder/notes/runner.sh"
printf '#!/bin/sh\nprintf "%%s" "$1" >"%s/opened-file.txt"\n' "$(cd "$out" && pwd)" >"$out/file-opener.sh"
chmod +x "$out/file-opener.sh"
rm -f "$out/opened-file.txt"
unset XDG_CURRENT_DESKTOP XDG_SESSION_DESKTOP DESKTOP_SESSION GDMSESSION DBUS_SESSION_BUS_ADDRESS \
  GNOME_DESKTOP_SESSION_ID KDE_FULL_SESSION KDE_SESSION_VERSION MATE_DESKTOP_SESSION_ID
files_xdg=$(cd "$out" && pwd)/files-xdg
rm -rf "$files_xdg"
mkdir -p "$files_xdg/applications"
cat >"$files_xdg/applications/pane-smoke-file-opener.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Pane Smoke File Opener
Exec=$(cd "$out" && pwd)/file-opener.sh %f
MimeType=text/plain;
NoDisplay=true
EOF
printf '[Default Applications]\ntext/plain=pane-smoke-file-opener.desktop\n' >"$files_xdg/mimeapps.list"
cp "$files_xdg/mimeapps.list" "$files_xdg/applications/mimeapps.list"
export BROWSER="$(cd "$out" && pwd)/file-opener.sh" XDG_CONFIG_HOME="$files_xdg" XDG_DATA_HOME="$files_xdg"
if command -v xdg-mime >/dev/null; then
  handler=$(xdg-mime query default text/plain)
  [ "$handler" = pane-smoke-file-opener.desktop ] || { echo "text files would open with $handler, not the smoke's script"; exit 1; }
fi
export PANE_TEST_CHOOSE_FOLDER=$files_folder
start_pane --install target/guests/packages/files
"$xdotool" windowfocus --sync "$window"
"$xdotool" key Return; sleep 2   # Install; Files is selected
"$xdotool" key Return; sleep 3   # open Files; "Choose folder…" is selected
"$xdotool" key Return; sleep 2   # the folder PANE_TEST_CHOOSE_FOLDER names
capture 220-files-folder-granted.png
check 220-files-folder-granted.png success   # "Files may now list “Pane smoke files”"
"$xdotool" key Escape; sleep 1
"$xdotool" type --delay 50 'plan'; sleep 3
capture 221-files-found.png
check 221-files-found.png selected 3000   # the selected file row, "Résumé plan ü.txt"
"$xdotool" key Return; sleep 4   # Open: "Opened Résumé plan ü.txt", and the window closes
capture 222-files-opened.png   # evidence only: the window is hidden
[ -f "$out/opened-file.txt" ] || { echo "the handler for files was not asked to open anything"; exit 1; }
# Both sides resolved, as the same file.
[ "$(realpath "$(cat "$out/opened-file.txt")")" = "$(realpath "$files_folder/Résumé plan ü.txt")" ] || { echo "the handler for files was not asked to open the found file"; exit 1; }
rm -f "$out/opened-file.txt"
stop_pane
start_pane
"$xdotool" windowfocus --sync "$window"
"$xdotool" type --delay 50 'runner'; sleep 3
capture 223-files-program-found.png
check 223-files-program-found.png selected 3000   # the script's row
"$xdotool" key Return; sleep 3   # Reveal in File Manager
capture 224-files-program-revealed.png   # evidence only: the window is hidden
# What xdg-open was handed, if anything, is the script's folder.
if [ -e "$out/opened-file.txt" ]; then
  [ "$(realpath "$(cat "$out/opened-file.txt")")" = "$(realpath "$files_folder/notes")" ] || { echo "the script was handed to the handler: $(cat "$out/opened-file.txt")"; exit 1; }
fi
[ ! -e "$files_fixture/runner-ran" ] || { echo "the script ran"; exit 1; }
python3 "$(dirname "$0")/check_screenshot.py" --distinct "$out"/{220-files-folder-granted,221-files-found,223-files-program-found}.png
stop_pane
unset PANE_TEST_CHOOSE_FOLDER
rm -rf "$files_fixture"

# Searching an online service inside its command: Package search, the Rust
# search sample, queries the fixture service (a made-up package registry on
# a free port of 127.0.0.1, set as the sample's address through its form;
# nothing leaves this computer), whose log lists each request. Typed into
# root search, "aurora" finds nothing and sends the service nothing. Opened,
# the command's own search field sends it: its results are listed, Enter
# shows a package's details. A search the service holds ("slow...") is stopped when the text
# changes: the service sees its client hang up and the newer results show.
# The service's own error, then the service stopped (offline), are errors in
# place of results; once it is back, searching works again: the extension
# was not paused. A data folder of its own keeps the rows in a known order.
export PANE_DATA_DIR=$out/search-data
rm -rf "$PANE_DATA_DIR"
cargo build --locked --quiet -p pane-core --example fixture_service
service_log=$out/fixture-service.log
service_pid=
service_port=0
# Starts the fixture service, the `$1`th time: first on a free port, which
# it prints, then on that same port again.
start_service() {
  target/debug/examples/fixture_service --port "$service_port" >>"$service_log" 2>&1 &
  service_pid=$!
  for _ in $(seq 50); do
    if [ "$(grep -c 'listening on' "$service_log" 2>/dev/null)" -ge "$1" ]; then
      service_port=$(sed -n 's#.*listening on http://127\.0\.0\.1:\([0-9]*\).*#\1#p' "$service_log" | tail -n 1)
      return
    fi
    kill -0 "$service_pid" 2>/dev/null || break
    sleep 0.1
  done
  echo "the fixture service did not start (see $service_log)"; exit 1
}
stop_service() { kill "$service_pid"; wait "$service_pid" 2>/dev/null || true; service_pid=; }
trap '[ -z "$service_pid" ] || kill "$service_pid" 2>/dev/null || true; cleanup' EXIT
rm -f "$service_log"
start_service 1
start_pane --install target/guests/packages/sample-search
"$xdotool" windowfocus --sync "$window"
"$xdotool" key Return; sleep 2   # Install; Package search is selected
capture 160-search-installed.png
check 160-search-installed.png success   # "Installed Search sample"
"$xdotool" type --delay 50 aurora; sleep 2
capture 161-root-typed.png   # root search: "No results for “aurora”"
if grep -q '^GET' "$service_log"; then echo "root search reached the service"; exit 1; fi
"$xdotool" key Escape; sleep 1   # clears the query
"$xdotool" type --delay 50 'package search'; sleep 1
"$xdotool" key Return; sleep 3   # open Package search
capture 162-command-opened.png   # its own list, its search field empty
check 162-command-opened.png selected 3000   # its first row, selected
"$xdotool" key Down Return; sleep 2   # Service address: its form
"$xdotool" type --delay 20 "http://127.0.0.1:$service_port"
"$xdotool" key Return; sleep 2   # Save
capture 163-service-set.png   # "Searching http://127.0.0.1:<port> from now on"
check 163-service-set.png success
"$xdotool" key Escape; sleep 1   # back to the command, its search field empty
"$xdotool" type --delay 50 aurora; sleep 3
capture 164-search-results.png   # aurora-charts, selected, and aurora-cli
check 164-search-results.png selected 3000
grep -q '^GET /search?q=aurora$' "$service_log" || { echo "the command's search did not reach the service"; exit 1; }
"$xdotool" key Down Return; sleep 3   # aurora-cli's details
capture 165-details.png
check 165-details.png success   # "aurora-cli 0.9.3 (Apache-2.0): Command-line parsing with subcommands"
"$xdotool" key ctrl+a; "$xdotool" type --delay 50 slow; sleep 2   # held by the service
"$xdotool" key ctrl+a; "$xdotool" type --delay 50 ember; sleep 3
capture 166-newer-search.png   # ember-tz, not what "slow" would list
check 166-newer-search.png selected 3000
grep -q '^ABANDONED /search?q=slow$' "$service_log" || { echo "the replaced search was not stopped"; exit 1; }
"$xdotool" key ctrl+a; "$xdotool" type --delay 50 down; sleep 3
capture 167-service-error.png
check 167-service-error.png error   # "... The service answered 503: the registry is down for maintenance"
stop_service
"$xdotool" key ctrl+a; "$xdotool" type --delay 50 basalt; sleep 3
capture 168-offline.png
check 168-offline.png error   # "... Could not reach the service at http://127.0.0.1:<port>: connection refused"
start_service 2
"$xdotool" key ctrl+a; "$xdotool" type --delay 50 cobalt; sleep 3
capture 169-back-online.png   # cobalt-http, selected: not paused
check 169-back-online.png selected 3000
python3 "$(dirname "$0")/check_screenshot.py" --distinct "$out"/{161-root-typed,162-command-opened,163-service-set,164-search-results,165-details,166-newer-search,167-service-error,168-offline,169-back-online}.png
stop_pane
stop_service

# Clipboard history (#35): the Clipboard History default extension keeps
# nothing until it is turned on in its command (its first item); then the
# text this smoke copies is kept; nothing is kept while it is paused or the
# extension is disabled, also after a restart, and once enabled again it is
# kept again, also after a restart. Enter on a kept item runs its first
# action, Paste (#150): Pane cannot paste on Linux yet, so it copies the
# item again instead, closes the window and says so in a HUD; the Open Pane
# hotkey (Ctrl+Alt+Space) brings the window back, and the copy really is on
# the clipboard: pasting it into root search shows what was typed. The smoke copies only text of
# its own ("pane-smoke-..."), by typing it into root search, selecting it
# with Ctrl+A and copying it with Ctrl+C through the window's X11
# clipboard, and so replaces what is on the clipboard without reading or
# putting it back: run it on CI's runner or a desktop given to it, as the
# rest of the smoke already takes over the keyboard and the display. X11
# has no marker formats a password manager could set, so no marked copy is
# checked here (an excluded program is the only way to keep one out; the
# adapter test checks that). A data folder of its own.
export PANE_DATA_DIR=$out/clipboard-data
rm -rf "$PANE_DATA_DIR"
extensions=$PANE_DATA_DIR/extensions
history=$extensions/clipboard-history.json
# The kept texts, newest first, joined by commas.
kept_texts() { python3 "$(dirname "$0")/clipboard_history.py" texts "$extensions"; }
# The newest kept text.
first_kept() { kept_texts | cut -d, -f1; }
# Whether `text` is among the kept texts.
kept_one() { case ",$(kept_texts)," in (*",$1,"*) true;; (*) false;; esac; }
not_kept() { sleep 2; if kept_one "$1"; then echo "$1 was kept"; exit 1; fi; }
# Back to a blank root search from wherever the smoke is, with the return
# to root key (Shift+Escape): Escape at a blank root search hides the
# launcher since the redesign (1e61793), so it cannot be pressed blind.
to_root() { "$xdotool" key shift+Escape; sleep 1; }
# Copies `text`: from wherever the smoke is, back at root search, types it,
# selects it and copies it.
copy() {
  to_root
  "$xdotool" type --delay 50 "$1"; sleep 0.5
  "$xdotool" key ctrl+a ctrl+c; sleep 1
  to_root
}
# Opens the Clipboard History command from wherever the smoke is.
open_history() {
  to_root
  "$xdotool" type --delay 50 clipboard; sleep 1
  "$xdotool" key Return; sleep 2
}
start_pane --install target/guests/packages/clipboard-history
"$xdotool" windowfocus --sync "$window"
"$xdotool" key Return   # Install; Clipboard History is selected
wait_for "$extensions/installed.json" clipboard-history present; sleep 1
copy pane-smoke-before   # while history is off
open_history
capture 280-clipboard-off.png
check 280-clipboard-off.png subtitle   # "Off · Pane keeps nothing you copy until you turn it on ..."
[ ! -e "$history" ] || { echo "clipboard history was kept before it was turned on"; exit 1; }
"$xdotool" key Return   # Turn on clipboard history
wait_for "$history" '"capture": "on"' present; sleep 1
capture 281-clipboard-on.png
check 281-clipboard-on.png success   # "Clipboard history is on"
copy pane-smoke-kept
copy pane-smoke-second
wait_for "$history" pane-smoke-second present
[ "$(kept_texts)" = pane-smoke-second,pane-smoke-kept ] || { echo "kept: $(kept_texts)"; exit 1; }
open_history
capture 282-clipboard-kept.png
check 282-clipboard-kept.png subtitle   # the two kept items, newest first
"$xdotool" key Return   # Pause clipboard history
wait_for "$history" '"capture": "paused"' present
copy pane-smoke-paused
not_kept pane-smoke-paused
open_history
"$xdotool" key Return   # Resume clipboard history
wait_for "$history" '"capture": "on"' present
copy pane-smoke-resumed
wait_for "$history" pane-smoke-resumed present
open_history
"$xdotool" key Down Down Down Down Down Down Down Down; sleep 1   # the second kept item, pane-smoke-second, after Pause, Turn off, Keep items for, Exclude, Clear, Turn off and delete, Delete recent and the first
"$xdotool" key Return; sleep 2   # Paste: not available yet, so it copies it again (#150)
capture 283-clipboard-copied.png   # evidence only: the HUD "Copied — paste is not available here yet", Pane hidden
[ "$(first_kept)" = pane-smoke-second ] || { echo "the copied item did not move to the front: $(kept_texts)"; exit 1; }
# Pane keeps the X11 clipboard while it runs: the Open Pane hotkey brings
# its window back rather than a restart.
"$xdotool" key ctrl+alt+space; sleep 2
"$xdotool" windowfocus --sync "$window"
# The clipboard really holds the item again: pasting it over root search
# shows exactly what typing it shows.
to_root
"$xdotool" key ctrl+a ctrl+v; sleep 1
capture 284-clipboard-pasted.png
"$xdotool" key Escape; sleep 1
"$xdotool" type --delay 50 pane-smoke-second; sleep 1
capture 285-clipboard-typed.png
python3 "$(dirname "$0")/check_screenshot.py" --same "$out"/{284-clipboard-pasted,285-clipboard-typed}.png
"$xdotool" key Escape; sleep 1
"$xdotool" type --delay 50 manage; sleep 1
"$xdotool" key Return; sleep 1
"$xdotool" key Return; sleep 2   # disable Clipboard History, the first row
wait_for "$extensions/installed.json" '"disabled": true' present; sleep 1
capture 286-clipboard-disabled.png
check 286-clipboard-disabled.png success   # "Disabled Clipboard History"
copy pane-smoke-disabled
not_kept pane-smoke-disabled
stop_pane
start_pane
"$xdotool" windowfocus --sync "$window"
copy pane-smoke-restarted-disabled
not_kept pane-smoke-restarted-disabled
"$xdotool" type --delay 50 manage; sleep 1
"$xdotool" key Return; sleep 1
"$xdotool" key Return; sleep 2   # enable Clipboard History
wait_for "$extensions/installed.json" '"disabled": true' absent; sleep 1
copy pane-smoke-enabled
wait_for "$history" pane-smoke-enabled present
stop_pane
start_pane
"$xdotool" windowfocus --sync "$window"
copy pane-smoke-after-restart
wait_for "$history" pane-smoke-after-restart present
open_history
capture 287-clipboard-after-restart.png
check 287-clipboard-after-restart.png subtitle   # kept again after the restart
python3 "$(dirname "$0")/check_screenshot.py" --distinct "$out"/{280-clipboard-off,281-clipboard-on,282-clipboard-kept,284-clipboard-pasted,286-clipboard-disabled,287-clipboard-after-restart}.png
stop_pane
[ "$(kept_texts)" = pane-smoke-after-restart,pane-smoke-enabled,pane-smoke-second,pane-smoke-resumed,pane-smoke-kept ] || { echo "kept: $(kept_texts)"; exit 1; }
for never in before paused disabled restarted-disabled; do
  kept_one "pane-smoke-$never" && { echo "pane-smoke-$never was kept"; exit 1; }
done

# Clipboard history expiry and deletion (#36), on the history just kept.
# With Pane stopped, the smoke makes pane-smoke-kept 8 days old (past the
# default 7-day retention) and pane-smoke-enabled 2 hours old, as a
# downtime would: once Pane starts again, before the command shows
# anything, pane-smoke-kept is gone from the file and the list. Then, in
# the command: Delete, pane-smoke-second's third action (Ctrl+Shift+Enter,
# #150), deletes that item alone; Delete recent items (the last hour) deletes the two copied
# in this smoke's last minutes and keeps pane-smoke-enabled; keeping items
# for 1 hour deletes pane-smoke-enabled at once; and after one more copy,
# "Turn off and delete clipboard history" deletes it and turns history
# off, so a later copy is not kept, while the clipboard still holds what
# was copied last (pasting it into root search shows it; on Windows the
# smoke reads the clipboard directly, on Linux pasting is the only way to
# see it). The rows: Pause, Turn off, Keep items for…, Exclude a program,
# Clear, Turn off and delete, Delete recent items, then the items, newest
# first.
backdate() { python3 "$(dirname "$0")/clipboard_history.py" backdate "$extensions" "$@"; }
field() { python3 "$(dirname "$0")/clipboard_history.py" field "$extensions" "$1"; }
backdate 8 pane-smoke-kept || { echo "could not backdate the history"; exit 1; }
backdate 0.084 pane-smoke-enabled || { echo "could not backdate the history"; exit 1; }
start_pane
"$xdotool" windowfocus --sync "$window"
sleep 1
[ "$(kept_texts)" = pane-smoke-after-restart,pane-smoke-enabled,pane-smoke-second,pane-smoke-resumed ] || { echo "kept after starting: $(kept_texts)"; exit 1; }
open_history
capture 400-clipboard-expired.png
check 400-clipboard-expired.png subtitle   # pane-smoke-kept is no longer listed
"$xdotool" key Down Down Down Down Down Down Down Down Down; sleep 1   # pane-smoke-second: Paste, Copy, Delete
"$xdotool" key ctrl+shift+Return; sleep 1   # Delete, its third action
wait_for "$history" pane-smoke-second absent; sleep 1
capture 401-clipboard-item-deleted.png
check 401-clipboard-item-deleted.png success   # "Deleted the kept item"
[ "$(kept_texts)" = pane-smoke-after-restart,pane-smoke-enabled,pane-smoke-resumed ] || { echo "kept: $(kept_texts)"; exit 1; }
open_history
"$xdotool" key Down Down Down Down Down Down Return; sleep 1   # Delete recent items: 15 minutes, hour or day
"$xdotool" key Down Return; sleep 1   # the last hour
wait_for "$history" pane-smoke-resumed absent; sleep 1
capture 402-clipboard-recent-deleted.png
check 402-clipboard-recent-deleted.png success   # "Deleted 2 kept items"
[ "$(kept_texts)" = pane-smoke-enabled ] || { echo "kept: $(kept_texts)"; exit 1; }
"$xdotool" key Escape; sleep 1
open_history
"$xdotool" key Down Down Return; sleep 1   # Keep items for 7 days: 7 days (the retention now, chosen), 1 hour, 1 day, 30 or 90 days
"$xdotool" key Down Return; sleep 1   # 1 hour, the second choice
wait_for "$history" '"retentionSeconds": 3600' present; sleep 1
capture 403-clipboard-retention-changed.png
check 403-clipboard-retention-changed.png success   # "Items are kept for 1 hour; deleted 1 older item"
[ -z "$(kept_texts)" ] || { echo "kept: $(kept_texts)"; exit 1; }
# The retention form stays open after its choice, so one Escape returns
# to the command's list before `copy` leaves it for root search (without
# this, the typed text goes to the list, which has no text field, and
# nothing is copied).
"$xdotool" key Escape; sleep 1   # from the retention form to the command's list
copy pane-smoke-final
wait_for "$history" pane-smoke-final present
open_history
"$xdotool" key Down Down Down Down Down Return; sleep 1   # Turn off and delete clipboard history
wait_for "$history" pane-smoke-final absent; sleep 1
capture 404-clipboard-turned-off-and-deleted.png
check 404-clipboard-turned-off-and-deleted.png success   # "Clipboard history is off; deleted 1 kept item"
[ -z "$(field capture)" ] || { echo "history is still $(field capture)"; exit 1; }
# Deleting history never changes the system's clipboard: pasting what was
# copied last into root search still shows pane-smoke-final.
"$xdotool" key Escape Escape; sleep 1
"$xdotool" key ctrl+a ctrl+v; sleep 1
capture 405-clipboard-still-held.png
"$xdotool" key Escape; sleep 1
"$xdotool" type --delay 50 pane-smoke-final; sleep 1
capture 406-clipboard-held-typed.png
python3 "$(dirname "$0")/check_screenshot.py" --same "$out"/{405-clipboard-still-held,406-clipboard-held-typed}.png
"$xdotool" key Escape; sleep 1
copy pane-smoke-after-off
not_kept pane-smoke-after-off
python3 "$(dirname "$0")/check_screenshot.py" --distinct "$out"/{400-clipboard-expired,401-clipboard-item-deleted,402-clipboard-recent-deleted,403-clipboard-retention-changed,404-clipboard-turned-off-and-deleted,405-clipboard-still-held}.png
stop_pane
[ -z "$(kept_texts)" ] || { echo "kept: $(kept_texts)"; exit 1; }
[ "$(field retentionSeconds)" = 3600 ] || { echo "retention: $(field retentionSeconds)"; exit 1; }

# Installing Pane and acquiring its calculator (#53): the package
# `cargo xtask package-linux --dev` builds is installed on a clean machine
# — a fresh home folder, a PATH that holds nothing at all, so no Rust,
# Node, npm, Git or compiler can be reached — and Pane, started from what
# the install script installed, acquires its default extensions (the
# calculator, and the prebuilt-helper sample with it) from the artifact
# source this smoke serves on 127.0.0.1 (scripts/artifact_server.py, the
# payloads `cargo xtask package-linux` assembled; nothing reaches the
# network or Pane's published downloads). The calculator answers "6*7"
# with 42, and the helper sample's pane-echo runs: a prebuilt program
# from the acquired payload, no developer tool anywhere. The package is
# the development profile, because only a development build takes its
# artifact source from PANE_ARTIFACTS; a release build uses Pane's
# published downloads, which no controlled source may replace.
# (The binaries are removed again at the end of the phase: the uploaded
# evidence is the screenshots and records, not the program.)
cargo xtask package-linux --dev >/dev/null
package=$(ls target/dist/pane-*-linux-*-dev.tar.gz | head -1)
[ -n "$package" ] || { echo "the package was not built"; exit 1; }
# The clean home and unpacked package are absolute: Pane's HOME lands in
# its compile cache's directory, which Wasmtime needs absolute, and the
# smoke runs from the repository with a relative $out.
home=$PWD/$out/clean-home
unpack=$PWD/$out/package-unpacked
rm -rf "$home" "$unpack"
mkdir -p "$home" "$unpack"
rm -f "$out/artifact-server.port"
python3 "$(dirname "$0")/artifact_server.py" target/dist/artifacts "$out/artifact-server.port" 2>>"$out/artifact-server.log" &
artifact_server_pid=$!
for _ in $(seq 600); do [ -s "$out/artifact-server.port" ] && break; kill -0 "$artifact_server_pid" 2>/dev/null || break; sleep 0.1; done
[ -s "$out/artifact-server.port" ] || { echo "the local artifact source did not start (see $out/artifact-server.log)"; exit 1; }
tar -xzf "$package" -C "$unpack"
clean_bin=$out/clean-bin
rm -rf "$clean_bin"; mkdir -p "$clean_bin"
# Nothing can be reached at all from the PATH Pane runs with. The shell
# is named absolutely, so the check really runs: env would search for a
# bare `sh` in the empty PATH and fail before checking anything.
[ -z "$(env -i PATH="$clean_bin" /bin/sh -c 'command -v cargo rustc node npm git cc clang make' 2>/dev/null)" ] \
  || { echo "the clean machine still reaches a development tool"; exit 1; }
env -i HOME="$home" PATH="/usr/bin:/bin" bash "$unpack/pane/install.sh" >>"$out/install.log" 2>&1 \
  || { echo "the install script failed (see $out/install.log)"; exit 1; }
[ -x "$home/.local/bin/pane" ] || { echo "the install script installed no pane"; exit 1; }
start_installed() {
  env -i HOME="$home" PATH="$clean_bin" DISPLAY="$display" \
    PANE_THEME=dark PANE_MATERIAL=opaque \
    PANE_ARTIFACTS="http://127.0.0.1:$(cat "$out/artifact-server.port")/" \
    "$home/.local/bin/pane" "$@" 2>>"$out/installed-stderr.log" &
  pane_pid=$!
  window=
  for _ in $(seq 100); do
    window=$("$xdotool" search --onlyvisible --pid "$pane_pid" 2>/dev/null | head -1) && [ -n "$window" ] && break
    sleep 0.2
  done
  [ -n "$window" ] || { echo "the installed Pane window did not appear"; exit 1; }
  sleep 2
}
start_installed
# Pane ran with the PATH that holds nothing.
[ "$(tr '\0' '\n' <"/proc/$pane_pid/environ" | grep '^PATH=')" = "PATH=$clean_bin" ] \
  || { echo "the installed Pane did not run with the clean PATH"; exit 1; }
"$xdotool" windowfocus --sync "$window"
installed=$home/.local/share/pane/extensions
# Generous: a slow runner may take a while to check both payloads'
# components (300 s each). A wait that fails records the screen and the
# clean home's files — the artifact upload skips hidden folders, so the
# records are copied out where it can see them.
record_setup_state() {
  rm -rf "$out/clean-home-records"
  mkdir -p "$out/clean-home-records"
  cp -f "$installed/installed.json" "$out/clean-home-records/" 2>/dev/null || true
  cp -r "$installed/acquired" "$out/clean-home-records/" 2>/dev/null || true
  cp -r "$installed/downloads" "$out/clean-home-records/" 2>/dev/null || true
  cp -f "$out/installed-stderr.log" "$out/clean-home-records/" 2>/dev/null || true
  cp -f "$out/artifact-server.log" "$out/clean-home-records/" 2>/dev/null || true
}
record_setup_problem() {
  capture 499-setup-problem.png
  record_setup_state
}
wait_recorded() {
  for _ in $(seq 6000); do
    grep -q "$1" "$installed/installed.json" 2>/dev/null && return
    sleep 0.1
  done
  echo "$installed/installed.json: $1 is not present"
  record_setup_problem
  exit 1
}
kill -0 "$pane_pid" 2>/dev/null || { echo "the installed Pane exited during setup"; exit 1; }
# The release's default set (#60): all five, plus the helper sample a
# development build acquires with them.
for default_ in calculator applications quicklinks files clipboard-history helper-sample; do
  wait_recorded "\"default\": \"$default_\""
done
sleep 1
capture 500-installed-root.png
check 500-installed-root.png subtitle   # root search: the default extensions' commands are listed
"$xdotool" type --delay 50 '6*7'; sleep 2
capture 501-calculator-answer.png
check 501-calculator-answer.png answer   # "42", the calculator's selected answer card
"$xdotool" key Return; sleep 1
capture 502-calculator-copied.png
check 502-calculator-copied.png success   # "Copied 42 to the clipboard"
"$xdotool" key ctrl+a; "$xdotool" type --delay 50 helper; sleep 1
"$xdotool" key Return; sleep 2   # Helper sample
"$xdotool" key Return; sleep 3   # "Echo through the helper"
capture 503-helper-echoed.png
check 503-helper-echoed.png success   # "Echoed \"hello from Pane\" on Linux x86-64"
[ -n "$(ls "$installed"/packages/*/helpers/*/pane-echo)" ] \
  || { echo "the acquired payload's helper was not installed"; exit 1; }
[ -z "$(pgrep -f pane-echo)" ] || { echo "a helper is still running"; exit 1; }
# The payload the calculator acquired is kept, exactly its one current
# entry. GNU wc prints a bare count, but the padding is trimmed anyway,
# as the macOS smoke's does: one wording, and no platform's wc formatting
# can fail it.
[ "$(ls "$installed/acquired/calculator" | wc -l | tr -d ' ')" = 1 ] || { echo "the calculator's payload is not cached"; exit 1; }
[ -z "$(ls -A "$installed/downloads" 2>/dev/null)" ] || { echo "downloads were left behind"; exit 1; }
python3 "$(dirname "$0")/check_screenshot.py" --distinct "$out"/{500-installed-root,501-calculator-answer,503-helper-echoed}.png
stop_pane
kill "$artifact_server_pid"; wait "$artifact_server_pid" 2>/dev/null || true; artifact_server_pid=
# The program files go again: the evidence is the screenshots, the
# installed.json record and the logs.
record_setup_state
rm -f "$home/.local/bin/pane" "$unpack/pane/pane"

# Installing a Pane application update by the user's choice (#56): a
# second package is built with --package-version 99.0.0, whose program
# reports 99.0.0 and whose index entry names it (the two runnable builds
# the update goes between); the 0.1.0 package the #53 phase built is
# installed on another clean home, and its Pane, running from
# ~/.local/bin/pane, is told by the check it makes at start that 99.0.0
# exists: a row in root search with the version, and a word on the
# status line. Nothing is downloaded until that row is chosen; the
# choice is proven by the artifact server's log, which must hold no
# request for the package until then. A corrupted package is explained
# first (its bytes do not match the sha512 its index gives), everything
# untouched and the row ready to try again; then the real install
# downloads the tarball, checks it, and swaps the running pane — the old
# one renamed pane.old, removed on a later start — so the new version is
# used the next time Pane starts (Pane never restarts itself). The new
# Pane, started again, reports 99.0.0, with the old version's data (the
# calculator acquired at first setup) and the extension the user
# disabled kept, and with nothing of the update left in the bin folder.
cargo xtask package-linux --dev --package-version 99.0.0 >/dev/null
newer=$(ls target/dist/pane-99.0.0-linux-*-dev.tar.gz | head -1)
older=$(ls target/dist/pane-0.1.0-linux-*-dev.tar.gz | head -1)
[ -n "$newer" ] && [ -n "$older" ] || { echo "the two packages were not built"; exit 1; }
update_home=$PWD/$out/update-home
unpack_old=$PWD/$out/update-unpacked-old
unpack_new=$PWD/$out/update-unpacked-new
rm -rf "$update_home" "$unpack_old" "$unpack_new"
mkdir -p "$update_home" "$unpack_old" "$unpack_new"
rm -f "$out/update-artifact-server.port" "$out/update-artifact-server.log"
python3 "$(dirname "$0")/artifact_server.py" target/dist/artifacts "$out/update-artifact-server.port" \
  2>>"$out/update-artifact-server.log" &
artifact_server_pid=$!
for _ in $(seq 600); do [ -s "$out/update-artifact-server.port" ] && break; kill -0 "$artifact_server_pid" 2>/dev/null || break; sleep 0.1; done
[ -s "$out/update-artifact-server.port" ] \
  || { echo "the update artifact source did not start (see $out/update-artifact-server.log)"; exit 1; }
tar -xzf "$older" -C "$unpack_old"
tar -xzf "$newer" -C "$unpack_new"
# The 0.1.0 package installed on another clean home, as the #53 phase
# installed it: the install script, the empty PATH, the data under
# ~/.local/share/pane of that home.
env -i HOME="$update_home" PATH="/usr/bin:/bin" bash "$unpack_old/pane/install.sh" \
  >>"$out/update-install.log" 2>&1 \
  || { echo "the install script failed (see $out/update-install.log)"; exit 1; }
[ -x "$update_home/.local/bin/pane" ] || { echo "the install script installed no pane"; exit 1; }
update_program=$update_home/.local/bin/pane
update_registry=$update_home/.local/share/pane/extensions/installed.json
update_log=$out/update-artifact-server.log
# Starts the installed Pane of the update home, with the PATH that holds
# nothing and the controlled artifact source, as the #53 phase's
# start_installed does for its own home.
start_update_pane() {
  env -i HOME="$update_home" PATH="$clean_bin" DISPLAY="$display" \
    PANE_THEME=dark PANE_MATERIAL=opaque \
    PANE_ARTIFACTS="http://127.0.0.1:$(cat "$out/update-artifact-server.port")/" \
    "$update_program" "$@" 2>>"$out/update-stderr.log" &
  pane_pid=$!
  window=
  for _ in $(seq 100); do
    window=$("$xdotool" search --onlyvisible --pid "$pane_pid" 2>/dev/null | head -1) && [ -n "$window" ] && break
    sleep 0.2
  done
  [ -n "$window" ] || { echo "the installed Pane window did not appear"; exit 1; }
  sleep 2
}
start_update_pane
"$xdotool" windowfocus --sync "$window"
# First setup: the default extensions are acquired (2 index reads), and
# Pane's own check reads the index once more — its request is the third.
for _ in $(seq 2000); do
  [ "$(grep -c pane-defaults.json "$update_log" 2>/dev/null || true)" -ge 3 ] && break
  kill -0 "$pane_pid" 2>/dev/null || { echo "the installed Pane exited during setup"; exit 1; }
  sleep 0.1
done
[ "$(grep -c pane-defaults.json "$update_log" 2>/dev/null || true)" -ge 3 ] \
  || { echo "Pane never checked for its own update"; exit 1; }
wait_for "$update_registry" '"default": "calculator"' present 6000
wait_for "$update_registry" '"default": "helper-sample"' present 6000
# The check has told the user what it found; nothing has been downloaded.
capture_until 600-notification.png success 30   # "Pane 99.0.0 is available" (or the setup's own outcome)
"$xdotool" key ctrl+a; "$xdotool" type --delay 50 update; sleep 1
capture_until 601-offered.png subtitle 30   # the offer row: "Your extensions and settings are kept; ..."
check 601-offered.png selected 3000   # the row, selected
# Taking no action downloads nothing: no package was asked for.
[ -z "$(grep "\.tar\.gz" "$update_log")" ] || { echo "a package was downloaded without the user choosing it"; exit 1; }

# Disable the Helper sample first: an extension the user disabled before
# the update must stay disabled after it.
"$xdotool" key ctrl+a; "$xdotool" type --delay 50 manage; sleep 1
"$xdotool" key Return; sleep 1   # Manage extensions…
# The Helper sample is the sixth extension now (#60's set is listed
# first), so five Downs reach it.
"$xdotool" key Down Down Down Down Down Return; sleep 2   # Helper sample: disabled
wait_for "$update_registry" '"disabled": true' present 600
"$xdotool" key Escape; sleep 1

# A package that does not match the integrity its index gives is
# explained and not installed: the bytes of the served package are
# damaged, and the program keeps running the one it was.
served=target/dist/artifacts/$(basename "$newer")
python3 - "$served" <<'EOF'
import sys
with open(sys.argv[1], "rb") as file:
    package = bytearray(file.read())
package[len(package) // 2] ^= 1
with open(sys.argv[1], "wb") as file:
    file.write(package)
EOF
"$xdotool" key ctrl+a; "$xdotool" type --delay 50 update; sleep 1
"$xdotool" key Return
capture_until 602-corrupt-package.png error 60   # "Could not update Pane to 99.0.0: ... does not match the sha512 integrity"
[ ! -e "$update_home/.local/bin/pane.old" ] || { echo "a failed install replaced the program"; exit 1; }
cmp -s "$update_program" "$unpack_old/pane/pane" || { echo "a failed install changed the program"; exit 1; }
[ ! -e "$update_home/.local/bin/update" ] || { echo "a failed install left its staging behind"; exit 1; }

# The source works again; the row that stays tries again, and the update
# is installed: the new program takes the old one's name and place, the
# old one is renamed out of its way.
cp "$newer" "$served"
"$xdotool" key ctrl+a; "$xdotool" type --delay 50 update; sleep 1
"$xdotool" key Return
for _ in $(seq 1200); do
  kill -0 "$pane_pid" 2>/dev/null || { echo "Pane exited while updating itself"; exit 1; }
  [ -e "$update_home/.local/bin/pane.old" ] && break
  sleep 0.2
done
[ -e "$update_home/.local/bin/pane.old" ] || { echo "the update was not installed"; exit 1; }
capture_until 603-installed.png success 30   # "Installed Pane 99.0.0; the new version is used the next time Pane starts"
cmp -s "$update_program" "$unpack_new/pane/pane" || { echo "the new program was not installed"; exit 1; }
cmp -s "$update_home/.local/bin/pane.old" "$unpack_old/pane/pane" \
  || { echo "the old program was not kept out of the new one's way"; exit 1; }
[ ! -e "$update_home/.local/bin/update" ] || { echo "the install left its staging behind"; exit 1; }
# The package was downloaded once for each attempt: the damaged one and
# the one that installed.
[ "$(grep -c "\.tar\.gz" "$update_log")" = 2 ] || { echo "the package was not downloaded exactly twice"; exit 1; }
stop_pane

# The next start runs the new version: it reports 99.0.0, removes what
# the update left, and the old version's data is kept — the calculator
# answers and the Helper sample stays disabled.
version=$("$update_program" --version) || { echo "the new pane --version failed"; exit 1; }
[ "$version" = "Pane 99.0.0" ] || { echo "the new program reports the wrong version: $version"; exit 1; }
start_update_pane
"$xdotool" windowfocus --sync "$window"
for _ in $(seq 500); do [ ! -e "$update_home/.local/bin/pane.old" ] && break; sleep 0.2; done
[ ! -e "$update_home/.local/bin/pane.old" ] || { echo "the old program's file was not removed on the new start"; exit 1; }
"$xdotool" key ctrl+a; "$xdotool" type --delay 50 '6*7'
capture_until 604-answer-after-update.png answer 30   # "42", the calculator's selected answer card
"$xdotool" key Return
capture_until 605-copied-after-update.png success 10   # "Copied 42 to the clipboard"
wait_for "$update_registry" '"disabled": true' present
python3 "$(dirname "$0")/check_screenshot.py" --distinct \
  "$out"/{600-notification,601-offered,602-corrupt-package,603-installed,604-answer-after-update}.png
stop_pane
# The program files go again, as the #53 phase's do.
rm -f "$update_program" "$unpack_old/pane/pane" "$unpack_new/pane/pane"
kill "$artifact_server_pid"; wait "$artifact_server_pid" 2>/dev/null || true; artifact_server_pid=
echo "screenshots in $out"
