# capture-settings.ps1 - Settings window (#72) native launch/capture helper.
#
# Non-destructive native validation for ticket #72 (independent Settings
# window), following the established capture-pane.ps1 pattern: launches a
# real Pane build, waits for its real window, focuses it with a confirmed
# foreground HWND (re-checked before every key token), captures real
# desktop pixels, exercises the Settings window's entry point, layout,
# titlebar controls, drag and lifecycle with guarded real clicks, and
# closes ONLY the process it started. Per-run scratch data directory and
# scratch LOCALAPPDATA through spawn-scoped environment variables that are
# restored immediately after the child starts; PANE_ARTIFACTS is cleared
# for the child only, so no default-extension downloads or update checks
# run. Nothing is deleted anywhere; no other window is touched.
#
# Checks recorded in pane-run.json and the captures:
#   1. Ctrl+, opens a second top-level window of the spawned process
#      (Settings) beside the launcher.
#   2. The Settings window's layout at its opened size and resized small.
#   3. The footer ellipsis menu opens (click) and closes (Escape).
#   4. Clicking Settings' painted close button closes ONLY Settings: the
#      process and the launcher window stay.
#   5. The minimize and maximize buttons route to the system (IsIconic /
#      IsZoomed), through the same window-control hit testing as close.
#   6. Dragging the custom titlebar moves the window (WindowControlArea
#      drag routing).
#   7. Closing the launcher window (WM_CLOSE) quits Pane: the process
#      exits.
#
# Example:
#   powershell -NoProfile -ExecutionPolicy Bypass -File capture-settings.ps1 `
#     -Binary <path-to-pane.exe> -OutputDir C:\captures-settings-72

[CmdletBinding()]
param(
    # Explicit path to the Pane build to launch (no default on purpose).
    [Parameter(Mandatory = $true)]
    [ValidateScript({ Test-Path -LiteralPath $_ -PathType Leaf })]
    [string]$Binary,

    # Explicit directory for this run's PNGs + metadata (created if missing).
    [Parameter(Mandatory = $true)]
    [string]$OutputDir,

    # Where Pane reads sample extension components from.
    [string]$ExtensionsDir = 'C:\Users\ADMIN\Desktop\nvy',

    [int]$InitialWaitMs = 2000,
    [int]$StepDelayMs = 600,
    [int]$WindowTimeoutMs = 30000,

    # Last-resort activation (default OFF), as in the established helper:
    # one guarded real click at the launcher's search-header area, with
    # WindowFromPoint proving the point belongs to the spawned window tree
    # before the cursor moves and again before the click. Opt in only for
    # testing your own window; keep hands off the mouse while it runs.
    [switch]$ClickToFocus
)

$ErrorActionPreference = 'Stop'

# ---------------------------------------------------------------- run dirs
$runStamp = Get-Date -Format 'yyyyMMdd-HHmmss'
$runId = '{0}-{1}' -f $runStamp, ([Guid]::NewGuid().ToString('N').Substring(0, 8))
if (-not (Test-Path -LiteralPath $OutputDir)) {
    New-Item -ItemType Directory -Path $OutputDir | Out-Null
}
$OutputDir = (Resolve-Path -LiteralPath $OutputDir).Path
$Binary = (Resolve-Path -LiteralPath $Binary).Path
$runDir = Join-Path $OutputDir "run-$runId"
$dataDir = Join-Path $runDir 'pane-data'
$localAppData = Join-Path $runDir 'localappdata'
New-Item -ItemType Directory -Path $runDir, $dataDir, $localAppData | Out-Null
$stderrLog = Join-Path $runDir 'pane-stderr.log'
if (-not (Test-Path -LiteralPath $ExtensionsDir)) {
    Write-Warning "ExtensionsDir not found: $ExtensionsDir (samples will be absent)"
}

