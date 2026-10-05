# native-capture.ps1 - the visual workbench's native side (#91).
#
# Starts the native fixture (pane-visual-fixture) once per scenario, each
# with its own temporary data directory and scratch LOCALAPPDATA, waits for
# the manifest it writes after its first frame, verifies the client is the
# scenario's logical size at the window's DPI, then drives the scenario's
# steps and captures the client at every capture step. It closes only the
# process it started and deletes only the temporary directories it created.
#
# Input and capture never touch the user's desktop session: the fixture's
# window opens without activation, and the moment its handle exists it is
# moved past the right edge of the virtual screen (GPUI itself re-centers a
# window asked to open off every display), where the operator's pointer
# cannot reach it and it covers nothing; the real pointer never moves and
# no global key is sent. Steps are delivered as window messages posted to the fixture's
# own HWND - WM_MOUSEMOVE at the step's client point (WM_LBUTTONDOWN/UP
# there for a click), WM_KEYDOWN/WM_KEYUP
# for keys (GPUI's message loop turns them into its own key events, as it
# does for typed keys), WM_CHAR for typed text - and each capture is the
# client area rendered by PrintWindow(PW_CLIENTONLY | PW_RENDERFULLCONTENT),
# which reads the window's own composition even when another window covers
# it. So no cursor glyph appears in a capture, and the operator can keep
# using the machine during a run. What this does not exercise is the OS's
# own hit-testing and focus arbitration; the real-app smoke does
# (scripts/capture-pane-windows.ps1).
#
#   ./scripts/visual-workbench/native-capture.ps1 -Fixture target/debug/pane-visual-fixture.exe `
#     -Registry <registry.json> -OutputDir <dir> -ApplicationRevision (git rev-parse HEAD)

[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateScript({ Test-Path -LiteralPath $_ -PathType Leaf })]
    [string]$Fixture,

    [Parameter(Mandatory = $true)]
    [ValidateScript({ Test-Path -LiteralPath $_ -PathType Leaf })]
    [string]$Registry,

    [Parameter(Mandatory = $true)]
    [string]$OutputDir,

    [Parameter(Mandatory = $true)]
    [string]$ApplicationRevision,

    # Scenario names; default every registered scenario.
    [string[]]$Scenario = @(),

    [ValidateSet('dark', 'light')]
    [string]$Theme = 'dark',

    # Strict color checks run in the opaque material; glass is captured
    # for the separate matched-backdrop review.
    [ValidateSet('glass', 'opaque')]
    [string]$Material = 'opaque',

    [ValidateSet('none', 'row-padding-plus-4', 'selected-fill', 'hover-fill', 'nav-selected-fill', 'segment-on-fill', 'well-fill')]
    [string]$Perturb = 'none',

    [int]$StepDelayMs = 450,
    [int]$WindowTimeoutMs = 30000
)

$ErrorActionPreference = 'Stop'
$Fixture = (Resolve-Path -LiteralPath $Fixture).Path
$registryData = Get-Content -LiteralPath $Registry -Raw -Encoding utf8 | ConvertFrom-Json
if (-not (Test-Path -LiteralPath $OutputDir)) { New-Item -ItemType Directory -Path $OutputDir | Out-Null }
$OutputDir = (Resolve-Path -LiteralPath $OutputDir).Path

# Accept a comma-separated list too (powershell -File passes one string).
$Scenario = @($Scenario | ForEach-Object { $_ -split ',' } | Where-Object { $_ })
$selected = @($registryData.scenarios)
if ($Scenario.Count -gt 0) {
    foreach ($name in $Scenario) {
        if (-not ($registryData.scenarios | Where-Object { $_.name -eq $name })) {
            $pending = $registryData.pending | Where-Object { $_.name -eq $name }
            if ($pending) { throw "Scenario '$name' is pending: $($pending.ticket) registers it." }
            throw "Unknown scenario '$name'."
        }
    }
    $selected = @($registryData.scenarios | Where-Object { $Scenario -contains $_.name })
}

