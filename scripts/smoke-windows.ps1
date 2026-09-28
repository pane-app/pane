# Native GUI smoke on Windows: launches Pane, drives it with real key events
# and captures the screen. Pane keeps installed packages in <output-dir>\data,
# not the user's data folder.
# Usage: scripts/smoke-windows.ps1 -OutDir <output-dir>
param([string]$OutDir = "smoke")
$ErrorActionPreference = "Stop"
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$data = Join-Path $OutDir "data"
if (Test-Path $data) { Remove-Item -Recurse -Force $data }
$env:PANE_DATA_DIR = $data
# The tested OS version and architecture
"$([System.Environment]::OSVersion.VersionString) $env:PROCESSOR_ARCHITECTURE" | Set-Content (Join-Path $OutDir "system.txt")
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type @"
using System; using System.Runtime.InteropServices;
public static class Win {
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint x, uint y, uint data, UIntPtr extra);
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int command);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
}
"@
# Screenshots, screen bounds and SetCursorPos then all use physical pixels,
# so a position found in a screenshot is where the click lands at any
# display scaling.
[Win]::SetProcessDPIAware() | Out-Null
function Capture($name) {
    $bounds = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
    $bitmap = New-Object System.Drawing.Bitmap $bounds.Width, $bounds.Height
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    $graphics.CopyFromScreen($bounds.Location, [System.Drawing.Point]::Empty, $bounds.Size)
    $bitmap.Save((Join-Path $OutDir $name))
}
function Check($name, $color, $minimum = 20) {
    python "$PSScriptRoot/check_screenshot.py" (Join-Path $OutDir $name) $color $minimum
    if ($LASTEXITCODE -ne 0) { throw "screenshot check failed: $name" }
}
# Returns x, y: where the screenshot shows the given color.
function Locate($name, $color) {
    $at = python "$PSScriptRoot/check_screenshot.py" --locate (Join-Path $OutDir $name) $color
    if ($LASTEXITCODE -ne 0) { throw "color not found: $name $color" }
    return [int[]]($at -split " ")
}
# Clicks the primary button at x, y in the screenshot's pixels.
function Click-At($x, $y) {
    [Win]::SetCursorPos($x, $y) | Out-Null
    [Win]::mouse_event(0x2, 0, 0, 0, [UIntPtr]::Zero)   # left button down
    [Win]::mouse_event(0x4, 0, 0, 0, [UIntPtr]::Zero)   # left button up
}
function Send($keys) { [System.Windows.Forms.SendKeys]::SendWait($keys) }
# Brings Pane's window to the front, so that key events reach it.
function Focus-Pane($process) {
    [Win]::SetForegroundWindow($process.MainWindowHandle) | Out-Null
    Start-Sleep -Milliseconds 500
}
# Starts Pane with the given arguments, writing its errors to $log, and
# brings its window to the front.
function Start-Pane($log, [string[]]$arguments) {
    $options = @{
        FilePath = "target/debug/pane.exe"
        PassThru = $true
        RedirectStandardError = (Join-Path $OutDir $log)
    }
    if ($arguments) { $options.ArgumentList = $arguments }
    $process = Start-Process @options
    for ($i = 0; $i -lt 50 -and $process.MainWindowHandle -eq 0; $i++) {
        Start-Sleep -Milliseconds 200; $process.Refresh()
    }
    if ($process.MainWindowHandle -eq 0) { throw "Pane window did not appear" }
    Start-Sleep -Seconds 2
    Focus-Pane $process
    return $process
}
function Stop-Pane($process) {
    if ($process.HasExited) { throw "Pane exited during the smoke" }
    Stop-Process -Id $process.Id
    $process.WaitForExit()
}

$process = Start-Pane "stderr.log"
Capture "1-root.png"
Check "1-root.png" "8a96a3"   # the hint line: text renders
# Open each sample command (Rust, JavaScript, TypeScript) and run an item.
foreach ($index in 0..2) {
    for ($i = 0; $i -lt $index; $i++) { Send "{DOWN}" }
    Send "{ENTER}"; Start-Sleep -Seconds 3
    Capture "$($index + 2)-command-$index.png"
    Send "{DOWN}{ENTER}"; Start-Sleep -Seconds 2
    Capture "$($index + 2)-result-$index.png"
    Check "$($index + 2)-result-$index.png" "9fd8a8"   # the guest's answer
    Send "{ESC}"; Start-Sleep -Seconds 1
}
Capture "5-back-to-root.png"
# Each command must have answered from its own guest, not the same view twice.
python "$PSScriptRoot/check_screenshot.py" --distinct @(2..4 | ForEach-Object { Join-Path $OutDir "$_-result-$($_ - 2).png" })
if ($LASTEXITCODE -ne 0) { throw "screenshot check failed: result screenshots are not distinct" }

# The Rust command's form (its fifth item): submitting it empty is rejected
# and focus returns to the name, so typing there and choosing a greeting with
# Tab and Down makes the guest answer.
Send "{ENTER}"; Start-Sleep -Seconds 3
Send "{DOWN}{DOWN}{DOWN}{DOWN}{ENTER}"; Start-Sleep -Seconds 1
Capture "6-form.png"
Send "{ENTER}"; Start-Sleep -Seconds 2
Capture "7-form-error.png"
Check "7-form-error.png" "f08c8c"   # the rejected field's message
Send "Ada{TAB}{DOWN}{ENTER}"; Start-Sleep -Seconds 2
Capture "8-form-result.png"
Check "8-form-result.png" "9fd8a8"   # the guest's answer
Send "{ESC}{ESC}"; Start-Sleep -Seconds 1
Stop-Pane $process

# Install the assembled Rust sample package (the folder the picker would
# return), then run its command. Root lists the three samples, the installed
# command, then the install and Manage extensions rows.
$process = Start-Pane "stderr-install.log" @("--install", "target/guests/packages/sample-rust")
Capture "9-package.png"
Check "9-package.png" "aab4c0"   # the package's identity and compatibility lines
Send "{ENTER}"; Start-Sleep -Seconds 2
Capture "10-installed.png"
Check "10-installed.png" "9fd8a8"   # "Installed Rust sample"
Send "{ENTER}"; Start-Sleep -Seconds 3
Send "{ENTER}"; Start-Sleep -Seconds 2
Capture "11-installed-result.png"
Check "11-installed-result.png" "9fd8a8"   # the installed guest's answer
Stop-Pane $process

# The installed command is still listed after a restart.
$process = Start-Pane "stderr-restart.log"
Capture "12-restarted.png"
Check "12-restarted.png" "8a96a3"
if (-not (Test-Path (Join-Path $data "extensions/installed.json"))) { throw "no install record" }
Focus-Pane $process

# The Rust command's seventh item is declared for Windows only, its eighth
# for macOS and Linux only. Here the first runs and the second is explained
# without running.
Send "{ENTER}"; Start-Sleep -Seconds 3
Send "{DOWN}{DOWN}{DOWN}{DOWN}{DOWN}{DOWN}{ENTER}"; Start-Sleep -Seconds 2
Capture "13-windows-only.png"
Check "13-windows-only.png" "9fd8a8"   # Windows: the guest's answer
Send "{DOWN}{ENTER}"; Start-Sleep -Seconds 2
Capture "14-not-windows.png"
Check "14-not-windows.png" "d6a36a"   # the row's reason
Check "14-not-windows.png" "f08c8c"   # Windows: the reason as the error
Send "{ESC}"; Start-Sleep -Seconds 1
Stop-Pane $process

# A package that supports only the other two systems has nothing for this
# one: it is explained instead of offered for installation.
$elsewhere = Join-Path $OutDir "elsewhere"
New-Item -ItemType Directory -Force -Path $elsewhere | Out-Null
Copy-Item "target/guests/sample_rust.wasm" $elsewhere
@'
{
  "manifestVersion": 1,
  "title": "Elsewhere",
  "apiVersion": "0.1",
  "platforms": ["macos", "linux"],
  "commands": [{ "id": "sample", "title": "Elsewhere sample", "component": "sample_rust.wasm" }]
}
'@ | Set-Content -Encoding ascii (Join-Path $elsewhere "pane.json")
$process = Start-Pane "stderr-elsewhere.log" @("--install", $elsewhere)
Capture "15-no-compatible-package.png"
Check "15-no-compatible-package.png" "f08c8c"   # "Not available on Windows: ..."
Stop-Pane $process

