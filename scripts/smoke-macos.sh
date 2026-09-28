#!/usr/bin/env bash
# Native GUI smoke on macOS: launches Pane, drives it with real key events
# (System Events via osascript, which needs the Accessibility permission the
# GitHub macOS runners grant) and captures and checks the screen.
# Requires Python 3 with Pillow for the screenshot checks. Pane keeps
# installed packages in <output-dir>/data, not the user's data folder.
# Usage: scripts/smoke-macos.sh <output-dir> [pane-binary]
set -euo pipefail
out=${1:-smoke}
pane=${2:-target/debug/pane}
mkdir -p "$out"
rm -rf "$out/data"
export PANE_DATA_DIR=$out/data
{ sw_vers; uname -m; } >"$out/system.txt"   # the tested OS version and architecture

pid=
trap '[ -n "$pid" ] && kill "$pid" 2>/dev/null || true' EXIT

capture() { screencapture -x "$out/$1"; }
check() { python3 "$(dirname "$0")/check_screenshot.py" "$out/$1" "$2" ${3:+"$3"}; }
# Prints "x y": where the screenshot shows the given color.
locate() { python3 "$(dirname "$0")/check_screenshot.py" --locate "$out/$1" "$2"; }
# Clicks the primary button at x y in the pixels of screenshot $3: Quartz
# events through ctypes, in points, which are half the pixels on Retina.
click_at() {
  python3 - "$1" "$2" "$out/$3" <<'PY'
import ctypes, sys
from PIL import Image
class CGPoint(ctypes.Structure): _fields_ = [("x", ctypes.c_double), ("y", ctypes.c_double)]
class CGRect(ctypes.Structure): _fields_ = [("origin", CGPoint), ("size", CGPoint)]
cg = ctypes.cdll.LoadLibrary("/System/Library/Frameworks/CoreGraphics.framework/CoreGraphics")
cg.CGMainDisplayID.restype = ctypes.c_uint32
cg.CGDisplayBounds.restype, cg.CGDisplayBounds.argtypes = CGRect, [ctypes.c_uint32]
cg.CGEventCreateMouseEvent.restype = ctypes.c_void_p
cg.CGEventCreateMouseEvent.argtypes = [ctypes.c_void_p, ctypes.c_uint32, CGPoint, ctypes.c_uint32]
cg.CGEventPost.argtypes = [ctypes.c_uint32, ctypes.c_void_p]
scale = cg.CGDisplayBounds(cg.CGMainDisplayID()).size.x / Image.open(sys.argv[3]).width
at = CGPoint(int(sys.argv[1]) * scale, int(sys.argv[2]) * scale)
for event in (1, 2):  # left mouse down, left mouse up
    cg.CGEventPost(0, cg.CGEventCreateMouseEvent(None, event, at, 0))
PY
}
key() {  # macOS virtual key codes: 36 Return, 125 Down, 124 Right, 53 Escape, 48 Tab
  osascript -e "tell application \"System Events\" to key code $1"
}
type_text() { osascript -e "tell application \"System Events\" to keystroke \"$1\""; }
command_key() { osascript -e "tell application \"System Events\" to keystroke \"$1\" using command down"; }

# Brings the running Pane to the front, so that key events reach it.
focus_pane() {
  osascript -e "tell application \"System Events\" to set frontmost of (first process whose unix id is $pid) to true"
  sleep 1
}

# Starts Pane with the given arguments and brings it to the front.
start_pane() {
  "$pane" "$@" 2>>"$out/stderr.log" &
  pid=$!
  sleep 8
  focus_pane
}

stop_pane() {
  kill -0 "$pid" || { echo "Pane exited during the smoke"; exit 1; }
  kill "$pid"
  wait "$pid" 2>/dev/null || true
  pid=
}

start_pane
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