Add-Type -AssemblyName System.Drawing, System.Windows.Forms
Add-Type @"
using System; using System.Runtime.InteropServices;
public static class PaneFixtureWin {
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h, ref POINT p);
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr hdc, uint flags);
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] public static extern IntPtr SendMessageTimeout(IntPtr h, uint m, IntPtr w, IntPtr l, uint flags, uint timeout, out IntPtr result);
    [DllImport("user32.dll")] public static extern int GetDpiForWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern uint MapVirtualKey(uint code, uint type);
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int cx, int cy, uint flags);
    [DllImport("dwmapi.dll")] public static extern int DwmGetWindowAttribute(IntPtr h, int attr, out RECT rect, int size);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
    [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
}
"@
[void][PaneFixtureWin]::SetProcessDPIAware()

$WM_MOUSEMOVE = 0x0200; $WM_KEYDOWN = 0x0100; $WM_KEYUP = 0x0101; $WM_CHAR = 0x0102
$WM_LBUTTONDOWN = 0x0201; $WM_LBUTTONUP = 0x0202; $MK_LBUTTON = 0x0001
$VK = @{ down = 0x28; up = 0x26; escape = 0x1B }
$fixtureSha = (Get-FileHash -LiteralPath $Fixture -Algorithm SHA256).Hash
# Past the right edge of every display, in physical pixels.
$virtual = [System.Windows.Forms.SystemInformation]::VirtualScreen
$parkX = $virtual.Right + 400
$parkY = $virtual.Top + 100
$windowsBuild = Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion' -ErrorAction SilentlyContinue |
    Select-Object ProductName, DisplayVersion, CurrentBuild, UBR

function Format-Rect($r) { '{0},{1},{2},{3}' -f $r.Left, $r.Top, $r.Right, $r.Bottom }

function Save-Client([IntPtr]$hwnd, [string]$path) {
    $client = New-Object PaneFixtureWin+RECT
    [void][PaneFixtureWin]::GetClientRect($hwnd, [ref]$client)
    $bitmap = New-Object System.Drawing.Bitmap $client.Right, $client.Bottom
    try {
        $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
        $hdc = $graphics.GetHdc()
        try { $ok = [PaneFixtureWin]::PrintWindow($hwnd, $hdc, 3) }   # PW_CLIENTONLY | PW_RENDERFULLCONTENT
        finally { $graphics.ReleaseHdc($hdc); $graphics.Dispose() }
        if (-not $ok) { throw "PrintWindow failed for $path" }
        $bitmap.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
    } finally { $bitmap.Dispose() }
    return @($client.Right, $client.Bottom)
}

function Send-Key([IntPtr]$hwnd, [int]$vk) {
    $scan = [PaneFixtureWin]::MapVirtualKey($vk, 0)
    $extended = if ($vk -in 0x25, 0x26, 0x27, 0x28) { 1 -shl 24 } else { 0 }
    $down = 1 -bor ($scan -shl 16) -bor $extended
    $up = $down -bor (1 -shl 30) -bor ([int64]1 -shl 31)
    [void][PaneFixtureWin]::PostMessage($hwnd, $WM_KEYDOWN, [IntPtr]$vk, [IntPtr][int64]$down)
    Start-Sleep -Milliseconds 30
    [void][PaneFixtureWin]::PostMessage($hwnd, $WM_KEYUP, [IntPtr]$vk, [IntPtr][int64]$up)
}

# Waits until the fixture has handled the input posted to it. A capture is
# a message sent to the window, and Windows dispatches sent messages before
# posted ones, so a capture taken while the fixture is busy (a debug
# build's first raster of a large SVG takes seconds) would show the state
# before the keys still queued behind it. A WM_NULL sent to a busy thread
# waits for it; two prompt replies 120ms apart mean the queue has drained.
# Returns how long the wait took, in milliseconds.
function Wait-Idle([IntPtr]$hwnd) {
    $total = [Diagnostics.Stopwatch]::StartNew()
    $prompt = 0
    while ($prompt -lt 2 -and $total.ElapsedMilliseconds -lt 30000) {
        $reply = [Diagnostics.Stopwatch]::StartNew()
        $result = [IntPtr]::Zero
        [void][PaneFixtureWin]::SendMessageTimeout($hwnd, 0, [IntPtr]::Zero, [IntPtr]::Zero, 0, 20000, [ref]$result)
        if ($reply.ElapsedMilliseconds -lt 60) { $prompt++ } else { $prompt = 0 }
        Start-Sleep -Milliseconds 120
    }
    if ($prompt -lt 2) { throw 'the fixture did not become idle within 30s' }
    return [int]$total.ElapsedMilliseconds
}