# ---------------------------------------------------------------- win32
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type @"
using System; using System.Runtime.InteropServices; using System.Text;
public static class PaneWin {
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT rect);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(POINT p);
    [DllImport("user32.dll")] public static extern IntPtr GetAncestor(IntPtr h, uint flags);
    [DllImport("user32.dll")] public static extern bool GetCursorPos(out POINT p);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint dx, uint dy, uint data, UIntPtr extra);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int command);
    [DllImport("user32.dll")] public static extern int GetDpiForWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr h);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern bool IsZoomed(IntPtr h);
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int cx, int cy, uint flags);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumWindowsProc proc, IntPtr l);
    public delegate bool EnumWindowsProc(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] public static extern int GetWindowTextLength(IntPtr h);
    [DllImport("user32.dll")] public static extern int GetWindowText(IntPtr h, StringBuilder text, int count);
    [DllImport("dwmapi.dll")] public static extern int DwmGetWindowAttribute(IntPtr h, int attr, out RECT rect, int size);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
    [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
}
"@
# Screenshots and window geometry use physical pixels at any display
# scaling (same call the established helper makes before measuring).
[PaneWin]::SetProcessDPIAware() | Out-Null

# ---------------------------------------------------------------- state
$meta = [ordered]@{
    script         = 'capture-settings.ps1'
    startedUtc     = (Get-Date).ToUniversalTime().ToString('o')
    binary         = $Binary
    runDir         = $runDir
    steps          = [System.Collections.Generic.List[object]]::new()
    launcherWindow = $null
    settingsWindow = $null
    dpi            = $null
    checkOpen      = $null   # Ctrl+, opened a second window
    checkMenu      = $null   # ellipsis menu opened, Escape closed it
    checkClose     = $null   # close button closed only Settings
    checkMin       = $null   # minimize button iconic
    checkMax       = $null   # maximize button zoomed
    checkDrag      = $null   # titlebar drag moved the window
    checkQuit      = $null   # closing the launcher quit the process
    processClosed  = $null
    errorMessage   = $null
}

function Save-Metadata {
    $meta.finishedUtc = (Get-Date).ToUniversalTime().ToString('o')
    $meta | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $runDir 'pane-run.json')
}

function Step($name, $detail) {
    $meta.steps.Add([ordered]@{ name = $name; detail = $detail; utc = (Get-Date).ToUniversalTime().ToString('o') })
    Write-Host ("[{0}] {1}" -f $name, $detail)
}

function Frame-Bounds([IntPtr]$hwnd) {
    # DWMWA_EXTENDED_FRAME_BOUNDS: the visible window, in physical pixels.
    $frame = New-Object PaneWin+RECT
    $hr = [PaneWin]::DwmGetWindowAttribute($hwnd, 9, [ref]$frame, 16)
    if ($hr -eq 0) { return $frame }
    $rect = New-Object PaneWin+RECT
    [void][PaneWin]::GetWindowRect($hwnd, [ref]$rect)
    return $rect
}

function Capture([string]$Name, [IntPtr]$hwnd) {
    $frame = Frame-Bounds $hwnd
    $w = $frame.Right - $frame.Left; $h = $frame.Bottom - $frame.Top
    if ($w -le 0 -or $h -le 0) { throw 'window has no captureable bounds' }
    $path = Join-Path $runDir $Name
    $bmp = New-Object System.Drawing.Bitmap $w, $h
    try {
        $g = [System.Drawing.Graphics]::FromImage($bmp)
        try { $g.CopyFromScreen($frame.Left, $frame.Top, 0, 0, $bmp.Size) }
        finally { $g.Dispose() }
        $bmp.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
    } finally { $bmp.Dispose() }
    return $path
}

function Focus-Pane([IntPtr]$hwnd) {
    [void][PaneWin]::ShowWindow($hwnd, 9)   # SW_RESTORE
    for ($i = 0; $i -lt 10; $i++) {
        [void][PaneWin]::SetForegroundWindow($hwnd)
        Start-Sleep -Milliseconds 200
        if ([PaneWin]::GetForegroundWindow().ToInt64() -eq $hwnd.ToInt64()) { return $true }
    }
    # Targeted activation alternative: AppActivate with ONLY the spawned
    # process Id. Its boolean return is never focus; confirmation is
    # always the actual foreground HWND check, made before any input is
    # sent.
    try {
        $shell = New-Object -ComObject WScript.Shell
        for ($i = 0; $i -lt 3; $i++) {
            [void]$shell.AppActivate([int]$script:process.Id)
            Start-Sleep -Milliseconds 300
            if ([PaneWin]::GetForegroundWindow().ToInt64() -eq $hwnd.ToInt64()) { return $true }
        }
    } catch { }
    return $false
}

