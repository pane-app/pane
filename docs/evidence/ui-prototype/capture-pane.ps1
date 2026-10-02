# capture-pane.ps1 - Pane native launch/capture helper (review support).
#
# Launches an interactive Pane build, waits for its real window, focuses it,
# captures real desktop pixels (window crop by default), optionally sends a
# key sequence through native SendKeys (focus re-checked before EVERY key),
# and closes ONLY the process it started (unless -LeaveOpen). The script
# never deletes anything, never touches the user's data folder or existing
# windows, and gives Pane a per-run scratch data directory and a per-run
# scratch LOCALAPPDATA (Pane's cache is %LOCALAPPDATA%\Pane\cache on
# Windows - crates/pane/src/lib.rs cache_dir(); PANE_CACHE_DIR is NOT read)
# through spawn-scoped environment variables that are restored immediately
# after the child starts. PANE_ARTIFACTS is cleared for the child only, so
# no default-extension downloads or update checks run.
#
# Focus handling: SetWindowPos-untouched activation via SetForegroundWindow
# with an EXPLICIT Int64 HWND comparison, then a WScript.Shell.AppActivate
# fallback that names ONLY the spawned process Id. Neither method's return
# value is treated as focus: confirmation is always the actual foreground
# HWND (GetForegroundWindow), checked again before every key token. When
# focus cannot be confirmed, keys are never sent and the run exits 1, with
# the foreground diagnosis (HWND, owning PID, whether it is Pane's process)
# recorded in pane-run.json.
#
# Last-resort opt-in -ClickToFocus (default OFF): one REAL guarded click
# inside Pane's own window - WindowFromPoint must prove the point belongs
# to the spawned window tree before the cursor moves and again immediately
# before the click (no click when a foreign window covers it), the cursor
# is saved and restored only if nothing moved it, and 'guarded-click' is
# recorded as the activation method only when the real foreground check
# then confirms Pane's exact HWND. Normal native UI testing with the real
# pointer: opt in only for testing your own window.
#
# Ownership: pane-ui-prototype/.scratch/native-review/ (review helper, not
# app source). Reference for Win32/SendKeys/DPI patterns: the main
# checkout's scripts/smoke-windows.ps1 (which is a destructive fixture
# workflow - do NOT run it wholesale; this helper is the safe subset).
#
# Examples:
#   powershell -NoProfile -ExecutionPolicy Bypass -File capture-pane.ps1 `
#     -Binary C:\Users\ADMIN\Desktop\nguyenvu\pane\target\debug\pane.exe `
#     -OutputDir C:\captures
#
#   ... -Theme dark -Material glass -Keys 'rust','{ESC}'   (prototype selectors)
#   ... -Backdrop -BackdropPattern light                      (blur evidence)
#   ... -FullScreen                                           (explicit opt-in)
#   ... -WindowWidthPixels 380 -WindowHeightPixels 420        (explicit size)
#
# PANE_THEME / PANE_MATERIAL are prototype selectors (implementation in
# progress): the helper only passes them through and records them in the run
# metadata. Verify the build under test supports them before trusting mode
# differences; if it does not, captures with different values look identical.