# The Rust command's form (its fifth item): submitting it empty is rejected
# and focus returns to the name, so typing there and choosing a greeting with
# Tab and Down makes the guest answer.
key 36; sleep 3
for _ in 1 2 3 4; do key 125; done
key 36; sleep 1
capture 6-form.png
key 36; sleep 2
capture 7-form-error.png
check 7-form-error.png f08c8c   # the rejected field's message
type_text Ada
key 48; key 125; key 36; sleep 2
capture 8-form-result.png
check 8-form-result.png 9fd8a8   # the guest's answer
key 53; key 53; sleep 1
stop_pane

# Install the assembled Rust sample package (the folder the picker would
# return), then run its command. Root lists the three samples, the installed
# command, then the install and Manage extensions… rows.
start_pane --install target/guests/packages/sample-rust
capture 9-package.png
check 9-package.png aab4c0   # the package's identity and compatibility lines
key 36; sleep 2
capture 10-installed.png
check 10-installed.png 9fd8a8   # "Installed Rust sample"
key 36; sleep 3
key 36; sleep 2
capture 11-installed-result.png
check 11-installed-result.png 9fd8a8   # the installed guest's answer
stop_pane

# The installed command is still listed after a restart.
start_pane
capture 12-restarted.png
check 12-restarted.png 8a96a3
[ -f "$out/data/extensions/installed.json" ] || { echo "no install record"; exit 1; }
focus_pane

# The Rust command's seventh item is declared for Windows only, its eighth
# for macOS and Linux only. Here the first is explained without running and
# the second runs.
key 36; sleep 3
for _ in 1 2 3 4 5 6; do key 125; done
key 36; sleep 2
capture 13-windows-only.png
check 13-windows-only.png d6a36a   # the row's reason
check 13-windows-only.png f08c8c   # macOS: the reason as the error
key 125; key 36; sleep 2
capture 14-not-windows.png
check 14-not-windows.png 9fd8a8    # macOS: the guest's answer
key 53; sleep 1
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
  "platforms": ["windows", "linux"],
  "commands": [{ "id": "sample", "title": "Elsewhere sample", "component": "sample_rust.wasm" }]
}
JSON
start_pane --install "$out/elsewhere"
capture 15-no-compatible-package.png
check 15-no-compatible-package.png f08c8c   # "Not available on macOS: ..."
stop_pane

# Install the settings sample, save a choice with it, then disable it in
# Manage extensions. Root lists the three samples, Rust sample, Greeting, the
# install row, then Manage extensions… last; the extension list holds Rust
# sample, then Settings sample.
start_pane --install target/guests/packages/sample-settings
key 36; sleep 2   # Install; Greeting is selected
key 36; sleep 3   # open Greeting
key 36; sleep 2   # "Use a formal greeting"
capture 16-setting-saved.png
check 16-setting-saved.png 9fd8a8   # "Saved the formal greeting"
key 53; sleep 1
for ((i = 0; i < 10; i++)); do key 125; done   # the last row
key 36; sleep 1
key 125; key 36; sleep 2
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
capture 18-restarted-disabled.png
check 18-restarted-disabled.png 8a96a3
python3 "$(dirname "$0")/check_screenshot.py" --same "$out/12-restarted.png" "$out/18-restarted-disabled.png"
for ((i = 0; i < 10; i++)); do key 125; done
key 36; sleep 1
key 125; key 36; sleep 2
capture 19-enabled.png
check 19-enabled.png 9fd8a8   # "Enabled Settings sample"
key 53; sleep 1
for ((i = 0; i < 4; i++)); do key 125; done   # Greeting
key 36; sleep 3
key 125; key 125; key 36; sleep 2   # "Greet me"
capture 20-greeted.png
check 20-greeted.png 9fd8a8   # "Good day to you"
stop_pane

# Restarted, root lists Greeting again, after Rust sample.
start_pane
focus_pane

# The Rust command's color picker (its sixth item), which the guest draws:
# Right chooses purple, and a click on the dark green swatch chooses it. The
# chosen color fills its swatch and the preview, far more pixels than any
# other swatch covers.
key 36; sleep 3
for _ in 1 2 3 4 5; do key 125; done
key 36; sleep 2
capture 21-color.png
check 21-color.png 1e88e5 3000   # blue, chosen when the view opens
key 124; sleep 1
capture 22-color-key.png
check 22-color-key.png 8e24aa 3000   # purple
read -r x y < <(locate 22-color-key.png 1b5e20)
click_at "$x" "$y" 22-color-key.png; sleep 1
capture 23-color-click.png
check 23-color-click.png 1b5e20 3000   # dark green
key 53; key 53; sleep 1
stop_pane

