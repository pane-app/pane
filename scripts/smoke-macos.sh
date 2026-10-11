#!/usr/bin/env bash
# Native GUI smoke on macOS: launches Pane, drives it with real key events
# (System Events via osascript, which needs the Accessibility permission the
# GitHub macOS runners grant) and captures and checks the screen.
# Requires Python 3 with Pillow for the screenshot checks. Pane keeps
# installed packages in <output-dir>/data, not the user's data folder.
# Settings is driven through its accessibility tree too (see `a11y`). The
# phases that set the default extensions up serve their repositories
# (made from the packages `cargo xtask guests` assembles) on 127.0.0.1, so
# they run after it. The update phases serve the artifacts the #52 phase's
# package build leaves in target/dist/artifacts, so they run after it too.
# Usage: scripts/smoke-macos.sh <output-dir> [pane-binary]

set -euo pipefail
# Behavior captures use a fixed palette without desktop-dependent glass.
export PANE_THEME=dark PANE_MATERIAL=opaque
out=${1:-smoke}
pane=${2:-target/debug/pane}
mkdir -p "$out"
rm -rf "$out/data"
export PANE_DATA_DIR=$out/data
# A development build takes its default extensions' pins from PANE_DEFAULTS;
# without it, the committed pins point at the real repositories, which no
# check may reach. Until a phase names its own pins, the file names none:
# first setup adds nothing, which the phases that install samples by hand
# need (the phase that checks first setup points it at the repositories it
# serves).
no_default_pins=$out/no-default-pins.json
printf '[]\n' >"$no_default_pins"
export PANE_DEFAULTS=$no_default_pins
{ sw_vers; uname -m; } >"$out/system.txt"   # the tested OS version and architecture

printf "%s\n" "theme=dark material=opaque (behavior smoke; not blur evidence)" >>"$out/system.txt"

pid=
npm_registry_pid=
repository_server_pid=
trap '[ -n "$pid" ] && kill "$pid" 2>/dev/null; [ -n "$npm_registry_pid" ] && kill "$npm_registry_pid" 2>/dev/null; [ -n "$repository_server_pid" ] && kill "$repository_server_pid" 2>/dev/null || true' EXIT

capture() { screencapture -x "$out/$1"; }
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
# Waits, for at most $2 seconds, until the Pane window no longer shows text
# in color $1: a toast leaves 3 seconds after it shows (#141), but a slow
# runner can still show it after a fixed wait, and the next answer's toast,
# of the same color, would then be taken for it (release run 37725624283's
# macOS frame 91 showed the earlier answer).
until_toast_gone() {
  # Hover pauses the toast's timer. Move off the window without clicking
  # or changing which control has the keyboard.
  python3 - <<'PY'
import ctypes
class CGPoint(ctypes.Structure): _fields_ = [("x", ctypes.c_double), ("y", ctypes.c_double)]
cg = ctypes.cdll.LoadLibrary("/System/Library/Frameworks/CoreGraphics.framework/CoreGraphics")
cg.CGEventCreateMouseEvent.restype = ctypes.c_void_p
cg.CGEventCreateMouseEvent.argtypes = [ctypes.c_void_p, ctypes.c_uint32, CGPoint, ctypes.c_uint32]
cg.CGEventPost.argtypes = [ctypes.c_uint32, ctypes.c_void_p]
cg.CGEventPost(0, cg.CGEventCreateMouseEvent(None, 5, CGPoint(1, 40), 0))
PY
  local deadline=$((SECONDS + $2))
  while :; do
    capture toast-wait.png
    python3 "$(dirname "$0")/check_screenshot.py" --absent "$out/toast-wait.png" "$1" >/dev/null 2>&1 && return 0
    [ "$SECONDS" -lt "$deadline" ] || { echo "the $1 toast did not leave within $2 seconds"; exit 1; }
    sleep 0.5
  done
}
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
key() {  # macOS virtual key codes: 36 Return, 125 Down, 126 Up, 124 Right, 53 Escape, 48 Tab
  osascript -e "tell application \"System Events\" to key code $1"
}
type_text() { osascript -e "tell application \"System Events\" to keystroke \"$1\""; }
command_key() { osascript -e "tell application \"System Events\" to keystroke \"$1\" using command down"; }

# Clicks the primary button at x y in points (where System Events places
# an element), as click_at does.
click_points() {
  python3 - "$1" "$2" <<'PY'
import ctypes, sys
class CGPoint(ctypes.Structure): _fields_ = [("x", ctypes.c_double), ("y", ctypes.c_double)]
cg = ctypes.cdll.LoadLibrary("/System/Library/Frameworks/CoreGraphics.framework/CoreGraphics")
cg.CGEventCreateMouseEvent.restype = ctypes.c_void_p
cg.CGEventCreateMouseEvent.argtypes = [ctypes.c_void_p, ctypes.c_uint32, CGPoint, ctypes.c_uint32]
cg.CGEventPost.argtypes = [ctypes.c_uint32, ctypes.c_void_p]
at = CGPoint(float(sys.argv[1]), float(sys.argv[2]))
for event in (1, 2):  # left mouse down, left mouse up
    cg.CGEventPost(0, cg.CGEventCreateMouseEvent(None, event, at, 0))
PY
}
# Back to a blank root search from wherever the launcher is, with the
# return to root key (Command+Escape): Escape at a blank root search hides
# the launcher since the redesign (1e61793), so it cannot be pressed blind.
to_root() {
  osascript -e 'tell application "System Events" to key code 53 using command down'
  sleep 1
}

# Extensions are managed in Settings (#168): root search's "Manage
# Extensions" command opens the Settings window at its Extensions group,
# one page per installed extension, and the launcher has no screen for
# them any more. Settings' switches, menus and confirmation rows answer
# the pointer, not the keyboard, so the smoke finds them by their
# accessible names (Pane's own labels, which AccessKit gives the macOS
# accessibility API; System Events reads them with the Accessibility
# permission the key events already need) in the Settings window and
# presses them; an element that offers no press is clicked at its center.
# The names used: an extension's sidebar entry and its group-page item are
# its title, its page's switch is its title too (the one that is an
# AXCheckBox), its menu button "Actions for <title>" and the
# menu's items Reload, Retry, Why Paused, Clear Cache, Develop, Stop
# Developing and Uninstall; a confirmation's or a details screen's rows are
# their titles; a command's alias and hotkey cells "Alias for <command>: …"
# and "Hotkey for <command>: …", its fallback switch "Offer <command> as a
# fallback"; the page's status line is the operation's outcome.
#
# a11y <verb> <name> [prefix]: finds what the Settings window of the
# running Pane names <name> (its title, description or value; whose name
# starts with it, with "prefix"), waiting up to a minute for it to be
# drawn. "press" presses it, "toggle" the switch of that name, "shown" only
# waits for it, and "absent" asserts that, settled, nothing of that name is
# shown.
a11y() {
  local at
  at=$(osascript - "$pid" "$@" <<'APPLESCRIPT'
on run argv
	set thePid to (item 1 of argv) as integer
	set verb to item 2 of argv
	set wanted to item 3 of argv
	set prefix to ((count of argv) > 3)
	set patience to 60
	if verb is "absent" then
		delay 1
		set patience to 0
	end if
	set started to current date
	set found to missing value
	repeat
		set found to my find(thePid, verb, wanted, prefix)
		if found is not missing value then exit repeat
		if (current date) - started > patience then exit repeat
		delay 0.25
	end repeat
	if verb is "absent" then
		if found is missing value then return ""
		error "Settings still shows " & wanted
	end if
	if found is missing value then error "Settings shows nothing named " & wanted
	if verb is "press" or verb is "toggle" then
		tell application "System Events"
			try
				perform action "AXPress" of found
			on error
				set {x, y} to position of found
				set {w, h} to size of found
				return ((x + w div 2) as text) & " " & ((y + h div 2) as text)
			end try
		end tell
	end if
	return ""
end run

on find(thePid, verb, wanted, prefix)
	tell application "System Events"
		set proc to first process whose unix id is thePid
		if not (exists window "Settings" of proc) then return missing value
		set everything to entire contents of window "Settings" of proc
		repeat with candidate in everything
			set theRole to ""
			try
				set theRole to role of candidate
			end try
			-- A switch is a checkbox to the macOS accessibility API (AccessKit
			-- gives a switch and a toggle button AXCheckBox), never the sidebar
			-- entry (AXStaticText), the group page's item (AXLink) or the page's
			-- heading of the same name, whose role AccessKit gives as "Heading",
			-- not AXHeading (release run 37689872256 pressed the heading).
			if verb is not "toggle" or theRole is "AXCheckBox" then
				repeat with attributeName in {"AXTitle", "AXDescription", "AXValue"}
					try
						set theLabel to value of attribute (contents of attributeName) of candidate

						if theLabel is not missing value then
							set theLabel to theLabel as text
							considering case
								if prefix then
									if theLabel starts with wanted then return contents of candidate
								else
									if theLabel is wanted then return contents of candidate
								end if
							end considering
						end if
					end try
				end repeat
			end if
		end repeat
	end tell
	return missing value
end find
APPLESCRIPT
  ) || exit 1
  if [ -n "$at" ]; then click_points $at; fi
  case $1 in press|toggle) sleep 1;; esac
}
# Whether Settings is open.
settings_open() {
  [ "$(osascript -e "tell application \"System Events\" to exists window \"Settings\" of (first process whose unix id is $pid)")" = true ]
}
# Gives the Settings window the keyboard: Pane in front, Settings raised.
focus_settings() {
  osascript -e "tell application \"System Events\" to tell (first process whose unix id is $pid)" \
    -e 'set frontmost to true' -e 'perform action "AXRaise" of window "Settings"' -e 'end tell'
  sleep 1
}
# Closes Settings (its close shortcut, Command+W) and goes back to a blank
# root search in the launcher, its only window then.
close_settings() {
  if settings_open; then
    focus_settings
    command_key w; sleep 1
  fi
  focus_pane
  to_root
}
# Opens Settings at the Extensions group from root search: "manage" finds
# the Manage Extensions command, the only root row it matches, and Return
# runs it. Settings is closed first, so the launcher has the keyboard, and
# the launcher is brought back to a blank root search: it is wherever the
# user left it, a Settings operation leaving it there (#168).
manage_extensions() {
  close_settings
  command_key a
  type_text manage; sleep 1
  key 36; sleep 2
  a11y shown Extensions
}
# Opens the Settings page of the installed extension titled $1.
open_extension() {
  manage_extensions
  a11y press "$1"
  a11y shown "Actions for $1"
}
# Opens the Actions menu of the page of the extension titled $1 and
# chooses its item $2.
extension_action() {
  a11y press "Actions for $1"
  a11y press "$2"
}

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
  # A --install preview arrives once its check finishes; until then the
  # screen is root search. Wait for preview metadata below its heading, then
  # send the phase's first Enter to the preview:
  # run 36796103906's Linux frame 31 lost that race (the check outlasted
  # the wait and the Enter opened root search's own first row instead).
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
  kill -0 "$pid" || { echo "Pane exited during the smoke"; exit 1; }
  kill "$pid"
  wait "$pid" 2>/dev/null || true
  pid=
}

# Waits until file $1 contains text $2 ("present") or no longer does
# ("absent"), up to a tenth of a second times $4 (100 by default).
wait_for() {
  local tries=${4:-100}
  for _ in $(seq "$tries"); do
    if grep -q "$2" "$1" 2>/dev/null; then [ "$3" = present ] && return; else [ "$3" = absent ] && return; fi
    sleep 0.1
  done
  echo "$1: $2 is not $3"; exit 1
}

# The Rust, JavaScript and TypeScript samples are no commands of Pane's
# own (#162): installed with `pane --install` into a data folder of their
# own, they are root's first three rows, in install order (Rust sample,
# JavaScript sample, TypeScript sample), then Pane's install rows. The
# first phase and the root search phase run there.
export PANE_DATA_DIR=$out/samples-data
rm -rf "$PANE_DATA_DIR"
for sample in sample-rust sample-js sample-ts; do
  start_pane --install "target/guests/packages/$sample"
  key 36   # Install
  # 120 s: the install reads and checks the whole package, which a loaded
  # runner can take past the 10 s default.
  wait_for "$PANE_DATA_DIR/extensions/installed.json" "$sample" present 1200; sleep 1
  stop_pane
done
start_pane
capture 1-root.png
check 1-root.png hint   # the hint line: text renders