# Install the settings sample, save a choice with it, then disable it in
# Manage extensions. Root lists the three samples, Rust sample, Greeting, the
# install row, then Manage extensions... last; the extension list holds Rust
# sample, then Settings sample.
$process = Start-Pane "stderr-settings.log" @("--install", "target/guests/packages/sample-settings")
Send "{ENTER}"; Start-Sleep -Seconds 2   # Install; Greeting is selected
Send "{ENTER}"; Start-Sleep -Seconds 3   # open Greeting
Send "{ENTER}"; Start-Sleep -Seconds 2   # "Use a formal greeting"
Capture "16-setting-saved.png"
Check "16-setting-saved.png" "9fd8a8"   # "Saved the formal greeting"
Send "{ESC}"; Start-Sleep -Seconds 1
Send "{DOWN 10}"   # the last row
Send "{ENTER}"; Start-Sleep -Seconds 1
Send "{DOWN}{ENTER}"; Start-Sleep -Seconds 2
Capture "17-disabled.png"
Check "17-disabled.png" "9fd8a8"   # "Disabled Settings sample"
Stop-Pane $process
if (-not (Select-String -Quiet -SimpleMatch '"disabled": true' (Join-Path $data "extensions/installed.json"))) { throw "disabled state not recorded" }
if (-not (Select-String -Quiet -SimpleMatch '"greeting-style": "formal"' (Join-Path $data "extensions/settings.json"))) { throw "setting not saved" }

# After a restart Greeting is no longer in root search: root looks exactly as
# it did before the settings sample was installed. Enabling the package again
# brings it back with its setting: "Greet me" answers in the saved formal
# style, where without a saved style it reports an error.
$process = Start-Pane "stderr-reenable.log"
Capture "18-restarted-disabled.png"
Check "18-restarted-disabled.png" "8a96a3"
python "$PSScriptRoot/check_screenshot.py" --same (Join-Path $OutDir "12-restarted.png") (Join-Path $OutDir "18-restarted-disabled.png")
if ($LASTEXITCODE -ne 0) { throw "screenshot check failed: root after the restart lists the disabled package" }
Send "{DOWN 10}"
Send "{ENTER}"; Start-Sleep -Seconds 1
Send "{DOWN}{ENTER}"; Start-Sleep -Seconds 2
Capture "19-enabled.png"
Check "19-enabled.png" "9fd8a8"   # "Enabled Settings sample"
Send "{ESC}"; Start-Sleep -Seconds 1
Send "{DOWN 4}"   # Greeting
Send "{ENTER}"; Start-Sleep -Seconds 3
Send "{DOWN}{DOWN}{ENTER}"; Start-Sleep -Seconds 2   # "Greet me"
Capture "20-greeted.png"
Check "20-greeted.png" "9fd8a8"   # "Good day to you"
Stop-Pane $process

# Restarted, root lists Greeting again, after Rust sample.
$process = Start-Pane "stderr-color.log"

# The Rust command's color picker (its sixth item), which the guest draws:
# Right chooses purple, and a click on the dark green swatch chooses it. The
# chosen color fills its swatch and the preview, far more pixels than any
# other swatch covers.
Send "{ENTER}"; Start-Sleep -Seconds 3
Send "{DOWN}{DOWN}{DOWN}{DOWN}{DOWN}{ENTER}"; Start-Sleep -Seconds 2
Capture "21-color.png"
Check "21-color.png" "1e88e5" 3000   # blue, chosen when the view opens
Send "{RIGHT}"; Start-Sleep -Seconds 1
Capture "22-color-key.png"
Check "22-color-key.png" "8e24aa" 3000   # purple
$x, $y = Locate "22-color-key.png" "1b5e20"
Click-At $x $y; Start-Sleep -Seconds 1
Capture "23-color-click.png"
Check "23-color-click.png" "1b5e20" 3000   # dark green
Send "{ESC}{ESC}"; Start-Sleep -Seconds 1
Stop-Pane $process

# Root search: typing narrows root to the matching commands and Enter opens
# the best match. "typescr" matches only TypeScript sample, whose "Wait
# briefly" answers exactly as in step 4. A query that matches nothing shows
# no results, and Enter then opens nothing.
$process = Start-Pane "stderr-search.log"
Send "typescr"; Start-Sleep -Seconds 1
Capture "24-search.png"
Send "{ENTER}"; Start-Sleep -Seconds 3
Send "{DOWN}{ENTER}"; Start-Sleep -Seconds 2
Capture "25-search-result.png"
Check "25-search-result.png" "9fd8a8"   # the TypeScript guest's answer
python "$PSScriptRoot/check_screenshot.py" --same (Join-Path $OutDir "4-result-2.png") (Join-Path $OutDir "25-search-result.png")
if ($LASTEXITCODE -ne 0) { throw "screenshot check failed: the searched command is not the TypeScript sample" }
Send "{ESC}"; Start-Sleep -Seconds 1
Send "zzz"; Start-Sleep -Seconds 1
Send "{ENTER}"; Start-Sleep -Seconds 1
Capture "26-no-results.png"
$shots = "1-root", "24-search", "25-search-result", "26-no-results" | ForEach-Object { Join-Path $OutDir "$_.png" }
python "$PSScriptRoot/check_screenshot.py" --distinct @shots
if ($LASTEXITCODE -ne 0) { throw "screenshot check failed: root search showed the same window twice" }
Stop-Pane $process

# The calculator, a default extension: an expression typed into root search
# lists its answer first, selected, and Enter copies it. Pasting the copy
# over the query and typing on shows exactly the screen typing the whole
# expression shows, so the clipboard held the answer.
# SendKeys: {+} is a plus sign, ^ holds Ctrl.
$process = Start-Pane "stderr-calculator.log" @("--install", "target/guests/packages/calculator")
Send "{ENTER}"; Start-Sleep -Seconds 2   # Install
Send "6*7"; Start-Sleep -Seconds 2
Capture "27-answer.png"
Check "27-answer.png" "364355" 3000   # the selected answer row
Send "{ENTER}"; Start-Sleep -Seconds 1
Capture "28-copied.png"   # "Copied 42 to the clipboard"
Send "^a"; Send "42{+}1"; Start-Sleep -Seconds 2
Capture "29-typed.png"
Send "^a"; Send "^v"; Send "{+}1"; Start-Sleep -Seconds 2
Capture "30-pasted.png"
$shots = "27-answer", "28-copied", "29-typed" | ForEach-Object { Join-Path $OutDir "$_.png" }
python "$PSScriptRoot/check_screenshot.py" --distinct @shots
if ($LASTEXITCODE -ne 0) { throw "screenshot check failed: the calculator showed the same window twice" }
python "$PSScriptRoot/check_screenshot.py" --same (Join-Path $OutDir "29-typed.png") (Join-Path $OutDir "30-pasted.png")
if ($LASTEXITCODE -ne 0) { throw "screenshot check failed: pasting did not give the copied answer" }
Stop-Pane $process

# Operations: install the JavaScript operations sample, then the Rust one,
# whose command (Call from Rust, selected once installed) opens its form,
# takes the JavaScript package's identity (local: and the folder's resolved
# path) and a name, and calls that package's greet operation: "Hello, Rust,
# from JavaScript" comes from the other package's guest, started for the call.
$process = Start-Pane "stderr-operations-target.log" @("--install", "target/guests/packages/sample-operations-js")
Send "{ENTER}"; Start-Sleep -Seconds 2   # Install
Capture "31-operations-target.png"
Check "31-operations-target.png" "9fd8a8"   # "Installed JavaScript operations sample"
Stop-Pane $process
$process = Start-Pane "stderr-operations.log" @("--install", "target/guests/packages/sample-operations")
Send "{ENTER}"; Start-Sleep -Seconds 2   # Install; Call from Rust is selected
Send "{ENTER}"; Start-Sleep -Seconds 3   # open Call from Rust
Send "{ENTER}"; Start-Sleep -Seconds 2   # "Greet through another extension": its form
Send ("local:" + (Resolve-Path "target/guests/packages/sample-operations-js").Path)
Send "{TAB}Rust"
Send "{ENTER}"; Start-Sleep -Seconds 5   # Greet
Capture "32-operation-answer.png"
Check "32-operation-answer.png" "9fd8a8"   # the JavaScript guest's answer
$shots = "31-operations-target", "32-operation-answer" | ForEach-Object { Join-Path $OutDir "$_.png" }
python "$PSScriptRoot/check_screenshot.py" --distinct @shots
if ($LASTEXITCODE -ne 0) { throw "screenshot check failed: the operation's answer did not appear" }
Stop-Pane $process

