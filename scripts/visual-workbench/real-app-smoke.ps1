# real-app-smoke.ps1 - one real Windows search interaction with the real
# launcher (#91): the production pane.exe, not the fixture, so matching the
# fixture to the reference can never hide missing production wiring.
#
# Launches pane.exe with a per-run temporary PANE_DATA_DIR and scratch
# LOCALAPPDATA (PANE_ARTIFACTS cleared, so no downloads or update checks),
# moves its window past the right edge of the virtual screen as soon as it
# exists (the launcher itself opens centered and active, as it does for a
# user; that is the one moment this smoke is visible), then drives root
# search through window messages posted to its HWND: types "install"
# (edit + search), Down (select), Enter (open: the npm install form, which
# fetches nothing until submitted), Escape (back to root search, where the
# form was opened from), Escape again (the query clears) - and captures the
# client with PrintWindow after every step. Then it asserts on the captures
# with scripts/check_screenshot.py: every step up to the form changed what
# the window shows; the first Escape returned to exactly the root search
# the form was opened from; the second returned to exactly the rest state.
# Closes only the process it started and deletes only the temporary
# directory it created.
#
#   ./scripts/visual-workbench/real-app-smoke.ps1 -Binary target/debug/pane.exe -OutputDir <dir> `
#     -ApplicationRevision (git rev-parse HEAD)

[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateScript({ Test-Path -LiteralPath $_ -PathType Leaf })]
    [string]$Binary,

    [Parameter(Mandatory = $true)]
    [string]$OutputDir,

    [Parameter(Mandatory = $true)]
    [string]$ApplicationRevision,

    [string]$ExtensionsDir = (Join-Path $PSScriptRoot '../../target/guests'),
    [int]$StepDelayMs = 900,
    [int]$WindowTimeoutMs = 30000
)

$ErrorActionPreference = 'Stop'
$Binary = (Resolve-Path -LiteralPath $Binary).Path
if (-not (Test-Path -LiteralPath $OutputDir)) { New-Item -ItemType Directory -Path $OutputDir | Out-Null }
$OutputDir = (Resolve-Path -LiteralPath $OutputDir).Path

Add-Type -AssemblyName System.Drawing, System.Windows.Forms
Add-Type @"
using System; using System.Runtime.InteropServices;
public static class PaneSmokeWin {
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr hdc, uint flags);
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] public static extern int GetDpiForWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int cx, int cy, uint flags);
    [DllImport("user32.dll")] public static extern uint MapVirtualKey(uint code, uint type);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
}
"@
[void][PaneSmokeWin]::SetProcessDPIAware()

$scratch = Join-Path ([IO.Path]::GetTempPath()) ('pane-real-smoke-' + [Guid]::NewGuid().ToString('N').Substring(0, 12))
$dataDir = Join-Path $scratch 'data'
$localAppData = Join-Path $scratch 'localappdata'
New-Item -ItemType Directory -Path $dataDir, $localAppData | Out-Null
$stderr = Join-Path $OutputDir 'pane-stderr.log'
$virtual = [System.Windows.Forms.SystemInformation]::VirtualScreen
$meta = [ordered]@{
    script = 'real-app-smoke.ps1'
    binary = $Binary
    binarySha256 = (Get-FileHash -LiteralPath $Binary -Algorithm SHA256).Hash
    applicationRevision = $ApplicationRevision
    os = [Environment]::OSVersion.VersionString
    windowsBuild = Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion' -ErrorAction SilentlyContinue | Select-Object ProductName, DisplayVersion, CurrentBuild, UBR
    theme = 'dark'; material = 'opaque'
    paneDataDir = $dataDir; localAppData = $localAppData; extensionsDir = $ExtensionsDir
    inputMethod = 'window messages posted to the launcher HWND (WM_CHAR / WM_KEYDOWN+WM_KEYUP); the real pointer and keyboard are never used'
    captureMethod = 'PrintWindow(PW_CLIENTONLY | PW_RENDERFULLCONTENT)'
    pid = $null; dpi = $null; client = $null; windowRectAtOpen = $null; windowRectOffscreen = $null
    foregroundAtOpenWasPane = $null
    steps = @(); captures = @(); assertions = @()
    processExited = $null; exitCode = $null; scratchRemoved = $null; error = $null
    startedUtc = (Get-Date).ToUniversalTime().ToString('o')
}

