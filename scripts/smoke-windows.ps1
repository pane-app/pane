# Native GUI smoke on Windows: launches Pane, drives it with real key events
# and captures the screen. Pane keeps installed packages in <output-dir>\data,
# not the user's data folder.
# Usage: scripts/smoke-windows.ps1 -OutDir <output-dir>
param([string]$OutDir = "smoke")
$ErrorActionPreference = "Stop"
# Behavior captures use a fixed palette without desktop-dependent glass.
$env:PANE_THEME = "dark"
$env:PANE_MATERIAL = "opaque"
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$data = Join-Path $OutDir "data"
if (Test-Path $data) { Remove-Item -Recurse -Force $data }
$env:PANE_DATA_DIR = $data
# The tested OS version and architecture
"$([System.Environment]::OSVersion.VersionString) $env:PROCESSOR_ARCHITECTURE" | Set-Content (Join-Path $OutDir "system.txt")
'theme=dark material=opaque (behavior smoke; not blur evidence)' | Add-Content (Join-Path $OutDir "system.txt")
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type @"
using System; using System.Runtime.InteropServices;
public static class Win {
    [StructLayout(LayoutKind.Sequential)] public struct Rect { public int Left, Top, Right, Bottom; }
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out Rect rect);
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
    # Keep hover deterministic without changing focus or selection. The color
    # picker click leaves the pointer over a later result row; restoring the
    # window refreshes that hover even if keyboard navigation had cleared it.
    # Park inside our foreground window's header, never over a result. Leave
    # unfocused/minimized evidence captures alone (notably the hotkey checks).
    if ($process -and [Win]::GetForegroundWindow() -eq $process.MainWindowHandle) {
        $rect = New-Object Win+Rect
        if (-not [Win]::GetWindowRect($process.MainWindowHandle, [ref]$rect)) { throw "cannot locate Pane for capture" }
        if (-not [Win]::SetCursorPos($rect.Left + 32, $rect.Top + 32)) { throw "cannot park pointer for capture" }
        Start-Sleep -Milliseconds 150   # let the pointer-leave repaint complete
    }
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
# Opens Manage extensions from root search. A blind run of Downs to root's
# end was the way in until #72's Settings… root result made itself last of
# all (it is listed whatever is installed, so every phase's root ends with
# it): the run now opens the Settings window instead. Searching for the row
# by its title is order-proof: "manage" matches only the Manage extensions…
# row, which is selected when the list narrows to it, and Enter opens it.
# ^A first, so a query an earlier step left in the field is replaced, not
# extended.
function Manage-Extensions {
    Send "^a"; Send "manage"; Start-Sleep -Seconds 1
    Send "{ENTER}"; Start-Sleep -Seconds 1
}
# Brings Pane's window to the front, so that key events reach it.
function Focus-Pane($process) {
    [Win]::SetForegroundWindow($process.MainWindowHandle) | Out-Null
    Start-Sleep -Milliseconds 500
}
# Starts Pane with the given arguments, writing its errors to $log, and
# brings its window to the front. The program is the smoke's own debug
# build unless one is given (the installed Pane of the #51 phase).
function Start-Pane($log, [string[]]$arguments, $program = "target/debug/pane.exe") {
    $options = @{
        FilePath = $program
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
    # A --install preview arrives once its check finishes; until then the
    # screen is root search. Wait for preview metadata below its heading, then
    # send the phase's first Enter to the preview:
    # run 36796103906's Linux frame 31 lost that race (the check outlasted
    # the wait and the Enter opened root search's own first row instead).
    if ($arguments -and $arguments[0] -eq "--install") {
        $previewShown = $false
        for ($i = 0; $i -lt 60; $i++) {
            Capture "preview-wait.png"
            # $null swallows the checker's output: a function's return value
            # is everything it writes, and the process object Stop-Pane waits
            # on must not be followed by the checker's lines (run 36802787026
            # failed its first Stop-Pane on a string's WaitForExit).
            $null = python "$PSScriptRoot/check_screenshot.py" --preview (Join-Path $OutDir "preview-wait.png")
            if ($LASTEXITCODE -eq 0) { $previewShown = $true; break }
            Start-Sleep -Milliseconds 500
        }
        if (-not $previewShown) { throw "the --install preview did not appear (still root search)" }
    }
    return $process
}
function Stop-Pane($process) {
    if ($process.HasExited) { throw "Pane exited during the smoke" }
    Stop-Process -Id $process.Id
    $process.WaitForExit()
}

$process = Start-Pane "stderr.log"
Capture "1-root.png"
Check "1-root.png" "hint"   # the hint line: text renders
# Open each sample command (Rust, JavaScript, TypeScript) and run an item.
foreach ($index in 0..2) {
    for ($i = 0; $i -lt $index; $i++) { Send "{DOWN}" }
    Send "{ENTER}"; Start-Sleep -Seconds 3
    Capture "$($index + 2)-command-$index.png"
    Send "{DOWN}"; Start-Sleep -Milliseconds 120; Send "{ENTER}"; Start-Sleep -Seconds 2
    Capture "$($index + 2)-result-$index.png"
    Check "$($index + 2)-result-$index.png" "success"   # the guest's answer
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
Check "7-form-error.png" "error"   # the rejected field's message
Send "Ada{TAB}{DOWN}{ENTER}"; Start-Sleep -Seconds 2
Capture "8-form-result.png"
Check "8-form-result.png" "success"   # the guest's answer
Send "{ESC}{ESC}"; Start-Sleep -Seconds 1
Stop-Pane $process

# Install the assembled Rust sample package (the folder the picker would
# return), then run its command. Root lists the three samples, the installed
# command, then the install and Manage extensions rows.
$process = Start-Pane "stderr-install.log" @("--install", "target/guests/packages/sample-rust")
Capture "9-package.png"
Check "9-package.png" "details"   # the package's identity and compatibility lines
Send "{ENTER}"; Start-Sleep -Seconds 2
Capture "10-installed.png"
Check "10-installed.png" "success"   # "Installed Rust sample"
Send "{ENTER}"; Start-Sleep -Seconds 3
Send "{ENTER}"; Start-Sleep -Seconds 2
Capture "11-installed-result.png"
Check "11-installed-result.png" "success"   # the installed guest's answer
Stop-Pane $process

# The installed command is still listed after a restart.
$process = Start-Pane "stderr-restart.log"
Capture "12-restarted.png"
Check "12-restarted.png" "hint"
if (-not (Test-Path (Join-Path $data "extensions/installed.json"))) { throw "no install record" }
Focus-Pane $process

# The Rust command's seventh item is declared for Windows only, its eighth
# for macOS and Linux only. Here the first runs and the second is explained
# without running.
Send "{ENTER}"; Start-Sleep -Seconds 3
Send "{DOWN}{DOWN}{DOWN}{DOWN}{DOWN}{DOWN}{ENTER}"; Start-Sleep -Seconds 2
Capture "13-windows-only.png"
Check "13-windows-only.png" "success"   # Windows: the guest's answer
Send "{DOWN}"; Start-Sleep -Milliseconds 120; Send "{ENTER}"; Start-Sleep -Seconds 2
Capture "14-not-windows.png"
Check "14-not-windows.png" "warning"   # the row's reason
Check "14-not-windows.png" "error"   # Windows: the reason as the error
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
Check "15-no-compatible-package.png" "error"   # "Not available on Windows: ..."
Stop-Pane $process

# Install the settings sample, save a choice with it, then disable it in
# Manage extensions. Root lists the three samples, Rust sample, Greeting, the
# install rows, then Manage extensions… and Settings… last; the extension
# list holds Rust sample, then Settings sample.
$process = Start-Pane "stderr-settings.log" @("--install", "target/guests/packages/sample-settings")
Send "{ENTER}"; Start-Sleep -Seconds 2   # Install; Greeting is selected
Send "{ENTER}"; Start-Sleep -Seconds 3   # open Greeting
Send "{ENTER}"; Start-Sleep -Seconds 2   # "Use a formal greeting"
Capture "16-setting-saved.png"
Check "16-setting-saved.png" "success"   # "Saved the formal greeting"
Send "{ESC}"; Start-Sleep -Seconds 1
Manage-Extensions
Send "{DOWN}"; Start-Sleep -Milliseconds 120; Send "{ENTER}"; Start-Sleep -Seconds 2
Capture "17-disabled.png"
Check "17-disabled.png" "success"   # "Disabled Settings sample"
Stop-Pane $process
if (-not (Select-String -Quiet -SimpleMatch '"disabled": true' (Join-Path $data "extensions/installed.json"))) { throw "disabled state not recorded" }
if (-not (Select-String -Quiet -SimpleMatch '"greeting-style": "formal"' (Join-Path $data "extensions/settings.json"))) { throw "setting not saved" }

# After a restart Greeting is no longer in root search: root looks exactly as
# it did before the settings sample was installed. Enabling the package again
# brings it back with its setting: "Greet me" answers in the saved formal
# style, where without a saved style it reports an error.
$process = Start-Pane "stderr-reenable.log"
Capture "18-restarted-disabled.png"
Check "18-restarted-disabled.png" "hint"
python "$PSScriptRoot/check_screenshot.py" --same (Join-Path $OutDir "12-restarted.png") (Join-Path $OutDir "18-restarted-disabled.png")
if ($LASTEXITCODE -ne 0) { throw "screenshot check failed: root after the restart lists the disabled package" }
Manage-Extensions
Send "{DOWN}"; Start-Sleep -Milliseconds 120; Send "{ENTER}"; Start-Sleep -Seconds 2
Capture "19-enabled.png"
Check "19-enabled.png" "success"   # "Enabled Settings sample"
Send "{ESC}"; Start-Sleep -Seconds 1
Send "{DOWN 4}"   # Greeting
Send "{ENTER}"; Start-Sleep -Seconds 3
Send "{DOWN}{DOWN}{ENTER}"; Start-Sleep -Seconds 2   # "Greet me"
Capture "20-greeted.png"
Check "20-greeted.png" "success"   # "Good day to you"
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
Send "{DOWN}"; Start-Sleep -Milliseconds 120; Send "{ENTER}"; Start-Sleep -Seconds 2
Capture "25-search-result.png"
Check "25-search-result.png" "success"   # the TypeScript guest's answer
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
Check "27-answer.png" "answer"   # the selected answer card
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
Check "31-operations-target.png" "success"   # "Installed JavaScript operations sample"
Stop-Pane $process
$process = Start-Pane "stderr-operations.log" @("--install", "target/guests/packages/sample-operations")
Send "{ENTER}"; Start-Sleep -Seconds 2   # Install; Call from Rust is selected
Send "{ENTER}"; Start-Sleep -Seconds 3   # open Call from Rust
Send "{ENTER}"; Start-Sleep -Seconds 2   # "Greet through another extension": its form
Send ("local:" + (Resolve-Path "target/guests/packages/sample-operations-js").Path)
Send "{TAB}Rust"
Send "{ENTER}"; Start-Sleep -Seconds 5   # Greet
Capture "32-operation-answer.png"
Check "32-operation-answer.png" "success"   # the JavaScript guest's answer
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
Check "33-dev-before.png" "success"   # "Hello from the Rust guest"
Send "{ESC}"; Start-Sleep -Seconds 1
Copy-Item -Force "target/guests/sample_js.wasm" (Join-Path $dev "command.wasm")
Manage-Extensions
Send "{DOWN 11}"   # Reload Dev
Send "{ENTER}"; Start-Sleep -Seconds 3
Capture "34-reloaded.png"
Check "34-reloaded.png" "success"   # "Reloaded Dev"
Send "{ESC}"; Start-Sleep -Seconds 1
Send "{DOWN 8}"   # Dev sample
Send "{ENTER}"; Start-Sleep -Seconds 3
Send "{ENTER}"; Start-Sleep -Seconds 2   # "Say hello"
Capture "35-dev-after.png"
Check "35-dev-after.png" "success"   # "Hello from the JavaScript guest"
python "$PSScriptRoot/check_screenshot.py" --distinct (Join-Path $OutDir "33-dev-before.png") (Join-Path $OutDir "35-dev-after.png")
if ($LASTEXITCODE -ne 0) { throw "screenshot check failed: the reloaded command shows its earlier code" }
Send "{ESC}"; Start-Sleep -Seconds 1

# A build that fails the install checks (here its component is missing) is
# not reloaded: the working code keeps running, exactly as before.
Remove-Item (Join-Path $dev "command.wasm")
Manage-Extensions
Send "{DOWN 11}"
Send "{ENTER}"; Start-Sleep -Seconds 2
Capture "36-not-reloaded.png"
Check "36-not-reloaded.png" "error"   # "Dev was not reloaded: ..."
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
Manage-Extensions
Send "{DOWN 11}"
Send "{ENTER}"; Start-Sleep -Seconds 3
Capture "38-start-failed.png"
Check "38-start-failed.png" "error"   # "Reloaded Dev, but it failed to start; ..."
Send "{DOWN}"; Start-Sleep -Milliseconds 120; Send "{ENTER}"; Start-Sleep -Seconds 3   # Retry starting Dev
Capture "39-retried.png"
Check "39-retried.png" "success"   # "Started Dev"
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
Send "{DOWN}"; Start-Sleep -Milliseconds 120; Send "{ENTER}"; Start-Sleep -Seconds 2   # "Sign in"
Send "{DOWN}"; Start-Sleep -Milliseconds 120; Send "{ENTER}"; Start-Sleep -Seconds 2   # "Show what Pane keeps"
Capture "40-kept.png"
Check "40-kept.png" "success"   # every value, the cached greeting included
Send "{ESC}"; Start-Sleep -Seconds 1
Stop-Pane $process
if (-not (Select-String -Quiet -SimpleMatch '"note": "Water the plants"' (Join-Path $data "extensions/content.json"))) { throw "note not saved" }
if (-not (Select-String -Quiet -SimpleMatch '"token": "sample-token"' (Join-Path $data "extensions/credentials.json"))) { throw "credential not saved" }
if (-not (Select-String -Quiet -SimpleMatch '"last-greeting": "Good day to you"' (Join-Path $data "extensions/cache.json"))) { throw "greeting not cached" }

# Clear the settings sample's cache in Manage extensions: its row follows the
# six package rows, their six Reload rows and "Clear cache of Rust sample". Pane asks first, then deletes only the cached
# greeting, without running the extension.
$process = Start-Pane "stderr-clear-cache.log"
Manage-Extensions
Send "{DOWN 13}"
Send "{ENTER}"; Start-Sleep -Seconds 1   # "Clear cache of Settings sample"
Capture "41-confirm-clear-cache.png"
Check "41-confirm-clear-cache.png" "details"   # what is deleted and what is kept
Send "{ENTER}"; Start-Sleep -Seconds 2   # "Clear cache"
Capture "42-cache-cleared.png"
Check "42-cache-cleared.png" "success"   # "Cleared the cache of Settings sample"
Send "{ESC}"; Start-Sleep -Seconds 1
Send "{DOWN 4}"   # Greeting
Send "{ENTER}"; Start-Sleep -Seconds 3
Send "{DOWN 5}"
Send "{ENTER}"; Start-Sleep -Seconds 2   # "Show what Pane keeps"
Capture "43-kept-after-clear.png"
Check "43-kept-after-clear.png" "success"   # "... Cached greeting: none"
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
Check "44-application.png" "selected" 3000   # the selected application row
Send "{ENTER}"; Start-Sleep -Seconds 3
Focus-Pane $process
Capture "45-opened.png"
Check "45-opened.png" "success"   # "Opened Pane Smoke App"
for ($i = 0; $i -lt 50 -and -not (Test-Path $launched); $i++) { Start-Sleep -Milliseconds 200 }
if (-not (Test-Path $launched)) { throw "the application did not run" }
$shots = "44-application", "45-opened" | ForEach-Object { Join-Path $OutDir "$_.png" }
python "$PSScriptRoot/check_screenshot.py" --distinct @shots
if ($LASTEXITCODE -ne 0) { throw "screenshot check failed: opening the application changed nothing" }
Stop-Pane $process

# Quicklinks, a default extension: installed, its Create Quicklink command,
# found by typing its name, opens its form, which saves a quicklink and
# returns to root search. After a restart, typing part of its name lists it,
# selected. Enter would open the default browser, so this smoke stops there
# (the Linux smoke opens it through a recording handler).
$process = Start-Pane "stderr-quicklinks.log" @("--install", "target/guests/packages/quicklinks")
Send "{ENTER}"; Start-Sleep -Seconds 2   # Install
Send "create quicklink"; Start-Sleep -Seconds 2
Send "{ENTER}"; Start-Sleep -Seconds 3   # open Create Quicklink's form
Send "Pane issues"
Send "{TAB}"
Send "https://example.com/pane-issues"
Send "{ENTER}"; Start-Sleep -Seconds 2
Capture "46-quicklink-saved.png"
Check "46-quicklink-saved.png" "success"   # "Created “Pane issues”"
Stop-Pane $process
$process = Start-Pane "stderr-quicklinks-restart.log"
Send "pane iss"; Start-Sleep -Seconds 2
Capture "47-quicklink-found.png"
Check "47-quicklink-found.png" "selected" 3000   # the selected quicklink row
$shots = "46-quicklink-saved", "47-quicklink-found" | ForEach-Object { Join-Path $OutDir "$_.png" }
python "$PSScriptRoot/check_screenshot.py" --distinct @shots
if ($LASTEXITCODE -ne 0) { throw "screenshot check failed: the quicklink was not found" }
Stop-Pane $process

# Uninstall the settings sample, keeping its saved data: its row follows the
# eight Clear cache rows. Pane asks first, showing its saved data, and the first
# choice keeps its settings and content while its copy and credential go.
# Installing the same folder again finds its formal style and note, signed out.
$process = Start-Pane "stderr-uninstall.log"
Manage-Extensions
Send "{DOWN 25}"
Send "{ENTER}"; Start-Sleep -Seconds 1   # "Uninstall Settings sample"
Capture "49-confirm-uninstall.png"
Check "49-confirm-uninstall.png" "details"   # what is removed and the saved data
Send "{ENTER}"; Start-Sleep -Seconds 2   # "Uninstall and keep saved data"
Capture "50-uninstalled.png"
Check "50-uninstalled.png" "success"   # "Uninstalled Settings sample; its settings and content are kept"
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
Check "51-reinstalled.png" "success"   # "Style: formal · Note: Water the plants · Signed in: no ..."
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
function Press-Hotkey {
    # A global hotkey reaches Pane whatever window holds the focus, and the
    # phase's checks read which window is in the front: no refocus here
    # (Send's refocus is for typed keys, which must reach Pane).
    [System.Windows.Forms.SendKeys]::SendWait("^%g"); Start-Sleep -Seconds 3
}
function Check-Pane-In-Front($process) {
    if ([Win]::GetForegroundWindow() -ne $process.MainWindowHandle) { throw "the hotkey did not bring Pane to the front" }
}
$data = Join-Path $OutDir "hotkeys-data"
if (Test-Path $data) { Remove-Item -Recurse -Force $data }
$env:PANE_DATA_DIR = $data
$process = Start-Pane "stderr-hotkeys.log" @("--install", "target/guests/packages/sample-settings")
Send "{ENTER}"; Start-Sleep -Seconds 2   # Install; Greeting is selected
Manage-Extensions
Send "{DOWN 4}"; Start-Sleep -Milliseconds 120; Send "{ENTER}"; Start-Sleep -Seconds 1   # "Hotkey for Greeting"
Capture "52-hotkey-screen.png"
Check "52-hotkey-screen.png" "details"   # "Press the keys that should open Greeting ..."
Send "^%g"; Start-Sleep -Seconds 2
Capture "53-hotkey-assigned.png"
Check "53-hotkey-assigned.png" "success"   # "Ctrl+Alt+G now opens Greeting"
Send "{ESC}"; Start-Sleep -Seconds 1   # root search
Minimize-Pane $process
Capture "54-unfocused.png"   # evidence only: Pane is not on screen
Press-Hotkey
Check-Pane-In-Front $process
Capture "55-hotkey-opened.png"
Check "55-hotkey-opened.png" "selected" 3000   # Greeting's first item, selected
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
Check "56-hotkey-after-restart.png" "selected" 3000   # Greeting's first item, selected
$shots = "53-hotkey-assigned", "56-hotkey-after-restart" | ForEach-Object { Join-Path $OutDir "$_.png" }
python "$PSScriptRoot/check_screenshot.py" --distinct @shots
if ($LASTEXITCODE -ne 0) { throw "screenshot check failed: the hotkey did not open Greeting after a restart" }
Send "{ESC}"; Start-Sleep -Seconds 1
Manage-Extensions
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
Check "59-paused.png" "error"   # "Settings sample crashed 3 times within 5 minutes and is paused ..."
Check "59-paused.png" "warning"   # Greeting: "Settings sample is paused after an error; ..."
Stop-Pane $process
if (-not (Select-String -Quiet -SimpleMatch '"paused"' (Join-Path $data "extensions/installed.json"))) { throw "pause not recorded" }
$process = Start-Pane "stderr-pausing-restart.log"
Send "greet"; Start-Sleep -Seconds 1
Capture "60-paused-after-restart.png"
Check "60-paused-after-restart.png" "warning"   # Greeting is still paused
Send "{ESC}"; Start-Sleep -Seconds 1   # clears the query
Manage-Extensions
Send "{DOWN 3}"; Start-Sleep -Milliseconds 120; Send "{ENTER}"; Start-Sleep -Seconds 1   # "Why Settings sample is paused"
Capture "61-pause-details.png"
Check "61-pause-details.png" "details"   # the details
Send "{ENTER}"; Start-Sleep -Seconds 2   # Retry Settings sample
Capture "62-pause-retried.png"
Check "62-pause-retried.png" "success"   # "Started Settings sample"
$shots = "61-pause-details", "62-pause-retried" | ForEach-Object { Join-Path $OutDir "$_.png" }
python "$PSScriptRoot/check_screenshot.py" --distinct @shots
if ($LASTEXITCODE -ne 0) { throw "screenshot check failed: Retry changed nothing" }
Stop-Pane $process
if (Select-String -Quiet -SimpleMatch '"paused"' (Join-Path $data "extensions/installed.json")) { throw "pause not cleared" }

# Delete retained data: with a data folder of its own, the settings sample
# saves a note and is uninstalled keeping it (its Uninstall row follows its
# state, Reload and Clear cache rows); its retained data, the extension
# list's first row with nothing else installed, already selected when the
# list opens, is deleted after confirming (Cancel is selected first, so
# Down then Enter), without the extension. Installing the same folder again
# finds nothing. Steps that change Pane's files wait for the change instead of a
# fixed time.
$data = Join-Path $OutDir "retained-data"
if (Test-Path $data) { Remove-Item -Recurse -Force $data }
$env:PANE_DATA_DIR = $data
# Waits until $file contains $text ($present) or no longer does (-not
# $present), trying $tries times (100 by default: 10 seconds; the first
# setup of the installed Pane needs far more, as a payload's components
# are checked one at a time).
function Wait-For($file, $text, [bool]$present, $tries = 100) {
    for ($i = 0; $i -lt $tries; $i++) {
        $found = (Test-Path $file) -and (Select-String -Quiet -SimpleMatch $text $file)
        if ($found -eq $present) { return }
        Start-Sleep -Milliseconds 100
    }
    throw "${file}: $text is not $(if ($present) { 'present' } else { 'absent' })"
}
$registry = Join-Path $data "extensions/installed.json"
$process = Start-Pane "stderr-retained.log" @("--install", "target/guests/packages/sample-settings")
Send "{ENTER}"   # Install; Greeting is selected
# 120 s: the install reads and checks the whole package, which a loaded
# runner can take past the 10 s default (run 36856550072's leg).
Wait-For $registry "sample-settings" $true 1200; Start-Sleep -Seconds 1
Send "{ENTER}"; Start-Sleep -Seconds 3   # open Greeting
Send "{DOWN 3}"
Send "{ENTER}"   # "Save a note"
Wait-For (Join-Path $data "extensions/content.json") '"note": "Water the plants"' $true
Send "{ESC}"; Start-Sleep -Seconds 1   # root search
Manage-Extensions
Send "{DOWN 3}"; Start-Sleep -Milliseconds 120; Send "{ENTER}"; Start-Sleep -Seconds 1   # "Uninstall Settings sample"
Send "{ENTER}"   # "Uninstall and keep saved data"
Wait-For $registry '"retained"' $true; Start-Sleep -Seconds 1
# A restart before the deletion: the retained record is what survives one
# (that is its point).
Stop-Pane $process
$process = Start-Pane "stderr-retained.log"
Start-Sleep -Seconds 2
Manage-Extensions
Send "{ENTER}"; Start-Sleep -Seconds 1   # "Delete retained data of Settings sample" (the list's first row)
Capture "63-confirm-delete-retained.png"
Check "63-confirm-delete-retained.png" "details"   # what is kept and what is not touched
# The confirmation's status line is the idle hint, not a result: the
# extension list also shows hint subtitles, so that color alone let the
# wrong screen pass (what #58 turned out to be: Down to the list's end had
# landed on the automatic-update row, whose Enter toggles it and leaves its
# result on screen). No result color on screen says the right screen is up.
python "$PSScriptRoot/check_screenshot.py" --absent (Join-Path $OutDir "63-confirm-delete-retained.png") "success"
if ($LASTEXITCODE -ne 0) { throw "screenshot check failed: the confirmation shows a result status" }
Send "{DOWN}"; Start-Sleep -Milliseconds 120; Send "{ENTER}"   # "Delete retained data"
Start-Sleep -Seconds 2
Wait-For $registry '"retained"' $false; Start-Sleep -Seconds 1
Capture "64-retained-deleted.png"
Check "64-retained-deleted.png" "success"   # "Deleted the retained data of Settings sample"
Stop-Pane $process
if (Select-String -Quiet -SimpleMatch 'Water the plants' (Join-Path $data "extensions/content.json")) { throw "note not deleted" }
$process = Start-Pane "stderr-reinstall-empty.log" @("--install", "target/guests/packages/sample-settings")
Send "{ENTER}"   # Install; Greeting is selected
Wait-For $registry "sample-settings" $true 1200; Start-Sleep -Seconds 1
Send "{ENTER}"; Start-Sleep -Seconds 3   # open Greeting
Send "{DOWN 5}"
Send "{ENTER}"; Start-Sleep -Seconds 2   # "Show what Pane keeps"
Capture "65-reinstalled-empty.png"
Check "65-reinstalled-empty.png" "success"   # "Style: none · Note: none · Signed in: no ..."
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
Manage-Extensions
Send "{DOWN 5}"; Start-Sleep -Milliseconds 120; Send "{ENTER}"; Start-Sleep -Seconds 1   # "Alias for Echo"
Send "ec"
Send "{ENTER}"; Start-Sleep -Seconds 2
Capture "66-alias-saved.png"
Check "66-alias-saved.png" "success"   # "Typing “ec” now finds Echo"
Send "{DOWN}"; Start-Sleep -Milliseconds 120; Send "{ENTER}"; Start-Sleep -Seconds 2   # "Fallback: Echo"
Capture "67-fallback-on.png"
Check "67-fallback-on.png" "success"   # "Echo is now offered for any text typed in root search"
$shots = "66-alias-saved", "67-fallback-on" | ForEach-Object { Join-Path $OutDir "$_.png" }
python "$PSScriptRoot/check_screenshot.py" --distinct @shots
if ($LASTEXITCODE -ne 0) { throw "screenshot check failed: the fallback row changed nothing" }
Send "{ESC}"; Start-Sleep -Seconds 1   # root search
Send "ec hello"; Start-Sleep -Seconds 1
Capture "68-alias-row.png"
Check "68-alias-row.png" "selected" 3000   # Echo, sending “hello”, selected
Send "{ENTER}"; Start-Sleep -Seconds 3
Capture "69-alias-answer.png"
Check "69-alias-answer.png" "success"   # "Echo heard “hello”"
Send "{ESC}"; Start-Sleep -Seconds 1   # clears the query
Send "zqx"; Start-Sleep -Seconds 1
Capture "70-fallback-listed.png"   # "No results for “zqx”", then Echo, not selected
Send "{DOWN}"; Start-Sleep -Seconds 1
Capture "71-fallback-chosen.png"
Check "71-fallback-chosen.png" "selected" 3000   # Echo, now selected
$shots = "70-fallback-listed", "71-fallback-chosen" | ForEach-Object { Join-Path $OutDir "$_.png" }
python "$PSScriptRoot/check_screenshot.py" --distinct @shots
if ($LASTEXITCODE -ne 0) { throw "screenshot check failed: Down did not select the fallback" }
Send "{ENTER}"; Start-Sleep -Seconds 3
Capture "72-fallback-answer.png"
Check "72-fallback-answer.png" "success"   # "Echo heard “zqx”"
$shots = "69-alias-answer", "72-fallback-answer" | ForEach-Object { Join-Path $OutDir "$_.png" }
python "$PSScriptRoot/check_screenshot.py" --distinct @shots
if ($LASTEXITCODE -ne 0) { throw "screenshot check failed: the fallback got the alias's text" }
Stop-Pane $process
$aliases = Join-Path $data "extensions/aliases.json"
if (-not (Select-String -Quiet -SimpleMatch '"ec"' $aliases)) { throw "alias not recorded" }
if (-not (Select-String -Quiet -SimpleMatch '#echo"' $aliases)) { throw "fallback not recorded" }
$process = Start-Pane "stderr-aliases-restart.log"
Manage-Extensions
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
Check "75-dependencies-preview.png" "details"   # "Requires: JavaScript operations sample, installed with it ..."
Send "{ENTER}"; Start-Sleep -Seconds 3   # Install; Greet through dependencies is selected
Capture "76-dependencies-installed.png"
Check "76-dependencies-installed.png" "success"   # "Installed Dependencies sample with JavaScript operations sample, which it requires"
Send "{ENTER}"; Start-Sleep -Seconds 3   # open Greet through dependencies
Send "{ENTER}"; Start-Sleep -Seconds 5   # Greet through the required greeter
Capture "77-dependency-answer.png"
Check "77-dependency-answer.png" "success"   # the JavaScript guest's answer
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
Send "{ENTER}"   # Echo through the helper
# The helper is a process Pane starts and waits for; a cold spawn on a
# loaded runner can outlast a fixed sleep, so the answer is waited for,
# whenever it lands.
for ($i = 0; $i -lt 30; $i++) {
    Capture "90-helper-echoed.png"
    python "$PSScriptRoot/check_screenshot.py" (Join-Path $OutDir "90-helper-echoed.png") "success"
    if ($LASTEXITCODE -eq 0) { break }
    Start-Sleep -Milliseconds 500
}
Check "90-helper-echoed.png" "success"   # 'Echoed "hello from Pane" on Windows x86-64'
Send "{DOWN 2}"; Start-Sleep -Milliseconds 120; Send "{ENTER}"; Start-Sleep -Seconds 3   # Echo within a second
Capture "91-helper-cancelled.png"
Check "91-helper-cancelled.png" "success"   # "Stopped the helper after one second"
$shots = "90-helper-echoed", "91-helper-cancelled" | ForEach-Object { Join-Path $OutDir "$_.png" }
python "$PSScriptRoot/check_screenshot.py" --distinct @shots
if ($LASTEXITCODE -ne 0) { throw "screenshot check failed: the helper's answers look the same" }
if (Helpers-Running) { throw "a cancelled helper is still running" }
Send "{UP}{ENTER}"; Start-Sleep -Seconds 2   # Echo after waiting
if (-not (Helpers-Running)) { throw "the waiting helper is not running" }
Capture "92-helper-waiting.png"
Send "{ESC}"; Start-Sleep -Seconds 1   # root search; the helper keeps running
Manage-Extensions
Send "{ENTER}"; Start-Sleep -Seconds 2   # disable Helper sample
Capture "93-helper-disabled.png"
Check "93-helper-disabled.png" "success"   # "Disabled Helper sample"
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
Send "{DOWN}"; Start-Sleep -Milliseconds 120; Send "{ENTER}"; Start-Sleep -Seconds 2   # Echo after waiting
if (-not (Helpers-Running)) { throw "the waiting helper is not running" }
Capture "94-helper-before-quit.png"
Check "94-helper-before-quit.png" "progress"   # "Running…"
$alive = Get-ChildItem -Recurse -Filter "pane-echo.alive" $packages | Select-Object -First 1
if (-not $alive) { throw "the waiting helper does not beat" }
if (-not $process.CloseMainWindow()) { throw "Pane's window did not take the close request" }
if (-not $process.WaitForExit(5000)) { throw "Pane did not quit when its window closed" }
if (Helpers-Running) { throw "a helper outlived Pane quitting" }
$beats = (Get-Item $alive.FullName).Length; Start-Sleep -Milliseconds 500
if ((Get-Item $alive.FullName).Length -ne $beats) { throw "the helper still beats after Pane quit" }

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
    Send "{DOWN 3}"; Start-Sleep -Milliseconds 120; Send "{ENTER}"; Start-Sleep -Seconds 3
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
    Manage-Extensions
    # "Develop <title>": the row above the list's last, which is the
    # global automatic-update choice since #49 (the develop row was the
    # last row before it, and Down to the end now lands on that instead).
    Send "{DOWN 14}"; Start-Sleep -Milliseconds 120; Send "{UP}"; Start-Sleep -Milliseconds 120; Send "{ENTER}"; Start-Sleep -Seconds 2
    Capture "$n-$sample-develop-started.png"
    Check "$n-$sample-develop-started.png" "success"   # "Developing <title>: each save in ..."
    Send "{ESC}"; Start-Sleep -Seconds 1
    Say-Hello
    Capture "$($n + 1)-$sample-greeting-before.png"
    Check "$($n + 1)-$sample-greeting-before.png" "success"   # "Hello from ..."
    Send "{ESC}"; Start-Sleep -Seconds 1

    # An edit, saved: built and reloaded.
    Copy-Item -Force $built $before
    Set-Greeting (Join-Path $copy $source) ($greeting -f "Hello again")
    Wait-Reloaded $built $before
    Capture "$($n + 2)-$sample-rebuilt.png"
    Check "$($n + 2)-$sample-rebuilt.png" "success"   # "Reloaded <title>"
    Say-Hello
    Capture "$($n + 3)-$sample-greeting-after.png"
    Check "$($n + 3)-$sample-greeting-after.png" "success"   # "Hello again"
    Shots-Differ "$($n + 1)-$sample-greeting-before.png" "$($n + 3)-$sample-greeting-after.png" "the edit changed nothing"
    Send "{ESC}"; Start-Sleep -Seconds 1

    # A save that does not build: the working code stays.
    $failures = Failures $log
    Set-Greeting (Join-Path $copy $source) $broken
    Wait-Failed $log $failures
    Capture "$($n + 4)-$sample-build-failed.png"
    Check "$($n + 4)-$sample-build-failed.png" "error"   # "<title> did not build: ..."
    Say-Hello
    Capture "$($n + 5)-$sample-kept.png"
    Check "$($n + 5)-$sample-kept.png" "success"   # still "Hello again"
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
    Check "$($n + 6)-$sample-rebuilt-again.png" "success"   # "Reloaded <title>"
    Say-Hello
    Capture "$($n + 7)-$sample-greeting-fixed.png"
    Check "$($n + 7)-$sample-greeting-fixed.png" "success"   # "Hello at last"
    Shots-Differ "$($n + 3)-$sample-greeting-after.png" "$($n + 7)-$sample-greeting-fixed.png" "the fix changed nothing"
    Send "{ESC}"; Start-Sleep -Seconds 1

    # Stopped: a save builds nothing.
    Manage-Extensions
    # "Stop developing <title>": as above, the row above the list's last.
    Send "{DOWN 14}"; Start-Sleep -Milliseconds 120; Send "{UP}"; Start-Sleep -Milliseconds 120; Send "{ENTER}"; Start-Sleep -Seconds 2
    Capture "$($n + 8)-$sample-stopped.png"
    Check "$($n + 8)-$sample-stopped.png" "success"   # "Stopped developing <title>"
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
Manage-Extensions
Send "{ENTER}"; Start-Sleep -Seconds 1   # disable JavaScript operations sample: asks first
Capture "140-disable-dependents-asked.png"
Check "140-disable-dependents-asked.png" "details"   # "Dependencies sample, which requires JavaScript operations sample ..."
Send "{DOWN}"; Start-Sleep -Milliseconds 120; Send "{ENTER}"; Start-Sleep -Seconds 1   # Cancel
Capture "141-disable-dependents-cancelled.png"   # both still enabled
Send "{ENTER}"; Start-Sleep -Seconds 1   # asks again
Send "{ENTER}"; Start-Sleep -Seconds 2   # Disable all 2
Capture "142-disable-dependents-disabled.png"
Check "142-disable-dependents-disabled.png" "success"   # "Disabled JavaScript operations sample and Dependencies sample, which requires it"
Send "{ENTER}"; Start-Sleep -Seconds 2   # enable JavaScript operations sample
Capture "143-disable-dependents-enabled-alone.png"
Check "143-disable-dependents-enabled-alone.png" "success"   # "Enabled JavaScript operations sample"; Dependencies sample stays disabled
$shots = "140-disable-dependents-asked", "141-disable-dependents-cancelled", "142-disable-dependents-disabled", "143-disable-dependents-enabled-alone" | ForEach-Object { Join-Path $OutDir "$_.png" }
python "$PSScriptRoot/check_screenshot.py" --distinct @shots
if ($LASTEXITCODE -ne 0) { throw "screenshot check failed: disabling with dependents changed nothing" }
Stop-Pane $process
$record = Join-Path $data "extensions/installed.json"
if ((Select-String -SimpleMatch '"disabled": true' $record).Count -ne 1) { throw "not exactly the dependent left disabled" }

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
$data = Join-Path $OutDir "runtime-crash-data"
if (Test-Path $data) { Remove-Item -Recurse -Force $data }
$env:PANE_DATA_DIR = $data
$packages = [System.IO.Path]::GetFullPath((Join-Path $data "extensions/packages"))
$fault = [System.IO.Path]::GetFullPath((Join-Path $OutDir "runtime-fault"))
Remove-Item -Force -ErrorAction SilentlyContinue $fault, "$fault.tmp"
# Asks Pane to inject a fault; it takes the file within 100 ms.
function Inject-Fault($what) {
    Set-Content -NoNewline -Path "$fault.tmp" -Value $what
    Move-Item -Force "$fault.tmp" $fault
    for ($i = 0; $i -lt 50 -and (Test-Path $fault); $i++) { Start-Sleep -Milliseconds 100 }
    if (Test-Path $fault) { throw "Pane did not take the fault" }
    Start-Sleep -Seconds 2
}
# The count Count keeps in the settings sample's content.
function Saved-Count {
    $content = Get-Content -Raw (Join-Path $data "extensions/content.json") | ConvertFrom-Json
    foreach ($package in $content.packages.PSObject.Properties) {
        if ($package.Value.count) { return $package.Value.count }
    }
    "none"
}
function Helpers-Running {
    [bool](Get-Process -Name "pane-echo" -ErrorAction SilentlyContinue |
        Where-Object { $_.Path -and $_.Path.StartsWith($packages, [System.StringComparison]::OrdinalIgnoreCase) })
}
$process = Start-Pane "stderr-runtime-crash-install.log" @("--install", "target/guests/packages/sample-helper")
Send "{ENTER}"; Start-Sleep -Seconds 2   # Install
Stop-Pane $process
$env:PANE_TEST_RUNTIME_FAULTS = $fault
$process = Start-Pane "stderr-runtime-crash.log" @("--install", "target/guests/packages/sample-settings")
Send "{ENTER}"; Start-Sleep -Seconds 2   # Install; Greeting is selected
Send "{ENTER}"; Start-Sleep -Seconds 2   # open Greeting
Send "{DOWN 8}"   # Count
Send "{ENTER}"; Start-Sleep -Seconds 2
Capture "200-runtime-counted.png"
Check "200-runtime-counted.png" "success"   # "Counted 1"
if ((Saved-Count) -ne "1") { throw "Count did not count once" }
Send "{ESC}"; Start-Sleep -Seconds 1
Send "helper"; Start-Sleep -Seconds 1
Send "{ENTER}"; Start-Sleep -Seconds 2   # open Helper sample
Send "{DOWN}"; Start-Sleep -Milliseconds 120; Send "{ENTER}"; Start-Sleep -Seconds 2   # Echo after waiting
if (-not (Helpers-Running)) { throw "the waiting helper is not running" }
Capture "201-runtime-helper-waiting.png"
Check "201-runtime-helper-waiting.png" "progress"   # "Running…"
$alive = Get-ChildItem -Recurse -Filter "pane-echo.alive" $packages | Select-Object -First 1
if (-not $alive) { throw "the waiting helper does not beat" }
Inject-Fault "crash"
Capture "202-runtime-crashed.png"
Check "202-runtime-crashed.png" "error"   # "Pane's extension runtime stopped unexpectedly and was started again; ..."
if (Helpers-Running) { throw "the helper outlived the crashed runtime" }
$beats = (Get-Item $alive.FullName).Length; Start-Sleep -Milliseconds 500
if ((Get-Item $alive.FullName).Length -ne $beats) { throw "the helper still beats after the crash" }
$settings = Join-Path $data "extensions/settings.json"
if (-not (Select-String -Quiet -SimpleMatch '"helper-wait": "started"' $settings)) { throw "saved note lost" }
if (Select-String -Quiet -SimpleMatch '"helper-wait": "finished"' $settings) { throw "the stopped call finished" }
Send "{ESC}"; Start-Sleep -Seconds 1
Send "greet"; Start-Sleep -Seconds 1
Send "{ENTER}"; Start-Sleep -Seconds 2   # open Greeting in the restarted runtime
Send "{DOWN 8}"   # Count
Inject-Fault "crash-before-answer:count"
Send "{ENTER}"; Start-Sleep -Seconds 3   # counts, then the runtime crashes before answering
Capture "203-runtime-stopped.png"
Check "203-runtime-stopped.png" "error"   # the runtime stopped; its answer is lost
if ((Saved-Count) -ne "2") { throw "Count did not run once before the crash" }
Send "{ESC}"; Start-Sleep -Seconds 1
Send "greet"; Start-Sleep -Seconds 1
Send "{ENTER}"; Start-Sleep -Seconds 2   # Greeting: nothing runs
Capture "204-runtime-refused.png"
Check "204-runtime-refused.png" "error"   # "Extension runtime unavailable: it stopped after crashing ..."
Send "{ESC}"; Start-Sleep -Seconds 1   # clears the query
Manage-Extensions
Capture "205-runtime-manage.png"   # Restart the extension runtime, Why the extension runtime stopped
Send "{DOWN}"; Start-Sleep -Milliseconds 120; Send "{ENTER}"; Start-Sleep -Seconds 1   # Why the extension runtime stopped
Capture "206-runtime-details.png"
Check "206-runtime-details.png" "details"   # the details
Send "{ESC}"; Start-Sleep -Seconds 1   # back at its row
Send "{DOWN}"; Start-Sleep -Milliseconds 120; Send "{ENTER}"; Start-Sleep -Seconds 2   # disable Helper sample, the first package
Capture "207-runtime-disabled.png"
Check "207-runtime-disabled.png" "success"   # "Disabled Helper sample"
Send "{UP 2}{ENTER}"; Start-Sleep -Seconds 2   # Restart the extension runtime
Capture "208-runtime-restarted.png"
Check "208-runtime-restarted.png" "success"   # "Restarted the extension runtime"
if ((Saved-Count) -ne "2") { throw "Count was run again without asking" }
Send "{ESC}"; Start-Sleep -Seconds 1
Send "greet"; Start-Sleep -Seconds 1
Send "{ENTER}"; Start-Sleep -Seconds 2   # open Greeting
Send "{DOWN 8}"   # Count
Send "{ENTER}"; Start-Sleep -Seconds 2
Capture "209-runtime-counted-again.png"
Check "209-runtime-counted-again.png" "success"   # "Counted 3"
if ((Saved-Count) -ne "3") { throw "Count did not count once more" }
$shots = "200-runtime-counted", "202-runtime-crashed", "203-runtime-stopped", "204-runtime-refused", "205-runtime-manage", "206-runtime-details", "207-runtime-disabled", "208-runtime-restarted", "209-runtime-counted-again" | ForEach-Object { Join-Path $OutDir "$_.png" }
python "$PSScriptRoot/check_screenshot.py" --distinct @shots
if ($LASTEXITCODE -ne 0) { throw "screenshot check failed: recovering from a runtime crash changed nothing" }
Stop-Pane $process
Remove-Item Env:PANE_TEST_RUNTIME_FAULTS
if (Helpers-Running) { throw "a helper outlived Pane" }
$record = Join-Path $data "extensions/installed.json"
if (-not (Select-String -Quiet -SimpleMatch '"disabled": true' $record)) { throw "disable not recorded" }
if (Select-String -Quiet -SimpleMatch '"paused"' $record) { throw "a package was paused for the runtime's crash" }
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
$data = Join-Path $OutDir "unresponsive-data"
if (Test-Path $data) { Remove-Item -Recurse -Force $data }
$env:PANE_DATA_DIR = $data
$fault = [System.IO.Path]::GetFullPath((Join-Path $OutDir "unresponsive-fault"))
Remove-Item -Force -ErrorAction SilentlyContinue $fault, "$fault.tmp"
# What the settings sample saved under $key, or "none".
function Saved-Setting($key) {
    $settings = Get-Content -Raw (Join-Path $data "extensions/settings.json") | ConvertFrom-Json
    foreach ($package in $settings.packages.PSObject.Properties) {
        if ($package.Value.$key) { return $package.Value.$key }
    }
    "none"
}
# How many calls Pane stopped as unresponsive, as its standard error says.
function Stopped-Calls {
    @(Select-String -SimpleMatch "stopped responding" (Join-Path $OutDir "stderr-unresponsive.log") -ErrorAction SilentlyContinue).Count
}
$env:PANE_TEST_RUNTIME_FAULTS = $fault
$process = Start-Pane "stderr-unresponsive.log" @("--install", "target/guests/packages/sample-settings")
Inject-Fault "limits:60,4,15"   # a minute of computing: frame 240 is taken while it computes
Send "{ENTER}"; Start-Sleep -Seconds 2   # Install; Greeting is selected
Send "{ENTER}"; Start-Sleep -Seconds 2   # open Greeting
Send "{DOWN 9}"   # Stop responding
Send "{ENTER}"; Start-Sleep -Seconds 1   # it computes
if ((Saved-Setting "busy") -ne "started") { throw "Stop responding did not start" }
Send "{ESC}"; Start-Sleep -Seconds 1   # root search answers meanwhile
Manage-Extensions
Capture "240-unresponsive-window-answers.png"   # the extension list, while the guest computes
Check "240-unresponsive-window-answers.png" "subtitle"   # its rows' subtitles
if ((Stopped-Calls) -ne 0) { throw "Stop responding was stopped before frame 240" }
Inject-Fault "limits:2,4,15"   # it has computed longer: Pane stops it at its next tick
for ($i = 0; $i -lt 300 -and (Stopped-Calls) -lt 1; $i++) { Start-Sleep -Milliseconds 100 }
if ((Stopped-Calls) -lt 1) { throw "Stop responding was not stopped at the shorter limit" }
Send "{ESC}"; Start-Sleep -Seconds 1   # its answer is not shown here
Send "greet"; Start-Sleep -Seconds 1
Send "{ENTER}"; Start-Sleep -Seconds 2   # open Greeting
Send "{DOWN 9}"   # Stop responding
Send "{ENTER}"; Start-Sleep -Seconds 8
Capture "241-unresponsive-stopped.png"
Check "241-unresponsive-stopped.png" "error"   # "The extension stopped responding: it computed for 2 seconds ..."
Send "{ENTER}"; Start-Sleep -Seconds 8   # the third time
Send "greet"; Start-Sleep -Seconds 1   # Greeting and its reason at the top
Capture "242-unresponsive-paused.png"
Check "242-unresponsive-paused.png" "error"   # "Settings sample stopped responding 3 times within 5 minutes and is paused ..."
Check "242-unresponsive-paused.png" "warning"   # Greeting: "Settings sample is paused after an error; ..."
if ((Saved-Setting "busy") -ne "started") { throw "Stop responding finished or was lost" }
Send "{ESC}"; Start-Sleep -Seconds 1   # clears the query
Manage-Extensions
Send "{DOWN 3}"; Start-Sleep -Milliseconds 120; Send "{ENTER}"; Start-Sleep -Seconds 1   # "Why Settings sample is paused"
Capture "243-unresponsive-pause-details.png"
Check "243-unresponsive-pause-details.png" "details"   # the details
Send "{ENTER}"; Start-Sleep -Seconds 2   # Retry Settings sample
Capture "244-unresponsive-retried.png"
Check "244-unresponsive-retried.png" "success"   # "Started Settings sample"
Send "{ESC}"; Start-Sleep -Seconds 1
Inject-Fault "hang"
Send "greet"; Start-Sleep -Seconds 1
Send "{ENTER}"; Start-Sleep -Seconds 4   # open Greeting: the stuck runtime is not responding yet
Capture "245-unresponsive-not-yet.png"
Check "245-unresponsive-not-yet.png" "progress"   # "Pane's extension runtime is not responding yet. ..."
Start-Sleep -Seconds 14   # Pane gives up on it
Capture "246-unresponsive-runtime.png"
Check "246-unresponsive-runtime.png" "error"   # the runtime stopped responding and was started again
Send "{ESC}"; Start-Sleep -Seconds 1   # clears the query
Manage-Extensions
Send "{ENTER}"; Start-Sleep -Seconds 1   # Why the extension runtime stopped, its first row
Capture "247-unresponsive-runtime-details.png"
Check "247-unresponsive-runtime-details.png" "details"   # the details
Inject-Fault "release"
Send "{ESC}{ESC}"; Start-Sleep -Seconds 1
Send "greet"; Start-Sleep -Seconds 1
Send "{ENTER}"; Start-Sleep -Seconds 2   # open Greeting on a fresh runtime thread
Send "{ENTER}"; Start-Sleep -Seconds 2   # Use a formal greeting
Capture "248-unresponsive-runs-again.png"
Check "248-unresponsive-runs-again.png" "success"   # "Saved the formal greeting"
$shots = "240-unresponsive-window-answers", "241-unresponsive-stopped", "242-unresponsive-paused", "243-unresponsive-pause-details", "244-unresponsive-retried", "245-unresponsive-not-yet", "246-unresponsive-runtime", "247-unresponsive-runtime-details", "248-unresponsive-runs-again" | ForEach-Object { Join-Path $OutDir "$_.png" }
python "$PSScriptRoot/check_screenshot.py" --distinct @shots
if ($LASTEXITCODE -ne 0) { throw "screenshot check failed: recovering from an extension that stops responding changed nothing" }
Stop-Pane $process
Remove-Item Env:PANE_TEST_RUNTIME_FAULTS
if ((Saved-Setting "busy") -ne "started") { throw "Stop responding finished after it was stopped" }
if ((Saved-Setting "greeting-style") -ne "formal") { throw "the fresh runtime did not save" }
# No package record of installed.json holds a pause (read as JSON, not as text).
$record = Get-Content -Raw (Join-Path $data "extensions/installed.json") | ConvertFrom-Json
$paused = @($record.packages | Where-Object { $_.PSObject.Properties.Name -contains "paused" })
if ($paused.Count -ne 0) { throw "a package was paused for the runtime's hang" }

# Uninstalling a required dependency: installed with the dependencies sample
# (whose install and data folder are this phase's own), the JavaScript
# operations sample's Uninstall row is the seventh of Manage extensions.
# Enter asks first, listing the Dependencies sample, which requires it, and
# each one's saved data, with Uninstall all keeping or deleting saved data
# and Cancel; Cancel changes nothing, Uninstall all 2 (keeping) uninstalls
# both, and installing the JavaScript operations sample again installs it
# alone: the Dependencies sample is not restored, on record too.
$data = Join-Path $OutDir "uninstall-dependents-data"
if (Test-Path $data) { Remove-Item -Recurse -Force $data }
$env:PANE_DATA_DIR = $data
$process = Start-Pane "stderr-uninstall-dependents.log" @("--install", "target/guests/packages/sample-dependencies")
Send "{ENTER}"; Start-Sleep -Seconds 3   # Install
Manage-Extensions
for ($i = 0; $i -lt 6; $i++) { Send "{DOWN}" }   # Uninstall JavaScript operations sample
Send "{ENTER}"; Start-Sleep -Seconds 1   # asks first
Capture "180-uninstall-dependents-asked.png"
Check "180-uninstall-dependents-asked.png" "details"   # "Dependencies sample, which requires JavaScript operations sample ..."
Send "{DOWN}{DOWN}{ENTER}"; Start-Sleep -Seconds 1   # Cancel
Capture "181-uninstall-dependents-cancelled.png"   # both still installed
Send "{ENTER}"; Start-Sleep -Seconds 1   # asks again
Send "{ENTER}"; Start-Sleep -Seconds 3   # Uninstall all 2 and keep saved data
Capture "182-uninstall-dependents-uninstalled.png"
Check "182-uninstall-dependents-uninstalled.png" "success"   # "Uninstalled JavaScript operations sample and Dependencies sample, which requires it; ..."
Stop-Pane $process
$record = Join-Path $data "extensions/installed.json"
if ((Select-String -SimpleMatch '"dir"' $record).Count -ne 0) { throw "not both uninstalled" }
$process = Start-Pane "stderr-uninstall-dependents-again.log" @("--install", "target/guests/packages/sample-operations-js")
Send "{ENTER}"; Start-Sleep -Seconds 3   # Install the dependency alone
Manage-Extensions
Capture "183-uninstall-dependents-reinstalled-alone.png"   # only the JavaScript operations sample is listed
Check "183-uninstall-dependents-reinstalled-alone.png" "subtitle"
$shots = "180-uninstall-dependents-asked", "181-uninstall-dependents-cancelled", "182-uninstall-dependents-uninstalled", "183-uninstall-dependents-reinstalled-alone" | ForEach-Object { Join-Path $OutDir "$_.png" }
python "$PSScriptRoot/check_screenshot.py" --distinct @shots
if ($LASTEXITCODE -ne 0) { throw "screenshot check failed: uninstalling with dependents changed nothing" }
Stop-Pane $process
if ((Select-String -SimpleMatch '"dir"' $record).Count -ne 1) { throw "not the dependency alone reinstalled" }

# npm packages (#45), from a local registry on 127.0.0.1 serving the npm
# sample `cargo xtask guests` packed (scripts/npm_registry.py; nothing reaches
# the network), with a data folder of its own. Installing the local
# Dependencies from npm sample shows the npm package it requires and
# installs both; its command calls the npm package's greet operation. Then
# "Install extension from npm..." (searched for by title, as Manage-Extensions
# does: a blind run of Downs to root's end would open #72's Settings… row,
# last of all now) asks for the
# npm package in a form; naming the installed one offers Update, and its
# command runs: "Hello from the npm package".
$data = Join-Path $OutDir "npm-data"
if (Test-Path $data) { Remove-Item -Recurse -Force $data }
$env:PANE_DATA_DIR = $data
$portFile = Join-Path $OutDir "npm-registry.port"
if (Test-Path $portFile) { Remove-Item -Force $portFile }
$registry = Start-Process python -PassThru -NoNewWindow `
    -ArgumentList @("`"$PSScriptRoot/npm_registry.py`"", "target/guests/npm", "`"$portFile`"") `
    -RedirectStandardError (Join-Path $OutDir "npm-registry.log")
try {
    # Generous: a slow runner may take seconds to start Python.
    for ($i = 0; $i -lt 600 -and -not (Test-Path $portFile) -and -not $registry.HasExited; $i++) { Start-Sleep -Milliseconds 100 }
    if (-not (Test-Path $portFile)) { throw "the local npm registry did not start" }
    $env:PANE_NPM_REGISTRY = "http://127.0.0.1:$((Get-Content $portFile).Trim())/"
    $process = Start-Pane "stderr-npm.log" @("--install", "target/guests/packages/sample-dependencies-npm")
    Capture "260-npm-dependency-preview.png"
    Check "260-npm-dependency-preview.png" "details"   # "Requires: Greeter from npm, installed with it from npm:@pane-samples/greeter"
    Send "{ENTER}"; Start-Sleep -Seconds 3   # Install; Greet through an npm dependency is selected
    Capture "261-npm-dependency-installed.png"
    Check "261-npm-dependency-installed.png" "success"   # "Installed Dependencies from npm sample with Greeter from npm, which it requires"
    Send "{ENTER}"; Start-Sleep -Seconds 3   # open it
    Send "{ENTER}"; Start-Sleep -Seconds 3   # "Greet through the required greeter"
    Capture "262-npm-dependency-called.png"
    Check "262-npm-dependency-called.png" "success"   # "Hello, Pane, from the npm package"
    Send "{ESC}"; Start-Sleep -Seconds 1
    Send "^a"; Send "install npm"; Start-Sleep -Seconds 1
    Send "{ENTER}"; Start-Sleep -Seconds 1   # Install extension from npm...
    Capture "263-npm-form.png"
    Check "263-npm-form.png" "hint"   # the form's hint line
    Send "@pane-samples/greeter"
    Send "{ENTER}"; Start-Sleep -Seconds 3
    Capture "264-npm-preview.png"
    Check "264-npm-preview.png" "details"   # "Source: npm package @pane-samples/greeter", "npm version: 0.1.0, the latest", ...
    Send "{ENTER}"; Start-Sleep -Seconds 3   # Update; Greeter from npm is selected
    Capture "265-npm-updated.png"
    Check "265-npm-updated.png" "success"   # "Updated Greeter from npm to 0.1.0"
    Send "{ENTER}"; Start-Sleep -Seconds 3   # open Greeter from npm
    Send "{ENTER}"; Start-Sleep -Seconds 2   # "Say hello"
    Capture "266-npm-command-ran.png"
    Check "266-npm-command-ran.png" "success"   # "Hello from the npm package"

    # #49: the update Pane applies by itself. A 0.2.0 of the sample is
    # published into the registry this phase serves (it reads its folder on
    # request, so publishing is dropping the tarball in), and Pane is
    # stopped and started again: the first check, a second after the start,
    # finds the newer version and replaces the installed copy - unpinned,
    # and nothing of it running, so the safe boundary is at once - saying
    # so in the status line. The new copy's command runs as the old one did.
    python "$PSScriptRoot/npm_publish.py" "target/guests/npm/pane-samples-greeter-0.1.0.tgz" "0.2.0"
    Stop-Pane $process
    $process = Start-Pane "stderr-npm.log"
    # The check a second after the start, then the download and the apply:
    # poll until the status line says the update landed, whenever that is,
    # so a slow runner is waited for rather than slept past.
    for ($i = 0; $i -lt 120; $i++) {
        Capture "267-npm-updated-automatically.png"
        python "$PSScriptRoot/check_screenshot.py" (Join-Path $OutDir "267-npm-updated-automatically.png") "success"
        if ($LASTEXITCODE -eq 0) { break }
        Start-Sleep -Milliseconds 500
    }
    Check "267-npm-updated-automatically.png" "success"   # "Updated Greeter from npm to 0.2.0"
    Send "{ENTER}"; Start-Sleep -Seconds 3   # open Greeter from npm, the new copy
    Send "{ENTER}"; Start-Sleep -Seconds 2   # "Say hello"
    Capture "268-npm-new-copy-ran.png"
    Check "268-npm-new-copy-ran.png" "success"   # "Hello from the npm package"
    $shots = "260-npm-dependency-preview", "261-npm-dependency-installed", "262-npm-dependency-called", "263-npm-form", "264-npm-preview", "265-npm-updated", "266-npm-command-ran", "267-npm-updated-automatically", "268-npm-new-copy-ran" | ForEach-Object { Join-Path $OutDir "$_.png" }
    python "$PSScriptRoot/check_screenshot.py" --distinct @shots
    if ($LASTEXITCODE -ne 0) { throw "screenshot check failed: installing from npm changed nothing" }
    Stop-Pane $process
} finally {
    Stop-Process -Id $registry.Id -ErrorAction SilentlyContinue
    Remove-Item Env:PANE_NPM_REGISTRY -ErrorAction SilentlyContinue
}
$record = Join-Path $data "extensions/installed.json"
if (-not (Select-String -Quiet -SimpleMatch '"npm": "@pane-samples/greeter"' $record)) { throw "npm package not recorded" }
if (-not (Select-String -Quiet -SimpleMatch '"npmVersion": "0.2.0"' $record)) { throw "the automatic update was not recorded" }
if ((Select-String -SimpleMatch '"dir"' $record).Count -ne 2) { throw "not both installed" }

# Git packages (#46), from a repository the smoke makes with git from the
# Git sample `cargo xtask guests` assembled (target/guests/git/greeter: its
# source on main, its built component on the branch release, tagged v0.1.0),
# served over Git's smart HTTP protocol from 127.0.0.1
# (scripts/repository_server.py; nothing reaches the network), with a data
# folder of its own. `--install git:<address>` names the default branch,
# which holds the source only: explained, nothing offered. Then "Install
# extension from Git..." (searched for by title rather than counted to, as
# Manage-Extensions does: root's last row is #72's Settings… now, and with
# nothing installed in this data folder there is no Manage extensions... row
# to find either) asks for the repository
# in a form; naming the tag previews the release revision, pinned, and
# installs it, and its command runs: "Hello from the Git repository".
$data = Join-Path $OutDir "git-data"
if (Test-Path $data) { Remove-Item -Recurse -Force $data }
$env:PANE_DATA_DIR = $data
$repositories = Join-Path $OutDir "git-repositories"
if (Test-Path $repositories) { Remove-Item -Recurse -Force $repositories }
python "$PSScriptRoot/repository_server.py" make-sample target/guests/git/greeter (Join-Path $repositories "greeter")
if ($LASTEXITCODE -ne 0) { throw "the Git sample's repository was not made" }
$portFile = Join-Path $OutDir "repository-server.port"
if (Test-Path $portFile) { Remove-Item -Force $portFile }
# Captures $name until it shows text in $color, for at most $seconds, then
# checks it: for a view that appears once work in the background ends,
# whenever that is.
function Capture-Until($name, $color, $seconds) {
    $deadline = (Get-Date).AddSeconds($seconds)
    while ($true) {
        Capture $name
        # Only its exit code matters. Windows PowerShell 5.1 turns a native
        # program's redirected standard error into errors, which "Stop" would
        # throw at the first failed check, so it runs with "Continue" in a
        # scope of its own.
        & {
            $ErrorActionPreference = "Continue"
            python "$PSScriptRoot/check_screenshot.py" (Join-Path $OutDir $name) $color 20 *> $null
        }
        if ($LASTEXITCODE -eq 0) { return }
        if ((Get-Date) -gt $deadline) { Check $name $color; return }
        Start-Sleep -Milliseconds 500
    }
}
$server = Start-Process python -PassThru -NoNewWindow `
    -ArgumentList @("`"$PSScriptRoot/repository_server.py`"", "serve", "`"$repositories`"", "`"$portFile`"") `
    -RedirectStandardError (Join-Path $OutDir "repository-server.log")
$process = $null
try {
    for ($i = 0; $i -lt 600 -and -not (Test-Path $portFile) -and -not $server.HasExited; $i++) { Start-Sleep -Milliseconds 100 }
    if (-not (Test-Path $portFile)) { throw "the local repository server did not start" }
    $repository = "http://127.0.0.1:$((Get-Content $portFile).Trim())/greeter.git"
    $process = Start-Pane "stderr-git.log" @("--install", "git:$repository")
    # The fetch runs after the window shows: capture until its explanation does.
    Capture-Until "300-git-source-only.png" "error" 60   # "The default branch, main (commit ...) of the Git repository ... holds only the source of ..."
    Send "{ESC}"; Start-Sleep -Seconds 1
    Send "^a"; Send "install git"; Start-Sleep -Seconds 1
    Send "{ENTER}"; Start-Sleep -Seconds 1   # Install extension from Git...
    Capture "301-git-form.png"
    Check "301-git-form.png" "hint"   # the form's hint line
    Send "$repository@v0.1.0"
    Send "{ENTER}"; Start-Sleep -Seconds 3
    Capture "302-git-preview.png"
    Check "302-git-preview.png" "details"   # "Source: Git repository 127.0.0.1:<port>/greeter", "Revision: tag v0.1.0, which you named: ..."
    Send "{ENTER}"; Start-Sleep -Seconds 3   # Install; Greeter from Git is selected
    Capture "303-git-installed.png"
    Check "303-git-installed.png" "success"   # "Installed Greeter from Git"
    Send "{ENTER}"; Start-Sleep -Seconds 3   # open Greeter from Git
    Send "{ENTER}"; Start-Sleep -Seconds 2   # "Say hello"
    Capture "304-git-command-ran.png"
    Check "304-git-command-ran.png" "success"   # "Hello from the Git repository"
    $shots = "300-git-source-only", "301-git-form", "302-git-preview", "303-git-installed", "304-git-command-ran" | ForEach-Object { Join-Path $OutDir "$_.png" }
    python "$PSScriptRoot/check_screenshot.py" --distinct @shots
    if ($LASTEXITCODE -ne 0) { throw "screenshot check failed: installing from Git changed nothing" }
    Stop-Pane $process
} finally {
    # A failure above leaves Pane running: stop it too, before the server.
    if ($process -and -not $process.HasExited) {
        Stop-Process -Id $process.Id -ErrorAction SilentlyContinue
        $process.WaitForExit()
    }
    Stop-Process -Id $server.Id -ErrorAction SilentlyContinue
}
$release = (python "$PSScriptRoot/repository_server.py" commit (Join-Path $repositories "greeter") v0.1.0)
if ($LASTEXITCODE -ne 0) { throw "the tag's commit was not found" }
$record = Join-Path $data "extensions/installed.json"
$fromGit = @((Get-Content -Raw $record | ConvertFrom-Json).packages | Where-Object { $_.git })
if ($fromGit.Count -ne 1) { throw "not one package from Git recorded: $($fromGit.Count)" }
if ($fromGit[0].gitRef -ne "refs/tags/v0.1.0") { throw "Git reference not recorded: $($fromGit[0].gitRef)" }
if ($fromGit[0].gitCommit -ne $release.Trim()) { throw "Git commit not recorded: $($fromGit[0].gitCommit)" }
if ($fromGit[0].pinned -ne $true) { throw "Git tag not recorded as pinned" }
$downloads = Join-Path $data "extensions/downloads"
if ((Test-Path $downloads) -and (Get-ChildItem $downloads)) { throw "a Git download was left" }

# Git packages update themselves (#50): a second repository of the same
# sample, served by a server of its own, installed in a data folder of its
# own from its tracked release branch -- `--install` naming the branch, so
# the copy is tracked, not pinned -- with its command run; the branch then
# moves to a 0.2.0 (repository_server.py move-sample) while Pane is
# stopped, and the check a second after the restart replaces the installed
# copy by itself, the new code running. Nothing reaches the network.
$updateData = Join-Path $OutDir "git-update-data"
if (Test-Path $updateData) { Remove-Item -Recurse -Force $updateData }
$env:PANE_DATA_DIR = $updateData
python "$PSScriptRoot/repository_server.py" make-sample target/guests/git/greeter (Join-Path $repositories "greeter-tracked")
if ($LASTEXITCODE -ne 0) { throw "the second Git sample's repository was not made" }
$updatePortFile = Join-Path $OutDir "repository-update-server.port"
if (Test-Path $updatePortFile) { Remove-Item -Force $updatePortFile }
$server2 = Start-Process python -PassThru -NoNewWindow `
    -ArgumentList @("`"$PSScriptRoot/repository_server.py`"", "serve", "`"$repositories`"", "`"$updatePortFile`"") `
    -RedirectStandardError (Join-Path $OutDir "repository-update-server.log")
$process = $null
try {
    for ($i = 0; $i -lt 600 -and -not (Test-Path $updatePortFile) -and -not $server2.HasExited; $i++) { Start-Sleep -Milliseconds 100 }
    if (-not (Test-Path $updatePortFile)) { throw "the second local repository server did not start" }
    $tracked = "http://127.0.0.1:$((Get-Content $updatePortFile).Trim())/greeter-tracked.git"
    $process = Start-Pane "stderr-git-update.log" @("--install", "git:$tracked@release")
    # The fetch runs after the window shows: capture until its preview does.
    Capture-Until "305-git-tracked-preview.png" "details" 60   # "Revision: branch release, tracked: an update fetches that branch again"
    Send "{ENTER}"; Start-Sleep -Seconds 3   # Install; Greeter from Git is selected
    Capture "306-git-tracked-installed.png"
    Check "306-git-tracked-installed.png" "success"   # "Installed Greeter from Git"
    python "$PSScriptRoot/repository_server.py" move-sample (Join-Path $repositories "greeter-tracked") 0.2.0
    if ($LASTEXITCODE -ne 0) { throw "the tracked branch did not move" }
    Stop-Pane $process
    $process = Start-Pane "stderr-git-update.log"
    # The check a second after the start, then the fetch and the apply:
    # capture until the status line says the update landed.
    Capture-Until "307-git-updated-automatically.png" "success" 60   # "Updated Greeter from Git to 0.2.0"
    Send "{ENTER}"; Start-Sleep -Seconds 3   # open Greeter from Git, the new copy
    Send "{ENTER}"; Start-Sleep -Seconds 2   # "Say hello"
    Capture "308-git-new-copy-ran.png"
    Check "308-git-new-copy-ran.png" "success"   # "Hello from the Git repository"
    $shots = "305-git-tracked-preview", "306-git-tracked-installed", "307-git-updated-automatically", "308-git-new-copy-ran" | ForEach-Object { Join-Path $OutDir "$_.png" }
    python "$PSScriptRoot/check_screenshot.py" --distinct @shots
    if ($LASTEXITCODE -ne 0) { throw "screenshot check failed: updating from Git changed nothing" }
    Stop-Pane $process
} finally {
    # A failure above leaves Pane running: stop it too, before the server.
    if ($process -and -not $process.HasExited) {
        Stop-Process -Id $process.Id -ErrorAction SilentlyContinue
        $process.WaitForExit()
    }
    Stop-Process -Id $server2.Id -ErrorAction SilentlyContinue
}
$moved = (python "$PSScriptRoot/repository_server.py" commit (Join-Path $repositories "greeter-tracked") release)
if ($LASTEXITCODE -ne 0) { throw "the moved branch's commit was not found" }
$record = Join-Path $updateData "extensions/installed.json"
$fromGit = @((Get-Content -Raw $record | ConvertFrom-Json).packages | Where-Object { $_.git })
if ($fromGit.Count -ne 1) { throw "not one package from Git recorded: $($fromGit.Count)" }
if ($fromGit[0].gitRef -ne "refs/heads/release") { throw "Git reference not recorded: $($fromGit[0].gitRef)" }
if ($fromGit[0].gitCommit -ne $moved.Trim()) { throw "Git commit not recorded: $($fromGit[0].gitCommit)" }
if ($fromGit[0].pinned) { throw "the tracked branch recorded as pinned" }
$downloads = Join-Path $updateData "extensions/downloads"
if ((Test-Path $downloads) -and (Get-ChildItem $downloads)) { throw "a Git download was left" }

# File search (#29): Files, a default extension (its data folder is this
# phase's own; Files is selected once installed, and Pane's own "Choose
# folder..." row is the first of its command). Enter on it would show the
# system's folder picker; the smoke names the folder in
# PANE_TEST_CHOOSE_FOLDER instead (a debug build's hook). The fixture folder's
# path has spaces, and a file in it has non-ASCII letters too; typing "plan"
# lists that file, selected, and Enter hands it to Pane's handler for files,
# which PANE_TEST_OPEN_FILE_LOG (a debug build's hook) makes record the path
# instead of running Invoke-Item, which could show the "Open with" dialog or
# open the user's own program. A batch file in the folder is found but
# refused.
$data = Join-Path $OutDir "files-data"
if (Test-Path $data) { Remove-Item -Recurse -Force $data }
$env:PANE_DATA_DIR = $data
$filesFixture = Join-Path $OutDir "files-fixture"
if (Test-Path $filesFixture) { Remove-Item -Recurse -Force $filesFixture }
$filesFolder = Join-Path $filesFixture "Pane smoke files"
New-Item -ItemType Directory -Force (Join-Path $filesFolder "notes") | Out-Null
$planName = "R$([char]0xE9)sum$([char]0xE9) plan $([char]0xFC).txt"
Set-Content -Encoding UTF8 -LiteralPath (Join-Path $filesFolder $planName) "plan"
Set-Content -Encoding UTF8 -LiteralPath (Join-Path $filesFolder "notes/todo.txt") "todo"
Set-Content -Encoding ASCII -LiteralPath (Join-Path $filesFolder "notes/runner.bat") "@echo ran > `"$filesFixture\runner-ran`""
$openLog = Join-Path $OutDir "opened-file.txt"
if (Test-Path $openLog) { Remove-Item -Force $openLog }
$env:PANE_TEST_CHOOSE_FOLDER = (Resolve-Path -LiteralPath $filesFolder).Path
$env:PANE_TEST_OPEN_FILE_LOG = $openLog
$process = Start-Pane "stderr-files.log" @("--install", "target/guests/packages/files")
Send "{ENTER}"; Start-Sleep -Seconds 2   # Install; Files is selected
Send "{ENTER}"; Start-Sleep -Seconds 3   # open Files; "Choose folder..." is selected
Send "{ENTER}"; Start-Sleep -Seconds 2   # the folder PANE_TEST_CHOOSE_FOLDER names
Capture "220-files-folder-granted.png"
Check "220-files-folder-granted.png" "success"   # "Files may now list "Pane smoke files""
Send "{ESC}"; Start-Sleep -Seconds 1
Send "plan"; Start-Sleep -Seconds 3
Capture "221-files-found.png"
Check "221-files-found.png" "selected" 3000   # the selected file row
Send "{ENTER}"; Start-Sleep -Seconds 3
Capture "222-files-opened.png"
Check "222-files-opened.png" "success"   # "Opened Resume plan u.txt"
if (-not (Test-Path $openLog)) { throw "the handler for files was not asked to open anything" }
$recorded = (Get-Content -Encoding UTF8 -LiteralPath $openLog | Select-Object -First 1)
$expected = (Resolve-Path -LiteralPath (Join-Path $filesFolder $planName)).Path
if ((Resolve-Path -LiteralPath $recorded).Path -ne $expected) { throw "the handler for files was not asked to open the found file: $recorded" }
Remove-Item -Force $openLog
Send "{ESC}"; Start-Sleep -Seconds 1
Send "runner"; Start-Sleep -Seconds 3
Send "{ENTER}"; Start-Sleep -Seconds 2
Capture "223-files-program-refused.png"
Check "223-files-program-refused.png" "error"   # "Could not open runner.bat: it is a program or script, ..."
if (Test-Path $openLog) { throw "the batch file was handed to the handler" }
if (Test-Path (Join-Path $filesFixture "runner-ran")) { throw "the batch file ran" }
$shots = "220-files-folder-granted", "221-files-found", "222-files-opened", "223-files-program-refused" | ForEach-Object { Join-Path $OutDir "$_.png" }
python "$PSScriptRoot/check_screenshot.py" --distinct @shots
if ($LASTEXITCODE -ne 0) { throw "screenshot check failed: file search changed nothing" }
Stop-Pane $process
Remove-Item Env:PANE_TEST_CHOOSE_FOLDER
Remove-Item Env:PANE_TEST_OPEN_FILE_LOG
Remove-Item -Recurse -Force $filesFixture

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
$data = Join-Path $OutDir "search-data"
if (Test-Path $data) { Remove-Item -Recurse -Force $data }
$env:PANE_DATA_DIR = $data
cargo build --locked --quiet -p pane-core --example fixture_service
if ($LASTEXITCODE -ne 0) { throw "could not build the fixture service" }
# Starts the fixture service on `$port` (0: a free one), logging to `$log`;
# the process, and sets $servicePort to the port it listens on.
function Start-FixtureService($log, $port) {
    $path = Join-Path $OutDir $log
    $service = Start-Process -FilePath "target/debug/examples/fixture_service.exe" -ArgumentList "--port", "$port" `
        -PassThru -NoNewWindow -RedirectStandardOutput $path -RedirectStandardError "$path.err"
    for ($i = 0; $i -lt 50; $i++) {
        $listening = if (Test-Path $path) { Select-String -Pattern 'listening on http://127\.0\.0\.1:(\d+)' $path }
        if ($listening) {
            $script:servicePort = $listening.Matches[0].Groups[1].Value
            return $service
        }
        if ($service.HasExited) { break }
        Start-Sleep -Milliseconds 100
    }
    throw "the fixture service did not start (see $path)"
}
$serviceLog = Join-Path $OutDir "fixture-service.log"
$service = Start-FixtureService "fixture-service.log" 0
try {
    $process = Start-Pane "stderr-search.log" @("--install", "target/guests/packages/sample-search")
    Send "{ENTER}"; Start-Sleep -Seconds 2   # Install; Package search is selected
    Capture "160-search-installed.png"
    Check "160-search-installed.png" "success"   # "Installed Search sample"
    Send "aurora"; Start-Sleep -Seconds 2
    Capture "161-root-typed.png"   # root search: "No results for “aurora”"
    if (Select-String -Quiet -Pattern '^GET' $serviceLog) { throw "root search reached the service" }
    Send "{ESC}"; Start-Sleep -Seconds 1   # clears the query
    Send "package search"; Start-Sleep -Seconds 1
    Send "{ENTER}"; Start-Sleep -Seconds 3   # open Package search
    Capture "162-command-opened.png"   # its own list, its search field empty
    Check "162-command-opened.png" "selected" 3000   # its first row, selected
    Send "{DOWN}"; Start-Sleep -Milliseconds 120; Send "{ENTER}"; Start-Sleep -Seconds 2   # Service address: its form
    Send "http://127.0.0.1:$servicePort"
    Send "{ENTER}"; Start-Sleep -Seconds 2   # Save
    Capture "163-service-set.png"   # "Searching http://127.0.0.1:<port> from now on"
    Check "163-service-set.png" "success"
    Send "{ESC}"; Start-Sleep -Seconds 1   # back to the command, its search field empty
    Send "aurora"; Start-Sleep -Seconds 3
    Capture "164-search-results.png"   # aurora-charts, selected, and aurora-cli
    Check "164-search-results.png" "selected" 3000
    if (-not (Select-String -Quiet -Pattern '^GET /search\?q=aurora$' $serviceLog)) { throw "the command's search did not reach the service" }
    Send "{DOWN}"; Start-Sleep -Milliseconds 120; Send "{ENTER}"; Start-Sleep -Seconds 3   # aurora-cli's details
    Capture "165-details.png"
    Check "165-details.png" "success"   # "aurora-cli 0.9.3 (Apache-2.0): Command-line parsing with subcommands"
    Send "^a"; Send "slow"; Start-Sleep -Seconds 2   # held by the service
    Send "^a"; Send "ember"; Start-Sleep -Seconds 3
    Capture "166-newer-search.png"   # ember-tz, not what "slow" would list
    Check "166-newer-search.png" "selected" 3000
    if (-not (Select-String -Quiet -Pattern '^ABANDONED /search\?q=slow$' $serviceLog)) { throw "the replaced search was not stopped" }
    Send "^a"; Send "down"; Start-Sleep -Seconds 3
    Capture "167-service-error.png"
    Check "167-service-error.png" "error"   # "... The service answered 503: the registry is down for maintenance"
    Stop-Process -Id $service.Id; $service.WaitForExit()
    Send "^a"; Send "basalt"; Start-Sleep -Seconds 6   # Windows retries a refused connection for about two seconds
    Capture "168-offline.png"
    Check "168-offline.png" "error"   # "... Could not reach the service at http://127.0.0.1:<port>: connection refused"
    $service = Start-FixtureService "fixture-service-again.log" $servicePort
    Send "^a"; Send "cobalt"; Start-Sleep -Seconds 3
    Capture "169-back-online.png"   # cobalt-http, selected: not paused
    Check "169-back-online.png" "selected" 3000
    $shots = "161-root-typed", "162-command-opened", "163-service-set", "164-search-results", "165-details", "166-newer-search", "167-service-error", "168-offline", "169-back-online" | ForEach-Object { Join-Path $OutDir "$_.png" }
    python "$PSScriptRoot/check_screenshot.py" --distinct @shots
    if ($LASTEXITCODE -ne 0) { throw "screenshot check failed: searching inside the command changed nothing" }
    Stop-Pane $process
} finally {
    if (-not $service.HasExited) { Stop-Process -Id $service.Id }
}

# Clipboard history (#35): the Clipboard History default extension keeps
# nothing until it is turned on in its command (its first item); then the
# text this smoke copies is kept, except text marked as a password manager
# marks it (ExcludeClipboardContentFromMonitorProcessing,
# CanIncludeInClipboardHistory, CanUploadToCloudClipboard); nothing is kept
# while it is paused or the extension is disabled, also after a restart,
# and once enabled again it is kept again, also after a restart. Enter on
# a kept item, then on its first choice, copies it again. The smoke copies only text of its own
# ("pane-smoke-..."), and so replaces what was on the clipboard without
# reading or putting it back: run it on CI's runner or a desktop given to
# it, as the rest of the smoke already takes over the keyboard. A data
# folder of its own.
$data = Join-Path $OutDir "clipboard-data"
if (Test-Path $data) { Remove-Item -Recurse -Force $data }
$env:PANE_DATA_DIR = $data
$registry = Join-Path $data "extensions/installed.json"
$history = Join-Path $data "extensions/clipboard-history.json"
Add-Type @"
using System; using System.Runtime.InteropServices; using System.Text; using System.Threading;
public static class PaneClip {
    [DllImport("user32.dll")] static extern bool OpenClipboard(IntPtr owner);
    [DllImport("user32.dll")] static extern bool CloseClipboard();
    [DllImport("user32.dll")] static extern bool EmptyClipboard();
    [DllImport("user32.dll")] static extern IntPtr GetClipboardData(uint format);
    [DllImport("user32.dll")] static extern IntPtr SetClipboardData(uint format, IntPtr memory);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern uint RegisterClipboardFormatW(string name);
    [DllImport("kernel32.dll")] static extern IntPtr GlobalAlloc(uint flags, UIntPtr bytes);
    [DllImport("kernel32.dll")] static extern IntPtr GlobalLock(IntPtr memory);
    [DllImport("kernel32.dll")] static extern bool GlobalUnlock(IntPtr memory);
    [DllImport("kernel32.dll")] static extern UIntPtr GlobalSize(IntPtr memory);
    const uint CF_UNICODETEXT = 13;
    static void Open() {
        for (int i = 0; i < 50; i++) { if (OpenClipboard(IntPtr.Zero)) return; Thread.Sleep(20); }
        throw new Exception("another application keeps the clipboard open");
    }
    static void Put(uint format, byte[] bytes) {
        IntPtr memory = GlobalAlloc(2, (UIntPtr)Math.Max(bytes.Length, 1));   // GMEM_MOVEABLE
        IntPtr data = GlobalLock(memory);
        Marshal.Copy(bytes, 0, data, bytes.Length);
        GlobalUnlock(memory);
        if (SetClipboardData(format, memory) == IntPtr.Zero) throw new Exception("SetClipboardData failed");
    }
    static byte[] Get(uint format) {
        IntPtr memory = GetClipboardData(format);
        if (memory == IntPtr.Zero) return null;
        IntPtr data = GlobalLock(memory);
        if (data == IntPtr.Zero) return null;
        byte[] bytes = new byte[(int)(ulong)GlobalSize(memory)];
        Marshal.Copy(data, bytes, 0, bytes.Length);
        GlobalUnlock(memory);
        return bytes;
    }
    // Puts text on the clipboard, with the registered format `marker` (a DWORD of 0) if named.
    public static void SetText(string text, string marker) {
        Open();
        try {
            EmptyClipboard();
            Put(CF_UNICODETEXT, Encoding.Unicode.GetBytes(text + "\0"));
            if (!String.IsNullOrEmpty(marker)) Put(RegisterClipboardFormatW(marker), new byte[4]);
        } finally { CloseClipboard(); }
    }
    public static string GetText() {
        Open();
        try {
            byte[] bytes = Get(CF_UNICODETEXT);
            if (bytes == null) return null;
            string text = Encoding.Unicode.GetString(bytes);
            int end = text.IndexOf('\0');
            return end < 0 ? text : text.Substring(0, end);
        } finally { CloseClipboard(); }
    }
}
"@
function Copy-Text($text, $marker) { [PaneClip]::SetText($text, $marker); Start-Sleep -Milliseconds 500 }
# The kept texts, newest first, as clipboard-history.json holds them.
function Kept-Texts {
    if (-not (Test-Path $history)) { return @() }
    $file = Get-Content -Raw $history | ConvertFrom-Json
    # {"version": 1, "packages": {<identity>: {"items": [...newest first]}}}
    $items = foreach ($package in $file.packages.PSObject.Properties) { $package.Value.items }
    return @($items | ForEach-Object { $_.text })
}
function Not-Kept($text) {
    Start-Sleep -Seconds 2
    if ((Kept-Texts) -contains $text) { throw "$text was kept" }
}
# Back to a blank root search from wherever the smoke is, with the return
# to root key (Shift+Escape): Escape at a blank root search hides the
# launcher since the redesign (1e61793), so it cannot be pressed blind.
function To-Root { Send "+{ESC}"; Start-Sleep -Seconds 1 }
function Open-History {
    To-Root
    Send "clipboard"; Start-Sleep -Seconds 1
    Send "{ENTER}"; Start-Sleep -Seconds 2
}
function Open-Manage {
    To-Root
    Send "manage"; Start-Sleep -Seconds 1
    Send "{ENTER}"; Start-Sleep -Seconds 1
}
$process = Start-Pane "stderr-clipboard.log" @("--install", "target/guests/packages/clipboard-history")
Send "{ENTER}"   # Install; Clipboard History is selected
Wait-For $registry "clipboard-history" $true; Start-Sleep -Seconds 1
Copy-Text "pane-smoke-before" $null   # while history is off
Send "{ENTER}"; Start-Sleep -Seconds 3   # open Clipboard History
Capture "280-clipboard-off.png"
Check "280-clipboard-off.png" "subtitle"   # "Off · Pane keeps nothing you copy until you turn it on ..."
Send "{ENTER}"   # Turn on clipboard history
Wait-For $history '"capture": "on"' $true; Start-Sleep -Seconds 1
Capture "281-clipboard-on.png"
Check "281-clipboard-on.png" "success"   # "Clipboard history is on"
Copy-Text "pane-smoke-kept" $null
Copy-Text "pane-smoke-secret" "ExcludeClipboardContentFromMonitorProcessing"
Copy-Text "pane-smoke-no-history" "CanIncludeInClipboardHistory"
Copy-Text "pane-smoke-no-cloud" "CanUploadToCloudClipboard"
Copy-Text "pane-smoke-second" $null
Wait-For $history "pane-smoke-second" $true
if (((Kept-Texts) -join ",") -ne "pane-smoke-second,pane-smoke-kept") { throw "kept: $(Kept-Texts)" }
Open-History
Capture "282-clipboard-kept.png"
Check "282-clipboard-kept.png" "subtitle"   # the two kept items, newest first
Send "{ENTER}"   # Pause clipboard history
Wait-For $history '"capture": "paused"' $true
Copy-Text "pane-smoke-paused" $null
Not-Kept "pane-smoke-paused"
Open-History
Send "{ENTER}"   # Resume clipboard history
Wait-For $history '"capture": "on"' $true
Copy-Text "pane-smoke-resumed" $null
Wait-For $history "pane-smoke-resumed" $true
Open-History
Send "{DOWN 8}"; Start-Sleep -Milliseconds 120; Send "{ENTER}"; Start-Sleep -Seconds 1   # the second kept item, pane-smoke-second, after Pause, Turn off, Keep items for, Exclude, Clear, Turn off and delete, Delete recent and the first
Send "{ENTER}"; Start-Sleep -Seconds 2   # Copy it again, the first of its choices (#36)
Capture "283-clipboard-copied.png"
Check "283-clipboard-copied.png" "success"   # "Copied to the clipboard"
if ([PaneClip]::GetText() -ne "pane-smoke-second") { throw "Enter did not copy the item" }
Start-Sleep -Seconds 1
if ((Kept-Texts)[0] -ne "pane-smoke-second") { throw "the copied item did not move to the front" }
Send "{ESC}"   # from the item's form to the command's list
Open-Manage
Send "{ENTER}"   # disable Clipboard History, the first row
Wait-For $registry '"disabled": true' $true; Start-Sleep -Seconds 1
Capture "284-clipboard-disabled.png"
Check "284-clipboard-disabled.png" "success"   # "Disabled Clipboard History"
Copy-Text "pane-smoke-disabled" $null
Not-Kept "pane-smoke-disabled"
Stop-Pane $process
$process = Start-Pane "stderr-clipboard-disabled.log"
Copy-Text "pane-smoke-restarted-disabled" $null
Not-Kept "pane-smoke-restarted-disabled"
Open-Manage
Send "{ENTER}"   # enable Clipboard History
Wait-For $registry '"disabled": true' $false; Start-Sleep -Seconds 1
Copy-Text "pane-smoke-enabled" $null
Wait-For $history "pane-smoke-enabled" $true
Stop-Pane $process
$process = Start-Pane "stderr-clipboard-restarted.log"
Copy-Text "pane-smoke-after-restart" $null
Wait-For $history "pane-smoke-after-restart" $true
Open-History
Capture "285-clipboard-after-restart.png"
Check "285-clipboard-after-restart.png" "subtitle"   # kept again after the restart
$shots = "280-clipboard-off", "281-clipboard-on", "282-clipboard-kept", "283-clipboard-copied", "284-clipboard-disabled", "285-clipboard-after-restart" | ForEach-Object { Join-Path $OutDir "$_.png" }
python "$PSScriptRoot/check_screenshot.py" --distinct @shots
if ($LASTEXITCODE -ne 0) { throw "screenshot check failed: clipboard history changed nothing" }
Stop-Pane $process
$expected = "pane-smoke-after-restart,pane-smoke-enabled,pane-smoke-second,pane-smoke-resumed,pane-smoke-kept"
if (((Kept-Texts) -join ",") -ne $expected) { throw "kept: $(Kept-Texts)" }
foreach ($never in "before", "secret", "no-history", "no-cloud", "paused", "disabled", "restarted-disabled") {
    if (Select-String -Quiet -SimpleMatch "pane-smoke-$never" $history) { throw "pane-smoke-$never was kept" }
}

# Clipboard history expiry and deletion (#36), on the history just kept.
# With Pane stopped, the smoke makes pane-smoke-kept 8 days old (past the
# default 7-day retention) and pane-smoke-enabled 2 hours old, as a downtime
# would: once Pane starts again, before the command shows anything,
# pane-smoke-kept is gone from the file and the list. Then, in the command:
# Enter on pane-smoke-second and "Delete it" deletes that item alone; Delete
# recent items (the last hour) deletes the two copied in this smoke's last
# minutes and keeps pane-smoke-enabled; keeping items for 1 hour deletes
# pane-smoke-enabled at once; and after one more copy, "Turn off and delete
# clipboard history" deletes it and turns history off, so a later copy is not
# kept. Deleting never changes what is on the clipboard. The rows: Pause,
# Turn off, Keep items for…, Exclude a program, Clear, Turn off and delete,
# Delete recent items, then the items, newest first.
$extensions = Join-Path $data "extensions"
function History-Field($name) { (python "$PSScriptRoot/clipboard_history.py" field $extensions $name) -join "" }
# The kept texts, newest first, joined by commas ("" when none).
function Kept-Joined { (python "$PSScriptRoot/clipboard_history.py" texts $extensions) -join "" }
python "$PSScriptRoot/clipboard_history.py" backdate $extensions 8 pane-smoke-kept
if ($LASTEXITCODE -ne 0) { throw "could not backdate the history" }
python "$PSScriptRoot/clipboard_history.py" backdate $extensions 0.084 pane-smoke-enabled
if ($LASTEXITCODE -ne 0) { throw "could not backdate the history" }
$process = Start-Pane "stderr-clipboard-expiry.log"
Start-Sleep -Seconds 1
if ((Kept-Joined) -ne "pane-smoke-after-restart,pane-smoke-enabled,pane-smoke-second,pane-smoke-resumed") { throw "kept after starting: $(Kept-Joined)" }
Open-History
Capture "400-clipboard-expired.png"
Check "400-clipboard-expired.png" "subtitle"   # pane-smoke-kept is no longer listed
$onClipboard = [PaneClip]::GetText()
Send "{DOWN 9}"; Start-Sleep -Milliseconds 120; Send "{ENTER}"; Start-Sleep -Seconds 1   # pane-smoke-second: Copy it again or Delete it
Send "{DOWN}"; Start-Sleep -Milliseconds 120; Send "{ENTER}"   # Delete it
Wait-For $history "pane-smoke-second" $false; Start-Sleep -Seconds 1
Capture "401-clipboard-item-deleted.png"
Check "401-clipboard-item-deleted.png" "success"   # "Deleted the kept item"
if ((Kept-Joined) -ne "pane-smoke-after-restart,pane-smoke-enabled,pane-smoke-resumed") { throw "kept: $(Kept-Joined)" }
if ([PaneClip]::GetText() -ne $onClipboard) { throw "deleting an item changed the clipboard" }
Send "{ESC}"   # from the item's form to the command's list
Open-History
Send "{DOWN 6}"; Start-Sleep -Milliseconds 120; Send "{ENTER}"; Start-Sleep -Seconds 1   # Delete recent items: 15 minutes, hour or day
Send "{DOWN}"; Start-Sleep -Milliseconds 120; Send "{ENTER}"   # the last hour
Wait-For $history "pane-smoke-resumed" $false; Start-Sleep -Seconds 1
Capture "402-clipboard-recent-deleted.png"
Check "402-clipboard-recent-deleted.png" "success"   # "Deleted 2 kept items"
if ((Kept-Joined) -ne "pane-smoke-enabled") { throw "kept: $(Kept-Joined)" }
Send "{ESC}"
Open-History
Send "{DOWN 2}"; Start-Sleep -Milliseconds 120; Send "{ENTER}"; Start-Sleep -Seconds 1   # Keep items for 7 days: 7 days (the retention now, chosen), 1 hour, 1 day, 30 or 90 days
Send "{DOWN}"; Start-Sleep -Milliseconds 120; Send "{ENTER}"   # 1 hour, the second choice
Wait-For $history '"retentionSeconds": 3600' $true; Start-Sleep -Seconds 1
Capture "403-clipboard-retention-changed.png"
Check "403-clipboard-retention-changed.png" "success"   # "Items are kept for 1 hour; deleted 1 older item"
if ((Kept-Joined) -ne "") { throw "kept: $(Kept-Joined)" }
Copy-Text "pane-smoke-final" $null
Wait-For $history "pane-smoke-final" $true
Send "{ESC}"
Open-History
Send "{DOWN 5}"; Start-Sleep -Milliseconds 120; Send "{ENTER}"   # Turn off and delete clipboard history
Wait-For $history "pane-smoke-final" $false; Start-Sleep -Seconds 1
Capture "404-clipboard-turned-off-and-deleted.png"
Check "404-clipboard-turned-off-and-deleted.png" "success"   # "Clipboard history is off; deleted 1 kept item"
if ((History-Field "capture") -ne "") { throw "history is still $(History-Field 'capture')" }
if ([PaneClip]::GetText() -ne "pane-smoke-final") { throw "deleting history changed the clipboard" }
Copy-Text "pane-smoke-after-off" $null
Not-Kept "pane-smoke-after-off"
$shots = "400-clipboard-expired", "401-clipboard-item-deleted", "402-clipboard-recent-deleted", "403-clipboard-retention-changed", "404-clipboard-turned-off-and-deleted" | ForEach-Object { Join-Path $OutDir "$_.png" }
python "$PSScriptRoot/check_screenshot.py" --distinct @shots
if ($LASTEXITCODE -ne 0) { throw "screenshot check failed: clipboard history expiry and deletion changed nothing" }
Stop-Pane $process
if ((Kept-Joined) -ne "") { throw "kept: $(Kept-Joined)" }
if ((History-Field "retentionSeconds") -ne "3600") { throw "retention: $(History-Field 'retentionSeconds')" }

# Installing Pane and acquiring its calculator (#51): the package
# `cargo xtask package-windows --dev` builds is installed on a clean
# machine — a fresh user profile (LOCALAPPDATA and APPDATA pointing into
# the smoke's own output folder, so the install, Pane's data and the
# shortcut touch nothing of the runner's user) and a PATH that holds
# nothing at all, so no Rust, Node, npm, Git or compiler can be reached —
# and Pane, started from what the install script installed, acquires its
# default extensions (the calculator, and the prebuilt-helper sample with
# it) from the artifact source this smoke serves on 127.0.0.1
# (scripts/artifact_server.py, the payloads `cargo xtask package-windows`
# assembled; nothing reaches the network or Pane's published downloads).
# The calculator answers "6*7" with 42, and the helper sample's pane-echo
# runs: a prebuilt program from the acquired payload, no developer tool
# anywhere. The package is the development profile, because only a
# development build takes its artifact source from PANE_ARTIFACTS; a
# release build uses Pane's published downloads, which no controlled
# source may replace. (The program files are removed again at the end of
# the phase: the uploaded evidence is the screenshots and records, not the
# program.)
# The task's own output goes to CI's log, as the smoke's other cargo runs'
# do (the runner's console), not to a file of the smoke's.
cargo xtask package-windows --dev
if ($LASTEXITCODE -ne 0) { throw "the package was not built" }
$package = Get-ChildItem "target/dist/pane-*-windows-*-dev.zip" | Select-Object -First 1
if (-not $package) { throw "the package was not built" }
# Not $PROFILE: PowerShell fills that variable with the user's own
# profile script's path.
$cleanProfile = Join-Path $OutDir "clean-profile"
$unpack = Join-Path $OutDir "package-unpacked"
foreach ($folder in $cleanProfile, $unpack) { if (Test-Path $folder) { Remove-Item -Recurse -Force $folder } }
New-Item -ItemType Directory -Force -Path $cleanProfile, $unpack | Out-Null
$portFile = Join-Path $OutDir "artifact-server.port"
if (Test-Path $portFile) { Remove-Item -Force $portFile }
$server = Start-Process python -PassThru -NoNewWindow `
    -ArgumentList @("`"$PSScriptRoot/artifact_server.py`"", "target/dist/artifacts", "`"$portFile`"") `
    -RedirectStandardError (Join-Path $OutDir "artifact-server.log")
# What a clean machine's environment is: saved, put back in the finally
# below. PANE_DATA_DIR comes off, so the installed Pane keeps its data
# where a real one does, in the fresh profile's LOCALAPPDATA.
$realAppData = $env:APPDATA; $realLocal = $env:LOCALAPPDATA; $realPath = $env:PATH
Remove-Item Env:PANE_DATA_DIR -ErrorAction SilentlyContinue
try {
    # Generous: a slow runner may take seconds to start Python.
    for ($i = 0; $i -lt 600 -and -not (Test-Path $portFile) -and -not $server.HasExited; $i++) { Start-Sleep -Milliseconds 100 }
    if (-not (Test-Path $portFile)) { throw "the local artifact source did not start (see artifact-server.log)" }
    Expand-Archive -Path $package.FullName -DestinationPath $unpack
    $env:LOCALAPPDATA = Join-Path $cleanProfile "Local"
    $env:APPDATA = Join-Path $cleanProfile "Roaming"
    New-Item -ItemType Directory -Force -Path $env:LOCALAPPDATA, $env:APPDATA | Out-Null
    # As the README says a user runs it (nothing is signed, so the policy
    # is bypassed for this one script); PowerShell 5.1 stands in if this
    # smoke is not run with pwsh.
    $shell = if (Get-Command pwsh -ErrorAction SilentlyContinue) { "pwsh" } else { "powershell" }
    & $shell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $unpack "pane\install.ps1") *>> (Join-Path $OutDir "install.log")
    if ($LASTEXITCODE -ne 0) { throw "the install script failed (see install.log)" }
    $install = Join-Path $env:LOCALAPPDATA "Pane"
    $installed = Join-Path $install "pane.exe"
    if (-not (Test-Path $installed)) { throw "the install script installed no pane" }
    if (-not (Test-Path (Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs\Pane.lnk"))) { throw "the install script made no shortcut" }
    # Nothing can be reached at all from the PATH Pane runs with: an empty
    # folder, so no development tool resolves (Start-Process hands the
    # child this process's environment, which was checked; a running
    # process's own environment cannot be read on Windows).
    $cleanBin = Join-Path $OutDir "clean-bin"
    if (Test-Path $cleanBin) { Remove-Item -Recurse -Force $cleanBin }
    New-Item -ItemType Directory -Force -Path $cleanBin | Out-Null
    $env:PATH = $cleanBin
    $tools = Get-Command cargo, rustc, node, npm, git, cc, clang, make -ErrorAction SilentlyContinue
    if ($tools) { throw "the clean machine still reaches a development tool" }
    $env:PANE_ARTIFACTS = "http://127.0.0.1:$((Get-Content $portFile).Trim())/"
    $process = Start-Pane "stderr-installed.log" @() $installed
    $env:PATH = $realPath
    $extensions = Join-Path $install "data\extensions"
    # Generous: a slow runner may take a while to check every payload's
    # components (120 s each).
    if ($process.HasExited) { throw "the installed Pane exited during setup" }
    # The release's default extensions (#60): all five, plus the helper
    # sample a development build acquires with them.
    foreach ($default in "calculator", "applications", "quicklinks", "files", "clipboard-history", "helper-sample") {
        Wait-For (Join-Path $extensions "installed.json") ('"default": "' + $default + '"') $true 1200
    }
    Start-Sleep -Seconds 1
    Capture "500-installed-root.png"
    Check "500-installed-root.png" "subtitle"   # root search: the default extensions' commands are listed
    Send "6*7"; Start-Sleep -Seconds 2
    Capture "501-calculator-answer.png"
    Check "501-calculator-answer.png" "answer"   # "42", the calculator's selected answer card
    Send "{ENTER}"; Start-Sleep -Seconds 1
    Capture "502-calculator-copied.png"
    Check "502-calculator-copied.png" "success"   # "Copied 42 to the clipboard"
    Send "^a"; Send "helper"; Start-Sleep -Seconds 1
    Send "{ENTER}"; Start-Sleep -Seconds 2   # Helper sample
    Send "{ENTER}"; Start-Sleep -Seconds 3   # "Echo through the helper"
    Capture "503-helper-echoed.png"
    Check "503-helper-echoed.png" "success"   # 'Echoed "hello from Pane" on Windows x86-64'
    if (-not (Get-ChildItem (Join-Path $extensions "packages\*\helpers\*\pane-echo.exe") -ErrorAction SilentlyContinue)) {
        throw "the acquired payload's helper was not installed"
    }
    if (Get-Process -Name "pane-echo" -ErrorAction SilentlyContinue) { throw "a helper is still running" }
    if ((Get-ChildItem (Join-Path $extensions "acquired\calculator")).Count -ne 1) { throw "the calculator's payload is not cached" }
    $downloads = Join-Path $extensions "downloads"
    if ((Test-Path $downloads) -and (Get-ChildItem $downloads)) { throw "downloads were left behind" }
    $shots = "500-installed-root", "501-calculator-answer", "503-helper-echoed" | ForEach-Object { Join-Path $OutDir "$_.png" }
    python "$PSScriptRoot/check_screenshot.py" --distinct @shots
    if ($LASTEXITCODE -ne 0) { throw "screenshot check failed: the installed Pane changed nothing" }
    Stop-Pane $process
    # The program files go again: the evidence is the screenshots, the
    # installed.json record and the logs.
    Remove-Item -Force $installed
    Remove-Item -Force (Join-Path $unpack "pane\pane.exe")
} finally {
    Stop-Process -Id $server.Id -ErrorAction SilentlyContinue
    Remove-Item Env:PANE_ARTIFACTS -ErrorAction SilentlyContinue
    $env:PATH = $realPath
    $env:APPDATA = $realAppData
    $env:LOCALAPPDATA = $realLocal
}

# Installing a Pane application update by the user's choice (#54): a
# second package is built with --package-version 99.0.0, whose program
# reports 99.0.0 and whose index entry names it (the two runnable builds
# the update goes between); the 0.1.0 package the #51 phase built is
# installed on another clean profile, and its Pane, running from
# %LOCALAPPDATA%\Pane\pane.exe, is told by the check it makes at start
# that 99.0.0 exists: a row in root search with the version, and a word on
# the status line. Nothing is downloaded until that row is chosen; the
# choice is proven by the artifact server's log, which must hold no
# request for the package until then. A corrupted package is explained
# first (its bytes do not match the sha512 its index gives), everything
# untouched and the row ready to try again; then the real install
# downloads the package, checks it, and swaps the running pane.exe — the
# old one renamed pane.exe.old, removed on a later start — so the new
# version is used the next time Pane starts (Pane never restarts itself).
# The new Pane, started again, reports 99.0.0, with the old version's
# data (the calculator acquired at first setup) and the extension the
# user disabled kept, and with nothing of the update left in the install
# folder.
cargo xtask package-windows --dev --package-version 99.0.0
if ($LASTEXITCODE -ne 0) { throw "the update package was not built" }
$older = Get-ChildItem "target/dist/pane-0.1.0-windows-*-dev.zip" | Select-Object -First 1
$newer = Get-ChildItem "target/dist/pane-99.0.0-windows-*-dev.zip" | Select-Object -First 1
if (-not $older -or -not $newer) { throw "the two packages were not built" }
$cleanProfile = Join-Path $OutDir "update-profile"
$unpackOld = Join-Path $OutDir "update-unpacked-old"
$unpackNew = Join-Path $OutDir "update-unpacked-new"
foreach ($folder in $cleanProfile, $unpackOld, $unpackNew) {
    if (Test-Path $folder) { Remove-Item -Recurse -Force $folder }
}
New-Item -ItemType Directory -Force -Path $cleanProfile, $unpackOld, $unpackNew | Out-Null
Expand-Archive -Path $older.FullName -DestinationPath $unpackOld
Expand-Archive -Path $newer.FullName -DestinationPath $unpackNew
$portFile = Join-Path $OutDir "update-artifact-server.port"
if (Test-Path $portFile) { Remove-Item -Force $portFile }
$serverLog = Join-Path $OutDir "update-artifact-server.log"
$server = Start-Process python -PassThru -NoNewWindow `
    -ArgumentList @("`"$PSScriptRoot/artifact_server.py`"", "target/dist/artifacts", "`"$portFile`"") `
    -RedirectStandardError $serverLog
$realAppData = $env:APPDATA; $realLocal = $env:LOCALAPPDATA; $realPath = $env:PATH
try {
    for ($i = 0; $i -lt 600 -and -not (Test-Path $portFile) -and -not $server.HasExited; $i++) { Start-Sleep -Milliseconds 100 }
    if (-not (Test-Path $portFile)) { throw "the update artifact source did not start (see update-artifact-server.log)" }
    # The 0.1.0 package installed on another clean profile, as the #51
    # phase installed it (nothing is signed, so the policy is bypassed for
    # the one script).
    $env:LOCALAPPDATA = Join-Path $cleanProfile "Local"
    $env:APPDATA = Join-Path $cleanProfile "Roaming"
    New-Item -ItemType Directory -Force -Path $env:LOCALAPPDATA, $env:APPDATA | Out-Null
    $shell = if (Get-Command pwsh -ErrorAction SilentlyContinue) { "pwsh" } else { "powershell" }
    & $shell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $unpackOld "pane\install.ps1") *>> (Join-Path $OutDir "update-install.log")
    if ($LASTEXITCODE -ne 0) { throw "the install script failed (see update-install.log)" }
    $install = Join-Path $env:LOCALAPPDATA "Pane"
    $installed = Join-Path $install "pane.exe"
    $extensions = Join-Path $install "data\extensions"
    $registry = Join-Path $extensions "installed.json"
    # A PATH that holds nothing, and the controlled artifact source.
    $cleanBin = Join-Path $OutDir "update-clean-bin"
    if (Test-Path $cleanBin) { Remove-Item -Recurse -Force $cleanBin }
    New-Item -ItemType Directory -Force -Path $cleanBin | Out-Null
    $env:PATH = $cleanBin
    $env:PANE_ARTIFACTS = "http://127.0.0.1:$((Get-Content $portFile).Trim())/"
    $process = Start-Pane "update-stderr-0.1.0.log" @() $installed
    $env:PATH = $realPath
    # First setup: the default extensions are acquired (one index read
    # per payload), and Pane's own check reads the index once more.
    if ($process.HasExited) { throw "the installed Pane exited during setup" }
    # The release's default set (#60) plus the helper sample.
    foreach ($default in "calculator", "applications", "quicklinks", "files", "clipboard-history", "helper-sample") {
        Wait-For $registry ('"default": "' + $default + '"') $true 1200
    }
    # The check has read the index (its request is the third): the offer
    # is in root search. The status line tells what it found; nothing has
    # been downloaded.
    for ($i = 0; $i -lt 100; $i++) {
        if ((Select-String -SimpleMatch "pane-defaults.json" $serverLog).Count -ge 3) { break }
        Start-Sleep -Milliseconds 100
    }
    if ((Select-String -SimpleMatch "pane-defaults.json" $serverLog).Count -lt 3) { throw "Pane never checked for its own update" }
    Start-Sleep -Seconds 2
    Capture "600-notification.png"
    Check "600-notification.png" "success"   # "Pane 99.0.0 is available" (or the setup's own outcome)
    Send "^a"; Send "update"; Start-Sleep -Seconds 1
    Capture "601-offered.png"
    Check "601-offered.png" "subtitle"   # the offer row: "Your extensions and settings are kept; ..."
    Check "601-offered.png" "selected" 3000   # the row, selected
    # Taking no action downloads nothing: no package was asked for.
    if (Select-String -SimpleMatch ".zip" $serverLog) { throw "a package was downloaded without the user choosing it" }

    # Disable the Helper sample first: an extension the user disabled
    # before the update must stay disabled after it.
    Send "^a"; Send "manage"; Start-Sleep -Seconds 1
    Send "{ENTER}"; Start-Sleep -Seconds 1   # Manage extensions…
    # The Helper sample is the sixth extension now (#60's set is listed
    # first), so five Downs reach it.
    Send "{DOWN 5}"; Start-Sleep -Milliseconds 120; Send "{ENTER}"; Start-Sleep -Seconds 2   # Helper sample: disabled
    Wait-For $registry '"disabled": true' $true
    Send "{ESC}"; Start-Sleep -Seconds 1

    # A package that does not match the integrity its index gives is
    # explained and not installed: the bytes of the served package are
    # damaged, and the program keeps running the one it was.
    $served = Join-Path "target/dist/artifacts" $newer.Name
    $bytes = [IO.File]::ReadAllBytes($served)
    $at = [int]($bytes.Length / 2)
    $bytes[$at] = $bytes[$at] -bxor 1
    [IO.File]::WriteAllBytes($served, $bytes)
    Send "^a"; Send "update"; Start-Sleep -Seconds 1
    Send "{ENTER}"; Start-Sleep -Seconds 10
    Capture "602-corrupt-package.png"
    Check "602-corrupt-package.png" "error"   # "Could not update Pane to 99.0.0: ... does not match the sha512 integrity"
    if (Test-Path (Join-Path $install "pane.exe.old")) { throw "a failed install replaced the program" }
    if ((Get-FileHash $installed).Hash -ne (Get-FileHash (Join-Path $unpackOld "pane\pane.exe")).Hash) {
        throw "a failed install changed the program"
    }
    if (Test-Path (Join-Path $install "update")) { throw "a failed install left its staging behind" }

    # The source works again; the row that stays tries again, and the
    # update is installed: the new program takes the old one's name and
    # place, the old one is renamed out of its way.
    Copy-Item $newer.FullName $served -Force
    Send "^a"; Send "update"; Start-Sleep -Seconds 1
    Send "{ENTER}"
    for ($i = 0; $i -lt 1200 -and -not (Test-Path (Join-Path $install "pane.exe.old")); $i++) {
        if ($process.HasExited) { throw "Pane exited while updating itself" }
        Start-Sleep -Milliseconds 200
    }
    if (-not (Test-Path (Join-Path $install "pane.exe.old"))) { throw "the update was not installed" }
    Start-Sleep -Seconds 2
    Capture "603-installed.png"
    Check "603-installed.png" "success"   # "Installed Pane 99.0.0; the new version is used the next time Pane starts"
    if ((Get-FileHash $installed).Hash -ne (Get-FileHash (Join-Path $unpackNew "pane\pane.exe")).Hash) {
        throw "the new program was not installed"
    }
    if ((Get-FileHash (Join-Path $install "pane.exe.old")).Hash -ne (Get-FileHash (Join-Path $unpackOld "pane\pane.exe")).Hash) {
        throw "the old program was not kept out of the new one's way"
    }
    if (Test-Path (Join-Path $install "update")) { throw "the install left its staging behind" }
    # The package was downloaded once for each attempt: the damaged one
    # and the one that installed.
    if ((Select-String -SimpleMatch ".zip" $serverLog).Count -ne 2) { throw "the package was not downloaded exactly twice" }
    Stop-Pane $process

    # The next start runs the new version: it reports 99.0.0, removes what
    # the update left, and the old version's data is kept — the
    # calculator answers and the Helper sample stays disabled.
    $version = Join-Path $OutDir "update-version.txt"
    $check = Start-Process -FilePath $installed -ArgumentList "--version" -Wait -PassThru -RedirectStandardOutput $version
    if ($check.ExitCode -ne 0) { throw "the new pane.exe --version failed" }
    if (((Get-Content $version) -join "") -ne "Pane 99.0.0") { throw "the new program reports the wrong version" }
    $env:PATH = $cleanBin
    $process = Start-Pane "update-stderr-99.0.0.log" @() $installed
    $env:PATH = $realPath
    Wait-For (Join-Path $install "pane.exe.old") "x" $false 100
    if (Test-Path (Join-Path $install "pane.exe.old")) { throw "the old program's file was not removed on the new start" }
    Send "^a"; Send "6*7"; Start-Sleep -Seconds 2
    Capture "604-answer-after-update.png"
    Check "604-answer-after-update.png" "answer"   # "42", the calculator's selected answer card
    Send "{ENTER}"; Start-Sleep -Seconds 1
    Capture "605-copied-after-update.png"
    Check "605-copied-after-update.png" "success"   # "Copied 42 to the clipboard"
    Wait-For $registry '"disabled": true' $true
    $shots = "600-notification", "601-offered", "602-corrupt-package", "603-installed", "604-answer-after-update" |
        ForEach-Object { Join-Path $OutDir "$_.png" }
    python "$PSScriptRoot/check_screenshot.py" --distinct @shots
    if ($LASTEXITCODE -ne 0) { throw "screenshot check failed: the update changed nothing" }
    Stop-Pane $process
    # The program files go again, as the #51 phase's do.
    Remove-Item -Force $installed
} finally {
    Stop-Process -Id $server.Id -ErrorAction SilentlyContinue
    Remove-Item Env:PANE_ARTIFACTS -ErrorAction SilentlyContinue
    $env:PATH = $realPath
    $env:APPDATA = $realAppData
    $env:LOCALAPPDATA = $realLocal
}
Write-Output "screenshots in $OutDir"