# Root search: typing narrows root to the matching commands and Enter opens
# the best match. "typescr" matches only TypeScript sample, whose "Wait
# briefly" answers exactly as in step 4. A query that matches nothing shows
# no results, and Enter then opens nothing.
start_pane
type_text typescr; sleep 1
capture 24-search.png
key 36; sleep 3
key 125; key 36; sleep 2
capture 25-search-result.png
check 25-search-result.png 9fd8a8   # the TypeScript guest's answer
python3 "$(dirname "$0")/check_screenshot.py" --same "$out/4-result-2.png" "$out/25-search-result.png"
key 53; sleep 1
type_text zzz; sleep 1
key 36; sleep 1
capture 26-no-results.png
python3 "$(dirname "$0")/check_screenshot.py" --distinct "$out"/{1-root,24-search,25-search-result,26-no-results}.png
stop_pane

# The calculator, a default extension: an expression typed into root search
# lists its answer first, selected, and Enter copies it. Pasting the copy
# over the query and typing on shows exactly the screen typing the whole
# expression shows, so the clipboard held the answer.
start_pane --install target/guests/packages/calculator
key 36; sleep 2   # Install
type_text '6*7'; sleep 2
capture 27-answer.png
check 27-answer.png 364355 3000   # the selected answer row
key 36; sleep 1
capture 28-copied.png   # "Copied 42 to the clipboard"
command_key a; type_text '42+1'; sleep 2
capture 29-typed.png
command_key a; command_key v; type_text '+1'; sleep 2
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
key 36; sleep 2   # Install
capture 31-operations-target.png
check 31-operations-target.png 9fd8a8   # "Installed JavaScript operations sample"
stop_pane
start_pane --install target/guests/packages/sample-operations
key 36; sleep 2   # Install; Call from Rust is selected
key 36; sleep 3   # open Call from Rust
key 36; sleep 2   # "Greet through another extension": its form
type_text "local:$(cd target/guests/packages/sample-operations-js && pwd -P)"
key 48; type_text Rust
key 36; sleep 5   # Greet
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
key 36; sleep 2   # Install; Dev sample is selected
key 36; sleep 3
key 36; sleep 2   # "Say hello"
capture 33-dev-before.png
check 33-dev-before.png 9fd8a8   # "Hello from the Rust guest"
key 53; sleep 1
cp target/guests/sample_js.wasm "$out/dev/command.wasm"
for ((i = 0; i < 12; i++)); do key 125; done   # the last row
key 36; sleep 1
for ((i = 0; i < 11; i++)); do key 125; done   # Reload Dev
key 36; sleep 3
capture 34-reloaded.png
check 34-reloaded.png 9fd8a8   # "Reloaded Dev"
key 53; sleep 1
for ((i = 0; i < 8; i++)); do key 125; done   # Dev sample
key 36; sleep 3
key 36; sleep 2   # "Say hello"
capture 35-dev-after.png
check 35-dev-after.png 9fd8a8   # "Hello from the JavaScript guest"
python3 "$(dirname "$0")/check_screenshot.py" --distinct "$out/33-dev-before.png" "$out/35-dev-after.png"
key 53; sleep 1

# A build that fails the install checks (here its component is missing) is
# not reloaded: the working code keeps running, exactly as before.
rm "$out/dev/command.wasm"
for ((i = 0; i < 12; i++)); do key 125; done
key 36; sleep 1
for ((i = 0; i < 11; i++)); do key 125; done
key 36; sleep 2
capture 36-not-reloaded.png
check 36-not-reloaded.png f08c8c   # "Dev was not reloaded: ..."
key 53; sleep 1
for ((i = 0; i < 8; i++)); do key 125; done
key 36; sleep 3
key 36; sleep 2
capture 37-still-running.png
python3 "$(dirname "$0")/check_screenshot.py" --same "$out/35-dev-after.png" "$out/37-still-running.png"
key 53; sleep 1