# Open each sample command (Rust, JavaScript, TypeScript) and run an item.
for index in 0 1 2; do
  for ((i = 0; i < index; i++)); do key 125; done   # not seq: BSD "seq 0" prints 1 0
  key 36; sleep 3
  capture "$((index + 2))-command-$index.png"
  key 125; key 36; sleep 2
  capture "$((index + 2))-result-$index.png"
  check "$((index + 2))-result-$index.png" success   # the guest's answer
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
check 7-form-error.png error   # the rejected field's message
type_text Ada
key 48; key 125; key 36; sleep 2
capture 8-form-result.png
check 8-form-result.png success   # the guest's answer
key 53; key 53; sleep 1
stop_pane

# Root search, over the samples' data folder still: typing narrows root to
# the matching commands and Enter opens the best match. "typescr" matches
# only TypeScript sample, whose "Wait briefly" answers exactly as in step
# 4. A query that matches nothing shows no results, and Enter then opens
# nothing.
start_pane
type_text typescr; sleep 1
capture 24-search.png
key 36; sleep 3
key 125; key 36; sleep 2
capture 25-search-result.png
check 25-search-result.png success   # the TypeScript guest's answer
python3 "$(dirname "$0")/check_screenshot.py" --same "$out/4-result-2.png" "$out/25-search-result.png"
key 53; sleep 1
type_text zzz; sleep 1
key 36; sleep 1
capture 26-no-results.png
python3 "$(dirname "$0")/check_screenshot.py" --distinct "$out"/{1-root,24-search,25-search-result,26-no-results}.png
stop_pane

# The phases below share $out/data, which starts with nothing installed:
# root lists no sample until one is installed.
export PANE_DATA_DIR=$out/data

# Install the assembled Rust sample package (the folder the picker would
# return), then run its command. Root lists the installed command, then
# the install and Manage Extensions rows.
start_pane --install target/guests/packages/sample-rust
capture 9-package.png
check 9-package.png details   # the package's identity and compatibility lines
key 36; sleep 2
capture 10-installed.png
check 10-installed.png success   # "Installed Rust sample"
key 36; sleep 3
key 36; sleep 2
capture 11-installed-result.png
check 11-installed-result.png success   # the installed guest's answer
stop_pane

# The installed command is still listed after a restart, root's first row.
start_pane
capture 12-restarted.png
check 12-restarted.png hint
[ -f "$out/data/extensions/installed.json" ] || { echo "no install record"; exit 1; }
focus_pane

# The Rust command's seventh item is declared for Windows only, its eighth
# for macOS and Linux only. Here the first is explained without running and
# the second runs.
key 36; sleep 3
for _ in 1 2 3 4 5 6; do key 125; done
key 36; sleep 2
capture 13-windows-only.png
check 13-windows-only.png warning   # the row's reason
check 13-windows-only.png error   # macOS: the reason as the error
key 125; key 36; sleep 2
capture 14-not-windows.png
check 14-not-windows.png success    # macOS: the guest's answer
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
check 15-no-compatible-package.png error   # "Not available on macOS: ..."
stop_pane

# Install the settings sample, save a choice with it, then disable it with
# the switch on its page in Settings (#168). Root lists Rust sample,
# Greeting, the install rows, then Manage Extensions and Settings… last.
start_pane --install target/guests/packages/sample-settings
key 36; sleep 2   # Install; Greeting is selected
key 36; sleep 3   # open Greeting
key 36; sleep 2   # "Use a formal greeting"
capture 16-setting-saved.png
check 16-setting-saved.png success   # "Saved the formal greeting"
key 53; sleep 1
open_extension "Settings sample"
a11y toggle "Settings sample"   # its switch: off
a11y shown "Disabled Settings sample"
capture 17-disabled.png   # Settings: the page's status says so
stop_pane
grep -q '"disabled": true' "$out/data/extensions/installed.json" || { echo "disabled state not recorded"; exit 1; }
grep -q '"greeting-style": "formal"' "$out/data/extensions/settings.json" || { echo "setting not saved"; exit 1; }

# After a restart Greeting is no longer in root search: root looks exactly as
# it did before the settings sample was installed. Enabling the package again
# brings it back with its setting: "Greet me" answers in the saved formal
# style, where without a saved style it reports an error.
start_pane
capture 18-restarted-disabled.png
check 18-restarted-disabled.png hint
python3 "$(dirname "$0")/check_screenshot.py" --same "$out/12-restarted.png" "$out/18-restarted-disabled.png"
open_extension "Settings sample"
a11y toggle "Settings sample"   # its switch: on
a11y shown "Enabled Settings sample"
capture 19-enabled.png
close_settings
key 125   # Greeting, after Rust sample
key 36; sleep 3
key 125; key 125; key 36; sleep 2   # "Greet me"
capture 20-greeted.png
check 20-greeted.png success   # "Good day to you"
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

# Operations: install the JavaScript operations sample, then the Rust one,
# whose command (Call from Rust, selected once installed) opens its form,
# takes the JavaScript package's identity (local: and the folder's resolved
# path) and a name, and calls that package's greet operation: "Hello, Rust,
# from JavaScript" comes from the other package's guest, started for the call.
start_pane --install target/guests/packages/sample-operations-js
key 36; sleep 2   # Install
capture 31-operations-target.png
check 31-operations-target.png success   # "Installed JavaScript operations sample"
stop_pane
start_pane --install target/guests/packages/sample-operations
key 36; sleep 2   # Install; Call from Rust is selected
key 36; sleep 3   # open Call from Rust
key 36; sleep 2   # "Greet through another extension": its form
type_text "local:$(cd target/guests/packages/sample-operations-js && pwd -P)"
key 48; type_text Rust
key 36; sleep 5   # Greet
capture 32-operation-answer.png
check 32-operation-answer.png success   # the JavaScript guest's answer
python3 "$(dirname "$0")/check_screenshot.py" --distinct "$out"/{31-operations-target,32-operation-answer}.png
stop_pane

# Reload a development package while Pane stays open. Its command starts as
# the Rust sample; a new build of it is the JavaScript sample. Root lists
# Rust sample, Greeting, Call from JavaScript, Call from Rust, Dev sample
# (the fifth row: Calculator only answers root search), the install row,
# then Manage Extensions and Settings… last. Reload is an item of the
# Actions menu on Dev's page in Settings.
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
check 33-dev-before.png success   # "Hello from the Rust guest"
key 53; sleep 1
cp target/guests/sample_js.wasm "$out/dev/command.wasm"
open_extension Dev
extension_action Dev Reload
a11y shown "Reloaded Dev"
capture 34-reloaded.png
close_settings
for ((i = 0; i < 4; i++)); do key 125; done   # Dev sample
key 36; sleep 3
key 36; sleep 2   # "Say hello"
capture 35-dev-after.png
check 35-dev-after.png success   # "Hello from the JavaScript guest"
python3 "$(dirname "$0")/check_screenshot.py" --distinct "$out/33-dev-before.png" "$out/35-dev-after.png"
key 53; sleep 1

# A build that fails the install checks (here its component is missing) is
# not reloaded: the working code keeps running, exactly as before.
rm "$out/dev/command.wasm"
open_extension Dev
extension_action Dev Reload
a11y shown "Dev was not reloaded" prefix
capture 36-not-reloaded.png
close_settings
for ((i = 0; i < 4; i++)); do key 125; done
key 36; sleep 3
key 36; sleep 2
capture 37-still-running.png
python3 "$(dirname "$0")/check_screenshot.py" --same "$out/35-dev-after.png" "$out/37-still-running.png"
key 53; sleep 1

# A build whose start fails is reported with Retry, an item of the same
# Actions menu; this one saves a setting and fails its first start only,
# so Retry starts it.
cp target/guests/failing_start.wasm "$out/dev/command.wasm"
open_extension Dev
extension_action Dev Reload
a11y shown "Reloaded Dev, but it failed to start" prefix
capture 38-start-failed.png
extension_action Dev Retry   # Retry starting Dev
a11y shown "Started Dev"
capture 39-retried.png
stop_pane
grep -q '"start-attempted": "yes"' "$out/data/extensions/settings.json" || { echo "the failed start's setting was not kept"; exit 1; }

# The settings sample keeps one value of each kind of data: its formal style
# (settings) and "Good day to you" (cache) are saved above; its fourth and
# fifth items save a note (content) and sign in (a local credential), and its
# sixth shows all four.
start_pane
key 125   # Greeting, after Rust sample
key 36; sleep 3
for ((i = 0; i < 3; i++)); do key 125; done
key 36; sleep 2   # "Save a note"
key 125; key 36; sleep 2   # "Sign in"
key 125; key 36; sleep 2   # "Show what Pane keeps"
capture 40-kept.png
check 40-kept.png success   # every value, the cached greeting included
key 53; sleep 1
stop_pane
grep -q '"note": "Water the plants"' "$out/data/extensions/content.json" || { echo "note not saved"; exit 1; }
grep -q '"token": "sample-token"' "$out/data/extensions/credentials.json" || { echo "credential not saved"; exit 1; }
grep -q '"last-greeting": "Good day to you"' "$out/data/extensions/cache.json" || { echo "greeting not cached"; exit 1; }

# Clear the settings sample's cache from its page in Settings: Clear Cache,
# an item of its Actions menu. Pane asks first, on the page, then deletes
# only the cached greeting, without running the extension.
start_pane
open_extension "Settings sample"
extension_action "Settings sample" "Clear Cache"
a11y shown "Clear cache"   # the confirmation's row: what is deleted and what is kept above it
capture 41-confirm-clear-cache.png
a11y press "Clear cache"
a11y shown "Cleared the cache of Settings sample"
capture 42-cache-cleared.png
close_settings
key 125   # Greeting, after Rust sample
key 36; sleep 3
for ((i = 0; i < 5; i++)); do key 125; done
key 36; sleep 2   # "Show what Pane keeps"
capture 43-kept-after-clear.png
check 43-kept-after-clear.png success   # "... Cached greeting: none"
python3 "$(dirname "$0")/check_screenshot.py" --distinct "$out/40-kept.png" "$out/43-kept-after-clear.png"
key 53; sleep 1
stop_pane
if grep -q 'Good day to you' "$out/data/extensions/cache.json"; then echo "cache not cleared"; exit 1; fi
grep -q '"greeting-style": "formal"' "$out/data/extensions/settings.json" || { echo "setting lost"; exit 1; }
grep -q '"note": "Water the plants"' "$out/data/extensions/content.json" || { echo "note lost"; exit 1; }
grep -q '"token": "sample-token"' "$out/data/extensions/credentials.json" || { echo "credential lost"; exit 1; }

# Applications: an installed application is found by name in root search
# and Enter opens it, through the JavaScript applications sample (the same
# host import and results the Applications default extension holds). The
# application is a bundle the smoke adds in ~/Applications of a HOME of its
# own (for Pane only), whose program writes a marker file, so nothing else
# is started; Pane still searches the system's applications too.
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
HOME=$apps/home "$pane" --install target/guests/packages/sample-applications-js 2>>"$out/stderr.log" &
pid=$!
sleep 8
focus_pane
key 36; sleep 2   # Install
type_text 'pane smoke'; sleep 3
capture 44-application.png
check 44-application.png selected 3000   # the selected application row
key 36; sleep 3
focus_pane
capture 45-opened.png
check 45-opened.png success   # "Opened Launch Pane Smoke App"
for _ in $(seq 50); do [ -f "$apps/launched" ] && break; sleep 0.2; done
[ -f "$apps/launched" ] || { echo "the application did not run"; exit 1; }
python3 "$(dirname "$0")/check_screenshot.py" --distinct "$out"/{44-application,45-opened}.png
stop_pane

# Uninstall the settings sample, keeping its saved data: Uninstall, the last
# item of its page's Actions menu. Pane asks first, showing its saved data,
# and the first choice keeps its settings and content while its copy and
# credential go. Installing the same folder again finds its formal style
# and note, signed out.
start_pane
open_extension "Settings sample"
extension_action "Settings sample" Uninstall
a11y shown "Uninstall and keep saved data"   # the confirmation: what is removed and the saved data
capture 49-confirm-uninstall.png
a11y press "Uninstall and keep saved data"
a11y shown "Uninstalled Settings sample" prefix   # "...; its settings and content are kept"
capture 50-uninstalled.png
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
check 51-reinstalled.png success   # "Style: formal · Note: Water the plants · Signed in: no ..."
python3 "$(dirname "$0")/check_screenshot.py" --distinct "$out/43-kept-after-clear.png" "$out/51-reinstalled.png"
key 53; sleep 1
stop_pane
if grep -q '"retained"' "$out/data/extensions/installed.json"; then echo "retained record not dropped"; exit 1; fi