# Reload a development package while Pane stays open. Its command starts as
# the Rust sample; a new build of it is the JavaScript sample. Root lists the
# three samples, Rust sample, Greeting, Calculator, Call from JavaScript, Call
# from Rust, Dev sample (the ninth row), the install row, then Manage
# extensions... last; the extension list holds the six packages (Dev is the
# sixth), then their six Reload rows (Reload Dev is the twelfth).
$dev = Join-Path $OutDir "dev"
New-Item -ItemType Directory -Force -Path $dev | Out-Null
Copy-Item "target/guests/sample_rust.wasm" (Join-Path $dev "command.wasm")
@'
{
  "manifestVersion": 1,
  "title": "Dev",
  "apiVersion": "0.1",
  "commands": [{ "id": "sample", "title": "Dev sample", "component": "command.wasm" }]
}
'@ | Set-Content -Encoding ascii (Join-Path $dev "pane.json")
$process = Start-Pane "stderr-reload.log" @("--install", $dev)
Send "{ENTER}"; Start-Sleep -Seconds 2   # Install; Dev sample is selected
Send "{ENTER}"; Start-Sleep -Seconds 3
Send "{ENTER}"; Start-Sleep -Seconds 2   # "Say hello"
Capture "33-dev-before.png"
Check "33-dev-before.png" "9fd8a8"   # "Hello from the Rust guest"
Send "{ESC}"; Start-Sleep -Seconds 1
Copy-Item -Force "target/guests/sample_js.wasm" (Join-Path $dev "command.wasm")
Send "{DOWN 12}"   # the last row
Send "{ENTER}"; Start-Sleep -Seconds 1
Send "{DOWN 11}"   # Reload Dev
Send "{ENTER}"; Start-Sleep -Seconds 3
Capture "34-reloaded.png"
Check "34-reloaded.png" "9fd8a8"   # "Reloaded Dev"
Send "{ESC}"; Start-Sleep -Seconds 1
Send "{DOWN 8}"   # Dev sample
Send "{ENTER}"; Start-Sleep -Seconds 3
Send "{ENTER}"; Start-Sleep -Seconds 2   # "Say hello"
Capture "35-dev-after.png"
Check "35-dev-after.png" "9fd8a8"   # "Hello from the JavaScript guest"
python "$PSScriptRoot/check_screenshot.py" --distinct (Join-Path $OutDir "33-dev-before.png") (Join-Path $OutDir "35-dev-after.png")
if ($LASTEXITCODE -ne 0) { throw "screenshot check failed: the reloaded command shows its earlier code" }
Send "{ESC}"; Start-Sleep -Seconds 1

# A build that fails the install checks (here its component is missing) is
# not reloaded: the working code keeps running, exactly as before.
Remove-Item (Join-Path $dev "command.wasm")
Send "{DOWN 12}"
Send "{ENTER}"; Start-Sleep -Seconds 1
Send "{DOWN 11}"
Send "{ENTER}"; Start-Sleep -Seconds 2
Capture "36-not-reloaded.png"
Check "36-not-reloaded.png" "f08c8c"   # "Dev was not reloaded: ..."
Send "{ESC}"; Start-Sleep -Seconds 1
Send "{DOWN 8}"
Send "{ENTER}"; Start-Sleep -Seconds 3
Send "{ENTER}"; Start-Sleep -Seconds 2
Capture "37-still-running.png"
python "$PSScriptRoot/check_screenshot.py" --same (Join-Path $OutDir "35-dev-after.png") (Join-Path $OutDir "37-still-running.png")
if ($LASTEXITCODE -ne 0) { throw "screenshot check failed: a build that failed its checks replaced the working code" }
Send "{ESC}"; Start-Sleep -Seconds 1

# A build whose start fails is reported with Retry, after Reload Dev; this
# one saves a setting and fails its first start only, so Retry starts it.
Copy-Item "target/guests/failing_start.wasm" (Join-Path $dev "command.wasm")
Send "{DOWN 12}"
Send "{ENTER}"; Start-Sleep -Seconds 1
Send "{DOWN 11}"
Send "{ENTER}"; Start-Sleep -Seconds 3
Capture "38-start-failed.png"
Check "38-start-failed.png" "f08c8c"   # "Reloaded Dev, but it failed to start; ..."
Send "{DOWN}{ENTER}"; Start-Sleep -Seconds 3   # Retry starting Dev
Capture "39-retried.png"
Check "39-retried.png" "9fd8a8"   # "Started Dev"
Stop-Pane $process
if (-not (Select-String -Quiet -SimpleMatch '"start-attempted": "yes"' (Join-Path $data "extensions/settings.json"))) { throw "the failed start's setting was not kept" }

# The settings sample keeps one value of each kind of data: its formal style
# (settings) and "Good day to you" (cache) are saved above; its fourth and
# fifth items save a note (content) and sign in (a local credential), and its
# sixth shows all four.
$process = Start-Pane "stderr-kept.log"
Send "{DOWN 4}"   # Greeting
Send "{ENTER}"; Start-Sleep -Seconds 3
Send "{DOWN 3}"
Send "{ENTER}"; Start-Sleep -Seconds 2   # "Save a note"
Send "{DOWN}{ENTER}"; Start-Sleep -Seconds 2   # "Sign in"
Send "{DOWN}{ENTER}"; Start-Sleep -Seconds 2   # "Show what Pane keeps"
Capture "40-kept.png"
Check "40-kept.png" "9fd8a8"   # every value, the cached greeting included
Send "{ESC}"; Start-Sleep -Seconds 1
Stop-Pane $process
if (-not (Select-String -Quiet -SimpleMatch '"note": "Water the plants"' (Join-Path $data "extensions/content.json"))) { throw "note not saved" }
if (-not (Select-String -Quiet -SimpleMatch '"token": "sample-token"' (Join-Path $data "extensions/credentials.json"))) { throw "credential not saved" }
if (-not (Select-String -Quiet -SimpleMatch '"last-greeting": "Good day to you"' (Join-Path $data "extensions/cache.json"))) { throw "greeting not cached" }

# Clear the settings sample's cache in Manage extensions: its row follows the
# six package rows, their six Reload rows and "Clear cache of Rust sample". Pane asks first, then deletes only the cached
# greeting, without running the extension.
$process = Start-Pane "stderr-clear-cache.log"
Send "{DOWN 12}"   # the last row
Send "{ENTER}"; Start-Sleep -Seconds 1
Send "{DOWN 13}"
Send "{ENTER}"; Start-Sleep -Seconds 1   # "Clear cache of Settings sample"
Capture "41-confirm-clear-cache.png"
Check "41-confirm-clear-cache.png" "aab4c0"   # what is deleted and what is kept
Send "{ENTER}"; Start-Sleep -Seconds 2   # "Clear cache"
Capture "42-cache-cleared.png"
Check "42-cache-cleared.png" "9fd8a8"   # "Cleared the cache of Settings sample"
Send "{ESC}"; Start-Sleep -Seconds 1
Send "{DOWN 4}"   # Greeting
Send "{ENTER}"; Start-Sleep -Seconds 3
Send "{DOWN 5}"
Send "{ENTER}"; Start-Sleep -Seconds 2   # "Show what Pane keeps"
Capture "43-kept-after-clear.png"
Check "43-kept-after-clear.png" "9fd8a8"   # "... Cached greeting: none"
python "$PSScriptRoot/check_screenshot.py" --distinct (Join-Path $OutDir "40-kept.png") (Join-Path $OutDir "43-kept-after-clear.png")
if ($LASTEXITCODE -ne 0) { throw "screenshot check failed: the cached greeting is still shown" }
Send "{ESC}"; Start-Sleep -Seconds 1
Stop-Pane $process
if (Select-String -Quiet -SimpleMatch 'Good day to you' (Join-Path $data "extensions/cache.json")) { throw "cache not cleared" }
if (-not (Select-String -Quiet -SimpleMatch '"greeting-style": "formal"' (Join-Path $data "extensions/settings.json"))) { throw "setting lost" }
if (-not (Select-String -Quiet -SimpleMatch '"note": "Water the plants"' (Join-Path $data "extensions/content.json"))) { throw "note lost" }
if (-not (Select-String -Quiet -SimpleMatch '"token": "sample-token"' (Join-Path $data "extensions/credentials.json"))) { throw "credential lost" }