# A build whose start fails is reported with Retry, after Reload Dev; this
# one saves a setting and fails its first start only, so Retry starts it.
cp target/guests/failing_start.wasm "$out/dev/command.wasm"
for ((i = 0; i < 12; i++)); do key 125; done
key 36; sleep 1
for ((i = 0; i < 11; i++)); do key 125; done
key 36; sleep 3
capture 38-start-failed.png
check 38-start-failed.png f08c8c   # "Reloaded Dev, but it failed to start; ..."
key 125; key 36; sleep 3   # Retry starting Dev
capture 39-retried.png
check 39-retried.png 9fd8a8   # "Started Dev"
stop_pane
grep -q '"start-attempted": "yes"' "$out/data/extensions/settings.json" || { echo "the failed start's setting was not kept"; exit 1; }

# The settings sample keeps one value of each kind of data: its formal style
# (settings) and "Good day to you" (cache) are saved above; its fourth and
# fifth items save a note (content) and sign in (a local credential), and its
# sixth shows all four.
start_pane
for ((i = 0; i < 4; i++)); do key 125; done   # Greeting
key 36; sleep 3
for ((i = 0; i < 3; i++)); do key 125; done
key 36; sleep 2   # "Save a note"
key 125; key 36; sleep 2   # "Sign in"
key 125; key 36; sleep 2   # "Show what Pane keeps"
capture 40-kept.png
check 40-kept.png 9fd8a8   # every value, the cached greeting included
key 53; sleep 1
stop_pane
grep -q '"note": "Water the plants"' "$out/data/extensions/content.json" || { echo "note not saved"; exit 1; }
grep -q '"token": "sample-token"' "$out/data/extensions/credentials.json" || { echo "credential not saved"; exit 1; }
grep -q '"last-greeting": "Good day to you"' "$out/data/extensions/cache.json" || { echo "greeting not cached"; exit 1; }

# Clear the settings sample's cache in Manage extensions: its row follows the
# six package rows, their six Reload rows and "Clear cache of Rust sample". Pane asks first, then deletes only the cached
# greeting, without running the extension.
start_pane
for ((i = 0; i < 12; i++)); do key 125; done   # the last row
key 36; sleep 1
for ((i = 0; i < 13; i++)); do key 125; done
key 36; sleep 1   # "Clear cache of Settings sample"
capture 41-confirm-clear-cache.png
check 41-confirm-clear-cache.png aab4c0   # what is deleted and what is kept
key 36; sleep 2   # "Clear cache"
capture 42-cache-cleared.png
check 42-cache-cleared.png 9fd8a8   # "Cleared the cache of Settings sample"
key 53; sleep 1
for ((i = 0; i < 4; i++)); do key 125; done   # Greeting
key 36; sleep 3
for ((i = 0; i < 5; i++)); do key 125; done
key 36; sleep 2   # "Show what Pane keeps"
capture 43-kept-after-clear.png
check 43-kept-after-clear.png 9fd8a8   # "... Cached greeting: none"
python3 "$(dirname "$0")/check_screenshot.py" --distinct "$out/40-kept.png" "$out/43-kept-after-clear.png"
key 53; sleep 1
stop_pane
if grep -q 'Good day to you' "$out/data/extensions/cache.json"; then echo "cache not cleared"; exit 1; fi
grep -q '"greeting-style": "formal"' "$out/data/extensions/settings.json" || { echo "setting lost"; exit 1; }
grep -q '"note": "Water the plants"' "$out/data/extensions/content.json" || { echo "note lost"; exit 1; }
grep -q '"token": "sample-token"' "$out/data/extensions/credentials.json" || { echo "credential lost"; exit 1; }