# Global hotkeys: on the settings sample's page in Settings, its command,
# Greeting, is given Control+Option+G by pressing it in the command's
# hotkey recorder (its Commands section, #168). With Finder in front,
# pressing the hotkey brings Pane to the front with Greeting open, also
# after a restart; once the extension is disabled, pressing it does
# nothing. Carbon hot keys need no permission of Pane's own. A data folder
# of its own. (Screenshot 48 is the Linux smoke's opened quicklink.)
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
open_extension "Settings sample"
a11y press "Hotkey for Greeting:" prefix   # the recorder listens
a11y shown "Recording; Hotkey for Greeting" prefix
capture 52-hotkey-screen.png
focus_settings
press_hotkey   # Settings is in front: the recorder takes it
wait_for "$PANE_DATA_DIR/extensions/hotkeys.json" '"ctrl+alt+g"' present
a11y absent "Hotkey for Greeting: none"
capture 53-hotkey-assigned.png   # the recorder shows Control+Option+G
close_settings   # root search
unfocus_pane
capture 54-unfocused.png
press_hotkey
check_pane_in_front
capture 55-hotkey-opened.png
check 55-hotkey-opened.png selected 3000   # Greeting's first item, selected
python3 "$(dirname "$0")/check_screenshot.py" --distinct "$out"/{53-hotkey-assigned,55-hotkey-opened}.png
stop_pane
grep -q '"ctrl+alt+g"' "$PANE_DATA_DIR/extensions/hotkeys.json" || { echo "hotkey not recorded"; exit 1; }
start_pane
unfocus_pane
press_hotkey
check_pane_in_front
capture 56-hotkey-after-restart.png
check 56-hotkey-after-restart.png selected 3000   # Greeting's first item, selected
python3 "$(dirname "$0")/check_screenshot.py" --distinct "$out"/{53-hotkey-assigned,56-hotkey-after-restart}.png
key 53; sleep 1
open_extension "Settings sample"
a11y toggle "Settings sample"   # disable Settings sample
a11y shown "Disabled Settings sample"
close_settings
capture 57-disabled.png   # root search
unfocus_pane
press_hotkey
[ "$(frontmost)" != "$pid" ] || { echo "the released hotkey still brought Pane to the front"; exit 1; }
focus_pane
capture 58-disabled-pressed.png   # still root search: nothing opened
python3 "$(dirname "$0")/check_screenshot.py" --same "$out"/{57-disabled,58-disabled-pressed}.png
stop_pane

# Pausing a broken extension: the settings sample's last item, Crash, crashes
# on purpose; the third crash within five minutes pauses the package and
# returns to root search, where Greeting stays listed with why it does not
# run. The pause holds after a restart. On its page in Settings, Why
# Paused (an item of its Actions menu) shows the details, whose only row,
# Retry, starts it again. A data folder of its own.
export PANE_DATA_DIR=$out/pausing-data
rm -rf "$PANE_DATA_DIR"
start_pane --install target/guests/packages/sample-settings
key 36; sleep 2   # Install; Greeting is selected
key 36; sleep 2   # open Greeting
for ((i = 0; i < 7; i++)); do key 125; done   # Crash
for ((i = 0; i < 3; i++)); do key 36; sleep 2; done
type_text greet; sleep 1   # Greeting and its reason at the top on any window height
capture 59-paused.png
check 59-paused.png error   # "Settings sample crashed 3 times within 5 minutes and is paused ..."
check 59-paused.png warning   # Greeting: "Settings sample is paused after an error; ..."
stop_pane
grep -q '"paused"' "$PANE_DATA_DIR/extensions/installed.json" || { echo "pause not recorded"; exit 1; }
start_pane
type_text greet; sleep 1
capture 60-paused-after-restart.png
check 60-paused-after-restart.png warning   # Greeting is still paused
key 53; sleep 1   # Escape clears the query
open_extension "Settings sample"
extension_action "Settings sample" "Why Paused"
a11y shown "Retry Settings sample"   # "Why Settings sample is paused": the details, then Retry
capture 61-pause-details.png
a11y press "Retry Settings sample"
a11y shown "Started Settings sample"
capture 62-pause-retried.png
python3 "$(dirname "$0")/check_screenshot.py" --distinct "$out"/{61-pause-details,62-pause-retried}.png
stop_pane
if grep -q '"paused"' "$PANE_DATA_DIR/extensions/installed.json"; then echo "pause not cleared"; exit 1; fi

# Delete retained data: with a data folder of its own, the settings sample
# saves a note and is uninstalled keeping it (Uninstall, on its page in
# Settings); its retained data, a row of the Extensions group's own page
# once nothing is installed, is deleted after confirming, without the
# extension. Installing the same folder again finds nothing. Steps that
# change Pane's files wait for the change instead of a fixed time.
export PANE_DATA_DIR=$out/retained-data
rm -rf "$PANE_DATA_DIR"
registry=$PANE_DATA_DIR/extensions/installed.json
start_pane --install target/guests/packages/sample-settings
key 36   # Install; Greeting is selected
wait_for "$registry" sample-settings present; sleep 1
key 36; sleep 3   # open Greeting
for ((i = 0; i < 3; i++)); do key 125; done
key 36   # "Save a note"
wait_for "$PANE_DATA_DIR/extensions/content.json" '"note": "Water the plants"' present
key 53; sleep 1   # root search
open_extension "Settings sample"
extension_action "Settings sample" Uninstall
a11y press "Uninstall and keep saved data"
wait_for "$registry" '"retained"' present; sleep 1
manage_extensions
a11y press "Delete retained data of Settings sample"   # a row of the group's page
# The confirmation itself, found by its row, not by a color: the group's
# page shows hint lines too, so a color alone let the wrong screen pass
# once (#58).
a11y shown "Delete retained data"   # what is kept and what is not touched, above it
capture 63-confirm-delete-retained.png
a11y press "Delete retained data"
wait_for "$registry" '"retained"' absent; sleep 1
a11y shown "Deleted the retained data of Settings sample"
capture 64-retained-deleted.png
stop_pane

if grep -q 'Water the plants' "$PANE_DATA_DIR/extensions/content.json"; then echo "note not deleted"; exit 1; fi
start_pane --install target/guests/packages/sample-settings
key 36   # Install; Greeting is selected
wait_for "$registry" sample-settings present; sleep 1
key 36; sleep 3   # open Greeting
for ((i = 0; i < 5; i++)); do key 125; done
key 36; sleep 2   # "Show what Pane keeps"
capture 65-reinstalled-empty.png
check 65-reinstalled-empty.png success   # "Style: none · Note: none · Signed in: no ..."
python3 "$(dirname "$0")/check_screenshot.py" --distinct "$out/51-reinstalled.png" "$out/65-reinstalled-empty.png"
key 53; sleep 1
stop_pane

# Aliases and fallbacks: on the query sample's page in Settings, its
# command, Echo, is given the alias "ec" in its alias cell and made a
# fallback with its fallback switch (its Commands section, #168).
# In root search, "ec hello" lists the row that sends "hello" to Echo,
# selected, and Enter shows Echo's answer; text nothing matches lists "No
# results" with Echo below it, not selected, until Down selects it and Enter
# sends the text. After a restart with the extension disabled, "ec hello"
# lists nothing: the same screen as a Pane with nothing installed. Data
# folders of their own keep the rows in a known order.
export PANE_DATA_DIR=$out/aliases-data
rm -rf "$PANE_DATA_DIR"
start_pane --install target/guests/packages/sample-query
key 36; sleep 2   # Install; Echo is selected
open_extension "Query sample"
a11y press "Alias for Echo:" prefix   # its inline editor takes the keyboard
focus_settings
type_text ec
key 36
a11y shown "Alias for Echo: ec"
capture 66-alias-saved.png
a11y press "Offer Echo as a fallback"
a11y shown "Echo is now offered" prefix   # "... for any text typed in root search"
capture 67-fallback-on.png
python3 "$(dirname "$0")/check_screenshot.py" --distinct "$out"/{66-alias-saved,67-fallback-on}.png
close_settings   # root search
type_text 'ec hello'; sleep 1
capture 68-alias-row.png
check 68-alias-row.png selected 3000   # Echo, sending “hello”, selected
# Echo answers in a toast (#141), which leaves the footer 3 seconds after
# it appears: waited for, not slept past (run 37612772185 missed frame 72's).
key 36; sleep 0.5
capture_until 69-alias-answer.png success 15   # "Echo heard “hello”"
key 53; sleep 1   # clears the query
type_text zqx; sleep 1
capture 70-fallback-listed.png   # "No results for “zqx”", then Echo, not selected
key 125; sleep 1
capture 71-fallback-chosen.png
check 71-fallback-chosen.png selected 3000   # Echo, now selected
python3 "$(dirname "$0")/check_screenshot.py" --distinct "$out"/{70-fallback-listed,71-fallback-chosen}.png
key 36; sleep 0.5
capture_until 72-fallback-answer.png success 15   # Echo's toast: "Echo heard “zqx”"
python3 "$(dirname "$0")/check_screenshot.py" --distinct "$out"/{69-alias-answer,72-fallback-answer}.png
stop_pane
grep -q '"ec"' "$PANE_DATA_DIR/extensions/aliases.json" || { echo "alias not recorded"; exit 1; }
grep -q '#echo"' "$PANE_DATA_DIR/extensions/aliases.json" || { echo "fallback not recorded"; exit 1; }
start_pane
open_extension "Query sample"
a11y toggle "Query sample"   # disable Query sample
a11y shown "Disabled Query sample"
close_settings
type_text 'ec hello'; sleep 1
capture 73-alias-disabled.png   # "No results for “ec hello”"
stop_pane
grep -q '"disabled": true' "$PANE_DATA_DIR/extensions/installed.json" || { echo "not disabled"; exit 1; }
export PANE_DATA_DIR=$out/aliases-empty-data
rm -rf "$PANE_DATA_DIR"
start_pane
type_text 'ec hello'; sleep 1
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
capture 75-dependencies-preview.png
check 75-dependencies-preview.png details   # "Requires: JavaScript operations sample, installed with it ..."
key 36; sleep 3   # Install; Greet through dependencies is selected
capture 76-dependencies-installed.png
check 76-dependencies-installed.png success   # "Installed Dependencies sample with JavaScript operations sample, which it requires"
key 36; sleep 3   # open Greet through dependencies
key 36; sleep 0.5   # Greet through the required greeter
capture_until 77-dependency-answer.png success 20   # the JavaScript guest's answer, in a toast
python3 "$(dirname "$0")/check_screenshot.py" --distinct "$out"/{75-dependencies-preview,76-dependencies-installed,77-dependency-answer}.png
stop_pane
grep -q '"id": "greeter"' "$PANE_DATA_DIR/extensions/installed.json" || { echo "dependency not recorded"; exit 1; }
[ "$(grep -c '"dir"' "$PANE_DATA_DIR/extensions/installed.json")" = 2 ] || { echo "not exactly two packages installed"; exit 1; }

# Native helpers: the helper sample's command runs pane-echo, the file its
# package ships for this system (built by `cargo xtask guests`). Its first
# item shows the helper's answer, naming the system; its third races the
# helper against a one-second timer and cancels it. Its second has the
# helper wait ten seconds: disabling the package meanwhile (the switch on
# its page in Settings) ends the helper's process at once, and the note it
# saved before is kept. A data folder of its own; the helper runs from its
# managed copy there.
export PANE_DATA_DIR=$out/helper-data
rm -rf "$PANE_DATA_DIR"
# Pane's helper processes: pane-echo run from this data folder.
helpers_running() { pgrep -f "$PANE_DATA_DIR/extensions/packages/.*/pane-echo" >/dev/null; }
start_pane --install target/guests/packages/sample-helper
key 36; sleep 2   # Install; Helper sample is selected
key 36; sleep 2   # open Helper sample
key 36   # Echo through the helper
# The helper is a process Pane starts and waits for; a cold spawn on a
# loaded runner can outlast a fixed sleep (run 36850094416's macOS leg
# captured the answer's absence after two seconds), so the answer is
# waited for, whenever it lands.
capture_until 90-helper-echoed.png success 15   # 'Echoed "hello from Pane" on macOS arm64'
until_toast_gone success 20   # that answer's toast leaves, so the next one is the next answer's
key 125; key 125; key 36; sleep 0.5   # Echo within a second
capture_until 91-helper-cancelled.png success 15   # its toast: "Stopped the helper after one second"
python3 "$(dirname "$0")/check_screenshot.py" --distinct "$out"/{90-helper-echoed,91-helper-cancelled}.png
if helpers_running; then echo "a cancelled helper is still running"; exit 1; fi
key 126; key 36; sleep 1   # Up: Echo after waiting
helpers_running || { echo "the waiting helper is not running"; exit 1; }
capture 92-helper-waiting.png
# The helper waits ten seconds, so the switch must be reached well within
# them: Settings is opened the shortest way, without open_extension's
# pauses (release run 37698693722 reached it after the call had finished,
# the note "finished"). Escape leaves the command for a blank root search;
# the helper keeps running.
key 53; sleep 0.5
type_text manage; sleep 0.5
key 36   # Manage Extensions: Settings at the Extensions group
a11y press "Helper sample"   # its sidebar entry, once Settings shows it
a11y toggle "Helper sample"   # disable Helper sample
a11y shown "Disabled Helper sample"
capture 93-helper-disabled.png
if helpers_running; then echo "the helper outlived its disabled package"; exit 1; fi
grep -q '"helper-wait": "started"' "$PANE_DATA_DIR/extensions/settings.json" || { echo "saved note lost"; exit 1; }
if grep -q '"helper-wait": "finished"' "$PANE_DATA_DIR/extensions/settings.json"; then echo "the stopped call finished"; exit 1; fi
stop_pane
if helpers_running; then echo "a helper outlived Pane"; exit 1; fi