function Send-Pointer([IntPtr]$hwnd, [double]$x, [double]$y, [double]$scale) {
    $px = [int][Math]::Floor($x * $scale); $py = [int][Math]::Floor($y * $scale)
    [void][PaneFixtureWin]::PostMessage($hwnd, $WM_MOUSEMOVE, [IntPtr]::Zero, [IntPtr](($py -shl 16) -bor ($px -band 0xFFFF)))
}

# A left click at the client point: the button pressed and released there,
# as posted messages (the real pointer never moves).
function Send-Click([IntPtr]$hwnd, [double]$x, [double]$y, [double]$scale) {
    $px = [int][Math]::Floor($x * $scale); $py = [int][Math]::Floor($y * $scale)
    $at = [IntPtr](($py -shl 16) -bor ($px -band 0xFFFF))
    [void][PaneFixtureWin]::PostMessage($hwnd, $WM_LBUTTONDOWN, [IntPtr]$MK_LBUTTON, $at)
    Start-Sleep -Milliseconds 40
    [void][PaneFixtureWin]::PostMessage($hwnd, $WM_LBUTTONUP, [IntPtr]::Zero, $at)
}

$summary = [ordered]@{
    script = 'native-capture.ps1'
    fixture = $Fixture
    fixtureSha256 = $fixtureSha
    applicationRevision = $ApplicationRevision
    os = [Environment]::OSVersion.VersionString
    virtualScreen = '{0},{1} {2}x{3}' -f $virtual.X, $virtual.Y, $virtual.Width, $virtual.Height
    parkedAt = '{0},{1}' -f $parkX, $parkY
    windowsBuild = $windowsBuild
    theme = $Theme
    material = $Material
    perturbation = $Perturb
    inputMethod = 'window messages posted to the fixture HWND (WM_MOUSEMOVE / WM_LBUTTONDOWN+WM_LBUTTONUP / WM_KEYDOWN+WM_KEYUP / WM_CHAR); the window is off-screen and inactive, and the real pointer, keyboard and foreground window are never touched'
    captureMethod = 'PrintWindow(PW_CLIENTONLY | PW_RENDERFULLCONTENT) of the client area; no rescaling'
    startedUtc = (Get-Date).ToUniversalTime().ToString('o')
    scenarios = @()
}
$failed = $false