# Applications, a default extension: an installed application is found by
# name in root search and Enter opens it. The application is a bundle the
# smoke adds in ~/Applications of a HOME of its own (for Pane only), whose
# program writes a marker file, so nothing else is started; Pane still
# searches the system's applications too.
apps=$(cd "$out" && pwd)/apps
rm -rf "$apps"
bundle="$apps/home/Applications/Pane Smoke App.app"
mkdir -p "$bundle/Contents/MacOS"
cat >"$bundle/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>smoke</string>
<key>CFBundleIdentifier</key><string>dev.pane.smoke-app</string>
<key>CFBundleName</key><string>Pane Smoke App</string>
<key>CFBundlePackageType</key><string>APPL</string>
</dict></plist>
EOF
printf '#!/bin/sh\necho launched > "%s"\n' "$apps/launched" >"$bundle/Contents/MacOS/smoke"
chmod +x "$bundle/Contents/MacOS/smoke"
HOME=$apps/home "$pane" --install target/guests/packages/applications 2>>"$out/stderr.log" &
pid=$!
sleep 8
focus_pane
key 36; sleep 2   # Install
type_text 'pane smoke'; sleep 3
capture 44-application.png
check 44-application.png 364355 3000   # the selected application row
key 36; sleep 3
focus_pane
capture 45-opened.png
check 45-opened.png 9fd8a8   # "Opened Pane Smoke App"
for _ in $(seq 50); do [ -f "$apps/launched" ] && break; sleep 0.2; done
[ -f "$apps/launched" ] || { echo "the application did not run"; exit 1; }
python3 "$(dirname "$0")/check_screenshot.py" --distinct "$out"/{44-application,45-opened}.png
stop_pane

# Quicklinks, a default extension: installed, its command's form saves a
# quicklink (Quicklinks is selected once installed, and "Create quicklink" is
# its first item). After a restart, typing part of its name lists it,
# selected. Enter would open the default browser, so this smoke stops there
# (the Linux smoke opens it through a recording handler).
start_pane --install target/guests/packages/quicklinks
key 36; sleep 2   # Install
key 36; sleep 3   # open Quicklinks
key 36; sleep 1   # Create quicklink
type_text 'Pane issues'
key 48
type_text 'https://example.com/pane-issues'
key 36; sleep 2
capture 46-quicklink-saved.png
check 46-quicklink-saved.png 9fd8a8   # "Saved quicklink “Pane issues”"
key 53; key 53; sleep 1
stop_pane
start_pane
type_text 'pane iss'; sleep 2
capture 47-quicklink-found.png
check 47-quicklink-found.png 364355 3000   # the selected quicklink row
python3 "$(dirname "$0")/check_screenshot.py" --distinct "$out"/{46-quicklink-saved,47-quicklink-found}.png
stop_pane

# Uninstall the settings sample, keeping its saved data: its row follows the
# eight Clear cache rows. Pane asks first, showing its saved data, and the first
# choice keeps its settings and content while its copy and credential go.
# Installing the same folder again finds its formal style and note, signed out.
start_pane
for ((i = 0; i < 20; i++)); do key 125; done   # the last row
key 36; sleep 1
for ((i = 0; i < 25; i++)); do key 125; done
key 36; sleep 1   # "Uninstall Settings sample"
capture 49-confirm-uninstall.png
check 49-confirm-uninstall.png aab4c0   # what is removed and the saved data
key 36; sleep 2   # "Uninstall and keep saved data"
capture 50-uninstalled.png
check 50-uninstalled.png 9fd8a8   # "Uninstalled Settings sample; its settings and content are kept"
stop_pane
grep -q '"retained"' "$out/data/extensions/installed.json" || { echo "kept data not recorded"; exit 1; }
if grep -q 'sample-token' "$out/data/extensions/credentials.json"; then echo "credential not removed"; exit 1; fi
grep -q '"greeting-style": "formal"' "$out/data/extensions/settings.json" || { echo "setting not kept"; exit 1; }
grep -q '"note": "Water the plants"' "$out/data/extensions/content.json" || { echo "note not kept"; exit 1; }
start_pane --install target/guests/packages/sample-settings
key 36; sleep 2   # Install; Greeting is selected
key 36; sleep 3   # open Greeting
for ((i = 0; i < 5; i++)); do key 125; done
key 36; sleep 2   # "Show what Pane keeps"
capture 51-reinstalled.png
check 51-reinstalled.png 9fd8a8   # "Style: formal · Note: Water the plants · Signed in: no ..."
python3 "$(dirname "$0")/check_screenshot.py" --distinct "$out/43-kept-after-clear.png" "$out/51-reinstalled.png"
key 53; sleep 1
stop_pane
if grep -q '"retained"' "$out/data/extensions/installed.json"; then echo "retained record not dropped"; exit 1; fi