# Quitting Pane while a helper runs ends it: with "Echo after waiting"
# running (the helper beats in pane-echo.alive in its folder of the managed
# copy), asking Pane to quit the way the Dock's Quit does (a quit Apple
# event, sent by NSRunningApplication's terminate) ends the helper first. A
# data folder of its own again.
export PANE_DATA_DIR=$out/helper-quit-data
rm -rf "$PANE_DATA_DIR"
start_pane --install target/guests/packages/sample-helper
key 36; sleep 2   # Install; Helper sample is selected
key 36; sleep 2   # open Helper sample
key 125; key 36; sleep 2   # Echo after waiting
helpers_running || { echo "the waiting helper is not running"; exit 1; }
# The footer's "Running…" text is gone (#248): waited-for work is the late
# loading bar under the search field's rule, and a command's list has no
# rule, so there is nothing to check in the picture — the helper process
# check above is the state check, and the capture below is for the record.
capture 94-helper-before-quit.png
helpers_running || { echo "the helper ended before the quit check"; exit 1; }
alive=$(find "$PANE_DATA_DIR/extensions/packages" -name pane-echo.alive | head -1)
[ -n "$alive" ] || { echo "the waiting helper does not beat"; exit 1; }
python3 - "$pid" <<'PY'
import ctypes, ctypes.util, sys
objc = ctypes.cdll.LoadLibrary(ctypes.util.find_library("objc"))
ctypes.cdll.LoadLibrary(ctypes.util.find_library("AppKit"))
objc.objc_getClass.restype = ctypes.c_void_p
objc.objc_getClass.argtypes = [ctypes.c_char_p]
objc.sel_registerName.restype = ctypes.c_void_p
objc.sel_registerName.argtypes = [ctypes.c_char_p]
address = ctypes.cast(objc.objc_msgSend, ctypes.c_void_p).value
by_pid = ctypes.CFUNCTYPE(ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_int)(address)
terminate = ctypes.CFUNCTYPE(ctypes.c_bool, ctypes.c_void_p, ctypes.c_void_p)(address)
app = by_pid(objc.objc_getClass(b"NSRunningApplication"),
             objc.sel_registerName(b"runningApplicationWithProcessIdentifier:"), int(sys.argv[1]))
if not app:
    sys.exit("Pane is not a running application")
sys.exit(0 if terminate(app, objc.sel_registerName(b"terminate")) else "Pane did not take the quit request")
PY
for _ in $(seq 50); do kill -0 "$pid" 2>/dev/null || break; sleep 0.1; done
if kill -0 "$pid" 2>/dev/null; then echo "Pane did not quit when asked"; exit 1; fi
wait "$pid" 2>/dev/null || true
pid=
if helpers_running; then echo "a helper outlived Pane quitting"; exit 1; fi
beats=$(stat -f %z "$alive"); sleep 0.5
[ "$(stat -f %z "$alive")" = "$beats" ] || { echo "the helper still beats after Pane quit"; exit 1; }

# Development mode (#12, #13): a copy of each development sample
# (guests/hello-rust, hello-ts, hello-js) is built once, installed and
# developed from its page in Settings (Develop, an item of its Actions
# menu, #168). Saving
# an edit of its greeting builds it with the documented command and reloads
# it while Pane keeps running; a save that does not build keeps the working
# code and shows the error; two saves in a row (the second while the first
# builds) end with the newer greeting; after "Stop Developing", a save builds
# nothing. Each sample has a data folder of its own, so root lists its
# command first, then the install and Manage Extensions rows. The
# JavaScript and TypeScript samples need the JS toolchain
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
say_hello() {   # from root: open the developed command, the first row, and run its item
  key 36; sleep 3
  key 36; sleep 2
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
    python3 - "$copy/Cargo.toml" "$PWD/guests/pane-extension" <<'PY'
import sys
path, guest = sys.argv[1], sys.argv[2]
text = open(path, encoding="utf-8").read().replace('path = "../pane-extension"', "path = '%s'" % guest)
open(path, "w", encoding="utf-8").write(text)
PY
    (cd "$copy" && cargo build --release --target wasm32-wasip2 --quiet)
  else
    python3 tools/componentize-js/pane_js.py build "$copy" "$copy/$component" >/dev/null
  fi
  local built=$copy/$component before=$out/develop-$sample-before.wasm
  start_pane --install "$copy"
  key 36; sleep 2   # Install
  open_extension "$title"
  extension_action "$title" Develop
  a11y shown "Developing $title:" prefix   # "Developing <title>: each save in ..."
  capture "$n-$sample-develop-started.png"
  close_settings
  say_hello
  capture "$((n + 1))-$sample-greeting-before.png"
  check "$((n + 1))-$sample-greeting-before.png" success   # "Hello from ..."
  key 53; sleep 1

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
  key 53; sleep 1

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
  key 53; sleep 1

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
  key 53; sleep 1

  # Stopped: a save builds nothing.
  open_extension "$title"
  extension_action "$title" "Stop Developing"
  a11y shown "Stopped developing $title"
  capture "$((n + 8))-$sample-stopped.png"
  cp "$built" "$before"
  set_greeting "$copy/$source" "$(printf "$greeting" "Hello unseen")"
  sleep 8
  cmp -s "$built" "$before" || { echo "$title was built after development stopped"; exit 1; }
  stop_pane
}
develop_sample hello-rust "Hello Rust" target/wasm32-wasip2/release/hello_rust.wasm src/lib.rs 110 \
  'const GREETING: &str = "%s from Rust";' 'const GREETING: &str = 42;'
js_toolchain=${PANE_JS_TOOLCHAIN_DIR:-$HOME/Library/Caches/pane/componentize-js}
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
# operations sample's switch on its page in Settings asks first, listing
# the Dependencies sample, which requires it, with Disable all and Cancel;
# Cancel changes nothing, Disable all disables both, and the switch again
# enables the JavaScript operations sample alone: the Dependencies sample
# stays disabled, on record too.
export PANE_DATA_DIR=$out/disable-dependents-data
rm -rf "$PANE_DATA_DIR"
start_pane --install target/guests/packages/sample-dependencies
key 36; sleep 3   # Install
operations="JavaScript operations sample"
open_extension "$operations"
a11y toggle "$operations"   # disable it: asks first
a11y shown "Disable all 2"   # "Dependencies sample, which requires JavaScript operations sample · …", above it
capture 140-disable-dependents-asked.png
a11y press Cancel
a11y shown "Actions for $operations"   # its page again: both still enabled
capture 141-disable-dependents-cancelled.png
a11y toggle "$operations"   # asks again
a11y press "Disable all 2"
a11y shown "Disabled JavaScript operations sample and Dependencies sample" prefix   # "..., which requires it"
capture 142-disable-dependents-disabled.png
a11y toggle "$operations"   # enable JavaScript operations sample
a11y shown "Enabled JavaScript operations sample"   # Dependencies sample stays disabled
capture 143-disable-dependents-enabled-alone.png
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
# explains that nothing runs, the Extensions group's page in Settings
# shows why (its runtime rows), a disable still works, and Restart runs
# extensions again, Count only when asked. A data folder of its own.
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
key 36; sleep 2   # Install
stop_pane
export PANE_TEST_RUNTIME_FAULTS=$fault
start_pane --install target/guests/packages/sample-settings
key 36; sleep 2   # Install; Greeting is selected
key 36; sleep 2   # open Greeting
for ((i = 0; i < 8; i++)); do key 125; done   # Count
key 36; sleep 2
capture 200-runtime-counted.png
check 200-runtime-counted.png success   # "Counted 1"
[ "$(count)" = 1 ] || { echo "Count did not count once"; exit 1; }
key 53; sleep 1
type_text helper; sleep 1
key 36; sleep 2   # open Helper sample
key 125; key 36; sleep 2   # Echo after waiting
helpers_running || { echo "the waiting helper is not running"; exit 1; }
capture 201-runtime-helper-waiting.png
# The footer's "Running…" text is gone (#248): waited-for work is the late
# loading bar under the search field's rule, and a command's list has no
# rule, so there is nothing to check in the picture — the helper process
# check above is the state check.
alive=$(find "$PANE_DATA_DIR/extensions/packages" -name pane-echo.alive | head -1)
[ -n "$alive" ] || { echo "the waiting helper does not beat"; exit 1; }
inject crash
capture 202-runtime-crashed.png
check 202-runtime-crashed.png error   # "Pane's extension runtime stopped unexpectedly and was started again; ..."
if helpers_running; then echo "the helper outlived the crashed runtime"; exit 1; fi
beats=$(stat -f %z "$alive"); sleep 0.5
[ "$(stat -f %z "$alive")" = "$beats" ] || { echo "the helper still beats after the crash"; exit 1; }
grep -q '"helper-wait": "started"' "$PANE_DATA_DIR/extensions/settings.json" || { echo "saved note lost"; exit 1; }
if grep -q '"helper-wait": "finished"' "$PANE_DATA_DIR/extensions/settings.json"; then echo "the stopped call finished"; exit 1; fi
key 53; sleep 1
type_text greet; sleep 1
key 36; sleep 2   # open Greeting in the restarted runtime
for ((i = 0; i < 8; i++)); do key 125; done   # Count
inject crash-before-answer:count
key 36; sleep 3   # counts, then the runtime crashes before answering
capture 203-runtime-stopped.png
check 203-runtime-stopped.png error   # the runtime stopped; its answer is lost
[ "$(count)" = 2 ] || { echo "Count did not run once before the crash"; exit 1; }
key 53; sleep 1
type_text greet; sleep 1
key 36; sleep 2   # Greeting: nothing runs
capture 204-runtime-refused.png
check 204-runtime-refused.png error   # "Extension runtime unavailable: it stopped after crashing ..."
key 53; sleep 1   # clears the query
manage_extensions
a11y shown "Restart the extension runtime"
a11y shown "Why the extension runtime stopped"
capture 205-runtime-manage.png   # the group's page: Restart the extension runtime, Why the extension runtime stopped
a11y press "Why the extension runtime stopped"
a11y shown Back   # the details, with the way back
capture 206-runtime-details.png
a11y press Back
open_extension "Helper sample"
a11y toggle "Helper sample"   # disable Helper sample
a11y shown "Disabled Helper sample"
capture 207-runtime-disabled.png
manage_extensions
a11y press "Restart the extension runtime"
a11y shown "Restarted the extension runtime"
capture 208-runtime-restarted.png
[ "$(count)" = 2 ] || { echo "Count was run again without asking"; exit 1; }
close_settings
type_text greet; sleep 1
key 36; sleep 2   # open Greeting
for ((i = 0; i < 8; i++)); do key 125; done   # Count
key 36; sleep 2
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
# window answers keys, Escape returns to root search and Manage Extensions
# opens Settings. The compute limit then becomes 2 seconds, which the running call
# has passed, so Pane stops it at once; each later call is stopped after 2
# seconds of its computing, says why, and the third time pauses the
# package (a failure of its own); Retry starts it again. Then the runtime
# thread itself is made to hang through the fault file: the status line
# says it is not responding yet, then Pane gives up on it, names and
# pauses no extension, and the Extensions group's page in Settings says
# the runtime stopped responding; a fresh thread runs the next call. A
# data folder of its own.
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
key 36; sleep 2   # Install; Greeting is selected
key 36; sleep 2   # open Greeting
for ((i = 0; i < 9; i++)); do key 125; done   # Stop responding
key 36; sleep 1   # it computes
[ "$(saved busy)" = started ] || { echo "Stop responding did not start"; exit 1; }
key 53; sleep 1   # root search answers meanwhile
manage_extensions
a11y shown "Settings sample"
capture 240-unresponsive-window-answers.png   # Settings' Extensions page, while the guest computes
[ "$(stopped_calls)" = 0 ] || { echo "Stop responding was stopped before frame 240"; exit 1; }
inject limits:2,4,15   # it has computed longer: Pane stops it at its next tick
for _ in $(seq 300); do [ "$(stopped_calls)" -ge 1 ] && break; sleep 0.1; done
[ "$(stopped_calls)" -ge 1 ] || { echo "Stop responding was not stopped at the shorter limit"; exit 1; }
close_settings   # its answer is not shown here
type_text greet; sleep 1
key 36; sleep 2   # open Greeting
for ((i = 0; i < 9; i++)); do key 125; done   # Stop responding
key 36; sleep 8
capture 241-unresponsive-stopped.png
check 241-unresponsive-stopped.png error   # "The extension stopped responding: it computed for 2 seconds ..."
key 36; sleep 8   # the third time
type_text greet; sleep 1   # Greeting and its reason at the top
capture 242-unresponsive-paused.png
check 242-unresponsive-paused.png error   # "Settings sample stopped responding 3 times within 5 minutes and is paused ..."
check 242-unresponsive-paused.png warning   # Greeting: "Settings sample is paused after an error; ..."
[ "$(saved busy)" = started ] || { echo "Stop responding finished or was lost"; exit 1; }
key 53; sleep 1   # clears the query
open_extension "Settings sample"
extension_action "Settings sample" "Why Paused"
a11y shown "Retry Settings sample"   # "Why Settings sample is paused": the details, then Retry
capture 243-unresponsive-pause-details.png
a11y press "Retry Settings sample"
a11y shown "Started Settings sample"
capture 244-unresponsive-retried.png
close_settings
inject hang
type_text greet; sleep 1
key 36; sleep 4   # open Greeting: the stuck runtime is not responding yet
capture 245-unresponsive-not-yet.png
check 245-unresponsive-not-yet.png progress   # "Pane's extension runtime is not responding yet. ..."
sleep 14   # Pane gives up on it
capture 246-unresponsive-runtime.png
check 246-unresponsive-runtime.png error   # the runtime stopped responding and was started again
key 53; sleep 1   # clears the query
manage_extensions
a11y press "Why the extension runtime stopped"   # a row of the group's page
a11y shown Back   # the details, with the way back
capture 247-unresponsive-runtime-details.png
inject release
close_settings
type_text greet; sleep 1
key 36; sleep 2   # open Greeting on a fresh runtime thread
key 36; sleep 2   # Use a formal greeting
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
# operations sample's Uninstall (an item of its page's Actions menu in
# Settings) asks first, listing the Dependencies sample, which requires it, and
# each one's saved data, with Uninstall all keeping or deleting saved data
# and Cancel; Cancel changes nothing, Uninstall all 2 (keeping) uninstalls
# both, and installing the JavaScript operations sample again installs it
# alone: the Dependencies sample is not restored, on record too.
export PANE_DATA_DIR=$out/uninstall-dependents-data
rm -rf "$PANE_DATA_DIR"
start_pane --install target/guests/packages/sample-dependencies
key 36; sleep 3   # Install
operations="JavaScript operations sample"
open_extension "$operations"
extension_action "$operations" Uninstall   # asks first
a11y shown "Uninstall all 2 and keep saved data"   # "Dependencies sample, which requires JavaScript operations sample · …", above it
capture 180-uninstall-dependents-asked.png
a11y press Cancel
a11y shown "Actions for $operations"   # its page again: both still installed
capture 181-uninstall-dependents-cancelled.png
extension_action "$operations" Uninstall   # asks again
a11y press "Uninstall all 2 and keep saved data"
a11y shown "Uninstalled JavaScript operations sample and Dependencies sample" prefix   # "..., which requires it; …"
capture 182-uninstall-dependents-uninstalled.png
stop_pane
[ "$(grep -c '"dir"' "$PANE_DATA_DIR/extensions/installed.json")" = 0 ] || { echo "not both uninstalled"; exit 1; }
start_pane --install target/guests/packages/sample-operations-js
key 36; sleep 3   # Install the dependency alone
manage_extensions
a11y shown "$operations"
a11y absent "Dependencies sample"
capture 183-uninstall-dependents-reinstalled-alone.png   # only the JavaScript operations sample is listed
python3 "$(dirname "$0")/check_screenshot.py" --distinct "$out"/{180-uninstall-dependents-asked,181-uninstall-dependents-cancelled,182-uninstall-dependents-uninstalled,183-uninstall-dependents-reinstalled-alone}.png
stop_pane
[ "$(grep -c '"dir"' "$PANE_DATA_DIR/extensions/installed.json")" = 1 ] || { echo "not the dependency alone reinstalled"; exit 1; }

