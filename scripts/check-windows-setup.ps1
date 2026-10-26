# Checks Pane's Windows setup (#463) on a CI runner: installs the built
# Pane-Setup-<version>.exe silently into a scratch folder of the runner's
# user, with "Start Pane when I sign in" ticked, and checks what it made —
# the program and its version, the Start menu shortcut, the Apps entry and
# Pane's own launch-at-login registration; installs it again over the
# running Pane, which the setup closes, keeping Pane's data and settings;
# then uninstalls it silently from under a running Pane, and checks that
# everything the setup made is gone and Pane's data is kept.
#
# CI only (package.yml's Windows leg, after the setup is built): it changes
# the user's Start menu, Apps list and sign-in list, so it is never run on a
# computer of one's own.
#
# Usage: scripts/check-windows-setup.ps1 -Setup <Pane-Setup-x.exe> -Version <x>
param(
    [Parameter(Mandatory)] [string]$Setup,
    [Parameter(Mandatory)] [string]$Version
)
$ErrorActionPreference = "Stop"

# The setup's Apps entry: Inno Setup's key for the setup's AppId, which
# pane-core's application_update names too.
$entry = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\{0737054E-4FDB-4FAD-8AD1-20C980B0C16E}_is1"
$run = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run"
$link = Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs\Pane.lnk"
$appId = "HKCU:\Software\Classes\AppUserModelId\PaneApp.Pane"

$scratch = Join-Path ([System.IO.Path]::GetTempPath()) "pane-setup-check"
Remove-Item -Recurse -Force $scratch -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $scratch | Out-Null
$dir = Join-Path $scratch "Pane"
$program = Join-Path $dir "pane.exe"
# The Panes the setup starts keep their data in a scratch folder of their
# own (PANE_DATA_DIR, which they inherit from the setup), not the runner
# user's.
$data = Join-Path $scratch "data"
$env:PANE_DATA_DIR = $data
$logs = @()

function Fail([string]$why) {
    foreach ($log in $logs) {
        if (Test-Path $log) {
            Write-Output "---- $log"
            Get-Content $log | Write-Output
        }
    }
    throw "check-windows-setup.ps1: $why"
}

# Runs the program at $path with $arguments and waits for it, failing unless
# it exits with 0. Only the program itself is waited for (Start-Process
# -Wait would wait for the Pane the setup starts, too); its handle is held
# from the start, so its exit code can be read.
function Run([string]$path, [string[]]$arguments, [string]$what) {
    $process = Start-Process -FilePath $path -ArgumentList $arguments -PassThru
    $null = $process.Handle
    if (-not $process.WaitForExit(300000)) { Fail "$what was still running after 300 s" }
    if ($process.ExitCode -ne 0) { Fail "$what exited with $($process.ExitCode)" }
}

# Waits up to $seconds for $condition to hold, failing with $what if not.
function Until([scriptblock]$condition, [int]$seconds, [string]$what) {
    $deadline = (Get-Date).AddSeconds($seconds)
    while (-not (& $condition)) {
        if ((Get-Date) -gt $deadline) { Fail "timed out after $seconds s waiting for $what" }
        Start-Sleep -Milliseconds 500
    }
}

# The Panes running from the install folder.
function Panes {
    @(Get-Process -Name pane -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $program })
}

# The launch-at-login command Pane registered, if any.
function SignIn {
    (Get-ItemProperty -Path $run -Name Pane -ErrorAction SilentlyContinue).Pane
}

function Recorded {
    $settings = Join-Path $data "settings.json"
    (Test-Path $settings) -and ((Get-Content -Raw $settings) -match '"launchAtLogin": true')
}

$silent = @("/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART")