# Applications, a default extension: an installed application is found by
# name in root search and Enter opens it. The application is a Start menu
# shortcut the smoke adds under an APPDATA of its own (for Pane only), to
# cmd.exe writing a marker file, so nothing else is started; Pane still
# searches the system's applications too.
$apps = Join-Path (Resolve-Path $OutDir) "apps"
if (Test-Path $apps) { Remove-Item -Recurse -Force $apps }
$programs = Join-Path $apps "AppData\Microsoft\Windows\Start Menu\Programs"
New-Item -ItemType Directory -Force -Path $programs | Out-Null
$launched = Join-Path $apps "launched.txt"
$shortcut = (New-Object -ComObject WScript.Shell).CreateShortcut((Join-Path $programs "Pane Smoke App.lnk"))
$shortcut.TargetPath = "$env:SystemRoot\System32\cmd.exe"
$shortcut.Arguments = "/c echo launched> `"$launched`""
$shortcut.WindowStyle = 7   # minimized, so it does not cover Pane
$shortcut.Save()
$appData = $env:APPDATA
$env:APPDATA = Join-Path $apps "AppData"
$process = Start-Pane "stderr-applications.log" @("--install", "target/guests/packages/applications")
$env:APPDATA = $appData
Send "{ENTER}"; Start-Sleep -Seconds 2   # Install
Send "pane smoke"; Start-Sleep -Seconds 3
Capture "44-application.png"
Check "44-application.png" "364355" 3000   # the selected application row
Send "{ENTER}"; Start-Sleep -Seconds 3
Focus-Pane $process
Capture "45-opened.png"
Check "45-opened.png" "9fd8a8"   # "Opened Pane Smoke App"
for ($i = 0; $i -lt 50 -and -not (Test-Path $launched); $i++) { Start-Sleep -Milliseconds 200 }
if (-not (Test-Path $launched)) { throw "the application did not run" }
$shots = "44-application", "45-opened" | ForEach-Object { Join-Path $OutDir "$_.png" }
python "$PSScriptRoot/check_screenshot.py" --distinct @shots
if ($LASTEXITCODE -ne 0) { throw "screenshot check failed: opening the application changed nothing" }
Stop-Pane $process

# Quicklinks, a default extension: installed, its command's form saves a
# quicklink (Quicklinks is selected once installed, and "Create quicklink" is
# its first item). After a restart, typing part of its name lists it,
# selected. Enter would open the default browser, so this smoke stops there
# (the Linux smoke opens it through a recording handler).
$process = Start-Pane "stderr-quicklinks.log" @("--install", "target/guests/packages/quicklinks")
Send "{ENTER}"; Start-Sleep -Seconds 2   # Install
Send "{ENTER}"; Start-Sleep -Seconds 3   # open Quicklinks
Send "{ENTER}"; Start-Sleep -Seconds 1   # Create quicklink
Send "Pane issues"
Send "{TAB}"
Send "https://example.com/pane-issues"
Send "{ENTER}"; Start-Sleep -Seconds 2
Capture "46-quicklink-saved.png"
Check "46-quicklink-saved.png" "9fd8a8"   # "Saved quicklink “Pane issues”"
Send "{ESC}"; Send "{ESC}"; Start-Sleep -Seconds 1
Stop-Pane $process
$process = Start-Pane "stderr-quicklinks-restart.log"
Send "pane iss"; Start-Sleep -Seconds 2
Capture "47-quicklink-found.png"
Check "47-quicklink-found.png" "364355" 3000   # the selected quicklink row
$shots = "46-quicklink-saved", "47-quicklink-found" | ForEach-Object { Join-Path $OutDir "$_.png" }
python "$PSScriptRoot/check_screenshot.py" --distinct @shots
if ($LASTEXITCODE -ne 0) { throw "screenshot check failed: the quicklink was not found" }
Stop-Pane $process

# Uninstall the settings sample, keeping its saved data: its row follows the
# eight Clear cache rows. Pane asks first, showing its saved data, and the first
# choice keeps its settings and content while its copy and credential go.
# Installing the same folder again finds its formal style and note, signed out.
$process = Start-Pane "stderr-uninstall.log"
Send "{DOWN 20}"   # the last row
Send "{ENTER}"; Start-Sleep -Seconds 1
Send "{DOWN 25}"
Send "{ENTER}"; Start-Sleep -Seconds 1   # "Uninstall Settings sample"
Capture "49-confirm-uninstall.png"
Check "49-confirm-uninstall.png" "aab4c0"   # what is removed and the saved data
Send "{ENTER}"; Start-Sleep -Seconds 2   # "Uninstall and keep saved data"
Capture "50-uninstalled.png"
Check "50-uninstalled.png" "9fd8a8"   # "Uninstalled Settings sample; its settings and content are kept"
Stop-Pane $process
if (-not (Select-String -Quiet -SimpleMatch '"retained"' (Join-Path $data "extensions/installed.json"))) { throw "kept data not recorded" }
if (Select-String -Quiet -SimpleMatch 'sample-token' (Join-Path $data "extensions/credentials.json")) { throw "credential not removed" }
if (-not (Select-String -Quiet -SimpleMatch '"greeting-style": "formal"' (Join-Path $data "extensions/settings.json"))) { throw "setting not kept" }
if (-not (Select-String -Quiet -SimpleMatch '"note": "Water the plants"' (Join-Path $data "extensions/content.json"))) { throw "note not kept" }
$process = Start-Pane "stderr-reinstall.log" @("--install", "target/guests/packages/sample-settings")
Send "{ENTER}"; Start-Sleep -Seconds 2   # Install; Greeting is selected
Send "{ENTER}"; Start-Sleep -Seconds 3   # open Greeting
Send "{DOWN 5}"
Send "{ENTER}"; Start-Sleep -Seconds 2   # "Show what Pane keeps"
Capture "51-reinstalled.png"
Check "51-reinstalled.png" "9fd8a8"   # "Style: formal · Note: Water the plants · Signed in: no ..."
python "$PSScriptRoot/check_screenshot.py" --distinct (Join-Path $OutDir "43-kept-after-clear.png") (Join-Path $OutDir "51-reinstalled.png")
if ($LASTEXITCODE -ne 0) { throw "screenshot check failed: the credential is still shown" }
Send "{ESC}"; Start-Sleep -Seconds 1
Stop-Pane $process
if (Select-String -Quiet -SimpleMatch '"retained"' (Join-Path $data "extensions/installed.json")) { throw "retained record not dropped" }

# Global hotkeys: in Manage extensions, the settings sample's command,
# Greeting, is given Ctrl+Alt+G by pressing it on its hotkey screen (its row
# follows the package's state, Reload, Clear cache and Uninstall rows).
# With Pane minimized, pressing the hotkey brings Pane's window to the front with
# Greeting open, also after a restart; once the extension is disabled,
# pressing it does nothing. A data folder of its own keeps the rows in a
# known order. (Screenshot 48 is the Linux smoke's opened quicklink.)
function Minimize-Pane($process) {
    [Win]::ShowWindow($process.MainWindowHandle, 6) | Out-Null   # SW_MINIMIZE
    Start-Sleep -Seconds 1
    if ([Win]::GetForegroundWindow() -eq $process.MainWindowHandle) { throw "Pane is still in front" }
}
function Press-Hotkey { Send "^%g"; Start-Sleep -Seconds 3 }
function Check-Pane-In-Front($process) {
    if ([Win]::GetForegroundWindow() -ne $process.MainWindowHandle) { throw "the hotkey did not bring Pane to the front" }
}
$data = Join-Path $OutDir "hotkeys-data"
if (Test-Path $data) { Remove-Item -Recurse -Force $data }
$env:PANE_DATA_DIR = $data
$process = Start-Pane "stderr-hotkeys.log" @("--install", "target/guests/packages/sample-settings")
Send "{ENTER}"; Start-Sleep -Seconds 2   # Install; Greeting is selected
Send "{DOWN 10}"   # Manage extensions…
Send "{ENTER}"; Start-Sleep -Seconds 1
Send "{DOWN 4}{ENTER}"; Start-Sleep -Seconds 1   # "Hotkey for Greeting"
Capture "52-hotkey-screen.png"
Check "52-hotkey-screen.png" "aab4c0"   # "Press the keys that should open Greeting ..."
Send "^%g"; Start-Sleep -Seconds 2
Capture "53-hotkey-assigned.png"
Check "53-hotkey-assigned.png" "9fd8a8"   # "Ctrl+Alt+G now opens Greeting"
Send "{ESC}"; Start-Sleep -Seconds 1   # root search
Minimize-Pane $process
Capture "54-unfocused.png"   # evidence only: Pane is not on screen
Press-Hotkey
Check-Pane-In-Front $process
Capture "55-hotkey-opened.png"
Check "55-hotkey-opened.png" "364355" 3000   # Greeting's first item, selected
$shots = "53-hotkey-assigned", "55-hotkey-opened" | ForEach-Object { Join-Path $OutDir "$_.png" }
python "$PSScriptRoot/check_screenshot.py" --distinct @shots
if ($LASTEXITCODE -ne 0) { throw "screenshot check failed: the hotkey opened nothing" }
Stop-Pane $process
if (-not (Select-String -Quiet -SimpleMatch '"ctrl+alt+g"' (Join-Path $data "extensions/hotkeys.json"))) { throw "hotkey not recorded" }
$process = Start-Pane "stderr-hotkeys-restart.log"
Minimize-Pane $process
Press-Hotkey
Check-Pane-In-Front $process
Capture "56-hotkey-after-restart.png"
Check "56-hotkey-after-restart.png" "364355" 3000   # Greeting's first item, selected
$shots = "53-hotkey-assigned", "56-hotkey-after-restart" | ForEach-Object { Join-Path $OutDir "$_.png" }
python "$PSScriptRoot/check_screenshot.py" --distinct @shots
if ($LASTEXITCODE -ne 0) { throw "screenshot check failed: the hotkey did not open Greeting after a restart" }
Send "{ESC}"; Start-Sleep -Seconds 1
Send "{DOWN 10}"   # Manage extensions…
Send "{ENTER}"; Start-Sleep -Seconds 1
Send "{ENTER}"; Start-Sleep -Seconds 2   # disable Settings sample
Send "{ESC}"; Start-Sleep -Seconds 1
Capture "57-disabled.png"   # root search
Minimize-Pane $process
Press-Hotkey
if ([Win]::GetForegroundWindow() -eq $process.MainWindowHandle) { throw "the released hotkey still brought Pane to the front" }
[Win]::ShowWindow($process.MainWindowHandle, 9) | Out-Null   # SW_RESTORE
Focus-Pane $process
Capture "58-disabled-pressed.png"   # still root search: nothing opened
$shots = "57-disabled", "58-disabled-pressed" | ForEach-Object { Join-Path $OutDir "$_.png" }
python "$PSScriptRoot/check_screenshot.py" --same @shots
if ($LASTEXITCODE -ne 0) { throw "screenshot check failed: the released hotkey still did something" }
Stop-Pane $process

# Pausing a broken extension: the settings sample's last item, Crash, crashes
# on purpose; the third crash within five minutes pauses the package and
# returns to root search, where Greeting stays listed with why it does not
# run. The pause holds after a restart. In Manage extensions, the package's
# "Why ... is paused" row (after its Reload and Retry rows) shows the
# details, whose only row, Retry, starts it again. A data folder of its own
# keeps the rows in a known order.
$data = Join-Path $OutDir "pausing-data"
if (Test-Path $data) { Remove-Item -Recurse -Force $data }
$env:PANE_DATA_DIR = $data
$process = Start-Pane "stderr-pausing.log" @("--install", "target/guests/packages/sample-settings")
Send "{ENTER}"; Start-Sleep -Seconds 2   # Install; Greeting is selected
Send "{ENTER}"; Start-Sleep -Seconds 2   # open Greeting
Send "{DOWN 7}"   # Crash
for ($i = 0; $i -lt 3; $i++) { Send "{ENTER}"; Start-Sleep -Seconds 2 }
Send "greet"; Start-Sleep -Seconds 1   # Greeting and its reason at the top on any window height
Capture "59-paused.png"
Check "59-paused.png" "f08c8c"   # "Settings sample crashed 3 times within 5 minutes and is paused ..."
Check "59-paused.png" "d6a36a"   # Greeting: "Settings sample is paused after an error; ..."
Stop-Pane $process
if (-not (Select-String -Quiet -SimpleMatch '"paused"' (Join-Path $data "extensions/installed.json"))) { throw "pause not recorded" }
$process = Start-Pane "stderr-pausing-restart.log"
Send "greet"; Start-Sleep -Seconds 1
Capture "60-paused-after-restart.png"
Check "60-paused-after-restart.png" "d6a36a"   # Greeting is still paused
Send "{ESC}"; Start-Sleep -Seconds 1   # clears the query
Send "{DOWN 10}"   # Manage extensions…
Send "{ENTER}"; Start-Sleep -Seconds 1
Send "{DOWN 3}{ENTER}"; Start-Sleep -Seconds 1   # "Why Settings sample is paused"
Capture "61-pause-details.png"
Check "61-pause-details.png" "aab4c0"   # the details
Send "{ENTER}"; Start-Sleep -Seconds 2   # Retry Settings sample
Capture "62-pause-retried.png"
Check "62-pause-retried.png" "9fd8a8"   # "Started Settings sample"
$shots = "61-pause-details", "62-pause-retried" | ForEach-Object { Join-Path $OutDir "$_.png" }
python "$PSScriptRoot/check_screenshot.py" --distinct @shots
if ($LASTEXITCODE -ne 0) { throw "screenshot check failed: Retry changed nothing" }
Stop-Pane $process
if (Select-String -Quiet -SimpleMatch '"paused"' (Join-Path $data "extensions/installed.json")) { throw "pause not cleared" }

# Delete retained data: with a data folder of its own, the settings sample
# saves a note and is uninstalled keeping it (its Uninstall row follows its
# state, Reload and Clear cache rows); its retained data, the extension list's
# last row, is deleted after confirming (Cancel is selected first, so Down
# then Enter), without the extension. Installing the same folder again finds
# nothing. Steps that change Pane's files wait for the change instead of a
# fixed time.
$data = Join-Path $OutDir "retained-data"
if (Test-Path $data) { Remove-Item -Recurse -Force $data }
$env:PANE_DATA_DIR = $data
# Waits until $file contains $text ($present) or no longer does (-not $present).
function Wait-For($file, $text, [bool]$present) {
    for ($i = 0; $i -lt 100; $i++) {
        $found = (Test-Path $file) -and (Select-String -Quiet -SimpleMatch $text $file)
        if ($found -eq $present) { return }
        Start-Sleep -Milliseconds 100
    }
    throw "${file}: $text is not $(if ($present) { 'present' } else { 'absent' })"
}
$registry = Join-Path $data "extensions/installed.json"
$process = Start-Pane "stderr-retained.log" @("--install", "target/guests/packages/sample-settings")
Send "{ENTER}"   # Install; Greeting is selected
Wait-For $registry "sample-settings" $true; Start-Sleep -Seconds 1
Send "{ENTER}"; Start-Sleep -Seconds 3   # open Greeting
Send "{DOWN 3}"
Send "{ENTER}"   # "Save a note"
Wait-For (Join-Path $data "extensions/content.json") '"note": "Water the plants"' $true
Send "{ESC}"; Start-Sleep -Seconds 1   # root search
Send "{DOWN 10}"   # Manage extensions…
Send "{ENTER}"; Start-Sleep -Seconds 1
Send "{DOWN 3}{ENTER}"; Start-Sleep -Seconds 1   # "Uninstall Settings sample"
Send "{ENTER}"   # "Uninstall and keep saved data"
Wait-For $registry '"retained"' $true; Start-Sleep -Seconds 1
Send "{DOWN 40}"
Send "{ENTER}"; Start-Sleep -Seconds 1   # "Delete retained data of Settings sample"
Capture "63-confirm-delete-retained.png"
Check "63-confirm-delete-retained.png" "aab4c0"   # what is kept and what is not touched
Send "{DOWN}{ENTER}"   # "Delete retained data"
Wait-For $registry '"retained"' $false; Start-Sleep -Seconds 1
Capture "64-retained-deleted.png"
Check "64-retained-deleted.png" "9fd8a8"   # "Deleted the retained data of Settings sample"
Stop-Pane $process
if (Select-String -Quiet -SimpleMatch 'Water the plants' (Join-Path $data "extensions/content.json")) { throw "note not deleted" }
$process = Start-Pane "stderr-reinstall-empty.log" @("--install", "target/guests/packages/sample-settings")
Send "{ENTER}"   # Install; Greeting is selected
Wait-For $registry "sample-settings" $true; Start-Sleep -Seconds 1
Send "{ENTER}"; Start-Sleep -Seconds 3   # open Greeting
Send "{DOWN 5}"
Send "{ENTER}"; Start-Sleep -Seconds 2   # "Show what Pane keeps"
Capture "65-reinstalled-empty.png"
Check "65-reinstalled-empty.png" "9fd8a8"   # "Style: none · Note: none · Signed in: no ..."
python "$PSScriptRoot/check_screenshot.py" --distinct (Join-Path $OutDir "51-reinstalled.png") (Join-Path $OutDir "65-reinstalled-empty.png")
if ($LASTEXITCODE -ne 0) { throw "screenshot check failed: the deleted data is still shown" }
Send "{ESC}"; Start-Sleep -Seconds 1
Stop-Pane $process

# Aliases and fallbacks: in Manage extensions, the query sample's command,
# Echo, is given the alias "ec" (its row follows the package's state, Reload,
# Clear cache, Uninstall and hotkey rows) and made a fallback (the next row).
# In root search, "ec hello" lists the row that sends "hello" to Echo,
# selected, and Enter shows Echo's answer; text nothing matches lists "No
# results" with Echo below it, not selected, until Down selects it and Enter
# sends the text. After a restart with the extension disabled, "ec hello"
# lists nothing: the same screen as a Pane with nothing installed. Data
# folders of their own keep the rows in a known order.
$data = Join-Path $OutDir "aliases-data"
if (Test-Path $data) { Remove-Item -Recurse -Force $data }
$env:PANE_DATA_DIR = $data
$process = Start-Pane "stderr-aliases.log" @("--install", "target/guests/packages/sample-query")
Send "{ENTER}"; Start-Sleep -Seconds 2   # Install; Echo is selected
Send "{DOWN 10}"   # Manage extensions…
Send "{ENTER}"; Start-Sleep -Seconds 1
Send "{DOWN 5}{ENTER}"; Start-Sleep -Seconds 1   # "Alias for Echo"
Send "ec"
Send "{ENTER}"; Start-Sleep -Seconds 2
Capture "66-alias-saved.png"
Check "66-alias-saved.png" "9fd8a8"   # "Typing “ec” now finds Echo"
Send "{DOWN}{ENTER}"; Start-Sleep -Seconds 2   # "Fallback: Echo"
Capture "67-fallback-on.png"
Check "67-fallback-on.png" "9fd8a8"   # "Echo is now offered for any text typed in root search"
$shots = "66-alias-saved", "67-fallback-on" | ForEach-Object { Join-Path $OutDir "$_.png" }
python "$PSScriptRoot/check_screenshot.py" --distinct @shots
if ($LASTEXITCODE -ne 0) { throw "screenshot check failed: the fallback row changed nothing" }
Send "{ESC}"; Start-Sleep -Seconds 1   # root search
Send "ec hello"; Start-Sleep -Seconds 1
Capture "68-alias-row.png"
Check "68-alias-row.png" "364355" 3000   # Echo, sending “hello”, selected
Send "{ENTER}"; Start-Sleep -Seconds 3
Capture "69-alias-answer.png"
Check "69-alias-answer.png" "9fd8a8"   # "Echo heard “hello”"
Send "{ESC}"; Start-Sleep -Seconds 1   # clears the query
Send "zqx"; Start-Sleep -Seconds 1
Capture "70-fallback-listed.png"   # "No results for “zqx”", then Echo, not selected
Send "{DOWN}"; Start-Sleep -Seconds 1
Capture "71-fallback-chosen.png"
Check "71-fallback-chosen.png" "364355" 3000   # Echo, now selected
$shots = "70-fallback-listed", "71-fallback-chosen" | ForEach-Object { Join-Path $OutDir "$_.png" }
python "$PSScriptRoot/check_screenshot.py" --distinct @shots
if ($LASTEXITCODE -ne 0) { throw "screenshot check failed: Down did not select the fallback" }
Send "{ENTER}"; Start-Sleep -Seconds 3
Capture "72-fallback-answer.png"
Check "72-fallback-answer.png" "9fd8a8"   # "Echo heard “zqx”"
$shots = "69-alias-answer", "72-fallback-answer" | ForEach-Object { Join-Path $OutDir "$_.png" }
python "$PSScriptRoot/check_screenshot.py" --distinct @shots
if ($LASTEXITCODE -ne 0) { throw "screenshot check failed: the fallback got the alias's text" }
Stop-Pane $process
$aliases = Join-Path $data "extensions/aliases.json"
if (-not (Select-String -Quiet -SimpleMatch '"ec"' $aliases)) { throw "alias not recorded" }
if (-not (Select-String -Quiet -SimpleMatch '#echo"' $aliases)) { throw "fallback not recorded" }
$process = Start-Pane "stderr-aliases-restart.log"
Send "{DOWN 10}"   # Manage extensions…
Send "{ENTER}"; Start-Sleep -Seconds 1
Send "{ENTER}"; Start-Sleep -Seconds 2   # disable Query sample
Send "{ESC}"; Start-Sleep -Seconds 1
Send "ec hello"; Start-Sleep -Seconds 1
Capture "73-alias-disabled.png"   # "No results for “ec hello”"
Stop-Pane $process
if (-not (Select-String -Quiet -SimpleMatch '"disabled": true' (Join-Path $data "extensions/installed.json"))) { throw "not disabled" }
$data = Join-Path $OutDir "aliases-empty-data"
if (Test-Path $data) { Remove-Item -Recurse -Force $data }
$env:PANE_DATA_DIR = $data
$process = Start-Pane "stderr-aliases-empty.log"
Send "ec hello"; Start-Sleep -Seconds 1
Capture "74-nothing-installed.png"   # "No results for “ec hello”"
python "$PSScriptRoot/check_screenshot.py" --same (Join-Path $OutDir "73-alias-disabled.png") (Join-Path $OutDir "74-nothing-installed.png")
if ($LASTEXITCODE -ne 0) { throw "screenshot check failed: a disabled extension's alias still lists a row" }
Stop-Pane $process

# Dependencies: the dependencies sample requires the JavaScript operations
# sample (from ../sample-operations-js) and can use the Rust one, which is
# optional. Its preview lists both; Install installs it with the JavaScript
# sample only, and its command (selected once installed) calls that
# package's greet operation by its dependency id: "Hello, Pane, from
# JavaScript" comes from the other package's guest. A data folder of its own
# starts with nothing installed.
$data = Join-Path $OutDir "dependencies-data"
if (Test-Path $data) { Remove-Item -Recurse -Force $data }
$env:PANE_DATA_DIR = $data
$process = Start-Pane "stderr-dependencies.log" @("--install", "target/guests/packages/sample-dependencies")
Capture "75-dependencies-preview.png"
Check "75-dependencies-preview.png" "aab4c0"   # "Requires: JavaScript operations sample, installed with it ..."
Send "{ENTER}"; Start-Sleep -Seconds 3   # Install; Greet through dependencies is selected
Capture "76-dependencies-installed.png"
Check "76-dependencies-installed.png" "9fd8a8"   # "Installed Dependencies sample with JavaScript operations sample, which it requires"
Send "{ENTER}"; Start-Sleep -Seconds 3   # open Greet through dependencies
Send "{ENTER}"; Start-Sleep -Seconds 5   # Greet through the required greeter
Capture "77-dependency-answer.png"
Check "77-dependency-answer.png" "9fd8a8"   # the JavaScript guest's answer
$shots = "75-dependencies-preview", "76-dependencies-installed", "77-dependency-answer" | ForEach-Object { Join-Path $OutDir "$_.png" }
python "$PSScriptRoot/check_screenshot.py" --distinct @shots
if ($LASTEXITCODE -ne 0) { throw "screenshot check failed: installing with dependencies changed nothing" }
Stop-Pane $process
$record = Join-Path $data "extensions/installed.json"
if (-not (Select-String -Quiet -SimpleMatch '"id": "greeter"' $record)) { throw "dependency not recorded" }
if ((Select-String -SimpleMatch '"dir"' $record).Count -ne 2) { throw "not exactly two packages installed" }

# Native helpers: the helper sample's command runs pane-echo, the file its
# package ships for this system (built by `cargo xtask guests`). Its first
# item shows the helper's answer, naming the system; its third races the
# helper against a one-second timer and cancels it. Its second has the
# helper wait ten seconds: disabling the package meanwhile (its row is the
# first in Manage extensions) ends the helper's process at once, and the
# note it saved before is kept. A data folder of its own keeps the rows in a
# known order; the helper runs from its managed copy there.
$data = Join-Path $OutDir "helper-data"
if (Test-Path $data) { Remove-Item -Recurse -Force $data }
$env:PANE_DATA_DIR = $data
$packages = [System.IO.Path]::GetFullPath((Join-Path $data "extensions/packages"))
# Pane's helper processes: pane-echo run from this data folder.
function Helpers-Running {
    [bool](Get-Process -Name "pane-echo" -ErrorAction SilentlyContinue |
        Where-Object { $_.Path -and $_.Path.StartsWith($packages, [System.StringComparison]::OrdinalIgnoreCase) })
}
$process = Start-Pane "stderr-helper.log" @("--install", "target/guests/packages/sample-helper")
Send "{ENTER}"; Start-Sleep -Seconds 2   # Install; Helper sample is selected
Send "{ENTER}"; Start-Sleep -Seconds 2   # open Helper sample
Send "{ENTER}"; Start-Sleep -Seconds 2   # Echo through the helper
Capture "90-helper-echoed.png"
Check "90-helper-echoed.png" "9fd8a8"   # 'Echoed "hello from Pane" on Windows x86-64'
Send "{DOWN 2}{ENTER}"; Start-Sleep -Seconds 3   # Echo within a second
Capture "91-helper-cancelled.png"
Check "91-helper-cancelled.png" "9fd8a8"   # "Stopped the helper after one second"
$shots = "90-helper-echoed", "91-helper-cancelled" | ForEach-Object { Join-Path $OutDir "$_.png" }
python "$PSScriptRoot/check_screenshot.py" --distinct @shots
if ($LASTEXITCODE -ne 0) { throw "screenshot check failed: the helper's answers look the same" }
if (Helpers-Running) { throw "a cancelled helper is still running" }
Send "{UP}{ENTER}"; Start-Sleep -Seconds 2   # Echo after waiting
if (-not (Helpers-Running)) { throw "the waiting helper is not running" }
Capture "92-helper-waiting.png"
Send "{ESC}"; Start-Sleep -Seconds 1   # root search; the helper keeps running
Send "{DOWN 10}"   # Manage extensions…
Send "{ENTER}"; Start-Sleep -Seconds 1
Send "{ENTER}"; Start-Sleep -Seconds 2   # disable Helper sample
Capture "93-helper-disabled.png"
Check "93-helper-disabled.png" "9fd8a8"   # "Disabled Helper sample"
if (Helpers-Running) { throw "the helper outlived its disabled package" }
$settings = Join-Path $data "extensions/settings.json"
if (-not (Select-String -Quiet -SimpleMatch '"helper-wait": "started"' $settings)) { throw "saved note lost" }
if (Select-String -Quiet -SimpleMatch '"helper-wait": "finished"' $settings) { throw "the stopped call finished" }
Stop-Pane $process
if (Helpers-Running) { throw "a helper outlived Pane" }

# Quitting Pane while a helper runs ends it: with "Echo after waiting"
# running (the helper beats in pane-echo.alive in its folder of the managed
# copy), closing Pane's window (WM_CLOSE, as its close button does) quits
# Pane, which ends the helper first. A data folder of its own again.
$data = Join-Path $OutDir "helper-quit-data"
if (Test-Path $data) { Remove-Item -Recurse -Force $data }
$env:PANE_DATA_DIR = $data
$packages = [System.IO.Path]::GetFullPath((Join-Path $data "extensions/packages"))
$process = Start-Pane "stderr-helper-quit.log" @("--install", "target/guests/packages/sample-helper")
Send "{ENTER}"; Start-Sleep -Seconds 2   # Install; Helper sample is selected
Send "{ENTER}"; Start-Sleep -Seconds 2   # open Helper sample
Send "{DOWN}{ENTER}"; Start-Sleep -Seconds 2   # Echo after waiting
if (-not (Helpers-Running)) { throw "the waiting helper is not running" }
Capture "94-helper-before-quit.png"
Check "94-helper-before-quit.png" "d6c27a"   # "Running…"
$alive = Get-ChildItem -Recurse -Filter "pane-echo.alive" $packages | Select-Object -First 1
if (-not $alive) { throw "the waiting helper does not beat" }
if (-not $process.CloseMainWindow()) { throw "Pane's window did not take the close request" }
if (-not $process.WaitForExit(5000)) { throw "Pane did not quit when its window closed" }
if (Helpers-Running) { throw "a helper outlived Pane quitting" }
$beats = (Get-Item $alive.FullName).Length; Start-Sleep -Milliseconds 500
if ((Get-Item $alive.FullName).Length -ne $beats) { throw "the helper still beats after Pane quit" }

# Development mode (#12, #13): a copy of each development sample
# (guests/hello-rust, hello-ts, hello-js) is built once, installed and
# developed from Manage extensions ("Develop <title>", its last row). Saving
# an edit of its greeting builds it with the documented command and reloads
# it while Pane keeps running; a save that does not build keeps the working
# code and shows the error; two saves in a row (the second while the first
# builds) end with the newer greeting; after "Stop developing", a save builds
# nothing. Each sample has a data folder of its own, so root lists the three
# built-in samples, then its command, the install and Manage extensions
# rows. The JavaScript and TypeScript samples need the JS toolchain
# (guests/README.md) and are skipped without it.
function Set-Greeting($path, $line) {
    $text = [IO.File]::ReadAllText($path)
    $evaluator = [Text.RegularExpressions.MatchEvaluator] { param($match) $line }
    $text = ([regex]'(?m)^const GREETING[^\r\n]*').Replace($text, $evaluator, 1)
    [IO.File]::WriteAllText($path, $text)
}
function Same-File($a, $b) {
    (Test-Path $a) -and (Test-Path $b) -and ((Get-FileHash $a).Hash -eq (Get-FileHash $b).Hash)
}
# Waits until Pane has reloaded a new build: the component built in the
# copy differs from $before (the one before the save) and the managed copy is it.
function Wait-Reloaded($built, $before) {
    for ($i = 0; $i -lt 600; $i++) {
        $managed = Get-ChildItem -Recurse -File -Filter (Split-Path -Leaf $built) (Join-Path $env:PANE_DATA_DIR "extensions/packages") -ErrorAction SilentlyContinue | Select-Object -First 1
        if ($managed -and -not (Same-File $built $before) -and (Same-File $built $managed.FullName)) {
            Start-Sleep -Seconds 3; return
        }
        Start-Sleep -Milliseconds 500
    }
    throw "Pane did not reload $built"
}
function Failures($log) {
    @(Select-String -SimpleMatch "did not build" (Join-Path $OutDir $log) -ErrorAction SilentlyContinue).Count
}
# Waits until Pane has reported one more build that did not build.
function Wait-Failed($log, $before) {
    for ($i = 0; $i -lt 600; $i++) {
        if ((Failures $log) -gt $before) { Start-Sleep -Seconds 1; return }
        Start-Sleep -Milliseconds 500
    }
    throw "Pane did not report the failed build"
}
# From root: open the developed command, the 4th row, and run its item.
function Say-Hello {
    Send "{DOWN 3}{ENTER}"; Start-Sleep -Seconds 3
    Send "{ENTER}"; Start-Sleep -Seconds 2
}
function Shots-Differ($first, $second, $what) {
    python "$PSScriptRoot/check_screenshot.py" --distinct (Join-Path $OutDir $first) (Join-Path $OutDir $second)
    if ($LASTEXITCODE -ne 0) { throw "screenshot check failed: $what" }
}
function Develop-Sample($sample, $title, $component, $source, $n, $greeting, $broken) {
    $data = Join-Path $OutDir "develop-$sample-data"
    if (Test-Path $data) { Remove-Item -Recurse -Force $data }
    $env:PANE_DATA_DIR = $data
    $copy = Join-Path $OutDir "develop-$sample"
    if (Test-Path $copy) { Remove-Item -Recurse -Force $copy }
    New-Item -ItemType Directory -Force -Path $copy | Out-Null
    Get-ChildItem "guests/$sample" -Exclude target, dist, node_modules | Copy-Item -Destination $copy -Recurse
    if (Test-Path (Join-Path $copy "Cargo.toml")) {
        Copy-Item rust-toolchain.toml $copy
        $guest = (Resolve-Path "guests/pane-guest").Path -replace '\\', '/'
        $manifest = Join-Path $copy "Cargo.toml"
        $text = [IO.File]::ReadAllText($manifest).Replace('path = "../pane-guest"', "path = '$guest'")
        [IO.File]::WriteAllText($manifest, $text)
        Push-Location $copy
        cargo build --release --target wasm32-wasip2 --quiet
        $built = $LASTEXITCODE
        Pop-Location
        if ($built -ne 0) { throw "$title did not build" }
    } else {
        python tools/componentize-js/pane_js.py build $copy (Join-Path $copy $component) | Out-Null
        if ($LASTEXITCODE -ne 0) { throw "$title did not build" }
    }
    $built = Join-Path $copy $component
    $before = Join-Path $OutDir "develop-$sample-before.wasm"
    $log = "stderr-develop-$sample.log"
    $process = Start-Pane $log @("--install", $copy)
    Send "{ENTER}"; Start-Sleep -Seconds 2   # Install
    Send "{DOWN 10}{ENTER}"; Start-Sleep -Seconds 1   # Manage extensions
    Send "{DOWN 10}{ENTER}"; Start-Sleep -Seconds 2   # Develop <title>
    Capture "$n-$sample-develop-started.png"
    Check "$n-$sample-develop-started.png" "9fd8a8"   # "Developing <title>: each save in ..."
    Send "{ESC}"; Start-Sleep -Seconds 1
    Say-Hello
    Capture "$($n + 1)-$sample-greeting-before.png"
    Check "$($n + 1)-$sample-greeting-before.png" "9fd8a8"   # "Hello from ..."
    Send "{ESC}"; Start-Sleep -Seconds 1

    # An edit, saved: built and reloaded.
    Copy-Item -Force $built $before
    Set-Greeting (Join-Path $copy $source) ($greeting -f "Hello again")
    Wait-Reloaded $built $before
    Capture "$($n + 2)-$sample-rebuilt.png"
    Check "$($n + 2)-$sample-rebuilt.png" "9fd8a8"   # "Reloaded <title>"
    Say-Hello
    Capture "$($n + 3)-$sample-greeting-after.png"
    Check "$($n + 3)-$sample-greeting-after.png" "9fd8a8"   # "Hello again"
    Shots-Differ "$($n + 1)-$sample-greeting-before.png" "$($n + 3)-$sample-greeting-after.png" "the edit changed nothing"
    Send "{ESC}"; Start-Sleep -Seconds 1

    # A save that does not build: the working code stays.
    $failures = Failures $log
    Set-Greeting (Join-Path $copy $source) $broken
    Wait-Failed $log $failures
    Capture "$($n + 4)-$sample-build-failed.png"
    Check "$($n + 4)-$sample-build-failed.png" "f08c8c"   # "<title> did not build: ..."
    Say-Hello
    Capture "$($n + 5)-$sample-kept.png"
    Check "$($n + 5)-$sample-kept.png" "9fd8a8"   # still "Hello again"
    $shots = "$($n + 3)-$sample-greeting-after", "$($n + 5)-$sample-kept" | ForEach-Object { Join-Path $OutDir "$_.png" }
    python "$PSScriptRoot/check_screenshot.py" --same @shots
    if ($LASTEXITCODE -ne 0) { throw "screenshot check failed: the failed build replaced the code" }
    Send "{ESC}"; Start-Sleep -Seconds 1

    # Two saves, the second while the first builds: the newer one is reloaded.
    Copy-Item -Force $built $before
    Set-Greeting (Join-Path $copy $source) ($greeting -f "Hello once more")
    Start-Sleep -Milliseconds 500
    Set-Greeting (Join-Path $copy $source) ($greeting -f "Hello at last")
    Wait-Reloaded $built $before
    Capture "$($n + 6)-$sample-rebuilt-again.png"
    Check "$($n + 6)-$sample-rebuilt-again.png" "9fd8a8"   # "Reloaded <title>"
    Say-Hello
    Capture "$($n + 7)-$sample-greeting-fixed.png"
    Check "$($n + 7)-$sample-greeting-fixed.png" "9fd8a8"   # "Hello at last"
    Shots-Differ "$($n + 3)-$sample-greeting-after.png" "$($n + 7)-$sample-greeting-fixed.png" "the fix changed nothing"
    Send "{ESC}"; Start-Sleep -Seconds 1

    # Stopped: a save builds nothing.
    Send "{DOWN 10}{ENTER}"; Start-Sleep -Seconds 1   # Manage extensions
    Send "{DOWN 10}{ENTER}"; Start-Sleep -Seconds 2   # Stop developing <title>
    Capture "$($n + 8)-$sample-stopped.png"
    Check "$($n + 8)-$sample-stopped.png" "9fd8a8"   # "Stopped developing <title>"
    Copy-Item -Force $built $before
    Set-Greeting (Join-Path $copy $source) ($greeting -f "Hello unseen")
    Start-Sleep -Seconds 8
    if (-not (Same-File $built $before)) { throw "$title was built after development stopped" }
    Stop-Pane $process
}
Develop-Sample "hello-rust" "Hello Rust" "target/wasm32-wasip2/release/hello_rust.wasm" "src/lib.rs" 110 `
    'const GREETING: &str = "{0} from Rust";' 'const GREETING: &str = 42;'