# npm packages (#45), from a local registry on 127.0.0.1 serving the npm
# sample `cargo xtask guests` packed (scripts/npm_registry.py; nothing reaches
# the network), with a data folder of its own. Installing the local
# Dependencies from npm sample shows the npm package it requires and
# installs both; its command calls the npm package's greet operation. Then
# "Install extension from npm…" (searched for by title, as
# manage_extensions does) opens Settings at the Extensions group's npm
# field (#168), which takes the keyboard; Show Package previews the
# installed package there, offering Update, and its command runs: "Hello
# from the npm package".
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
capture 260-npm-dependency-preview.png
check 260-npm-dependency-preview.png details   # "Requires: Greeter from npm, installed with it from npm:@pane-samples/greeter"
key 36; sleep 3   # Install; Greet through an npm dependency is selected
capture 261-npm-dependency-installed.png
check 261-npm-dependency-installed.png success   # "Installed Dependencies from npm sample with Greeter from npm, which it requires"
key 36; sleep 3   # open it
key 36; sleep 0.5   # "Greet through the required greeter"
capture_until 262-npm-dependency-called.png success 20   # its toast: "Hello, Pane, from the npm package"
key 53; sleep 1
command_key a; type_text 'install npm'; sleep 1
key 36; sleep 2   # Install extension from npm…: Settings, its npm field
a11y shown "npm package:" prefix   # the field, named by its label
focus_settings
capture 263-npm-form.png
type_text @pane-samples/greeter
# Typed keys can still be on their way when a button is pressed through the
# accessibility API (release run 37725624283's Windows leg asked npm for
# "@pane-samples/gre"): the field's value, all of it, is waited for first.
a11y shown @pane-samples/greeter   # the field's value
a11y press "Show Package"
a11y shown Update   # the preview's row, under "Source: npm package @pane-samples/greeter", "npm version: 0.1.0, the latest", … (lines of text with no accessible name: the screenshot shows them)
capture 264-npm-preview.png
a11y press Update
a11y shown "Updated Greeter from npm to 0.1.0"
capture 265-npm-updated.png
close_settings
type_text 'greeter from npm'; sleep 1
key 36; sleep 3   # open Greeter from npm
key 36; sleep 2   # "Say hello"
capture 266-npm-command-ran.png
check 266-npm-command-ran.png success   # "Hello from the npm package"

# #49: the update Pane applies by itself. A 0.2.0 of the sample is
# published into the registry this phase serves (it reads its folder on
# request, so publishing is dropping the tarball in), and Pane is stopped
# and started again: the first check, a minute after the start (#256),
# finds the newer version and replaces the installed copy — unpinned, and
# nothing of it running, so the safe boundary is at once — quietly, its
# say the record it writes and the new copy's code. The new copy's
# command runs as the old one did.
python3 "$(dirname "$0")/npm_publish.py" target/guests/npm/pane-samples-greeter-0.1.0.tgz 0.2.0
stop_pane
start_pane
# The check a minute after the start, then the download and the apply:
# wait for the record to say the update landed, whenever that is, so a
# slow runner is waited for rather than slept past; the capture shows the
# quiet window after it landed.
wait_for "$PANE_DATA_DIR/extensions/installed.json" '"npmVersion": "0.2.0"' present 1800
capture 267-npm-updated-automatically.png
key 36; sleep 3   # open Greeter from npm, the new copy
key 36; sleep 2   # "Say hello"
capture 268-npm-new-copy-ran.png
check 268-npm-new-copy-ran.png success   # "Hello from the npm package"
python3 "$(dirname "$0")/check_screenshot.py" --distinct "$out"/{260-npm-dependency-preview,261-npm-dependency-installed,262-npm-dependency-called,263-npm-form,264-npm-preview,265-npm-updated,266-npm-command-ran,267-npm-updated-automatically}.png
# 0.2.0 is 0.1.0's code at a newer version (npm_publish.py), so the new
# copy's command answers exactly as the old one did.
python3 "$(dirname "$0")/check_screenshot.py" --same "$out/266-npm-command-ran.png" "$out/268-npm-new-copy-ran.png"
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
# manage_extensions does) opens Settings at the Extensions group's Git
# field (#168), which takes the keyboard; naming the tag and Show Package
# previews the release revision, pinned, there, Install installs it, and
# its command runs: "Hello from the Git repository".
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
# The fetch runs after the window shows: capture until its explanation does.
capture_until 300-git-source-only.png error 60   # "The default branch, main (commit …) of the Git repository … holds only the source of …"
key 53; sleep 1
command_key a; type_text 'install git'; sleep 1
key 36; sleep 2   # Install extension from Git…: Settings, its Git field
a11y shown "Git repository:" prefix   # the field, named by its label
focus_settings
capture 301-git-form.png
type_text "$repository@v0.1.0"
a11y shown "$repository@v0.1.0"   # the field's value, all of it (see the npm field)
a11y press "Show Package"
a11y shown Install   # the preview's row, under "Source: Git repository 127.0.0.1:<port>/greeter", "Revision: tag v0.1.0, which you named: …" (lines of text with no accessible name: the screenshot shows them)
capture 302-git-preview.png
a11y press Install
a11y shown "Installed Greeter from Git"
capture 303-git-installed.png
close_settings
type_text 'greeter from git'; sleep 1
key 36; sleep 3   # open Greeter from Git

key 36; sleep 2   # "Say hello"
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
# stopped, and the check a minute after the restart (#256) replaces the
# installed copy by itself, quietly, the new code running. Nothing
# reaches the network.
export PANE_DATA_DIR=$out/git-update-data
rm -rf "$PANE_DATA_DIR"
python3 "$(dirname "$0")/repository_server.py" make-sample target/guests/git/greeter "$out/git-repositories/greeter-tracked"
tracked=http://127.0.0.1:$(cat "$out/repository-server.port")/greeter-tracked.git
start_pane --install "git:$tracked@release"
capture_until 305-git-tracked-preview.png details 60   # "Revision: branch release, tracked: an update fetches that branch again"
key 36; sleep 3   # Install; Greeter from Git is selected
capture 306-git-tracked-installed.png
check 306-git-tracked-installed.png success   # "Installed Greeter from Git"
python3 "$(dirname "$0")/repository_server.py" move-sample "$out/git-repositories/greeter-tracked" 0.2.0
moved=$(python3 "$(dirname "$0")/repository_server.py" commit "$out/git-repositories/greeter-tracked" release)
stop_pane
start_pane
# The check a minute after the start, then the fetch and the apply: wait
# for the record to name the moved branch's commit, whenever that is (the
# update is quiet, its say the record and the new copy's code); the
# capture shows the quiet window after it landed.
wait_for "$PANE_DATA_DIR/extensions/installed.json" "$moved" present 1800
capture 307-git-updated-automatically.png
key 36; sleep 3   # open Greeter from Git, the new copy
key 36; sleep 2   # "Say hello"
capture 308-git-new-copy-ran.png
check 308-git-new-copy-ran.png success   # "Hello from the Git repository"
python3 "$(dirname "$0")/check_screenshot.py" --distinct "$out"/{305-git-tracked-preview,306-git-tracked-installed,307-git-updated-automatically,308-git-new-copy-ran}.png
stop_pane
kill "$repository_server_pid"; wait "$repository_server_pid" 2>/dev/null || true; repository_server_pid=
# The record was waited for above, before the new copy ran; the checks
# name it in full.
python3 "$(dirname "$0")/check_git_record.py" --ref refs/heads/release --unpinned "$PANE_DATA_DIR/extensions/installed.json" "$moved" || { echo "the tracked Git package was not recorded at its moved branch"; exit 1; }
[ -z "$(ls -A "$PANE_DATA_DIR/extensions/downloads" 2>/dev/null)" ] || { echo "a Git download was left"; exit 1; }

