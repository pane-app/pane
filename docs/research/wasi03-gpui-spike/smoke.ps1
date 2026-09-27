param(
    [Parameter(Mandatory=$true)][string]$InitialView,
    [Parameter(Mandatory=$true)][string]$ReplacementView,
    [string]$Executable = "$env:TEMP/kyoko-gpui-view-target/debug/kyoko-gpui-view-probe.exe",
    [string]$EvidenceDirectory = "$PSScriptRoot/evidence"
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class ProbeWindow {
    public delegate bool EnumCallback(IntPtr h, IntPtr l);
    [StructLayout(LayoutKind.Sequential)] public struct Rect { public int Left, Top, Right, Bottom; }
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out Rect r);
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint msg, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int command);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumCallback callback, IntPtr l);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    public static IntPtr FindProcessWindow(uint pid) {
        IntPtr found = IntPtr.Zero;
        EnumWindows((h,l) => { uint owner; GetWindowThreadProcessId(h, out owner); Rect r;
            if (owner == pid && GetWindowRect(h, out r) && r.Right-r.Left > 100 && r.Bottom-r.Top > 100) { found = h; return false; }
            return true;
        }, IntPtr.Zero);
        return found;
    }
}
'@
function Send-ProbeClick([IntPtr]$Handle, [int]$X, [int]$Y) {
    $scale = [ProbeWindow]::GetDpiForWindow($Handle) / 96.0
    $point = [IntPtr](([int]($Y * $scale) -shl 16) -bor [int]($X * $scale))
    [void][ProbeWindow]::PostMessage($Handle, 0x200, [IntPtr]::Zero, $point)
    Start-Sleep -Milliseconds 100
    [void][ProbeWindow]::PostMessage($Handle, 0x201, [IntPtr]1, $point)
    [void][ProbeWindow]::PostMessage($Handle, 0x202, [IntPtr]::Zero, $point)
    Start-Sleep -Milliseconds 500
}
function Save-ProbeImage([IntPtr]$Handle, [string]$Path) {
    [void][ProbeWindow]::SetForegroundWindow($Handle)
    Start-Sleep -Milliseconds 400
    $rect = New-Object ProbeWindow+Rect
    [void][ProbeWindow]::GetWindowRect($Handle, [ref]$rect)
    $bitmap = New-Object System.Drawing.Bitmap(($rect.Right - $rect.Left), ($rect.Bottom - $rect.Top))
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    try {
        $graphics.CopyFromScreen($rect.Left, $rect.Top, 0, 0, $bitmap.Size)
        $bitmap.Save($Path, [System.Drawing.Imaging.ImageFormat]::Png)
    } finally { $graphics.Dispose(); $bitmap.Dispose() }
}

$initial = Get-Content -LiteralPath $InitialView -Raw | ConvertFrom-Json
$replacement = Get-Content -LiteralPath $ReplacementView -Raw | ConvertFrom-Json
if ($initial.items.Count -lt 1 -or $replacement.items.Count -lt 1) { throw 'Both views need a row.' }
$runDir = Join-Path $env:TEMP ('kyoko-gpui-smoke-' + [Guid]::NewGuid().ToString('N'))
[void](New-Item -ItemType Directory -Path $runDir)
[void](New-Item -ItemType Directory -Force -Path $EvidenceDirectory)
$inputPath = Join-Path $runDir 'view.json'
$eventsPath = Join-Path $runDir 'events.jsonl'
Copy-Item -LiteralPath $InitialView -Destination $inputPath
Copy-Item -LiteralPath $InitialView -Destination (Join-Path $EvidenceDirectory 'initial-view.json')
Copy-Item -LiteralPath $ReplacementView -Destination (Join-Path $EvidenceDirectory 'replacement-view.json')
$process = Start-Process -FilePath $Executable -ArgumentList ('"{0}" "{1}"' -f $inputPath,$eventsPath) -WindowStyle Hidden -RedirectStandardError (Join-Path $EvidenceDirectory 'stderr.log') -PassThru
try {
    $handle = [IntPtr]::Zero
    for ($attempt = 0; $attempt -lt 100; $attempt++) {
        Start-Sleep -Milliseconds 200
        $process.Refresh()
        if ($process.HasExited) { throw "Probe exited before opening a window: $($process.ExitCode)" }
        $handle = [ProbeWindow]::FindProcessWindow($process.Id)
        if ($handle -ne [IntPtr]::Zero) { break }
    }
    if ($handle -eq [IntPtr]::Zero) { throw 'No native window after 20 seconds.' }
    # The helper starts hidden; reveal only its native window for the render test.
    [void][ProbeWindow]::ShowWindow($handle, 5)
    Start-Sleep -Milliseconds 800
    Save-ProbeImage $handle (Join-Path $EvidenceDirectory 'initial.png')
    Send-ProbeClick $handle 100 140
    Copy-Item -LiteralPath $ReplacementView -Destination $inputPath
    Send-ProbeClick $handle 100 80
    Save-ProbeImage $handle (Join-Path $EvidenceDirectory 'reloaded.png')
    Send-ProbeClick $handle 100 140
    $events = @(Get-Content -LiteralPath $eventsPath | ForEach-Object { $_ | ConvertFrom-Json })
    if ($events.Count -ne 2 -or $events[0].id -ne $initial.items[0].id -or $events[1].id -ne $replacement.items[0].id) {
        throw ('Unexpected activation events: ' + ($events | ConvertTo-Json -Compress))
    }
    Copy-Item -LiteralPath $eventsPath -Destination (Join-Path $EvidenceDirectory 'events.jsonl')
    [ordered]@{ result = 'pass'; initialTitle = $initial.title; replacementTitle = $replacement.title; events = $events; dpi = [ProbeWindow]::GetDpiForWindow($handle); initialSource = (Resolve-Path $InitialView).Path; replacementSource = (Resolve-Path $ReplacementView).Path } |
        ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $EvidenceDirectory 'result.json')
    Get-Content -LiteralPath (Join-Path $EvidenceDirectory 'result.json')
} finally {
    $process.Refresh()
    if (!$process.HasExited) {
        [void]$process.CloseMainWindow()
        if (!$process.WaitForExit(5000)) { $process.Kill() }
    }
}