foreach ($entry in $selected) {
    $dir = Join-Path $OutputDir $entry.name
    New-Item -ItemType Directory -Force -Path $dir | Out-Null
    $scratch = Join-Path ([IO.Path]::GetTempPath()) ('pane-visual-fixture-' + [Guid]::NewGuid().ToString('N').Substring(0, 12))
    $dataDir = Join-Path $scratch 'data'
    $localAppData = Join-Path $scratch 'localappdata'
    New-Item -ItemType Directory -Path $dataDir, $localAppData | Out-Null
    $manifestPath = Join-Path $dir 'fixture-manifest.json'
    if (Test-Path -LiteralPath $manifestPath) { Remove-Item -LiteralPath $manifestPath }
    $stderr = Join-Path $dir 'fixture-stderr.log'
    $meta = [ordered]@{
        scenario = $entry.name; pid = $null; dpi = $null; scale = $null
        expectedClient = $null; client = $null; clientOrigin = $null; windowRect = $null; frameBounds = $null
        foregroundWasFixture = $null; offscreen = $null; captures = @(); steps = @()
        dataDir = $dataDir; scratchRemoved = $null; processExited = $null; exitCode = $null; error = $null
    }
    $process = $null
    $oldLocal = $env:LOCALAPPDATA
    try {
        $env:LOCALAPPDATA = $localAppData
        try {
            $arguments = @('--scenario', $entry.name, '--manifest', ('"{0}"' -f $manifestPath), '--data-dir', ('"{0}"' -f $dataDir),
                '--theme', $Theme, '--material', $Material, '--perturb', $Perturb)
            $process = Start-Process -FilePath $Fixture -ArgumentList $arguments -PassThru -RedirectStandardError $stderr
        } finally { $env:LOCALAPPDATA = $oldLocal }
        $meta.pid = $process.Id
        $null = $process.Handle   # keep the handle, so the exit code stays readable

        $deadline = [DateTime]::UtcNow.AddMilliseconds($WindowTimeoutMs)
        $parked = $false
        while (-not (Test-Path -LiteralPath $manifestPath) -or $process.MainWindowHandle -eq 0) {
            if (-not $parked -and $process.MainWindowHandle -ne 0) {
                # SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE
                [void][PaneFixtureWin]::SetWindowPos($process.MainWindowHandle, [IntPtr]::Zero, $parkX, $parkY, 0, 0, 0x15)
                $parked = $true
            }
            if ($process.HasExited) { throw "the fixture exited ($($process.ExitCode)) before its manifest: $(Get-Content -Raw $stderr)" }
            if ([DateTime]::UtcNow -gt $deadline) { throw "no manifest/window within $WindowTimeoutMs ms" }
            Start-Sleep -Milliseconds 150
            $process.Refresh()
        }
        $hwnd = $process.MainWindowHandle
        if (-not $parked) { [void][PaneFixtureWin]::SetWindowPos($hwnd, [IntPtr]::Zero, $parkX, $parkY, 0, 0, 0x15) }
        Start-Sleep -Milliseconds 300
        $manifest = Get-Content -LiteralPath $manifestPath -Raw -Encoding utf8 | ConvertFrom-Json
        $dpi = [PaneFixtureWin]::GetDpiForWindow($hwnd)
        $scale = $dpi / 96.0
        $meta.dpi = $dpi; $meta.scale = $scale
        $client = New-Object PaneFixtureWin+RECT
        [void][PaneFixtureWin]::GetClientRect($hwnd, [ref]$client)
        $clientOrigin = New-Object PaneFixtureWin+POINT
        [void][PaneFixtureWin]::ClientToScreen($hwnd, [ref]$clientOrigin)
        $window = New-Object PaneFixtureWin+RECT
        [void][PaneFixtureWin]::GetWindowRect($hwnd, [ref]$window)
        $frame = New-Object PaneFixtureWin+RECT
        if ([PaneFixtureWin]::DwmGetWindowAttribute($hwnd, 9, [ref]$frame, 16) -eq 0) { $meta.frameBounds = Format-Rect $frame }
        $meta.windowRect = Format-Rect $window
        $meta.offscreen = ($window.Left -ge $virtual.Right -or $window.Right -le $virtual.Left -or
            $window.Top -ge $virtual.Bottom -or $window.Bottom -le $virtual.Top)
        if (-not $meta.offscreen) { throw "the fixture window is on a display ($($meta.windowRect)); the operator's pointer could reach it" }
        $meta.clientOrigin = '{0},{1}' -f $clientOrigin.X, $clientOrigin.Y
        $meta.client = @($client.Right, $client.Bottom)
        $expected = @([int][Math]::Round($entry.client[0] * $scale), [int][Math]::Round($entry.client[1] * $scale))
        $meta.expectedClient = $expected
        if ($client.Right -ne $expected[0] -or $client.Bottom -ne $expected[1]) {
            throw ("client is {0}x{1} physical px; the scenario needs {2}x{3} ({4}x{5} logical at {6} DPI)" -f
                $client.Right, $client.Bottom, $expected[0], $expected[1], $entry.client[0], $entry.client[1], $dpi)
        }
        # The fixture opens without activation; record that it really did
        # not take the foreground.
        $meta.foregroundWasFixture = ([PaneFixtureWin]::GetForegroundWindow().ToInt64() -eq $hwnd.ToInt64())
        Start-Sleep -Milliseconds 600   # first frames settle (fonts, list)

        $pointer = $null
        $index = 0
        foreach ($step in $manifest.steps) {
            $index++
            switch ($step.action) {
                'capture' {
                    # Re-assert the pointer before capturing: the posted move
                    # is the only pointer the fixture has, and Windows may
                    # report a leave because the real cursor is elsewhere.
                    if ($null -ne $pointer) { Send-Pointer $hwnd $pointer[0] $pointer[1] $scale; Start-Sleep -Milliseconds 300 }
                    $settled = Wait-Idle $hwnd
                    $file = Join-Path $dir ($step.name + '.png')
                    $size = Save-Client $hwnd $file
                    $meta.captures += [ordered]@{ name = $step.name; file = "$($entry.name)/$($step.name).png"; size = $size; settledMs = $settled; utc = (Get-Date).ToUniversalTime().ToString('o') }
                    Write-Host "native $($entry.name): $($step.name) ($($size -join 'x'))"
                }
                'pointer' {
                    # Arrive from a pixel to the left, as a real pointer
                    # reports two moves: the window's first event only
                    # records where the pointer is (#94).
                    $pointer = @($step.point[0], $step.point[1])
                    Send-Pointer $hwnd ($pointer[0] - 1) $pointer[1] $scale
                    Start-Sleep -Milliseconds 40
                    Send-Pointer $hwnd $pointer[0] $pointer[1] $scale
                }
                'key' {
                    if (-not $VK.ContainsKey($step.key)) { throw "unknown key $($step.key)" }
                    Send-Key $hwnd $VK[$step.key]
                }
                'click' {
                    # The pointer arrives at the element (from a pixel to
                    # its left, as a pointer step does), then presses.
                    if ($null -eq $step.point) { throw "the click on $($step.target) has no point" }
                    $pointer = @($step.point[0], $step.point[1])
                    Send-Pointer $hwnd ($pointer[0] - 1) $pointer[1] $scale
                    Start-Sleep -Milliseconds 40
                    Send-Pointer $hwnd $pointer[0] $pointer[1] $scale
                    Start-Sleep -Milliseconds 40
                    Send-Click $hwnd $pointer[0] $pointer[1] $scale
                }
                'point' {
                    # The pointer arrives at the element (a pinned slot)
                    # from a pixel to its left, and stays there unpressed.
                    if ($null -eq $step.point) { throw "the point at $($step.target) has no point" }
                    $pointer = @($step.point[0], $step.point[1])
                    Send-Pointer $hwnd ($pointer[0] - 1) $pointer[1] $scale
                    Start-Sleep -Milliseconds 40
                    Send-Pointer $hwnd $pointer[0] $pointer[1] $scale
                }
                'type' {
                    foreach ($character in $step.text.ToCharArray()) {
                        [void][PaneFixtureWin]::PostMessage($hwnd, $WM_CHAR, [IntPtr][int]$character, [IntPtr]1)
                        Start-Sleep -Milliseconds 40
                    }
                }
                default { throw "unknown step $($step.action)" }
            }
            $meta.steps += [ordered]@{ index = $index; action = $step.action; detail = ($step | ConvertTo-Json -Compress); utc = (Get-Date).ToUniversalTime().ToString('o') }
            if ($step.action -ne 'capture') { Start-Sleep -Milliseconds $StepDelayMs }
        }
    } catch {
        $failed = $true
        $meta.error = $_.Exception.Message
        [Console]::Error.WriteLine("native-capture $($entry.name) failed: $($meta.error)")
    } finally {
        # Close only the fixture this iteration started.
        if ($null -ne $process) {
            try {
                if (-not $process.HasExited) {
                    [void]$process.CloseMainWindow()
                    if (-not $process.WaitForExit(5000)) {
                        Stop-Process -Id $process.Id -ErrorAction SilentlyContinue
                        [void]$process.WaitForExit(5000)
                    }
                }
                $meta.processExited = $process.HasExited
                try { $meta.exitCode = $process.ExitCode } catch { }
            } catch { $meta.processExited = $false }
        }
        # Delete only the scratch directory this iteration created.
        try { Remove-Item -LiteralPath $scratch -Recurse -Force; $meta.scratchRemoved = $true }
        catch { $meta.scratchRemoved = $false }
        $meta | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $dir 'native-run.json') -Encoding utf8
        $summary.scenarios += [ordered]@{ name = $entry.name; error = $meta.error; captures = $meta.captures.Count; dpi = $meta.dpi; client = $meta.client }
    }
}

$summary.finishedUtc = (Get-Date).ToUniversalTime().ToString('o')
$summary | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $OutputDir 'native-summary.json') -Encoding utf8
if ($failed) { exit 1 } else { exit 0 }