# File search (#29, #175): the Rust files sample (the same contract the
# Files default extension holds; its data folder is this phase's own),
# answers from Pane's file index, which covers the home
# folder; the smoke names a fixture folder for it to cover instead in
# PANE_TEST_FILE_INDEX_HOME (a debug build's hook, which also keeps the index
# in the data folder). Installing it starts the index, and showing the
# window lets its first walk start. The fixture folder's
# path has spaces, and a file in it has non-ASCII letters too; typing "plan"
# lists that file under "Files", selected, and Enter hands it to Pane's handler for files,
# which PANE_TEST_OPEN_FILE_LOG (a debug build's hook) makes record the path
# instead of asking /usr/bin/open, since any application that opened it would
# be the user's own. Each file action closes the window after it acts
# (#150), so Pane is started again (the index is caught up from FSEvents'
# history) for the next file.
# An executable script in the folder is found, and Enter reveals it in
# Finder (ADR 0037: file search's Enter never runs a program; only its
# explicit Run does): it neither runs nor reaches the handler for files.
# The fixture is in the system's temporary folder, so no screenshot shows a
# home path.
export PANE_DATA_DIR=$out/files-data
rm -rf "$PANE_DATA_DIR"
files_fixture=$(mktemp -d "${TMPDIR:-/tmp}/pane-smoke-files.XXXXXX")
files_folder="$files_fixture/Pane smoke files"
mkdir -p "$files_folder/notes"
printf 'plan\n' >"$files_folder/Résumé plan ü.txt"
printf 'todo\n' >"$files_folder/notes/todo.txt"
printf '#!/bin/sh\ntouch "%s/runner-ran"\n' "$files_fixture" >"$files_folder/notes/runner.sh"
chmod +x "$files_folder/notes/runner.sh"
rm -f "$out/opened-file.txt"
export PANE_TEST_FILE_INDEX_HOME=$files_folder PANE_TEST_OPEN_FILE_LOG=$out/opened-file.txt
start_pane --install target/guests/packages/sample-files
key 36; sleep 3   # Install; the index walks the fixture
capture 220-files-installed.png
check 220-files-installed.png success   # "Installed Rust files sample"
# The install lands on a blank root search, where Escape hides the
# launcher (release run 37698693722's Windows frame 221 was the desktop):
# the return to root key keeps it.
to_root
type_text 'plan'; sleep 3
capture 221-files-found.png
check 221-files-found.png selected 3000   # the selected file row, "Résumé plan ü.txt"
key 36; sleep 3   # Open: the window closes, and a HUD says "Opened Résumé plan ü.txt"
capture 222-files-opened.png   # evidence only: the window is hidden
[ -f "$out/opened-file.txt" ] || { echo "the handler for files was not asked to open anything"; exit 1; }
[ "$(cd "$(dirname "$(head -1 "$out/opened-file.txt")")" && pwd -P)/$(basename "$(head -1 "$out/opened-file.txt")")" = "$(cd "$files_folder" && pwd -P)/Résumé plan ü.txt" ] || { echo "the handler for files was not asked to open the found file"; exit 1; }
rm -f "$out/opened-file.txt"
stop_pane
start_pane
type_text 'runner'; sleep 3
capture 223-files-program-found.png
check 223-files-program-found.png selected 3000   # the script's row
key 36; sleep 3   # Show in Finder
capture 224-files-program-revealed.png   # evidence only: Finder, Pane hidden
[ ! -e "$out/opened-file.txt" ] || { echo "the script was handed to the handler"; exit 1; }
[ ! -e "$files_fixture/runner-ran" ] || { echo "the script ran"; exit 1; }
osascript -e 'tell application "Finder" to close every window' || true
python3 "$(dirname "$0")/check_screenshot.py" --distinct "$out"/{220-files-installed,221-files-found,223-files-program-found}.png
stop_pane
unset PANE_TEST_FILE_INDEX_HOME PANE_TEST_OPEN_FILE_LOG
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
trap '[ -z "$service_pid" ] || kill "$service_pid" 2>/dev/null || true; [ -n "$pid" ] && kill "$pid" 2>/dev/null || true' EXIT
rm -f "$service_log"
start_service 1
start_pane --install target/guests/packages/sample-search
key 36; sleep 2   # Install; Package search is selected
capture 160-search-installed.png
check 160-search-installed.png success   # "Installed Search sample"
type_text aurora; sleep 2
capture 161-root-typed.png   # root search: "No results for “aurora”"
if grep -q '^GET' "$service_log"; then echo "root search reached the service"; exit 1; fi
key 53; sleep 1   # clears the query
type_text 'package search'; sleep 1
key 36; sleep 3   # open Package search
capture 162-command-opened.png   # its own list, its search field empty
check 162-command-opened.png selected 3000   # its first row, selected
key 125; key 36; sleep 2   # Service address: its form
type_text "http://127.0.0.1:$service_port"
key 36; sleep 2   # Save
capture 163-service-set.png   # "Searching http://127.0.0.1:<port> from now on"
check 163-service-set.png success
key 53; sleep 1   # back to the command, its search field empty
type_text aurora; sleep 3
capture 164-search-results.png   # aurora-charts, selected, and aurora-cli
check 164-search-results.png selected 3000
grep -q '^GET /search?q=aurora$' "$service_log" || { echo "the command's search did not reach the service"; exit 1; }
key 125; key 36; sleep 1   # aurora-cli's details
capture_until 165-details.png success 15   # its toast: "aurora-cli 0.9.3 (Apache-2.0): Command-line parsing with subcommands"
command_key a; type_text slow; sleep 2   # held by the service
command_key a; type_text ember; sleep 3
capture 166-newer-search.png   # ember-tz, not what "slow" would list
check 166-newer-search.png selected 3000
grep -q '^ABANDONED /search?q=slow$' "$service_log" || { echo "the replaced search was not stopped"; exit 1; }
# A search error stays in the status line until the query changes.
command_key a; type_text down; sleep 1
capture_until 167-service-error.png error 15   # "... The service answered 503: the registry is down for maintenance"
stop_service
key 53; sleep 1   # clear the failed search and return to the command's list
capture 167-service-error-cleared.png
python3 "$(dirname "$0")/check_screenshot.py" --absent "$out/167-service-error-cleared.png" error
command_key a; type_text basalt; sleep 1
capture_until 168-offline.png error 15   # "... Could not reach the service at http://127.0.0.1:<port>: connection refused"
start_service 2
command_key a; type_text cobalt; sleep 3
capture 169-back-online.png   # cobalt-http, selected: not paused
check 169-back-online.png selected 3000
python3 "$(dirname "$0")/check_screenshot.py" --distinct "$out"/{161-root-typed,162-command-opened,163-service-set,164-search-results,165-details,166-newer-search,167-service-error,168-offline,169-back-online}.png
stop_pane
stop_service

# Installing Pane and setting its default extensions up (#52, #278): the
# package `cargo xtask package-macos --dev` builds is installed on a clean
# machine — a fresh home folder, a PATH that holds nothing at all, so no
# Rust, Node, npm, Git or compiler can be reached — and Pane, started from
# the Pane.app bundle the install script made in that home's ~/Applications,
# fetches its default extensions (the five of #60; no sample is one,
# #162) from the commits this release pins: their repositories, cloned
# at those commits from their real addresses on GitHub (the smoke's own
# setup on the runner) and served on
# 127.0.0.1 over Git's smart HTTP protocol
# (scripts/repository_server.py; the Pane under test reaches no network
# address and no real Git host), named by the pins file the development
# build reads through
# PANE_DEFAULTS. The clones hold the release revisions' built components,
# so the first setup installs exactly what a release installs. The
# calculator answers "6*7" with 42, with no developer
# tool anywhere. The package is the development profile, because only a
# development build takes its pins from PANE_DEFAULTS; a release build
# uses the committed pins, which no controlled source may replace. The
# binaries are built on this machine, so they carry no Gatekeeper
# quarantine mark (nothing is signed). (The program files are removed
# again at the end of the phase: the uploaded evidence is the
# screenshots and records, not the program.)
artifact_server_pid=
defaults_server_pid=
# The whole smoke's cleanup in one place: this supersedes the traps the
# earlier phases set (each had replaced the one before), keeping every
# server's arm so nothing is lost whatever order the phases run in.
trap '[ -n "$pid" ] && kill "$pid" 2>/dev/null; [ -n "$npm_registry_pid" ] && kill "$npm_registry_pid" 2>/dev/null; [ -n "$repository_server_pid" ] && kill "$repository_server_pid" 2>/dev/null; [ -n "$defaults_server_pid" ] && kill "$defaults_server_pid" 2>/dev/null; [ -n "$service_pid" ] && kill "$service_pid" 2>/dev/null; [ -n "$artifact_server_pid" ] && kill "$artifact_server_pid" 2>/dev/null || true' EXIT
cargo xtask package-macos --dev >/dev/null
package=$(ls target/dist/pane-*-macos-*-dev.zip | head -1)
[ -n "$package" ] || { echo "the package was not built"; exit 1; }
# The default extensions' repositories, cloned at the commits the
# committed pins name and served from this computer for the rest of the
# smoke: the phases that check first setup point PANE_DEFAULTS at the
# pins naming them.
default_repositories=$PWD/$out/default-repositories
default_pins=$PWD/$out/default-pins.json
rm -rf "$default_repositories"
mkdir -p "$default_repositories"
rm -f "$out/default-repository-server.port"
python3 "$(dirname "$0")/repository_server.py" serve "$default_repositories" \
  "$out/default-repository-server.port" 2>>"$out/default-repository-server.log" &
defaults_server_pid=$!
for _ in $(seq 600); do [ -s "$out/default-repository-server.port" ] && break; kill -0 "$defaults_server_pid" 2>/dev/null || break; sleep 0.1; done
[ -s "$out/default-repository-server.port" ] \
  || { echo "the default extensions' repository server did not start (see $out/default-repository-server.log)"; exit 1; }
python3 "$(dirname "$0")/repository_server.py" clone-defaults crates/pane/defaults.json \
  "$default_repositories" "$default_pins" \
  "http://127.0.0.1:$(cat "$out/default-repository-server.port")/"
home=$out/clean-home
unpack=$out/package-unpacked
rm -rf "$home" "$unpack"
mkdir -p "$home" "$unpack"
rm -f "$out/artifact-server.port"
python3 "$(dirname "$0")/artifact_server.py" target/dist/artifacts "$out/artifact-server.port" 2>>"$out/artifact-server.log" &
artifact_server_pid=$!
for _ in $(seq 600); do [ -s "$out/artifact-server.port" ] && break; kill -0 "$artifact_server_pid" 2>/dev/null || break; sleep 0.1; done
[ -s "$out/artifact-server.port" ] || { echo "the local artifact source did not start (see $out/artifact-server.log)"; exit 1; }
unzip -q "$package" -d "$unpack"
clean_bin=$out/clean-bin
rm -rf "$clean_bin"; mkdir -p "$clean_bin"
# Nothing can be reached at all from the PATH Pane runs with (/bin/sh is
# named absolutely, so the check itself does not depend on the PATH).
[ -z "$(env -i PATH="$clean_bin" /bin/sh -c 'command -v cargo rustc node npm git cc clang make' 2>/dev/null)" ] \
  || { echo "the clean machine still reaches a development tool"; exit 1; }
env -i HOME="$home" PATH="/usr/bin:/bin" bash "$unpack/pane/install.sh" >>"$out/install.log" 2>&1 \
  || { echo "the install script failed (see $out/install.log)"; exit 1; }