[CmdletBinding()]
param(
    # Explicit path to the Pane build to launch (no default on purpose).
    [Parameter(Mandatory = $true)]
    [ValidateScript({ Test-Path -LiteralPath $_ -PathType Leaf })]
    [string]$Binary,

    # Explicit directory for this run's PNGs + metadata (created if missing).
    [Parameter(Mandatory = $true)]
    [string]$OutputDir,

    # Where Pane reads sample/default extension components from.
    [string]$ExtensionsDir = 'C:\Users\ADMIN\Desktop\nguyenvu\pane\target\guests',

    # Planned prototype selectors, passed through as PANE_THEME / PANE_MATERIAL
    # (implementation in progress: verify the build supports them).
    [ValidateSet('dark', 'light')]
    [string]$Theme,

    [ValidateSet('glass', 'opaque')]
    [string]$Material,

    # SendKeys tokens in order, e.g. @('rust', '{ESC}'). Literal text is
    # typed as-is; special keys use SendKeys braces. Escape + ^ % ~ ( ) { }
    # in literal text (SendKeys syntax) or the token is misparsed.
    # Focus is re-checked before EVERY token; if Pane lost the foreground,
    # the remaining keys are not sent (abort, recorded in metadata).
    [string[]]$Keys = @(),

    [int]$InitialWaitMs = 2000,   # settle time after the window appears
    [int]$KeyDelayMs = 700,       # wait after each key token before capture
    [int]$WindowTimeoutMs = 30000,

    # Optional explicit OUTER-window size for the spawned Pane window, in
    # PHYSICAL pixels (the helper process is DPI-aware; these are real screen
    # pixels, not logical ones). Both must be given together and each must be
    # greater than 100. When given, ONLY the spawned Pane HWND is resized
    # (SetWindowPos NOMOVE|NOZORDER|NOACTIVATE) after its handle appears and
    # before backdrop geometry/capture, then the window is given time to
    # settle its layout; metadata records the requested size beside the
    # actual rect. Default: no resize. Main uses this to check the 380x420
    # horizontal layout, which the 640px-wide test window never exercises.
    [int]$WindowWidthPixels = 0,
    [int]$WindowHeightPixels = 0,

    # Capture the whole virtual screen instead of the window crop. Only use
    # when the window crop is not sufficient (e.g. compositor blur evidence
    # needs the surroundings); default is the window crop alone.
    [switch]$FullScreen,

    # Show an EXTERNAL high-contrast test pattern window behind Pane to
    # evidence compositor blur through Pane's material. 'light' and 'dark'
    # use DIFFERENT luminance bases (#ECECEC vs #161616) with accent
    # blocks/bands in the opposite color, so their blurred averages differ.
    # The backdrop is arranged immediately behind Pane in Z-order (not
    # pushed to the bottom of the stack, where unrelated windows could hide
    # it) and the arrangement is verified and recorded. It is a helper-owned
    # window, created only on explicit request (no hidden helper windows
    # exist in any mode), never wallpaper injected into Pane. It lives in
    # this helper's process, so it ALWAYS closes when the helper ends - only
    # -LeaveOpen keeps Pane itself running.
    [switch]$Backdrop,
    [ValidateSet('light', 'dark')]
    [string]$BackdropPattern = 'light',

    # Leave Pane running for manual inspection; the PID is printed and
    # recorded so the operator can close it. The backdrop always closes
    # when the helper ends (it would not reliably outlive this process).
    [switch]$LeaveOpen,

    # Explicit opt-in (default OFF): when both SetForegroundWindow and the
    # AppActivate fallback fail, activate Pane with ONE real mouse click at
    # a known interior point of its window (center-x, visible top + 32
    # physical pixels - the search-header area), as normal native UI
    # testing does. Hard-guarded: WindowFromPoint at that point must
    # resolve to the spawned Pane HWND (or a child of it, via its root
    # ancestor and PID) BEFORE the cursor moves, and again immediately
    # BEFORE the click; a foreign window covering the point cancels the
    # click. The cursor position is saved first and restored afterwards
    # only if nothing moved it meanwhile (the user's hand always wins).
    # The exact-HWND foreground guard still decides whether any key is
    # sent; activationMethod records 'guarded-click' only on confirmed
    # focus. Real pointer in play: keep hands off the mouse during the run
    # and opt in only when testing your own window.
    [switch]$ClickToFocus
)

$ErrorActionPreference = 'Stop'

# ---------------------------------------------------------------- run dirs
# Per-run scratch under the explicit OutputDir: unique names, never reused,
# and this helper performs no filesystem deletion of any kind (old runs stay
# around; the only Remove-Item calls are environment variables).
$runStamp = Get-Date -Format 'yyyyMMdd-HHmmss'
$runId = '{0}-{1}' -f $runStamp, ([Guid]::NewGuid().ToString('N').Substring(0, 8))
if (-not (Test-Path -LiteralPath $OutputDir)) {
    New-Item -ItemType Directory -Path $OutputDir | Out-Null
}
$OutputDir = (Resolve-Path -LiteralPath $OutputDir).Path
$Binary = (Resolve-Path -LiteralPath $Binary).Path
$runDir = Join-Path $OutputDir "run-$runId"
$dataDir = Join-Path $runDir 'pane-data'            # PANE_DATA_DIR (read by the app)
$localAppData = Join-Path $runDir 'localappdata'    # child LOCALAPPDATA (cache_dir base)
$expectedCache = Join-Path $localAppData 'Pane\cache'   # the app's actual cache location
New-Item -ItemType Directory -Path $runDir, $dataDir, $localAppData | Out-Null
$stderrLog = Join-Path $runDir 'pane-stderr.log'
if (-not (Test-Path -LiteralPath $ExtensionsDir)) {
    Write-Warning "ExtensionsDir not found: $ExtensionsDir (samples will be absent)"
}

# Explicit outer-window size: both parameters together, each > 100.
if (($WindowWidthPixels -ne 0) -xor ($WindowHeightPixels -ne 0)) {
    throw 'WindowWidthPixels and WindowHeightPixels must be given together.'
}
if ($WindowWidthPixels -ne 0 -and ($WindowWidthPixels -le 100 -or $WindowHeightPixels -le 100)) {
    throw 'WindowWidthPixels and WindowHeightPixels must each be greater than 100.'
}

