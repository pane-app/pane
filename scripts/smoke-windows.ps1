# Native GUI smoke on Windows: launches Pane, drives it with real key events
# and captures the screen.
# Usage: scripts/smoke-windows.ps1 -OutDir <output-dir>
param([string]$OutDir = "smoke")
$ErrorActionPreference = "Stop"
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
# The tested OS version and architecture
"$([System.Environment]::OSVersion.VersionString) $env:PROCESSOR_ARCHITECTURE" | Set-Content (Join-Path $OutDir "system.txt")
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type @"
using System; using System.Runtime.InteropServices;
public static class Win { [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h); }
"@
function Capture($name) {
    $bounds = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
    $bitmap = New-Object System.Drawing.Bitmap $bounds.Width, $bounds.Height
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    $graphics.CopyFromScreen($bounds.Location, [System.Drawing.Point]::Empty, $bounds.Size)
    $bitmap.Save((Join-Path $OutDir $name))
}
$process = Start-Process -FilePath "target/debug/pane.exe" -PassThru `
    -RedirectStandardError (Join-Path $OutDir "stderr.log")
for ($i = 0; $i -lt 50 -and $process.MainWindowHandle -eq 0; $i++) {
    Start-Sleep -Milliseconds 200; $process.Refresh()
}
if ($process.MainWindowHandle -eq 0) { throw "Pane window did not appear" }
Start-Sleep -Seconds 2
Capture "1-root.png"
function Check($name, $color) {
    python "$PSScriptRoot/check_screenshot.py" (Join-Path $OutDir $name) $color
    if ($LASTEXITCODE -ne 0) { throw "screenshot check failed: $name" }
}
Check "1-root.png" "8a96a3"
[Win]::SetForegroundWindow($process.MainWindowHandle) | Out-Null
# Open each sample command (Rust, JavaScript, TypeScript) and run an item.
foreach ($index in 0..2) {
    for ($i = 0; $i -lt $index; $i++) { [System.Windows.Forms.SendKeys]::SendWait("{DOWN}") }
    [System.Windows.Forms.SendKeys]::SendWait("{ENTER}"); Start-Sleep -Seconds 3
    Capture "$($index + 2)-command-$index.png"
    [System.Windows.Forms.SendKeys]::SendWait("{DOWN}{ENTER}"); Start-Sleep -Seconds 2
    Capture "$($index + 2)-result-$index.png"
    Check "$($index + 2)-result-$index.png" "9fd8a8"
    [System.Windows.Forms.SendKeys]::SendWait("{ESC}"); Start-Sleep -Seconds 1
}
Capture "5-back-to-root.png"
# Each command must have answered from its own guest, not the same view twice.
python "$PSScriptRoot/check_screenshot.py" --distinct @(2..4 | ForEach-Object { Join-Path $OutDir "$_-result-$($_ - 2).png" })
if ($LASTEXITCODE -ne 0) { throw "screenshot check failed: result screenshots are not distinct" }
if ($process.HasExited) { throw "Pane exited during the smoke" }
Stop-Process -Id $process.Id