function Save-Client([IntPtr]$hwnd, [string]$name, [string]$label) {
    $client = New-Object PaneSmokeWin+RECT
    [void][PaneSmokeWin]::GetClientRect($hwnd, [ref]$client)
    $path = Join-Path $OutputDir $name
    $bitmap = New-Object System.Drawing.Bitmap $client.Right, $client.Bottom
    try {
        $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
        $hdc = $graphics.GetHdc()
        try { $ok = [PaneSmokeWin]::PrintWindow($hwnd, $hdc, 3) } finally { $graphics.ReleaseHdc($hdc); $graphics.Dispose() }
        if (-not $ok) { throw "PrintWindow failed for $name" }
        $bitmap.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
    } finally { $bitmap.Dispose() }
    $script:meta.captures += [ordered]@{ file = $name; label = $label; size = @($client.Right, $client.Bottom); utc = (Get-Date).ToUniversalTime().ToString('o') }
    Write-Host "smoke: $name ($label)"
}

function Send-Key([IntPtr]$hwnd, [int]$vk) {
    $scan = [PaneSmokeWin]::MapVirtualKey($vk, 0)
    $extended = if ($vk -in 0x25, 0x26, 0x27, 0x28) { 1 -shl 24 } else { 0 }
    $down = 1 -bor ($scan -shl 16) -bor $extended
    $up = $down -bor (1 -shl 30) -bor ([int64]1 -shl 31)
    [void][PaneSmokeWin]::PostMessage($hwnd, 0x0100, [IntPtr]$vk, [IntPtr][int64]$down)
    Start-Sleep -Milliseconds 30
    [void][PaneSmokeWin]::PostMessage($hwnd, 0x0101, [IntPtr]$vk, [IntPtr][int64]$up)
}