# ---------------------------------------------------------------- win32
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type @"
using System; using System.Runtime.InteropServices;
public static class PaneWin {
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int command);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT rect);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll")] public static extern bool GetCursorPos(out POINT p);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(POINT p);
    [DllImport("user32.dll")] public static extern IntPtr GetAncestor(IntPtr h, uint flags);
    [DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint dx, uint dy, uint data, UIntPtr extra);
    [DllImport("user32.dll")] public static extern int GetDpiForWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int cx, int cy, uint flags);
    [DllImport("user32.dll")] public static extern IntPtr GetWindow(IntPtr h, uint command);
    [DllImport("dwmapi.dll")] public static extern int DwmGetWindowAttribute(IntPtr h, int attr, out RECT rect, int size);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
    [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
}
"@
# Screenshots and window geometry use physical pixels at any display
# scaling (same call as scripts/smoke-windows.ps1 makes before measuring).
[PaneWin]::SetProcessDPIAware() | Out-Null

# ---------------------------------------------------------------- state
$meta = [ordered]@{
    script          = 'capture-pane.ps1'
    startedUtc      = (Get-Date).ToUniversalTime().ToString('o')
    binary          = $Binary
    runDir          = $runDir
    paneDataDir     = $dataDir
    localAppData    = $localAppData   # child LOCALAPPDATA (per-run scratch)
    cacheLocation   = $expectedCache  # %LOCALAPPDATA%\Pane\cache as the child sees it
    cacheDirExisted = $null           # set at the end: did the child use it?
    extensionsDir   = $ExtensionsDir
    theme           = $Theme
    material        = $Material
    keysRequested   = @($Keys)
    keysAbortedAt   = $null           # step at which focus was lost, if any
    captureMode     = 'window'
    backdrop        = $null
    zOrderVerified  = $null           # backdrop directly behind Pane?
    pid             = $null
    windowTitle     = $null
    windowRect      = $null      # GetWindowRect (includes DWM invisible borders)
    frameBounds     = $null      # DWMWA_EXTENDED_FRAME_BOUNDS (visible window)
    requestedWindowSize = $null  # explicit outer size requested, if any
    dpi             = $null
    focusAchieved   = $false
    activationMethod = $null    # 'SetForegroundWindow' | 'AppActivate' | $null
    foregroundHwnd   = $null    # final: what GetForegroundWindow actually reports
    foregroundPid    = $null    # final: the process that owns that window
    foregroundIsPane = $null    # final: does the foreground belong to our PID?
    foregroundBeforeFallback = $null  # snapshot after the SetForegroundWindow loop
    clickToFocusPoint  = $null   # guarded-click interior point (x,y physical px)
    clickToFocusGuards = $null   # WindowFromPoint guards: before move / before click
    cursorSaved        = $null   # cursor position before the guarded click
    cursorRestored     = $null   # restored only if the cursor never moved
    screenshots     = @()
    backdropClosed  = $null
    processClosed   = $null
    processExited   = $null
    exitCode        = $null
    errorMessage    = $null
}
if ($FullScreen) { $meta.captureMode = 'fullscreen' }
if ($Backdrop) {
    $base = 'ECECEC'; $accent = 'black'
    if ($BackdropPattern -eq 'dark') { $base = '161616'; $accent = 'white' }
    $meta.backdrop = '{0}: base #{1} with {2} blocks/bands (distinct blurred luminance)' -f $BackdropPattern, $base, $accent
}

# ------------------------------------------------- spawn-scoped environment
# Everything the child needs is set here and restored IMMEDIATELY after the
# child is spawned (the child keeps its own inherited copy); the helper's
# and the operator's environment is otherwise untouched for the whole run.
# PANE_ARTIFACTS is removed for the child so no default downloads or update
# checks run; if the operator had one, it is restored afterwards.
$spawnEnvNames = 'PANE_DATA_DIR', 'PANE_EXTENSIONS_DIR', 'PANE_THEME', 'PANE_MATERIAL', 'PANE_ARTIFACTS', 'LOCALAPPDATA'
$oldSpawnEnv = @{}
foreach ($name in $spawnEnvNames) {
    if (Test-Path "Env:$name") { $oldSpawnEnv[$name] = (Get-Item "Env:$name").Value }
}

$process = $null
$backdropForm = $null
$backdropBitmap = $null
$failed = $false
$spawned = $false

function Convert-Rect([PaneWin+RECT]$r) {
    # RECT -> capture rectangle (X, Y, Width, Height), guarded for size > 0.
    $w = $r.Right - $r.Left; $h = $r.Bottom - $r.Top
    if ($w -le 0 -or $h -le 0) { return $null }
    [pscustomobject]@{ X = $r.Left; Y = $r.Top; Width = $w; Height = $h }
}