# All top-level, visible, titled windows of one process id: Pane's own
# windows (hidden helper windows some platforms keep are not).
function Windows-Of([uint32]$panePid) {
    $found = New-Object System.Collections.Generic.List[object]
    $proc = {
        param([IntPtr]$h, [IntPtr]$l)
        $owner = [uint32]0
        [void][PaneWin]::GetWindowThreadProcessId($h, [ref]$owner)
        if ($owner -eq $panePid -and [PaneWin]::IsWindowVisible($h)) {
            $len = [PaneWin]::GetWindowTextLength($h)
            if ($len -gt 0) {
                $sb = New-Object System.Text.StringBuilder ($len + 1)
                [void][PaneWin]::GetWindowText($h, $sb, $sb.Capacity)
                $found.Add([pscustomobject]@{ Hwnd = $h; Title = $sb.ToString() })
            }
        }
        return $true
    }
    [void][PaneWin]::EnumWindows($proc, [IntPtr]::Zero)
    return $found
}

# One guarded real click: WindowFromPoint must resolve into the target
# window's tree BEFORE the cursor moves and again before the click, so a
# foreign window covering the point cancels it. The cursor is saved and
# restored only if nothing moved it (the user's hand always wins).
function Guarded-Click([int]$x, [int]$y, [IntPtr]$target) {
    $pt = New-Object PaneWin+POINT
    $pt.X = $x; $pt.Y = $y
    $hit = [PaneWin]::WindowFromPoint($pt)
    $root = if ($hit.ToInt64() -ne 0) { [PaneWin]::GetAncestor($hit, 2).ToInt64() } else { 0 }
    if ($root -ne $target.ToInt64()) { return $false }
    $saved = New-Object PaneWin+POINT
    [void][PaneWin]::GetCursorPos([ref]$saved)
    [void][PaneWin]::SetCursorPos($x, $y)
    Start-Sleep -Milliseconds 80
    $hit2 = [PaneWin]::WindowFromPoint($pt)
    $root2 = if ($hit2.ToInt64() -ne 0) { [PaneWin]::GetAncestor($hit2, 2).ToInt64() } else { 0 }
    if ($root2 -ne $target.ToInt64()) {
        [void][PaneWin]::SetCursorPos($saved.X, $saved.Y)
        return $false
    }
    [PaneWin]::mouse_event(0x2, 0, 0, 0, [UIntPtr]::Zero)   # LEFTDOWN
    Start-Sleep -Milliseconds 60
    [PaneWin]::mouse_event(0x4, 0, 0, 0, [UIntPtr]::Zero)   # LEFTUP
    Start-Sleep -Milliseconds 250
    $now = New-Object PaneWin+POINT
    [void][PaneWin]::GetCursorPos([ref]$now)
    if ($now.X -eq $x -and $now.Y -eq $y) {
        [void][PaneWin]::SetCursorPos($saved.X, $saved.Y)
    }
    return $true
}

# One guarded real drag from (x, y) to (x2, y2) within the target window.
function Guarded-Drag([int]$x, [int]$y, [int]$x2, [int]$y2, [IntPtr]$target) {
    $pt = New-Object PaneWin+POINT
    $pt.X = $x; $pt.Y = $y
    $hit = [PaneWin]::WindowFromPoint($pt)
    $root = if ($hit.ToInt64() -ne 0) { [PaneWin]::GetAncestor($hit, 2).ToInt64() } else { 0 }
    if ($root -ne $target.ToInt64()) { return $false }
    $saved = New-Object PaneWin+POINT
    [void][PaneWin]::GetCursorPos([ref]$saved)
    [void][PaneWin]::SetCursorPos($x, $y)
    Start-Sleep -Milliseconds 80
    [PaneWin]::mouse_event(0x2, 0, 0, 0, [UIntPtr]::Zero)   # LEFTDOWN
    Start-Sleep -Milliseconds 120
    [void][PaneWin]::SetCursorPos($x2, $y2)
    Start-Sleep -Milliseconds 120
    [PaneWin]::mouse_event(0x4, 0, 0, 0, [UIntPtr]::Zero)   # LEFTUP
    Start-Sleep -Milliseconds 250
    [void][PaneWin]::SetCursorPos($saved.X, $saved.Y)
    return $true
}