[ -x "$home/Applications/Pane.app/Contents/MacOS/pane" ] || { echo "the install script installed no pane"; exit 1; }
start_installed() {
  env -i HOME="$home" PATH="$clean_bin" \
    PANE_THEME=dark PANE_MATERIAL=opaque \
    PANE_ARTIFACTS="http://127.0.0.1:$(cat "$out/artifact-server.port")/" \
    PANE_DEFAULTS="$default_pins" \
    "$home/Applications/Pane.app/Contents/MacOS/pane" "$@" 2>>"$out/installed-stderr.log" &
  pid=$!
  sleep 8
  focus_pane
}
start_installed
# Pane's own data in the clean home (its path holds a space, so it is quoted).
installed=$home/Library/Application\ Support/Pane/extensions
# Generous: a slow runner may take a while to check the five revisions'
# components (300 s each). A wait that fails records the screen and the
# clean home's files — the artifact upload skips hidden folders, so the
# records are copied out where it can see them.
record_setup_state() {
  rm -rf "$out/clean-home-records"
  mkdir -p "$out/clean-home-records"
  cp -f "$installed/installed.json" "$out/clean-home-records/" 2>/dev/null || true
  cp -r "$installed/downloads" "$out/clean-home-records/" 2>/dev/null || true
  cp -f "$out/installed-stderr.log" "$out/clean-home-records/" 2>/dev/null || true
  cp -f "$out/artifact-server.log" "$out/clean-home-records/" 2>/dev/null || true
  cp -f "$out/default-repository-server.log" "$out/clean-home-records/" 2>/dev/null || true
}
record_setup_problem() {
  capture 499-setup-problem.png
  record_setup_state
}
wait_recorded() {
  for _ in $(seq 3000); do
    grep -q "$1" "$installed/installed.json" 2>/dev/null && return
    sleep 0.1
  done
  echo "$installed/installed.json: $1 is not present"
  record_setup_problem
  exit 1
}
kill -0 "$pid" 2>/dev/null || { echo "the installed Pane exited during setup"; exit 1; }
# The default set (#60): all five, in every build; no sample is
# set up (#162).
for default_ in calculator applications quicklinks files clipboard-history; do
  wait_recorded "\"default\": \"$default_\""
done
sleep 1
capture 500-installed-root.png
check 500-installed-root.png subtitle   # root search: the default extensions' commands are listed
type_text '6*7'; sleep 2
capture 501-calculator-answer.png
check 501-calculator-answer.png answer   # "42", the calculator's selected answer card
key 36; sleep 1
capture 502-calculator-copied.png
check 502-calculator-copied.png success   # "Copied 42 to the clipboard"
if grep -q '"default": "helper-sample"' "$installed/installed.json"; then
  echo "a sample was set up as a default extension"; exit 1
fi
# Each default's record keeps the Git source it was fetched from: the
# repository, the pin's release tag, its commit and that it is pinned.
python3 "$(dirname "$0")/check_git_record.py" --defaults "$default_pins" "$installed/installed.json" \
  || { echo "a default's record does not keep its Git source"; exit 1; }
[ -z "$(ls -A "$installed/downloads" 2>/dev/null)" ] || { echo "downloads were left behind"; exit 1; }
python3 "$(dirname "$0")/check_screenshot.py" --distinct "$out"/{500-installed-root,501-calculator-answer}.png
stop_pane
kill "$artifact_server_pid"; wait "$artifact_server_pid" 2>/dev/null || true; artifact_server_pid=
# The program files go again: the evidence is the screenshots, the
# installed.json record and the logs.
record_setup_state
rm -f "$home/Applications/Pane.app/Contents/MacOS/pane" "$unpack/pane/pane"

# Clipboard history (#35, #37, #166, #167): Pane's own Clipboard History
# records what is copied from the first start, with nothing to turn on.
# Only the registered default extension does (a copy installed from its
# folder starts off and shows the generic list), so this phase sets the
# default set up from the pinned repositories the #52 phase serves, with
# the smoke's own build; Files' index covers an
# empty folder of the smoke's (PANE_TEST_FILE_INDEX_HOME), not the
# runner's home. The text this smoke copies is kept; nothing is kept while
# recording is paused (Pause Recording and Resume Recording, in the view's
# Actions panel, Command+K) or the extension is disabled (its switch in
# Settings), also after a restart, and once enabled again it is kept
# again, also after a restart. The command opens Pane's split view: the
# records by day, a type dropdown and the selected record's Information.
# Enter on a record runs Paste (#150): Pane cannot paste on macOS yet, so
# it copies the record again instead, closes the window and says so in a
# HUD; the copy really is on the pasteboard: pbpaste prints it, and
# pasting it into root search shows exactly what typing it shows. The
# smoke copies only text of its own ("pane-smoke-...") and a file of its
# own folder, with AppleScript's "set the clipboard to", which replaces
# what is on the pasteboard without reading or putting it back: run it on
# CI's runner or a desktop given to it, as the rest of the smoke already
# takes over the keyboard. macOS marks a copy a password manager makes
# with the type org.nspasteboard.ConcealedType, which Pane honors
# (clipboard_adapter_macos.rs checks that on CI's macOS leg); this smoke
# copies nothing marked, and no application can be disabled here, because
# the pasteboard never names the program that copied. A data folder of its
# own.
export PANE_DATA_DIR=$out/clipboard-data
rm -rf "$PANE_DATA_DIR" "$out/clipboard-home"
mkdir -p "$out/clipboard-home"
export PANE_TEST_FILE_INDEX_HOME=$(cd "$out/clipboard-home" && pwd)
export PANE_DEFAULTS=$default_pins
extensions=$PANE_DATA_DIR/extensions
history=$extensions/clipboard-history.json
# The kept texts, newest first, joined by commas.
kept_texts() { python3 "$(dirname "$0")/clipboard_history.py" texts "$extensions"; }
# The newest kept text.
first_kept() { kept_texts | cut -d, -f1; }
# Whether `text` is among the kept texts.
kept_one() { case ",$(kept_texts)," in (*",$1,"*) true;; (*) false;; esac; }
not_kept() { sleep 2; if kept_one "$1"; then echo "$1 was kept"; exit 1; fi; }
# Copies `text`: AppleScript puts it on the pasteboard, as a program
# copying text would (the watcher notices within its poll).
copy() { osascript -e "set the clipboard to \"$1\""; sleep 1; }
# Copies the file at path $1, as Finder's Copy does (a file URL). The copy's
# process stays alive past the watcher's file-ready wait (FILE_READY_WAIT, 2 s):
# the pasteboard serves a declared file URL's data from its living owner, and an
# osascript that exits at once can leave the type declared but its data never
# readable (#231).
copy_file() { osascript -e "set the clipboard to (POSIX file \"$1\")" -e "delay 2"; sleep 1; }
# Opens Pane's Clipboard History from wherever the smoke is: its split
# view, the field ("Type to filter entries…") holding the keyboard.
open_history() {
  focus_pane
  to_root
  type_text 'clipboard history'; sleep 1
  key 36; sleep 2
}
# Runs the entry of the view's Actions panel (Command+K) its filter
# narrows to with $1.
history_action() {
  command_key k; sleep 1
  type_text "$1"; sleep 0.5
  key 36; sleep 1
}
start_pane
# The default set (#60), set up at this first start.
for default_ in calculator applications quicklinks files clipboard-history; do
  wait_for "$extensions/installed.json" "\"default\": \"$default_\"" present 1200
done
sleep 3   # Clipboard History runs, and the watch with it
copy pane-smoke-kept   # nothing was turned on
copy pane-smoke-second
wait_for "$history" pane-smoke-second present
[ "$(kept_texts)" = pane-smoke-second,pane-smoke-kept ] || { echo "kept: $(kept_texts)"; exit 1; }
open_history
capture 280-clipboard-recording.png
check 280-clipboard-recording.png hint   # the split view: Today, the two records, the field's placeholder
history_action 'Pause Recording'
wait_for "$history" '"capture":"paused"' present
capture_until 281-clipboard-paused.png success 10   # its toast: "Recording paused"
copy pane-smoke-paused
not_kept pane-smoke-paused
open_history
history_action 'Resume Recording'
wait_for "$history" '"capture":"on"' present
copy pane-smoke-resumed
wait_for "$history" pane-smoke-resumed present
open_history
capture 282-clipboard-kept.png   # evidence only: the three records, newest first
type_text pane-smoke-second; sleep 1   # the filter leaves that record, selected
key 36; sleep 2   # Paste: not available yet, so it copies it again (#150)
capture 283-clipboard-copied.png   # evidence only: the HUD "Copied — paste is not available here yet", Pane hidden
[ "$(first_kept)" = pane-smoke-second ] || { echo "the copied record did not move to the front: $(kept_texts)"; exit 1; }
# The pasteboard really holds the record again: pbpaste prints it, and,
# with Pane started again (Paste closed its window; the pasteboard keeps
# what was copied), pasting it over root search shows exactly what typing
# it shows.
[ "$(pbpaste)" = pane-smoke-second ] || { echo "the pasteboard holds: $(pbpaste)"; exit 1; }
stop_pane
start_pane
to_root
command_key a; command_key v; sleep 1
capture 284-clipboard-pasted.png
key 53; sleep 1
type_text pane-smoke-second; sleep 1
capture 285-clipboard-typed.png
python3 "$(dirname "$0")/check_screenshot.py" --same "$out"/{284-clipboard-pasted,285-clipboard-typed}.png
open_extension "Clipboard History"
a11y toggle "Clipboard History"   # disable Clipboard History
wait_for "$extensions/installed.json" '"disabled": true' present; sleep 1
a11y shown "Disabled Clipboard History"
capture 286-clipboard-disabled.png
close_settings
copy pane-smoke-disabled
not_kept pane-smoke-disabled
stop_pane
start_pane
copy pane-smoke-restarted-disabled
not_kept pane-smoke-restarted-disabled
open_extension "Clipboard History"
a11y toggle "Clipboard History"   # enable Clipboard History
wait_for "$extensions/installed.json" '"disabled": true' absent; sleep 1
close_settings
copy pane-smoke-enabled
wait_for "$history" pane-smoke-enabled present
stop_pane
start_pane
copy pane-smoke-after-restart
wait_for "$history" pane-smoke-after-restart present
open_history
capture 287-clipboard-after-restart.png
check 287-clipboard-after-restart.png hint   # kept again after the restart
python3 "$(dirname "$0")/check_screenshot.py" --distinct "$out"/{280-clipboard-recording,281-clipboard-paused,284-clipboard-pasted,286-clipboard-disabled,287-clipboard-after-restart}.png
stop_pane
[ "$(kept_texts)" = pane-smoke-after-restart,pane-smoke-enabled,pane-smoke-second,pane-smoke-resumed,pane-smoke-kept ] || { echo "kept: $(kept_texts)"; exit 1; }
for never in paused disabled restarted-disabled; do
  kept_one "pane-smoke-$never" && { echo "pane-smoke-$never was kept"; exit 1; }
done

# Clipboard history expiry and deletion (#36, #166), on the history just
# kept. With Pane stopped, the smoke makes pane-smoke-kept 8 days old (past
# the default 7-day retention) and pane-smoke-enabled 2 hours old, as a
# downtime would: once Pane starts again, before the command shows
# anything, pane-smoke-kept is gone from the file and the list. Then, in
# the view's Actions panel: Delete Entry on pane-smoke-second (the filter
# leaves it) deletes that record alone; keeping records for 1 Hour deletes
# pane-smoke-enabled at once; a copied file is kept as a file (#167); and
# Clear History, once confirmed, deletes every record while recording goes
# on, so a later copy is kept, while the pasteboard still holds what was
# copied last (pbpaste prints it, as on Windows, whose smoke reads the
# clipboard directly too; on Linux pasting is the only way to see it).
backdate() { python3 "$(dirname "$0")/clipboard_history.py" backdate "$extensions" "$@"; }
field() { python3 "$(dirname "$0")/clipboard_history.py" field "$extensions" "$1"; }
backdate 8 pane-smoke-kept || { echo "could not backdate the history"; exit 1; }
backdate 0.084 pane-smoke-enabled || { echo "could not backdate the history"; exit 1; }
start_pane
sleep 1
[ "$(kept_texts)" = pane-smoke-after-restart,pane-smoke-enabled,pane-smoke-second,pane-smoke-resumed ] || { echo "kept after starting: $(kept_texts)"; exit 1; }
open_history
capture 400-clipboard-expired.png
check 400-clipboard-expired.png hint   # pane-smoke-kept is no longer listed
type_text pane-smoke-second; sleep 1   # the filter leaves that record, selected
history_action 'Delete Entry'
wait_for "$history" pane-smoke-second absent
capture_until 401-clipboard-item-deleted.png success 10   # its toast: "Deleted the kept item"
[ "$(kept_texts)" = pane-smoke-after-restart,pane-smoke-enabled,pane-smoke-resumed ] || { echo "kept: $(kept_texts)"; exit 1; }
open_history
history_action '1 Hour'   # Keep History For: 1 Hour
wait_for "$history" '"retentionSeconds":3600' present
capture_until 402-clipboard-retention-changed.png success 10   # its toast: "History is kept for 1 hour; deleted 1 kept item older"
[ "$(kept_texts)" = pane-smoke-after-restart,pane-smoke-resumed ] || { echo "kept: $(kept_texts)"; exit 1; }
printf 'a file the smoke copies\n' >"$out/clipboard-home/pane-smoke-file.txt"
copy_file "$PANE_TEST_FILE_INDEX_HOME/pane-smoke-file.txt"