function Get-WindowGeometry([IntPtr]$hwnd) {
    $rect = New-Object PaneWin+RECT
    [void][PaneWin]::GetWindowRect($hwnd, [ref]$rect)
    $frame = New-Object PaneWin+RECT
    $hr = [PaneWin]::DwmGetWindowAttribute($hwnd, 9, [ref]$frame, 16)  # DWMWA_EXTENDED_FRAME_BOUNDS
    $crop = $null
    if ($hr -eq 0) { $crop = Convert-Rect $frame }
    if ($null -eq $crop) { $crop = Convert-Rect $rect }   # pre-DWM fallback
    if ($null -eq $crop) { throw 'Pane window has no captureable bounds' }
    [pscustomobject]@{
        Crop        = $crop
        WindowRect  = '{0},{1},{2},{3}' -f $rect.Left, $rect.Top, $rect.Right, $rect.Bottom
        FrameBounds = if ($hr -eq 0) { '{0},{1},{2},{3}' -f $frame.Left, $frame.Top, $frame.Right, $frame.Bottom } else { $null }
    }
}

function Save-Capture([string]$Name, [pscustomobject]$Rect) {
    $path = Join-Path $script:runDir $Name
    $bmp = New-Object System.Drawing.Bitmap $Rect.Width, $Rect.Height
    try {
        $g = [System.Drawing.Graphics]::FromImage($bmp)
        try { $g.CopyFromScreen($Rect.X, $Rect.Y, 0, 0, $bmp.Size) }
        finally { $g.Dispose() }
        $bmp.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
    } finally { $bmp.Dispose() }
    return $path
}

function Save-Metadata {
    $meta.finishedUtc = (Get-Date).ToUniversalTime().ToString('o')
    $meta | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $script:runDir 'pane-run.json')
}

function Get-PointHit([int]$x, [int]$y) {
    # Which window is at the physical pixel (x, y): the hit HWND, its
    # top-level ancestor (GA_ROOT) and the PID owning the hit. A point
    # "belongs to Pane" when its root ancestor IS Pane's HWND - that
    # accepts Pane itself and any child window of it (same PID through the
    # window tree), and rejects a foreign window covering the point.
    $pt = New-Object PaneWin+POINT
    $pt.X = $x; $pt.Y = $y
    $hit = [PaneWin]::WindowFromPoint($pt)
    $ownerPid = [uint32]0
    $rootInt = [long]0
    if ($hit.ToInt64() -ne 0) {
        $rootInt = [PaneWin]::GetAncestor($hit, 2).ToInt64()   # GA_ROOT
        [void][PaneWin]::GetWindowThreadProcessId($hit, [ref]$ownerPid)
    }
    [pscustomobject]@{
        HwndInt = $hit.ToInt64()
        RootInt = $rootInt
        Pid     = $ownerPid
        Summary = 'hit 0x{0:x} root 0x{1:x} pid {2}' -f $hit.ToInt64(), $rootInt, $ownerPid
    }
}

function Get-ForegroundInfo([IntPtr]$target) {
    # What is ACTUALLY foreground right now: its HWND and the process that
    # owns it (GetWindowThreadProcessId), plus an exact Int64 comparison
    # against the spawned Pane window. This diagnoses a focus mismatch -
    # another window of the same process, or a different process entirely -
    # instead of leaving it a bare false. Compare numeric HWND values
    # explicitly with ToInt64() throughout.
    $fg = [PaneWin]::GetForegroundWindow()
    $ownerPid = [uint32]0
    if ($fg.ToInt64() -ne 0) {
        [void][PaneWin]::GetWindowThreadProcessId($fg, [ref]$ownerPid)
    }
    [pscustomobject]@{
        Hwnd    = $fg
        HwndInt = $fg.ToInt64()
        Pid     = $ownerPid
        Matches = ($fg.ToInt64() -eq $target.ToInt64())
        Summary = 'hwnd 0x{0:x} pid {1}' -f $fg.ToInt64(), $ownerPid
    }
}

function New-BackdropBitmap([string]$Pattern) {
    # 256x256 tile. The two patterns share accent geometry (corner blocks +
    # two horizontal bands) but sit on DIFFERENT luminance bases, so their
    # blurred averages are clearly different (~3/4 bright vs ~3/4 dark) and
    # the blocks/bands give structure to recognize through the blur. This
    # replaces inverted 50/50 checkerboards, which blur to the same average.
    $bmp = New-Object System.Drawing.Bitmap 256, 256
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $baseRgb = 0x16; $accent = [System.Drawing.Brushes]::White   # dark default
    if ($Pattern -eq 'light') { $baseRgb = 0xEC; $accent = [System.Drawing.Brushes]::Black }
    $baseBrush = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb($baseRgb, $baseRgb, $baseRgb))
    try {
        $g.FillRectangle($baseBrush, 0, 0, 256, 256)
        $g.FillRectangle($accent, 16, 16, 64, 64)     # corner block
        $g.FillRectangle($accent, 0, 96, 256, 24)     # thick band
        $g.FillRectangle($accent, 0, 160, 256, 8)     # thin band
        $g.FillRectangle($accent, 176, 176, 64, 64)   # corner block
    } finally {
        $g.Dispose()
        $baseBrush.Dispose()
    }
    return $bmp
}

