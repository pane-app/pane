# Opt-in native smoke on Windows for revealing and the Recycle Bin (#145): a
# development build of Pane reveals one file and moves another to the
# Recycle Bin at once (`PANE_TEST_SYSTEM_LOG`, `PANE_TEST_REVEAL`,
# `PANE_TEST_TRASH`), through the system functions commands use
# (`pane_core::system::native`), and this checks what the system shows: a
# File Explorer window open on the first file's folder with that file
# selected, and the second file gone from its folder and held by the
# Recycle Bin. A screenshot of the Explorer window goes in the output
# folder for the release-validation ticket. The recycled file stays in the
# Recycle Bin (named "Recycle me <time>.txt"), for the person running the
# smoke to see and empty. Not part of the release matrix's smoke: run it by
# hand, or from a ticket's validation.
# Usage: scripts/smoke-windows-system.ps1 [-OutDir smoke-system] [-Program target/debug/pane.exe]
param([string]$OutDir = "smoke-system", [string]$Program = "target/debug/pane.exe")
$ErrorActionPreference = "Stop"
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$OutDir = (Resolve-Path $OutDir).Path
$data = Join-Path $OutDir "data"
if (Test-Path $data) { Remove-Item -Recurse -Force $data }
$env:PANE_DATA_DIR = $data
Add-Type -AssemblyName System.Windows.Forms, System.Drawing

function Capture($name) {
    $bounds = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
    $bitmap = New-Object System.Drawing.Bitmap $bounds.Width, $bounds.Height
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    $graphics.CopyFromScreen($bounds.Location, [System.Drawing.Point]::Empty, $bounds.Size)
    $bitmap.Save((Join-Path $OutDir $name))
}

# Two files of this run's own, in a folder of the output's (a long path,
# as File Explorer names it).
$stamp = Get-Date -Format "yyyyMMddHHmmss"
$folder = Join-Path $OutDir "files-$stamp"
New-Item -ItemType Directory -Force -Path $folder | Out-Null
$revealed = Join-Path $folder "Reveal me.txt"
$recycled = Join-Path $folder "Recycle me $stamp.txt"
Set-Content -Path $revealed -Value "Pane shows this file selected in File Explorer"
Set-Content -Path $recycled -Value "Pane moves this file to the Recycle Bin"

$log = Join-Path $OutDir "answers.txt"
if (Test-Path $log) { Remove-Item $log }
$env:PANE_TEST_SYSTEM_LOG = $log
$env:PANE_TEST_REVEAL = $revealed
$env:PANE_TEST_TRASH = $recycled
$shell = New-Object -ComObject Shell.Application
$explorer = $null
$pane = Start-Process $Program -PassThru -RedirectStandardError (Join-Path $OutDir "pane.stderr.txt")
try {
    for ($i = 0; $i -lt 600 -and -not (Test-Path $log); $i++) { Start-Sleep -Milliseconds 50 }
    if (-not (Test-Path $log)) { throw "Pane wrote no answers" }
    Start-Sleep -Milliseconds 200
    $answers = @(Get-Content $log)
    if ($answers -notcontains "reveal: ok") { throw "reveal answered: $answers" }
    if ($answers -notcontains "trash: ok") { throw "trash answered: $answers" }

    # A File Explorer window on the folder, with the file selected.
    for ($i = 0; $i -lt 200 -and $null -eq $explorer; $i++) {
        Start-Sleep -Milliseconds 50
        foreach ($window in $shell.Windows()) {
            try { $path = $window.Document.Folder.Self.Path } catch { continue }
            if ($path -eq $folder) { $explorer = $window }
        }
    }
    if ($null -eq $explorer) { throw "no File Explorer window shows $folder" }
    $selected = @($explorer.Document.SelectedItems() | ForEach-Object { $_.Path })
    if ($selected -notcontains $revealed) { throw "File Explorer selected '$selected', not $revealed" }
    Start-Sleep -Milliseconds 500
    Capture "1-revealed-in-explorer.png"

    # The recycled file left its folder, and the Recycle Bin holds it.
    if (Test-Path $recycled) { throw "the recycled file is still in its folder" }
    $bin = $shell.Namespace(10)   # ssfBITBUCKET
    $held = @($bin.Items() | Where-Object { $_.Name -like "Recycle me $stamp*" })
    if ($held.Count -ne 1) { throw "the Recycle Bin holds $($held.Count) items named Recycle me $stamp" }
    "Recycle Bin: $($held[0].Name)" | Add-Content $log

    "passed" | Set-Content (Join-Path $OutDir "result.txt")
    Write-Output "System smoke passed"
} finally {
    if (-not $pane.HasExited) { Stop-Process -Id $pane.Id -Force }
    if ($null -ne $explorer) { $explorer.Quit() }
}
