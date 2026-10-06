# Opt-in native smoke on Windows for the HUD (#141): with another
# application's window in front (a window of this script's own), a
# development build of Pane shows a HUD at
# once (`PANE_TEST_SHOW_HUD`, a failure's HUD, 3 seconds), and this checks
# its window as the system has it: centred near the bottom of its monitor,
# over other applications (topmost), letting the pointer through and never
# activating (transparent, non-activating), never the foreground window,
# and gone once its time is up. A screenshot of it over that window goes in the
# output folder for the release-validation ticket. Not part of the release
# matrix's smoke: run it by hand, or from a ticket's validation.
# Usage: scripts/smoke-windows-hud.ps1 [-OutDir smoke-hud] [-Program target/debug/pane.exe]
param([string]$OutDir = "smoke-hud", [string]$Program = "target/debug/pane.exe")
$ErrorActionPreference = "Stop"
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$data = Join-Path $OutDir "data"
if (Test-Path $data) { Remove-Item -Recurse -Force $data }
$env:PANE_DATA_DIR = $data
$env:PANE_THEME = "dark"
$env:PANE_MATERIAL = "opaque"
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type @"
using System; using System.Runtime.InteropServices; using System.Text;
public static class HudWin {
    [StructLayout(LayoutKind.Sequential)] public struct Rect { public int Left, Top, Right, Bottom; }
    [StructLayout(LayoutKind.Sequential)] public struct MonitorInfo { public int Size; public Rect Monitor; public Rect Work; public uint Flags; }
    public delegate bool EnumProc(IntPtr window, IntPtr parameter);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc each, IntPtr parameter);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetWindowText(IntPtr window, StringBuilder text, int length);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr window, out uint process);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr window, out Rect rect);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr window);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr window);
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll", EntryPoint = "GetWindowLongPtrW")] public static extern IntPtr GetWindowLongPtr(IntPtr window, int index);
    [DllImport("user32.dll")] public static extern IntPtr MonitorFromWindow(IntPtr window, uint flags);
    [DllImport("user32.dll")] public static extern bool GetMonitorInfo(IntPtr monitor, ref MonitorInfo info);
    // The visible window of process `process` titled `title`; zero if none.
    public static IntPtr Find(uint process, string title) {
        IntPtr found = IntPtr.Zero;
        EnumWindows(delegate (IntPtr window, IntPtr parameter) {
            uint owner;
            GetWindowThreadProcessId(window, out owner);
            if (owner != process || !IsWindowVisible(window)) { return true; }
            StringBuilder text = new StringBuilder(256);
            GetWindowText(window, text, 256);
            if (text.ToString() == title) { found = window; return false; }
            return true;
        }, IntPtr.Zero);
        return found;
    }
}
"@
# Rectangles in physical pixels, as the screenshot has them.
[HudWin]::SetProcessDPIAware() | Out-Null

function Capture($name) {
    $bounds = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
    $bitmap = New-Object System.Drawing.Bitmap $bounds.Width, $bounds.Height
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    $graphics.CopyFromScreen($bounds.Location, [System.Drawing.Point]::Empty, $bounds.Size)
    $bitmap.Save((Join-Path $OutDir $name))
}

# Another application's window in front: a plain window of this script's,
# filling the primary screen, so the HUD shows over it.
$other = New-Object System.Windows.Forms.Form
$other.Text = "Another application"
$other.BackColor = [System.Drawing.Color]::White
$other.StartPosition = "Manual"
$other.Bounds = [System.Windows.Forms.Screen]::PrimaryScreen.WorkingArea
$other.Show()
$other.Activate()
# Lets the window draw and answer the system while this script waits.
function Pump($milliseconds) {
    $until = (Get-Date).AddMilliseconds($milliseconds)
    do {
        [System.Windows.Forms.Application]::DoEvents()
        Start-Sleep -Milliseconds 20
    } while ((Get-Date) -lt $until)
}
Pump 500

$title = "HUD smoke"
$env:PANE_TEST_SHOW_HUD = $title
$pane = Start-Process $Program -PassThru -RedirectStandardError (Join-Path $OutDir "pane.stderr.txt")
try {
    $hud = [IntPtr]::Zero
    for ($i = 0; $i -lt 400 -and $hud -eq [IntPtr]::Zero; $i++) {
        Pump 50
        $hud = [HudWin]::Find([uint32]$pane.Id, "Pane HUD")
    }
    if ($hud -eq [IntPtr]::Zero) { throw "no HUD window appeared" }
    $shownAt = Get-Date
    Pump 200
    Capture "1-hud-over-another-window.png"

    $rect = New-Object HudWin+Rect
    if (-not [HudWin]::GetWindowRect($hud, [ref]$rect)) { throw "cannot read the HUD's rectangle" }
    $info = New-Object HudWin+MonitorInfo
    $info.Size = [System.Runtime.InteropServices.Marshal]::SizeOf($info)
    $monitor = [HudWin]::MonitorFromWindow($hud, 2)   # MONITOR_DEFAULTTONEAREST
    if (-not [HudWin]::GetMonitorInfo($monitor, [ref]$info)) { throw "cannot read the HUD's monitor" }
    $m = $info.Monitor
    "HUD $($rect.Left),$($rect.Top)-$($rect.Right),$($rect.Bottom) on monitor $($m.Left),$($m.Top)-$($m.Right),$($m.Bottom)" |
        Set-Content (Join-Path $OutDir "placement.txt")

    # Centred on its monitor.
    $centre = ($rect.Left + $rect.Right) / 2
    $monitorCentre = ($m.Left + $m.Right) / 2
    if ([math]::Abs($centre - $monitorCentre) -gt 2) { throw "the HUD is not centred: $centre against $monitorCentre" }
    # Near the bottom: in the monitor's lowest quarter, above its edge.
    $height = $m.Bottom - $m.Top
    $gap = $m.Bottom - $rect.Bottom
    if ($gap -lt 0 -or $gap -gt $height / 4) { throw "the HUD is not near the bottom: $gap px above it" }
    if ($rect.Top -lt $m.Top + $height / 2) { throw "the HUD is not in the lower half" }

    # Over other applications, letting the pointer through, never activating.
    $style = [HudWin]::GetWindowLongPtr($hud, -20).ToInt64()   # GWL_EXSTYLE
    if (($style -band 0x8) -eq 0) { throw "the HUD is not topmost" }                    # WS_EX_TOPMOST
    if (($style -band 0x20) -eq 0) { throw "the HUD does not let the pointer through" }  # WS_EX_TRANSPARENT
    if (($style -band 0x08000000) -eq 0) { throw "the HUD may activate" }              # WS_EX_NOACTIVATE
    if ([HudWin]::GetForegroundWindow() -eq $hud) { throw "the HUD took the focus" }

    # Gone once its time is up (a failure's 3 seconds), within a margin.
    while ([HudWin]::Find([uint32]$pane.Id, "Pane HUD") -ne [IntPtr]::Zero) {
        if (((Get-Date) - $shownAt).TotalSeconds -gt 8) { throw "the HUD stayed past its time" }
        Pump 100
    }
    Capture "2-hud-gone.png"
    "passed" | Set-Content (Join-Path $OutDir "result.txt")
    Write-Output "HUD smoke passed"
} finally {
    if (-not $pane.HasExited) { Stop-Process -Id $pane.Id -Force }
    $other.Close()
}
