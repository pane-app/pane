# Installs Pane for one user: the pane.exe program in %LOCALAPPDATA%\Pane
# and a Pane shortcut in the user's Start menu. Both are the user's own, so
# no administrator rights are needed; name another folder with -InstallDir.
#
# Run it in the folder the package unpacked (pane\), or name that folder
# with -PackageFolder. The install copies the program, makes the shortcut
# and runs `pane --version` to check what it installed.
#
# The package holds none of Pane's default extensions: Pane downloads them
# itself at first setup (see the README beside this script).
#
# Nothing is signed (no signing credentials exist), so if this script
# reached you inside a downloaded package, PowerShell may refuse it as a
# script from the internet: run it as the README says, with
#   powershell -ExecutionPolicy Bypass -File install.ps1
# or Unblock-File it once, and check the package's .sha256 file beside it.
param(
    [string]$InstallDir,
    [string]$PackageFolder
)
$ErrorActionPreference = "Stop"

$folder = if ($PackageFolder) { $PackageFolder } else { $PSScriptRoot }
$program = Join-Path $folder "pane.exe"
foreach ($file in @($program, (Join-Path $folder "README.txt"))) {
    if (-not (Test-Path $file)) {
        throw "install.ps1: $file is missing; run this in the folder the package unpacked"
    }
}
if (-not $env:LOCALAPPDATA) { throw "install.ps1: no user profile to install into (LOCALAPPDATA)" }
if (-not $env:APPDATA) { throw "install.ps1: no user profile to install into (APPDATA)" }
if (-not $InstallDir) { $InstallDir = Join-Path $env:LOCALAPPDATA "Pane" }

New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
Copy-Item $program (Join-Path $InstallDir "pane.exe") -Force

# A Start-menu shortcut of the user's own (%APPDATA% is the user's, not the
# machine's): no desktop entry, since an installer that asks nothing puts
# nothing on the desktop.
$menu = Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs"
if (-not (Test-Path $menu)) { New-Item -ItemType Directory -Force -Path $menu | Out-Null }
$link = Join-Path $menu "Pane.lnk"
$shortcut = (New-Object -ComObject WScript.Shell).CreateShortcut($link)
$shortcut.TargetPath = Join-Path $InstallDir "pane.exe"
$shortcut.WorkingDirectory = $InstallDir
$shortcut.Description = "Pane, a desktop launcher"
$shortcut.Save()

# What the install runs to check what it installed. Pane is a
# window-subsystem program, so its version text is read from a file rather
# than trusted to reach the console; the exit code is the check.
$version = Join-Path ([System.IO.Path]::GetTempPath()) "pane-version-$PID.txt"
$check = Start-Process -FilePath (Join-Path $InstallDir "pane.exe") -ArgumentList "--version" `
    -Wait -PassThru -RedirectStandardOutput $version
if ($check.ExitCode -ne 0) { throw "install.ps1: pane.exe --version failed ($($check.ExitCode))" }
if (Test-Path $version) {
    Write-Output ((Get-Content $version) -join "")
    Remove-Item $version -ErrorAction SilentlyContinue
}

Write-Output "Installed Pane to $InstallDir"
Write-Output "(remove `"$(Join-Path $InstallDir 'pane.exe')`" and `"$link`" to uninstall;"
Write-Output " Pane keeps its own data in `"$(Join-Path $InstallDir 'data')`".)"