# ---------------------------------------------------------------- main
try {
    try {
        Set-Item 'Env:LOCALAPPDATA' $localAppData     # child cache: scratch\Pane\cache
        Set-Item 'Env:PANE_DATA_DIR' $dataDir
        Set-Item 'Env:PANE_EXTENSIONS_DIR' $ExtensionsDir
        if ($Theme)    { Set-Item 'Env:PANE_THEME' $Theme }
        if ($Material) { Set-Item 'Env:PANE_MATERIAL' $Material }
        Remove-Item 'Env:PANE_ARTIFACTS' -ErrorAction SilentlyContinue
        $process = Start-Process -FilePath $Binary -PassThru -RedirectStandardError $stderrLog
        $spawned = $true
    }
    finally {
        # Restore the operator's environment immediately; the child keeps
        # the values it inherited at spawn.
        foreach ($name in $spawnEnvNames) {
            if ($oldSpawnEnv.ContainsKey($name)) { Set-Item "Env:$name" $oldSpawnEnv[$name] }
            else { Remove-Item "Env:$name" -ErrorAction SilentlyContinue }
        }
    }
    $meta.pid = $process.Id
    Write-Host "Launched Pane PID $($process.Id) (scratch LOCALAPPDATA: $localAppData); stderr -> $stderrLog"

    # Wait for the real top-level window to appear.
    $deadline = [DateTime]::UtcNow.AddMilliseconds($WindowTimeoutMs)
    while ($process.MainWindowHandle -eq 0) {
        if ($process.HasExited) {
            throw "Pane exited before showing a window (exit code $($process.ExitCode)); see $stderrLog"
        }
        if ([DateTime]::UtcNow -gt $deadline) { throw "Pane window did not appear within ${WindowTimeoutMs} ms" }
        Start-Sleep -Milliseconds 200
        $process.Refresh()
    }
    $hwnd = $process.MainWindowHandle
    $meta.windowTitle = $process.MainWindowTitle

    # Optional explicit resize of ONLY the spawned Pane window (outer size,
    # physical pixels), before backdrop geometry and any capture. Flags
    # SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE (0x16): the size alone
    # changes - position, Z-order and focus stay as they were, and no other
    # window is touched. The geometry read right after is the actual,
    # post-resize rect; metadata records the requested size beside it.
    if ($WindowWidthPixels -gt 0) {
        $meta.requestedWindowSize = '{0}x{1}' -f $WindowWidthPixels, $WindowHeightPixels
        [void][PaneWin]::SetWindowPos($hwnd, [IntPtr]::Zero, 0, 0, $WindowWidthPixels, $WindowHeightPixels, 0x16)
        Write-Host "Resized Pane window to ${WindowWidthPixels}x${WindowHeightPixels} (outer, physical pixels)."
        Start-Sleep -Milliseconds 500   # let the window and its layout settle
    }

    $geometry = Get-WindowGeometry $hwnd
    $meta.windowRect = $geometry.WindowRect
    $meta.frameBounds = $geometry.FrameBounds
    try { $meta.dpi = [PaneWin]::GetDpiForWindow($hwnd) } catch { $meta.dpi = $null }
    Write-Host ("Window 0x{0:x}: rect {1}, frame {2}, DPI {3}" -f $hwnd, $meta.windowRect, $meta.frameBounds, $meta.dpi)

    # Optional external backdrop. It is created hidden, sized over the window
    # area plus a margin, and shown WITHOUT activation (SW_SHOWNOACTIVATE).
    if ($Backdrop) {
        $backdropBitmap = New-BackdropBitmap $BackdropPattern
        $backdropForm = New-Object System.Windows.Forms.Form
        $backdropForm.FormBorderStyle = 'None'
        $backdropForm.ShowInTaskbar = $false
        $backdropForm.StartPosition = 'Manual'
        $backdropForm.TopMost = $false
        $backdropForm.BackgroundImage = $backdropBitmap
        $backdropForm.BackgroundImageLayout = 'Tile'
        $margin = 32
        $backdropForm.Location = New-Object System.Drawing.Point (($geometry.Crop.X - $margin), ($geometry.Crop.Y - $margin))
        $backdropForm.Size = New-Object System.Drawing.Size (($geometry.Crop.Width + 2 * $margin), ($geometry.Crop.Height + 2 * $margin))
        [void]$backdropForm.Handle                       # create the window
        [void][PaneWin]::ShowWindow($backdropForm.Handle, 4)   # SW_SHOWNOACTIVATE
        # Keep our own diagnostic window out of the guarded focus point.
        [void][PaneWin]::SetWindowPos($backdropForm.Handle, $hwnd, 0, 0, 0, 0, 0x13)
        [System.Windows.Forms.Application]::DoEvents()   # let it paint
    }

    # Focus the window Pane's own process owns; only then is any key sent.
    # HWND equality compares the numeric values explicitly with ToInt64().
    [void][PaneWin]::ShowWindow($hwnd, 9)   # SW_RESTORE: activate/restore
    $focusOk = $false
    for ($i = 0; $i -lt 10 -and -not $focusOk; $i++) {
        [void][PaneWin]::SetForegroundWindow($hwnd)
        Start-Sleep -Milliseconds 200
        $focusOk = ([PaneWin]::GetForegroundWindow().ToInt64() -eq $hwnd.ToInt64())
    }
    if ($focusOk) { $meta.activationMethod = 'SetForegroundWindow' }

    # Diagnosis (always): which window is actually foreground right after
    # the SetForegroundWindow loop, and which process owns it - recorded in
    # metadata and printed, so a mismatch is attributable (another window
    # of the spawned process vs a foreign process stealing foreground).
    $fgAfterLoop = Get-ForegroundInfo $hwnd
    Write-Host ("Foreground after SetForegroundWindow loop: {0} (Pane window 0x{1:x}, match: {2})" -f $fgAfterLoop.Summary, $hwnd.ToInt64(), $fgAfterLoop.Matches)

    # Targeted activation alternative when the plain call fails:
    # WScript.Shell.AppActivate with ONLY the spawned process Id. Its
    # boolean return alone is NOT focus and is never claimed as such -
    # confirmation is always the actual foreground HWND check, made before
    # any input is sent. No synthetic Alt key to unrelated windows, no
    # AttachThreadInput tricks: either the real foreground check confirms,
    # or keys are not sent at all.
    if (-not $focusOk) {
        $meta.foregroundBeforeFallback = $fgAfterLoop.Summary
        try {
            $shell = New-Object -ComObject WScript.Shell
            for ($i = 0; $i -lt 3 -and -not $focusOk; $i++) {
                [void]$shell.AppActivate($process.Id)
                Start-Sleep -Milliseconds 300
                $focusOk = ([PaneWin]::GetForegroundWindow().ToInt64() -eq $hwnd.ToInt64())
            }
            if ($focusOk) { $meta.activationMethod = 'AppActivate' }
        } catch {
            Write-Warning "AppActivate fallback failed: $($_.Exception.Message)"
        }
    }

    # Last-resort opt-in activation: ONE real mouse click at a known
    # interior point of Pane's window (center-x, visible top + 32 physical
    # pixels - the search-header area), the way a human tester activates an
    # app. Hard guards, in order: (1) WindowFromPoint at the point must
    # resolve into Pane's own window tree (root ancestor = the spawned
    # HWND) BEFORE the cursor moves; (2) after positioning, WindowFromPoint
    # and the actual cursor position are re-checked BEFORE the click; a
    # foreign window or a moved cursor cancels it. Cursor position is saved and
    # restored afterwards ONLY if it is still exactly at the click point -
    # the user's hand always wins. The real foreground HWND check decides
    # success; 'guarded-click' is recorded only on confirmed focus, and the
    # existing per-key guard still re-checks before every key.
    if (-not $focusOk -and $ClickToFocus) {
        $geomNow = Get-WindowGeometry $hwnd
        $clickX = $geomNow.Crop.X + [int]($geomNow.Crop.Width / 2)
        $clickY = $geomNow.Crop.Y + 32
        $meta.clickToFocusPoint = '{0},{1}' -f $clickX, $clickY
        $hit1 = Get-PointHit $clickX $clickY
        $guard1Ok = ($hit1.RootInt -eq $hwnd.ToInt64())
        $meta.clickToFocusGuards = 'guard1: {0}' -f $hit1.Summary
        Write-Host ("ClickToFocus guard1 at {0}: {1} (point is Pane's: {2})" -f $meta.clickToFocusPoint, $hit1.Summary, $guard1Ok)
        if ($guard1Ok) {
            $savedCursor = New-Object PaneWin+POINT
            [void][PaneWin]::GetCursorPos([ref]$savedCursor)
            $meta.cursorSaved = '{0},{1}' -f $savedCursor.X, $savedCursor.Y
            [void][PaneWin]::SetCursorPos($clickX, $clickY)
            Start-Sleep -Milliseconds 100
            $hit2 = Get-PointHit $clickX $clickY
            $cursorBeforeClick = New-Object PaneWin+POINT
            $cursorRead = [PaneWin]::GetCursorPos([ref]$cursorBeforeClick)
            $guard2Ok = ($hit2.RootInt -eq $hwnd.ToInt64() -and $cursorRead -and $cursorBeforeClick.X -eq $clickX -and $cursorBeforeClick.Y -eq $clickY)
            $meta.clickToFocusGuards = '{0} | guard2: {1}' -f $meta.clickToFocusGuards, $hit2.Summary
            if ($guard2Ok) {
                # One left click (down + up) at the current cursor position.
                [PaneWin]::mouse_event(0x2, 0, 0, 0, [UIntPtr]::Zero)   # LEFTDOWN
                Start-Sleep -Milliseconds 60
                [PaneWin]::mouse_event(0x4, 0, 0, 0, [UIntPtr]::Zero)   # LEFTUP
                Start-Sleep -Milliseconds 400
                $focusOk = ([PaneWin]::GetForegroundWindow().ToInt64() -eq $hwnd.ToInt64())
                if ($focusOk) { $meta.activationMethod = 'guarded-click' }
                Write-Host ("ClickToFocus clicked at {0}; focus confirmed: {1}" -f $meta.clickToFocusPoint, $focusOk)
            }
            else {
                $meta.errorMessage = 'ClickToFocus skipped: a foreign window covered the interior point immediately before the click; no click was made.'
                Write-Warning $meta.errorMessage
            }
            # Restore the cursor only if it is still exactly where the click
            # left it; if anything moved it meanwhile, leave it alone.
            $cursorNow = New-Object PaneWin+POINT
            [void][PaneWin]::GetCursorPos([ref]$cursorNow)
            if ($cursorNow.X -eq $clickX -and $cursorNow.Y -eq $clickY) {
                [void][PaneWin]::SetCursorPos($savedCursor.X, $savedCursor.Y)
                $meta.cursorRestored = $true
            }
            else {
                $meta.cursorRestored = $false   # moved by the user or another app
            }
        }
        else {
            $meta.errorMessage = 'ClickToFocus skipped: the interior point did not resolve to Pane window tree; cursor never moved, no click.'
            Write-Warning $meta.errorMessage
        }
    }
    $meta.focusAchieved = $focusOk

    # Final foreground snapshot: what is foreground now (after any fallback)
    # and who owns it.
    $fgFinal = Get-ForegroundInfo $hwnd
    $meta.foregroundHwnd = $fgFinal.Summary
    $meta.foregroundPid = $fgFinal.Pid
    $meta.foregroundIsPane = ($fgFinal.Pid -eq [uint32]$process.Id)
    Write-Host ("Foreground final: {0} (Pane PID {1}: {2}); focus {3} via {4}" -f $fgFinal.Summary, $process.Id, $meta.foregroundIsPane, $focusOk, $meta.activationMethod)

    if (-not $focusOk) {
        $meta.errorMessage = 'Pane window could not be focused; keys were NOT sent (SendKeys would reach another window). See foregroundHwnd/foregroundPid/foregroundIsPane in pane-run.json.'
        Write-Warning $meta.errorMessage
    }
    if (-not $focusOk -and $Keys.Count -gt 0) { $failed = $true }

    # Arrange the backdrop IMMEDIATELY BEHIND Pane in Z-order. Not
    # HWND_BOTTOM: that could bury it behind unrelated windows. With Pane
    # now focused (top of its band), inserting directly after Pane makes the
    # backdrop the window right below it, above everything else. Then
    # VERIFY: the window directly above the backdrop must be Pane itself
    # (GW_HWNDPREV on the backdrop).
    if ($null -ne $backdropForm) {
        [void][PaneWin]::SetWindowPos($backdropForm.Handle, $hwnd, 0, 0, 0, 0, 0x13)  # NOSIZE|NOMOVE|NOACTIVATE
        Start-Sleep -Milliseconds 100
        $above = [PaneWin]::GetWindow($backdropForm.Handle, 3)   # GW_HWNDPREV (0 is GW_HWNDFIRST)
        $meta.zOrderVerified = ($above.ToInt64() -eq $hwnd.ToInt64())
        if (-not $meta.zOrderVerified) {
            Write-Warning 'Backdrop is not directly behind Pane in Z-order; blur evidence may be invalid.'
        }
        else { Write-Host 'Backdrop verified directly behind Pane in Z-order.' }
    }

    Start-Sleep -Milliseconds $InitialWaitMs
    [System.Windows.Forms.Application]::DoEvents()   # keep the backdrop painted

    # Initial capture: window crop (default), whole virtual screen only on
    # explicit -FullScreen (collects unrelated desktop - opt in only).
    $captureRect = $geometry.Crop
    if ($FullScreen) {
        $vs = [System.Windows.Forms.SystemInformation]::VirtualScreen
        $captureRect = [pscustomobject]@{ X = $vs.X; Y = $vs.Y; Width = $vs.Width; Height = $vs.Height }
    }
    $initialName = if ($FullScreen) { '00-initial-fullscreen.png' } else { '00-initial-window.png' }
    $file = Save-Capture $initialName $captureRect
    $meta.screenshots += [ordered]@{ file = (Split-Path $file -Leaf); label = 'initial'; utc = (Get-Date).ToUniversalTime().ToString('o') }
    Write-Host "Saved $file"

    # Optional key sequence through native SendKeys, one capture per token.
    # Focus is re-checked BEFORE EVERY token: if Pane lost the foreground,
    # abort the sequence instead of typing into some other window.
    if ($Keys.Count -gt 0 -and $focusOk) {
        $step = 0
        foreach ($token in $Keys) {
            if ([PaneWin]::GetForegroundWindow().ToInt64() -ne $hwnd.ToInt64()) {
                # Record what stole the foreground at the moment of abort.
                $fgAbort = Get-ForegroundInfo $hwnd
                $meta.foregroundHwnd = $fgAbort.Summary
                $meta.foregroundPid = $fgAbort.Pid
                $meta.foregroundIsPane = ($fgAbort.Pid -eq [uint32]$process.Id)
                $meta.keysAbortedAt = $step
                $meta.errorMessage = "Foreground left Pane before key '$token' (now: $($fgAbort.Summary)); remaining keys were not sent."
                Write-Warning $meta.errorMessage
                $failed = $true
                break
            }
            $step++
            [System.Windows.Forms.SendKeys]::SendWait($token)
            Start-Sleep -Milliseconds $KeyDelayMs
            [System.Windows.Forms.Application]::DoEvents()
            $label = ($token -replace '[^A-Za-z0-9]+', '-').Trim('-')
            if ([string]::IsNullOrEmpty($label)) { $label = 'keys' }
            $name = '{0:d2}-after-{1}{2}.png' -f $step, $label, $(if ($FullScreen) { '-fullscreen' } else { '-window' })
            $file = Save-Capture $name $captureRect
            $meta.screenshots += [ordered]@{ file = (Split-Path $file -Leaf); label = "after '$token'"; utc = (Get-Date).ToUniversalTime().ToString('o') }
            Write-Host "Saved $file (after '$token')"
        }
    }
}
catch {
    $failed = $true
    $meta.errorMessage = $_.Exception.Message
    [Console]::Error.WriteLine("capture-pane failed: $($meta.errorMessage)")
}
finally {
    # The backdrop ALWAYS closes when the helper ends - it lives in this
    # helper's process and would not reliably outlive it; -LeaveOpen keeps
    # only Pane itself running.
    if ($null -ne $backdropForm) {
        try { $backdropForm.Close(); $backdropForm.Dispose() } catch { }
        if ($null -ne $backdropBitmap) { $backdropBitmap.Dispose() }
        $meta.backdropClosed = $true
    }
    # Close only what this run created: the Pane process it started
    # (graceful close, then that PID only) unless -LeaveOpen.
    if ($null -ne $process) {
        if ($LeaveOpen) {
            $meta.processClosed = $false
            Write-Host "Pane left running: PID $($process.Id)."
        } else {
            try {
                if (-not $process.HasExited) {
                    [void]$process.CloseMainWindow()      # WM_CLOSE to our window
                    if (-not $process.WaitForExit(5000)) {
                        Stop-Process -Id $process.Id -ErrorAction SilentlyContinue   # only the PID we started
                        $process.WaitForExit(5000) | Out-Null
                    }
                }
                $meta.processExited = $true
                try { $meta.exitCode = $process.ExitCode } catch { $meta.exitCode = $null }
            } catch {
                $meta.processExited = $false
                Write-Warning "Could not close Pane PID $($process.Id): $($_.Exception.Message)"
            }
            $meta.processClosed = $true
        }
    }
    # Did the child actually use the scratch cache location?
    if ($null -ne $meta.cacheLocation) {
        $meta.cacheDirExisted = (Test-Path -LiteralPath $meta.cacheLocation)
    }
    try { Save-Metadata } catch { Write-Warning "Could not write metadata: $($_.Exception.Message)" }
}

Write-Host ("Run dir: {0} (metadata: pane-run.json, stderr: pane-stderr.log, cache: {1})" -f $runDir, $expectedCache)
if ($failed) { exit 1 } else { exit 0 }