wait_for "$history" pane-smoke-file.txt present
grep -q '"files"' "$history" || { echo "the copied file was not kept as a file"; exit 1; }
open_history
type_text pane-smoke-file; sleep 1   # the filter leaves the file's record, selected
capture 403-clipboard-file.png   # evidence only: the record, the file's icon and its preview
copy pane-smoke-final
wait_for "$history" pane-smoke-final present
open_history
history_action 'Clear History'   # asks first
capture 404-clipboard-clear-asked.png   # evidence only: "Clear Clipboard History?"
key 36   # Clear History
wait_for "$history" pane-smoke-final absent; sleep 1
capture 405-clipboard-cleared.png
[ -z "$(kept_texts)" ] || { echo "kept: $(kept_texts)"; exit 1; }
[ "$(field capture)" = on ] || { echo "history is $(field capture) after Clear History"; exit 1; }
# Deleting history never changes the pasteboard: it still holds what was
# copied last (pbpaste prints pane-smoke-final).
[ "$(pbpaste)" = pane-smoke-final ] || { echo "the pasteboard holds: $(pbpaste)"; exit 1; }
copy pane-smoke-after-clear
wait_for "$history" pane-smoke-after-clear present
python3 "$(dirname "$0")/check_screenshot.py" --distinct "$out"/{400-clipboard-expired,401-clipboard-item-deleted,402-clipboard-retention-changed,403-clipboard-file,405-clipboard-cleared}.png
stop_pane
[ "$(kept_texts)" = pane-smoke-after-clear ] || { echo "kept: $(kept_texts)"; exit 1; }
[ "$(field retentionSeconds)" = 3600 ] || { echo "retention: $(field retentionSeconds)"; exit 1; }
unset PANE_TEST_FILE_INDEX_HOME
export PANE_DEFAULTS=$no_default_pins

# Installing a Pane application update by the user's choice (#55, the
# macOS half of #54): a second package is built with --package-version
# 99.0.0, whose program reports 99.0.0 and whose index entry names it for
# macos-aarch64 (the two runnable builds the update goes between); the
# 0.1.0 package the #52 phase built is installed on another clean home,
# and its Pane, running from the Pane.app bundle's own binary
# (Contents/MacOS/pane — what CFBundleExecutable names, so the swap
# replaces the binary inside the bundle and the bundle itself stays), is
# told by the check it makes at start that 99.0.0 exists: a row in root
# search with the version, and a word on the status line. Nothing is
# downloaded until that row is chosen; the choice is proven by the
# artifact server's log, which must hold no request for the package
# until then. A corrupted package is explained first (its bytes do not
# match the sha512 its index gives), everything untouched and the row
# ready to try again; then the real install downloads the package,
# checks it, and swaps the running binary — the old one renamed pane.old
# beside it in Contents/MacOS, removed on a later start — so the new
# version is used the next time Pane starts (Pane never restarts
# itself). The new Pane, started again, reports 99.0.0, with the old
# version's data (the calculator set up at first setup) and the
# extension the user disabled kept, and with nothing of the update left
# in the bundle.
update_server_pid=
# Every server's arm again, plus this phase's own: nothing is lost
# whatever order the phases run in.
trap '[ -n "$pid" ] && kill "$pid" 2>/dev/null; [ -n "$npm_registry_pid" ] && kill "$npm_registry_pid" 2>/dev/null; [ -n "$repository_server_pid" ] && kill "$repository_server_pid" 2>/dev/null; [ -n "$service_pid" ] && kill "$service_pid" 2>/dev/null; [ -n "$artifact_server_pid" ] && kill "$artifact_server_pid" 2>/dev/null; [ -n "$update_server_pid" ] && kill "$update_server_pid" 2>/dev/null || true' EXIT
cargo xtask package-macos --dev --package-version 99.0.0
older=$(ls target/dist/pane-0.1.0-macos-*-dev.zip | head -1)
newer=$(ls target/dist/pane-99.0.0-macos-*-dev.zip | head -1)
[ -n "$older" ] && [ -n "$newer" ] || { echo "the two packages were not built"; exit 1; }
update_home=$out/update-home
unpack_old=$out/update-unpacked-old
unpack_new=$out/update-unpacked-new
rm -rf "$update_home" "$unpack_old" "$unpack_new"
mkdir -p "$update_home" "$unpack_old" "$unpack_new"
unzip -q "$older" -d "$unpack_old"
unzip -q "$newer" -d "$unpack_new"
rm -f "$out/update-artifact-server.port"
python3 "$(dirname "$0")/artifact_server.py" target/dist/artifacts "$out/update-artifact-server.port" 2>>"$out/update-artifact-server.log" &
update_server_pid=$!
for _ in $(seq 600); do [ -s "$out/update-artifact-server.port" ] && break; kill -0 "$update_server_pid" 2>/dev/null || break; sleep 0.1; done
[ -s "$out/update-artifact-server.port" ] || { echo "the update artifact source did not start (see $out/update-artifact-server.log)"; exit 1; }
# The 0.1.0 package installed on another clean home, as the #52 phase
# installed it.
env -i HOME="$update_home" PATH="/usr/bin:/bin" bash "$unpack_old/pane/install.sh" >>"$out/update-install.log" 2>&1 \
  || { echo "the install script failed (see $out/update-install.log)"; exit 1; }
binary=$update_home/Applications/Pane.app/Contents/MacOS/pane
[ -x "$binary" ] || { echo "the install script installed no pane"; exit 1; }
update_clean_bin=$out/update-clean-bin
rm -rf "$update_clean_bin"; mkdir -p "$update_clean_bin"
# Pane's own data in the update home (its path holds a space, so it is
# quoted).
update_installed="$update_home/Library/Application Support/Pane/extensions"
update_registry=$update_installed/installed.json
start_updated() {
  env -i HOME="$update_home" PATH="$update_clean_bin" \
    PANE_THEME=dark PANE_MATERIAL=opaque \
    PANE_ARTIFACTS="http://127.0.0.1:$(cat "$out/update-artifact-server.port")/" \
    PANE_DEFAULTS="$default_pins" \
    "$binary" "$@" 2>>"$out/update-stderr.log" &
  pid=$!
  sleep 8
  focus_pane
}
# A wait that fails records the screen and the update home's records,
# where the artifact upload can see them (as the #52 phase's does).
record_update_state() {
  rm -rf "$out/update-home-records"
  mkdir -p "$out/update-home-records"
  cp -f "$update_registry" "$out/update-home-records/" 2>/dev/null || true
  cp -f "$out/update-artifact-server.log" "$out/update-home-records/" 2>/dev/null || true
  cp -f "$out/update-stderr.log" "$out/update-home-records/" 2>/dev/null || true
}
update_recorded() {
  for _ in $(seq 3000); do
    grep -q "$1" "$update_registry" 2>/dev/null && return
    sleep 0.1
  done
  echo "$update_registry: $1 is not present"
  capture 599-update-problem.png
  record_update_state
  exit 1
}
start_updated
kill -0 "$pid" 2>/dev/null || { echo "the installed Pane exited during setup"; exit 1; }
update_recorded '"default": "calculator"'
update_recorded '"default": "clipboard-history"'
# The check has read the index (first setup fetches the pinned commits
# from the repositories, so no request reaches the artifact source for
# them): the offer is in root search. The status line tells what
# it found; nothing has been downloaded.
for _ in $(seq 100); do
  [ "$(grep -c pane-defaults.json "$out/update-artifact-server.log")" -ge 1 ] && break
  sleep 0.1
done
[ "$(grep -c pane-defaults.json "$out/update-artifact-server.log")" -ge 1 ] \
  || { echo "Pane never checked for its own update"; exit 1; }
sleep 2
capture 600-notification.png
check 600-notification.png success   # "Pane 99.0.0 is available" (or the setup's own outcome)
command_key a; type_text update; sleep 1
capture 601-offered.png
check 601-offered.png subtitle   # the offer row: "Your extensions and settings are kept; ..."
check 601-offered.png selected 3000   # the row, selected
# Taking no action downloads nothing: no package was asked for.
[ -z "$(grep "\.zip" "$out/update-artifact-server.log")" ] \
  || { echo "a package was downloaded without the user choosing it"; exit 1; }

# Disable Clipboard History first: an extension the user disabled before
# the update must stay disabled after it (the switch on its page in
# Settings, #168).
open_extension "Clipboard History"
a11y toggle "Clipboard History"   # Clipboard History: disabled
for _ in $(seq 100); do grep -q '"disabled": true' "$update_registry" 2>/dev/null && break; sleep 0.1; done
grep -q '"disabled": true' "$update_registry" || { echo "Clipboard History was not disabled"; exit 1; }
close_settings

# A package that does not match the integrity its index gives is
# explained and not installed: the bytes of the served package are
# damaged, and the program keeps running the one it was.
served=target/dist/artifacts/$(basename "$newer")
python3 - "$served" <<'PY'
import sys
with open(sys.argv[1], "rb") as f:
    package = bytearray(f.read())
package[len(package) // 2] ^= 1
with open(sys.argv[1], "wb") as f:
    f.write(package)
PY
command_key a; type_text update; sleep 1
key 36   # the offer, tried: the damaged package is explained
capture_until 602-corrupt-package.png error 60   # "Could not update Pane to 99.0.0: ... does not match the sha512 integrity"
[ -e "$binary.old" ] && { echo "a failed install replaced the program"; exit 1; }
cmp -s "$binary" "$unpack_old/pane/pane" || { echo "a failed install changed the program"; exit 1; }
[ -e "$binary/../update" ] && { echo "a failed install left its staging behind"; exit 1; }

# The source works again; the row that stays tries again, and the update
# is installed: the new binary takes the old one's name and place inside
# the bundle, the old one renamed out of its way.
cp "$newer" "$served"
command_key a; type_text update; sleep 1
key 36
for _ in $(seq 1200); do
  [ -e "$binary.old" ] && break
  kill -0 "$pid" 2>/dev/null || { echo "Pane exited while updating itself"; exit 1; }
  sleep 0.2
done
[ -e "$binary.old" ] || { echo "the update was not installed"; exit 1; }
sleep 2
capture 603-installed.png
check 603-installed.png success   # "Installed Pane 99.0.0; the new version is used the next time Pane starts"
cmp -s "$binary" "$unpack_new/pane/pane" || { echo "the new program was not installed"; exit 1; }
cmp -s "$binary.old" "$unpack_old/pane/pane" || { echo "the old program was not kept out of the new one's way"; exit 1; }
[ -e "$binary/../update" ] && { echo "the install left its staging behind"; exit 1; }
# The package was downloaded once for each attempt: the damaged one and
# the one that installed.
[ "$(grep -c "\.zip" "$out/update-artifact-server.log")" = 2 ] \
  || { echo "the package was not downloaded exactly twice"; exit 1; }
stop_pane

# The next start runs the new version: it reports 99.0.0, removes what
# the update left, and the old version's data is kept — the calculator
# answers and Clipboard History stays disabled.
env -i HOME="$update_home" PATH="/usr/bin:/bin" "$binary" --version >"$out/update-version.txt" 2>>"$out/update-stderr.log" \
  || { echo "the new pane --version failed"; exit 1; }
[ "$(cat "$out/update-version.txt")" = "Pane 99.0.0" ] \
  || { echo "the new program reports the wrong version"; exit 1; }
start_updated
for _ in $(seq 100); do [ ! -e "$binary.old" ] && break; sleep 0.1; done
[ ! -e "$binary.old" ] || { echo "the old program's file was not removed on the new start"; exit 1; }
command_key a; type_text '6*7'; sleep 2
capture 604-answer-after-update.png
check 604-answer-after-update.png answer   # "42", the calculator's selected answer card
key 36; sleep 1
capture 605-copied-after-update.png
check 605-copied-after-update.png success   # "Copied 42 to the clipboard"
[ "$(pbpaste)" = "42" ] || { echo "the pasteboard holds: $(pbpaste)"; exit 1; }
grep -q '"disabled": true' "$update_registry" || { echo "the disabled extension did not stay disabled"; exit 1; }
python3 "$(dirname "$0")/check_screenshot.py" --distinct "$out"/{600-notification,601-offered,602-corrupt-package,603-installed,604-answer-after-update}.png
stop_pane
kill "$update_server_pid"; wait "$update_server_pid" 2>/dev/null || true; update_server_pid=
# The program files go again, as the #52 phase's do: the evidence is the
# screenshots and the update-home-records folder, not the program.
record_update_state
rm -f "$binary" "$binary.old" "$unpack_old/pane/pane" "$unpack_new/pane/pane"

echo "screenshots in $out"