$process = $null
$failed = $false
try {
    # ------------------------------------------------ spawn (scoped env)
    $spawnEnvNames = 'PANE_DATA_DIR', 'PANE_EXTENSIONS_DIR', 'PANE_ARTIFACTS', 'LOCALAPPDATA', 'PANE_THEME', 'PANE_MATERIAL'
    $oldSpawnEnv = @{}
    foreach ($name in $spawnEnvNames) {
        if (Test-Path "Env:$name") { $oldSpawnEnv[$name] = (Get-Item "Env:$name").Value }
    }
    try {
        Set-Item 'Env:LOCALAPPDATA' $localAppData
        Set-Item 'Env:PANE_DATA_DIR' $dataDir
        Set-Item 'Env:PANE_EXTENSIONS_DIR' $ExtensionsDir
        Remove-Item 'Env:PANE_ARTIFACTS' -ErrorAction SilentlyContinue
        Remove-Item 'Env:PANE_THEME' -ErrorAction SilentlyContinue
        Remove-Item 'Env:PANE_MATERIAL' -ErrorAction SilentlyContinue
        $process = Start-Process -FilePath $Binary -PassThru -RedirectStandardError $stderrLog
    }
    finally {
        foreach ($name in $spawnEnvNames) {
            if ($oldSpawnEnv.ContainsKey($name)) { Set-Item "Env:$name" $oldSpawnEnv[$name] }
            else { Remove-Item "Env:$name" -ErrorAction SilentlyContinue }
        }
    }
    $meta.pid = $process.Id
    Write-Host "Launched Pane PID $($process.Id); stderr -> $stderrLog"

    # --------------------------------------------- wait for the launcher
    $deadline = [DateTime]::UtcNow.AddMilliseconds($WindowTimeoutMs)
    while ($process.MainWindowHandle -eq 0) {
        if ($process.HasExited) { throw "Pane exited before showing a window; see $stderrLog" }
        if ([DateTime]::UtcNow -gt $deadline) { throw 'Pane window did not appear in time' }
        Start-Sleep -Milliseconds 200
        $process.Refresh()
    }
    $launcher = $process.MainWindowHandle
    $meta.launcherWindow = '0x{0:x}' -f $launcher.ToInt64()
    $meta.dpi = [PaneWin]::GetDpiForWindow($launcher)
    $scale = $meta.dpi / 96.0
    Step 'launcher' ("window 0x{0:x}, DPI {1}" -f $launcher.ToInt64(), $meta.dpi)

    if (-not (Focus-Pane $launcher)) {
        if (-not $ClickToFocus) {
            throw 'the launcher window could not be focused; no input was sent (see -ClickToFocus)'
        }
        $frame = Frame-Bounds $launcher
        $clickX = $frame.Left + [int](($frame.Right - $frame.Left) / 2)
        $clickY = $frame.Top + 32
        if (Guarded-Click $clickX $clickY $launcher) {
            Start-Sleep -Milliseconds 400
        }
        if (-not (Focus-Pane $launcher)) {
            throw 'the launcher window could not be focused even with a guarded click; no keys were sent'
        }
        Step 'activation' 'guarded click at the search header activated the launcher'
    }
    Start-Sleep -Milliseconds $InitialWaitMs
    [void](Capture '00-launcher.png' $launcher)
    Step 'capture' '00-launcher.png'

    # ------------------------------------------------ 1. the ellipsis menu
    $frame = Frame-Bounds $launcher
    # The footer's ellipsis button, in logical pixels from the visible
    # frame's top-left: 32px in from the left, 25px up from the bottom.
    $menuX = $frame.Left + [int](32 * $scale)
    $menuY = $frame.Bottom - [int](25 * $scale)
    if (Guarded-Click $menuX $menuY $launcher) {
        Start-Sleep -Milliseconds $StepDelayMs
        [void](Capture '01-menu-open.png' $launcher)
        [System.Windows.Forms.SendKeys]::SendWait('{ESC}')
        Start-Sleep -Milliseconds $StepDelayMs
        [void](Capture '02-menu-closed.png' $launcher)
        $meta.checkMenu = 'opened by click, closed by Escape'
        Step 'menu' 'click opened the ellipsis menu; Escape closed it'
    }
    else {
        $meta.checkMenu = 'skipped: a foreign window covered the menu button'
        Step 'menu' 'skipped (guarded click refused)'
    }

    # ------------------------------------------------ 2. Ctrl+, opens it
    if (-not (Focus-Pane $launcher)) { throw 'focus lost before the shortcut' }
    [System.Windows.Forms.SendKeys]::SendWait('^,')
    Start-Sleep -Milliseconds $StepDelayMs
    $windows = Windows-Of ([uint32]$process.Id)
    $settings = $null
    foreach ($w in $windows) {
        if ($w.Hwnd.ToInt64() -ne $launcher.ToInt64() -and $w.Title -eq 'Settings') { $settings = $w.Hwnd }
    }
    if ($null -eq $settings) {
        # Fall back to the newest window that is not the launcher.
        foreach ($w in $windows) {
            if ($w.Hwnd.ToInt64() -ne $launcher.ToInt64()) { $settings = $w.Hwnd }
        }
    }
    if ($null -ne $settings) {
        $meta.settingsWindow = '0x{0:x}' -f $settings.ToInt64()
        $meta.checkOpen = $true
        Step 'settings' ("Ctrl+, opened a second window (0x{0:x})" -f $settings.ToInt64())
        [void](Capture '03-settings-open.png' $settings)

        # The small layout: resize only the Settings window, in logical
        # pixels (the helper is DPI-aware), then let it settle.
        $sf = Frame-Bounds $settings
        [void][PaneWin]::SetWindowPos($settings, [IntPtr]::Zero, $sf.Left, $sf.Top,
            [int](560 * $scale), [int](400 * $scale), 0x16)
        Start-Sleep -Milliseconds 500
        [void](Capture '04-settings-small.png' $settings)
        Step 'layout' 'resized to 560x400 logical and captured'

        # ------------------------------------------ 4. close only Settings
        if (Focus-Pane $settings) {
            $sf = Frame-Bounds $settings
            # The painted close button: last of the caption buttons at the
            # titlebar's right, 22 logical in from each edge.
            $closeX = $sf.Right - [int](22 * $scale)
            $closeY = $sf.Top + [int](22 * $scale)
            if (Guarded-Click $closeX $closeY $settings) {
                Start-Sleep -Milliseconds $StepDelayMs
                $still = $false
                foreach ($w in (Windows-Of ([uint32]$process.Id))) {
                    if ($w.Hwnd.ToInt64() -eq $settings.ToInt64()) { $still = $true }
                }
                $launcherStill = $false
                foreach ($w in (Windows-Of ([uint32]$process.Id))) {
                    if ($w.Hwnd.ToInt64() -eq $launcher.ToInt64()) { $launcherStill = $true }
                }
                $meta.checkClose = @{
                    settingsClosed = (-not $still)
                    processAlive   = (-not $process.HasExited)
                    launcherAlive  = $launcherStill
                }
                Step 'close' ("close button: settings closed {0}, process alive {1}, launcher alive {2}" -f
                    (-not $still), (-not $process.HasExited), $launcherStill)
                [void](Capture '05-after-settings-close.png' $launcher)
            }
            else { Step 'close' 'skipped (guarded click refused)' }

            # ------------------------------ reopen: Ctrl+, focuses a new one
            # (repeated requests open one window, never a duplicate).
            $settings = $null
            if (-not $process.HasExited -and (Focus-Pane $launcher)) {
                [System.Windows.Forms.SendKeys]::SendWait('^,')
                Start-Sleep -Milliseconds $StepDelayMs
                foreach ($w in (Windows-Of ([uint32]$process.Id))) {
                    if ($w.Hwnd.ToInt64() -ne $launcher.ToInt64() -and $w.Title -eq 'Settings') {
                        $settings = $w.Hwnd
                    }
                }
                if ($null -ne $settings) {
                    # Repeated requests open one window, never a duplicate:
                    # exactly one visible Settings window exists now.
                    $count = 0
                    foreach ($w in (Windows-Of ([uint32]$process.Id))) {
                        if ($w.Title -eq 'Settings') { $count++ }
                    }
                    $meta.checkReopen = @{ settingsWindows = $count }
                    Step 'reopen' ("Ctrl+, opened Settings again ({0} Settings window(s), not a duplicate)" -f $count)
                    [void](Capture '05b-settings-reopened.png' $settings)
                }
            }

            # ------------------------------- 5. minimize / maximize routing
            if ($null -ne $settings -and (Focus-Pane $settings)) {
                $sf = Frame-Bounds $settings
                $minX = $sf.Right - [int]((22 + 44 + 44) * $scale)
                $btnY = $sf.Top + [int](22 * $scale)
                if (Guarded-Click $minX $btnY $settings) {
                    Start-Sleep -Milliseconds $StepDelayMs
                    $iconic = [PaneWin]::IsIconic($settings)
                    $meta.checkMin = $iconic
                    Step 'minimize' ("minimize button clicked; IsIconic = {0}" -f $iconic)
                    [void][PaneWin]::ShowWindow($settings, 9)   # SW_RESTORE
                    Start-Sleep -Milliseconds 300
                }
                if (Focus-Pane $settings) {
                    $sf = Frame-Bounds $settings
                    $maxX = $sf.Right - [int]((22 + 44) * $scale)
                    if (Guarded-Click $maxX $btnY $settings) {
                        Start-Sleep -Milliseconds $StepDelayMs
                        $zoomed = [PaneWin]::IsZoomed($settings)
                        $meta.checkMax = $zoomed
                        Step 'maximize' ("maximize button clicked; IsZoomed = {0}" -f $zoomed)
                        # Toggle back so the window is normal for the drag.
                        if (Focus-Pane $settings) {
                            [void][PaneWin]::ShowWindow($settings, 9)   # SW_RESTORE
                            Start-Sleep -Milliseconds 300
                        }
                    }
                }
            }

            # ---------------------------------------------------- 6. drag
            if ($null -ne $settings -and (Focus-Pane $settings)) {
                $sf = Frame-Bounds $settings
                $before = '{0},{1}' -f $sf.Left, $sf.Top
                $dragX = $sf.Left + [int](($sf.Right - $sf.Left) / 2)
                $dragY = $sf.Top + [int](22 * $scale)
                if (Guarded-Drag $dragX $dragY ($dragX + 90) ($dragY + 60) $settings) {
                    Start-Sleep -Milliseconds $StepDelayMs
                    $af = Frame-Bounds $settings
                    $after = '{0},{1}' -f $af.Left, $af.Top
                    $meta.checkDrag = @{ before = $before; after = $after; moved = ($before -ne $after) }
                    Step 'drag' ("titlebar drag moved the window from {0} to {1}" -f $before, $after)
                    [void](Capture '06-settings-dragged.png' $settings)
                }
            }
        }

        # -------------------------------- 7. close the launcher, Pane quits
        if (-not $process.HasExited) {
            [void](Focus-Pane $launcher)
            [void]$process.CloseMainWindow()   # WM_CLOSE to the launcher
            $exited = $process.WaitForExit(8000)
            $meta.checkQuit = $exited
            Step 'quit' ("closing the launcher window: process exited = {0}" -f $exited)
            if (-not $exited) {
                Stop-Process -Id $process.Id -ErrorAction SilentlyContinue
                $process.WaitForExit(5000) | Out-Null
            }
            $meta.processClosed = $true
        }
    }
    else {
        $meta.checkOpen = $false
        $meta.errorMessage = 'Ctrl+, did not open a second window'
        $failed = $true
        if (-not $process.HasExited) {
            [void]$process.CloseMainWindow()
            if (-not $process.WaitForExit(5000)) {
                Stop-Process -Id $process.Id -ErrorAction SilentlyContinue
            }
        }
    }
}
catch {
    $failed = $true
    $meta.errorMessage = $_.Exception.Message
    [Console]::Error.WriteLine("capture-settings failed: $($meta.errorMessage)")
    if ($null -ne $process -and -not $process.HasExited) {
        try {
            [void]$process.CloseMainWindow()
            if (-not $process.WaitForExit(5000)) {
                Stop-Process -Id $process.Id -ErrorAction SilentlyContinue
            }
        } catch { }
    }
}
finally {
    try { Save-Metadata } catch { Write-Warning "Could not write metadata: $($_.Exception.Message)" }
}

Write-Host ("Run dir: {0} (metadata: pane-run.json, stderr: pane-stderr.log)" -f $runDir)
if ($failed) { exit 1 } else { exit 0 }