# Global hotkeys: in Manage extensions, the settings sample's command,
# Greeting, is given Control+Option+G by pressing it on its hotkey screen
# (its row follows the package's state, Reload, Clear cache and Uninstall
# rows). With Finder in front, pressing the hotkey brings Pane to the front
# with Greeting open, also after a restart; once the extension is disabled, pressing it
# does nothing. Carbon hot keys need no permission of Pane's own. A data
# folder of its own keeps the rows in a known order. (Screenshot 48 is the
# Linux smoke's opened quicklink.)
frontmost() { osascript -e 'tell application "System Events" to get unix id of first process whose frontmost is true'; }
unfocus_pane() {   # another application in front
  osascript -e 'tell application "Finder" to activate'; sleep 2
  [ "$(frontmost)" != "$pid" ] || { echo "Pane is still in front"; exit 1; }
}
press_hotkey() {
  osascript -e 'tell application "System Events" to keystroke "g" using {control down, option down}'
  sleep 3
}
check_pane_in_front() {
  [ "$(frontmost)" = "$pid" ] || { echo "the hotkey did not bring Pane to the front"; exit 1; }
}
export PANE_DATA_DIR=$out/hotkeys-data
rm -rf "$PANE_DATA_DIR"
start_pane --install target/guests/packages/sample-settings
key 36; sleep 2   # Install; Greeting is selected
for ((i = 0; i < 10; i++)); do key 125; done   # Manage extensions…
key 36; sleep 1
key 125; key 125; key 125; key 125; key 36; sleep 1   # "Hotkey for Greeting"
capture 52-hotkey-screen.png
check 52-hotkey-screen.png aab4c0   # "Press the keys that should open Greeting ..."
press_hotkey   # Pane is in front: this assigns it
capture 53-hotkey-assigned.png
check 53-hotkey-assigned.png 9fd8a8   # "Control+Option+G now opens Greeting"
key 53; sleep 1   # root search
unfocus_pane
capture 54-unfocused.png
press_hotkey
check_pane_in_front
capture 55-hotkey-opened.png
check 55-hotkey-opened.png 364355 3000   # Greeting's first item, selected
python3 "$(dirname "$0")/check_screenshot.py" --distinct "$out"/{53-hotkey-assigned,55-hotkey-opened}.png
stop_pane
grep -q '"ctrl+alt+g"' "$PANE_DATA_DIR/extensions/hotkeys.json" || { echo "hotkey not recorded"; exit 1; }
start_pane
unfocus_pane
press_hotkey
check_pane_in_front
capture 56-hotkey-after-restart.png
check 56-hotkey-after-restart.png 364355 3000   # Greeting's first item, selected
python3 "$(dirname "$0")/check_screenshot.py" --distinct "$out"/{53-hotkey-assigned,56-hotkey-after-restart}.png
key 53; sleep 1
for ((i = 0; i < 10; i++)); do key 125; done   # Manage extensions…
key 36; sleep 1
key 36; sleep 2   # disable Settings sample
key 53; sleep 1
capture 57-disabled.png   # root search
unfocus_pane
press_hotkey
[ "$(frontmost)" != "$pid" ] || { echo "the released hotkey still brought Pane to the front"; exit 1; }
focus_pane
capture 58-disabled-pressed.png   # still root search: nothing opened
python3 "$(dirname "$0")/check_screenshot.py" --same "$out"/{57-disabled,58-disabled-pressed}.png
stop_pane
echo "screenshots in $out"