$jsToolchain = if ($env:PANE_JS_TOOLCHAIN_DIR) { $env:PANE_JS_TOOLCHAIN_DIR } else { Join-Path $env:LOCALAPPDATA "pane/componentize-js" }
if ((Test-Path (Join-Path $jsToolchain "bin/*/toolchain.json")) -and (Get-Command node -ErrorAction SilentlyContinue)) {
    Develop-Sample "hello-ts" "Hello TypeScript" "dist/hello_ts.wasm" "src/index.ts" 119 `
        'const GREETING: string = "{0} from TypeScript";' 'const GREETING: string = 42;'
    Develop-Sample "hello-js" "Hello JavaScript" "dist/hello_js.wasm" "src/index.js" 128 `
        'const GREETING = "{0} from JavaScript";' 'const GREETING = 42;'
} else {
    Write-Output "skipped the JavaScript and TypeScript development smoke: no JS toolchain in $jsToolchain"
}

# Disabling a required dependency: installed with the dependencies sample
# (whose install and data folder are this phase's own), the JavaScript
# operations sample is the first row of Manage extensions. Enter asks first,
# listing the Dependencies sample, which requires it, with Disable all and
# Cancel; Cancel changes nothing, Disable all disables both, and Enter again
# enables the JavaScript operations sample alone: the Dependencies sample
# stays disabled, on record too.
$data = Join-Path $OutDir "disable-dependents-data"
if (Test-Path $data) { Remove-Item -Recurse -Force $data }
$env:PANE_DATA_DIR = $data
$process = Start-Pane "stderr-disable-dependents.log" @("--install", "target/guests/packages/sample-dependencies")
Send "{ENTER}"; Start-Sleep -Seconds 3   # Install
for ($i = 0; $i -lt 10; $i++) { Send "{DOWN}" }   # Manage extensions...
Send "{ENTER}"; Start-Sleep -Seconds 1
Send "{ENTER}"; Start-Sleep -Seconds 1   # disable JavaScript operations sample: asks first
Capture "140-disable-dependents-asked.png"
Check "140-disable-dependents-asked.png" "aab4c0"   # "Dependencies sample, which requires JavaScript operations sample ..."
Send "{DOWN}{ENTER}"; Start-Sleep -Seconds 1   # Cancel
Capture "141-disable-dependents-cancelled.png"   # both still enabled
Send "{ENTER}"; Start-Sleep -Seconds 1   # asks again
Send "{ENTER}"; Start-Sleep -Seconds 2   # Disable all 2
Capture "142-disable-dependents-disabled.png"
Check "142-disable-dependents-disabled.png" "9fd8a8"   # "Disabled JavaScript operations sample and Dependencies sample, which requires it"
Send "{ENTER}"; Start-Sleep -Seconds 2   # enable JavaScript operations sample
Capture "143-disable-dependents-enabled-alone.png"
Check "143-disable-dependents-enabled-alone.png" "9fd8a8"   # "Enabled JavaScript operations sample"; Dependencies sample stays disabled
$shots = "140-disable-dependents-asked", "141-disable-dependents-cancelled", "142-disable-dependents-disabled", "143-disable-dependents-enabled-alone" | ForEach-Object { Join-Path $OutDir "$_.png" }
python "$PSScriptRoot/check_screenshot.py" --distinct @shots
if ($LASTEXITCODE -ne 0) { throw "screenshot check failed: disabling with dependents changed nothing" }
Stop-Pane $process
$record = Join-Path $data "extensions/installed.json"
if ((Select-String -SimpleMatch '"disabled": true' $record).Count -ne 1) { throw "not exactly the dependent left disabled" }
Write-Output "screenshots in $OutDir"