$process = $null
$failed = $false
$names = 'PANE_DATA_DIR', 'PANE_EXTENSIONS_DIR', 'PANE_THEME', 'PANE_MATERIAL', 'PANE_ARTIFACTS', 'LOCALAPPDATA'
$saved = @{}
foreach ($name in $names) { if (Test-Path "Env:$name") { $saved[$name] = (Get-Item "Env:$name").Value } }
try {
    try {
        $env:PANE_DATA_DIR = $dataDir
        $env:PANE_EXTENSIONS_DIR = $ExtensionsDir
        $env:PANE_THEME = 'dark'
        $env:PANE_MATERIAL = 'opaque'
        $env:LOCALAPPDATA = $localAppData
        Remove-Item Env:PANE_ARTIFACTS -ErrorAction SilentlyContinue
        $process = Start-Process -FilePath $Binary -PassThru -RedirectStandardError $stderr
        $null = $process.Handle
    } finally {
        foreach ($name in $names) {
            if ($saved.ContainsKey($name)) { Set-Item "Env:$name" $saved[$name] } else { Remove-Item "Env:$name" -ErrorAction SilentlyContinue }
        }
    }
    $meta.pid = $process.Id
    $deadline = [DateTime]::UtcNow.AddMilliseconds($WindowTimeoutMs)
    while ($process.MainWindowHandle -eq 0) {
        if ($process.HasExited) { throw "pane.exe exited ($($process.ExitCode)) before showing a window" }
        if ([DateTime]::UtcNow -gt $deadline) { throw "no window within $WindowTimeoutMs ms" }
        Start-Sleep -Milliseconds 100
        $process.Refresh()
    }
    $hwnd = $process.MainWindowHandle
    $rect = New-Object PaneSmokeWin+RECT
    [void][PaneSmokeWin]::GetWindowRect($hwnd, [ref]$rect)
    $meta.windowRectAtOpen = '{0},{1},{2},{3}' -f $rect.Left, $rect.Top, $rect.Right, $rect.Bottom
    $meta.foregroundAtOpenWasPane = ([PaneSmokeWin]::GetForegroundWindow().ToInt64() -eq $hwnd.ToInt64())
    # Past the right edge of every display: SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE.
    [void][PaneSmokeWin]::SetWindowPos($hwnd, [IntPtr]::Zero, $virtual.Right + 400, $virtual.Top + 100, 0, 0, 0x15)
    Start-Sleep -Milliseconds 1500   # first frames, fonts, root search's rows
    [void][PaneSmokeWin]::GetWindowRect($hwnd, [ref]$rect)
    $meta.windowRectOffscreen = '{0},{1},{2},{3}' -f $rect.Left, $rect.Top, $rect.Right, $rect.Bottom
    $meta.dpi = [PaneSmokeWin]::GetDpiForWindow($hwnd)
    $client = New-Object PaneSmokeWin+RECT
    [void][PaneSmokeWin]::GetClientRect($hwnd, [ref]$client)
    $meta.client = @($client.Right, $client.Bottom)

    Save-Client $hwnd '00-root.png' 'root search at rest'
    foreach ($character in 'install'.ToCharArray()) {
        [void][PaneSmokeWin]::PostMessage($hwnd, 0x0102, [IntPtr][int]$character, [IntPtr]1)
        Start-Sleep -Milliseconds 60
    }
    $meta.steps += 'typed "install" (WM_CHAR)'
    Start-Sleep -Milliseconds $StepDelayMs
    Save-Client $hwnd '01-typed-install.png' 'after typing "install": root search filtered'
    Send-Key $hwnd 0x28
    $meta.steps += 'Down'
    Start-Sleep -Milliseconds $StepDelayMs
    Save-Client $hwnd '02-down.png' 'after Down: the second result selected'
    Send-Key $hwnd 0x0D
    $meta.steps += 'Enter'
    Start-Sleep -Milliseconds $StepDelayMs
    Save-Client $hwnd '03-enter.png' 'after Enter: the selected command opened'
    Send-Key $hwnd 0x1B
    $meta.steps += 'Escape'
    Start-Sleep -Milliseconds $StepDelayMs
    Save-Client $hwnd '04-escape.png' 'after Escape: back to root search, where the form was opened'
    Send-Key $hwnd 0x1B
    $meta.steps += 'Escape'
    Start-Sleep -Milliseconds $StepDelayMs
    Save-Client $hwnd '05-escape-again.png' 'after Escape again: the query cleared, root search at rest'

    $checker = Join-Path $PSScriptRoot '../check_screenshot.py'
    $checks = @(
        @('--distinct', '00-root.png', '01-typed-install.png', '02-down.png', '03-enter.png'),
        @('--same', '02-down.png', '04-escape.png'),
        @('--same', '00-root.png', '05-escape-again.png')
    )
    foreach ($check in $checks) {
        $arguments = @($check[0]) + @($check[1..($check.Count - 1)] | ForEach-Object { Join-Path $OutputDir $_ })
        $previous = $ErrorActionPreference
        $ErrorActionPreference = 'Continue'
        try { $output = (& python $checker @arguments 2>&1 | ForEach-Object { "$_" }) -join ' ' }
        finally { $ErrorActionPreference = $previous }
        $passed = ($LASTEXITCODE -eq 0)
        $meta.assertions += [ordered]@{ check = ($check -join ' '); passed = $passed; output = $output }
        Write-Host ("smoke assertion {0}: {1}" -f ($check -join ' '), $output)
        if (-not $passed) { throw "smoke assertion failed: $($check -join ' '): $output" }
    }
} catch {
    $failed = $true
    $meta.error = $_.Exception.Message
    [Console]::Error.WriteLine("real-app-smoke failed: $($meta.error)")
} finally {
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
    try { Remove-Item -LiteralPath $scratch -Recurse -Force; $meta.scratchRemoved = $true } catch { $meta.scratchRemoved = $false }
    $meta.finishedUtc = (Get-Date).ToUniversalTime().ToString('o')
    $meta | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $OutputDir 'pane-run.json') -Encoding utf8
}
if ($failed) { exit 1 } else { exit 0 }