# Install, with the sign-in task ticked: the setup starts Pane with
# --launch-at-login, which registers it and records the choice.
$log = Join-Path $scratch "install.log"
$logs += $log
Run $Setup ($silent + @("/DIR=`"$dir`"", "/TASKS=startatsignin", "/LOG=`"$log`"")) "the setup"
if (-not (Test-Path $program)) { Fail "the setup installed no $program" }
foreach ($file in @("LICENSE.txt", "THIRD-PARTY-NOTICES.txt")) {
    if (-not (Test-Path (Join-Path $dir $file))) { Fail "the setup installed no $file" }
}

# The program's version, read from a file: Pane is a window-subsystem
# program (as install.ps1 checks it).
$said = Join-Path $scratch "version.txt"
$check = Start-Process -FilePath $program -ArgumentList "--version" -Wait -PassThru -RedirectStandardOutput $said
if ($check.ExitCode -ne 0) { Fail "pane.exe --version exited with $($check.ExitCode)" }
$reported = ((Get-Content $said) -join "").Trim()
if ($reported -ne "Pane $Version") { Fail "pane.exe --version said '$reported', not 'Pane $Version'" }
Write-Output "pane.exe --version: $reported"

# The Start menu shortcut, starting the installed program.
if (-not (Test-Path $link)) { Fail "the setup made no $link" }
$target = (New-Object -ComObject WScript.Shell).CreateShortcut($link).TargetPath
if ($target -ne $program) { Fail "the shortcut starts '$target', not $program" }
# It carries Pane's AppUserModelID, which Windows shows an extension's
# notifications under (#555), and the id is registered for the user.
$folder = (New-Object -ComObject Shell.Application).NameSpace((Split-Path $link))
$aumid = $folder.ParseName((Split-Path $link -Leaf)).ExtendedProperty("System.AppUserModel.ID")
if ($aumid -ne "PaneApp.Pane") { Fail "the shortcut's AppUserModelID is '$aumid', not PaneApp.Pane" }
if (-not (Test-Path $appId)) { Fail "the setup did not register Pane's AppUserModelID ($appId)" }

# The Apps entry: Pane, by pane-app, this version, Pane's icon, and the
# folder pane-core's updater compares with its own (InstallLocation).
if (-not (Test-Path $entry)) { Fail "the setup made no Apps entry ($entry)" }
$values = Get-ItemProperty -Path $entry
$values | Format-List | Out-String | Write-Output
$expected = @{
    DisplayName     = "Pane"
    DisplayVersion  = $Version
    Publisher       = "pane-app"
    DisplayIcon     = $program
    InstallLocation = "$dir\"
}
foreach ($name in $expected.Keys) {
    if ($values.$name -ne $expected[$name]) {
        Fail "the Apps entry's $name is '$($values.$name)', not '$($expected[$name])'"
    }
}
if (-not $values.UninstallString) { Fail "the Apps entry names no uninstaller" }

# Pane started, registered itself to start at sign-in and recorded the
# choice in its settings.
Until { (Panes).Count -gt 0 } 60 "the setup to start Pane"
Until { (SignIn) -eq "`"$program`"" } 120 "Pane to register itself to start at sign-in"
Until { Recorded } 60 "Pane to record the launch-at-login choice"
Write-Output "Pane registered itself to start at sign-in: $(SignIn)"

# Install again over the running Pane, as an upgrade does: the setup closes
# it first, and Pane's data, its settings and the sign-in choice stay.
$running = Panes | ForEach-Object { $_.Id }
$log = Join-Path $scratch "upgrade.log"
$logs += $log
Run $Setup ($silent + @("/DIR=`"$dir`"", "/LOG=`"$log`"")) "the setup run again"
Until { -not (Get-Process -Id $running -ErrorAction SilentlyContinue) } 30 "the setup to close the running Pane"
if (-not (Recorded)) { Fail "installing again lost Pane's launch-at-login choice" }
if ((SignIn) -ne "`"$program`"") { Fail "installing again lost Pane's sign-in registration" }
if ((Get-ItemProperty -Path $entry).DisplayVersion -ne $Version) { Fail "installing again changed the Apps entry's version" }
Write-Output "Installing over a running Pane closed it and kept its data and settings."

# Uninstall from under a running Pane, as Settings > Apps does with the
# uninstaller the entry names. A silent uninstall keeps Pane's data (the
# question's default answer): a marker in the data folder Pane uses by
# default stands in for it.
$kept = Join-Path $env:LOCALAPPDATA "Pane\data"
$madeKept = -not (Test-Path $kept)
New-Item -ItemType Directory -Force -Path $kept | Out-Null
$marker = Join-Path $kept "pane-setup-check.txt"
Set-Content -Path $marker -Value "kept"
Start-Process -FilePath $program | Out-Null
Until { (Panes).Count -gt 0 } 30 "Pane to start again"
Start-Sleep -Seconds 5
$uninstaller = Join-Path $dir "unins000.exe"
if (-not (Test-Path $uninstaller)) { Fail "the install holds no $uninstaller" }
$log = Join-Path $scratch "uninstall.log"
$logs += $log
Run $uninstaller ($silent + @("/LOG=`"$log`"")) "the uninstaller"
# The uninstaller runs on from a copy of itself in the temporary folder.
Until { -not (Test-Path $entry) } 120 "the uninstaller to remove the Apps entry"
Until { -not (Test-Path $program) } 60 "the uninstaller to remove pane.exe"
if ((Panes).Count -gt 0) { Fail "a Pane still runs from the removed install" }
if (Test-Path $link) { Fail "the uninstaller left $link" }
if (Test-Path $appId) { Fail "the uninstaller left Pane's AppUserModelID registered ($appId)" }
if ($null -ne (SignIn)) { Fail "the uninstaller left Pane's sign-in registration: $(SignIn)" }
if (-not (Test-Path $marker)) { Fail "a silent uninstall removed Pane's data" }
Remove-Item $marker
if ($madeKept) { Remove-Item -Recurse -Force (Join-Path $env:LOCALAPPDATA "Pane") -ErrorAction SilentlyContinue }
Write-Output "The uninstaller removed Pane, its shortcut, its AppUserModelID, its Apps entry and its sign-in registration, and kept its data."
Remove-Item -Recurse -Force $scratch -ErrorAction SilentlyContinue
