# visual-workbench.ps1 - the Windows reference/native comparison workbench
# (#91), in one command. See docs/visual-workbench.md.
#
#   ./scripts/visual-workbench.ps1                       # everything
#   ./scripts/visual-workbench.ps1 -Scenario root-selected -SkipSensitivity
#
# 1. verifies the authored reference's pinned SHA-256;
# 2. builds the native fixture (cargo build -p pane --bin pane-visual-fixture);
# 3. writes the fixture's scenario registry (the steps both sides follow);
# 4. captures the reference boards and scenarios (headless Chrome, Windows labels);
# 5. captures the native fixture's scenarios (off-screen, inactive, posted input);
# 6. compares them (report.json, summary.md, side-by-sides, diffs, crops);
# 7. re-runs the native capture with each deliberate perturbation (row
#    padding +4px, wrong selected fill, wrong hover fill, wrong selected
#    Settings section fill) and proves the comparison flips the checks
#    those faults drive (sensitivity);
# 8. with -RealAppSmoke, runs one real search interaction with pane.exe.
# A run that skips the build first verifies that the fixture was built by
# the workbench from the sources in the tree (freshness.py).
# Everything it writes goes under -OutputDir; every process it starts it
# closes, and every temporary directory it creates it deletes.

[CmdletBinding()]
param(
    [string]$OutputDir = (Join-Path $PSScriptRoot ('../.scratch/visual-workbench/run-' + (Get-Date -Format 'yyyyMMdd-HHmmss'))),
    [string]$Reference = (Join-Path $PSScriptRoot '../docs/evidence/ui-prototype/reference/launcher.html'),
    [string]$ExpectedSha256 = 'F7E81E030E2216FE61509B9AFD98A68147D1998C73F0A168BAB02D0BF00F0BB4',
    [string[]]$Scenario = @(),
    [ValidateSet('dark', 'light')]
    [string]$Theme = 'dark',
    [ValidateSet('glass', 'opaque')]
    [string]$Material = 'opaque',
    [switch]$SkipBuild,
    [switch]$SkipSensitivity,
    [switch]$RealAppSmoke,
    [string]$Chrome = 'C:/Program Files/Google/Chrome/Application/chrome.exe'
)

$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
Set-Location $repo
$Scenario = @($Scenario | ForEach-Object { $_ -split ',' } | Where-Object { $_ })
New-Item -ItemType Directory -Force -Path $OutputDir | Out-Null
$OutputDir = (Resolve-Path -LiteralPath $OutputDir).Path
$workbench = Join-Path $PSScriptRoot 'visual-workbench'
$fixture = Join-Path $repo 'target/debug/pane-visual-fixture.exe'
$revision = (git rev-parse HEAD).Trim()
$dirty = [bool](git status --porcelain --untracked-files=no)
$run = [ordered]@{
    command = $MyInvocation.Line
    revision = $revision
    workingTreeDirty = $dirty
    outputDir = $OutputDir
    theme = $Theme
    material = $Material
    steps = [ordered]@{}
}
function Save-Run { $run | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $OutputDir 'workbench-run.json') -Encoding utf8 }
function Step([string]$name, [scriptblock]$body) {
    Write-Host "== $name"
    $started = Get-Date
    try {
        & $body
        $run.steps[$name] = [ordered]@{ ok = $true; seconds = [int]((Get-Date) - $started).TotalSeconds }
    } catch {
        $run.steps[$name] = [ordered]@{ ok = $false; error = $_.Exception.Message }
        Save-Run
        throw
    }
}
function Invoke-Checked([string]$what, [scriptblock]$body) {
    # Native tools report progress on stderr (cargo does), which Windows
    # PowerShell would raise as an error under 'Stop'; their exit code is
    # what decides.
    $previous = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try { & $body 2>&1 | ForEach-Object { "$_" } | Out-Host }
    finally { $ErrorActionPreference = $previous }
    if ($LASTEXITCODE -ne 0) { throw "$what exited with $LASTEXITCODE" }
}

Step 'reference-hash' {
    $hash = (Get-FileHash -LiteralPath $Reference -Algorithm SHA256).Hash
    if ($hash -ne $ExpectedSha256) { throw "reference $Reference is $hash, expected $ExpectedSha256" }
    $run.reference = [ordered]@{ path = (Resolve-Path $Reference).Path; sha256 = $hash }
    $requested = Join-Path $repo '.scratch/ui-reference/launcher.html'
    if (Test-Path -LiteralPath $requested) {
        $run.reference.requestedCopySha256 = (Get-FileHash -LiteralPath $requested -Algorithm SHA256).Hash
    }
}

if (-not $SkipBuild) {
    Step 'build-fixture' { Invoke-Checked 'cargo build' { cargo build -p pane --bin pane-visual-fixture --locked -j 1 } }
}
if (-not (Test-Path -LiteralPath $fixture)) { throw "no fixture at $fixture; run without -SkipBuild" }
$run.fixture = [ordered]@{ path = $fixture; sha256 = (Get-FileHash -LiteralPath $fixture -Algorithm SHA256).Hash }
# No old binary passes unnoticed (#103): a build records the binary's hash
# and its sources' digest beside it; a run that skips the build verifies
# both, and refuses a binary built by hand or older than the tree.
Step 'fixture-freshness' {
    $action = if ($SkipBuild) { 'verify' } else { 'record' }
    $output = python (Join-Path $workbench 'freshness.py') $action --binary $fixture --repo $repo
    $code = $LASTEXITCODE
    $run.fixture.freshness = ($output | Select-Object -Last 1) | ConvertFrom-Json
    if ($code -ne 0) { throw "stale fixture: $($run.fixture.freshness.reason); run without -SkipBuild" }
}

$registry = Join-Path $OutputDir 'registry.json'
Step 'registry' {
    $process = Start-Process -FilePath $fixture -ArgumentList '--registry', ('"{0}"' -f $registry) -PassThru -Wait
    if ($process.ExitCode -ne 0 -or -not (Test-Path -LiteralPath $registry)) { throw "the fixture could not write its registry ($($process.ExitCode))" }
}

$scenarioArgs = @()
if ($Scenario.Count -gt 0) { $scenarioArgs = @('--scenario', ($Scenario -join ',')) }
Step 'reference-capture' {
    Invoke-Checked 'reference capture' {
        node (Join-Path $workbench 'reference-capture.mjs') --reference $Reference --sha256 $ExpectedSha256 `
            --registry $registry --out (Join-Path $OutputDir 'reference') --chrome $Chrome @scenarioArgs
    }
}

function Capture-Native([string]$dir, [string]$perturb, [string[]]$only) {
    $arguments = @{
        Fixture = $fixture; Registry = $registry; OutputDir = $dir; ApplicationRevision = $revision
        Theme = $Theme; Material = $Material; Perturb = $perturb
    }
    if ($only.Count -gt 0) { $arguments.Scenario = $only }
    & (Join-Path $workbench 'native-capture.ps1') @arguments
    if ($LASTEXITCODE -ne 0) { throw "native capture ($perturb) exited with $LASTEXITCODE" }
    Copy-Item -LiteralPath $registry -Destination (Join-Path $dir 'registry.json')
}

function Compare-Run([string]$native, [string]$out, [string]$label, [string]$baseline) {
    $arguments = @((Join-Path $workbench 'compare.py'), '--native', $native, '--reference', (Join-Path $OutputDir 'reference'),
        '--out', $out, '--label', $label)
    if ($baseline) { $arguments += @('--baseline', $baseline) }
    Invoke-Checked "compare ($label)" { python @arguments }
}

$native = Join-Path $OutputDir 'native'
Step 'native-capture' { Capture-Native $native 'none' $Scenario }
Step 'compare' { Compare-Run $native (Join-Path $OutputDir 'compare') 'baseline' $null }

if (-not $SkipSensitivity) {
    # Each fault must flip, from passed to failed, the checks it drives:
    # the row's horizontal edges for the padding, the selected and the
    # hover wash's alpha for the fills - in the harness section (against
    # the fixture's declared intent) at least, and in parity (against the
    # reference) wherever that check passed before.
    $expectations = @{
        'row-padding-plus-4' = 'wash left'
        'selected-fill' = 'wash alpha (selected)'
        'hover-fill' = 'wash alpha (hovered)'
        'nav-selected-fill' = 'nav wash alpha (selected)'
        'segment-on-fill' = 'segment on wash alpha'
        'well-fill' = 'well fill alpha'
    }
    $perturbedScenarios = @{
        'row-padding-plus-4' = @('root-rest', 'root-selected')
        'selected-fill' = @('root-rest', 'root-selected')
        # Movement selects the row under the pointer (#94): a hover wash
        # shows only where the keys moved the selection off the row the
        # pointer rests on.
        'hover-fill' = @('root-pointer-keys')
        # The Settings shell's own selection (#97).
        'nav-selected-fill' = @('settings-shell')
        # The Appearance page's chosen segment (#98).
        'segment-on-fill' = @('appearance-page')
        # The wells of Pane's own pages (#99): the Keyboard page's recorders.
        'well-fill' = @('settings-keyboard')
    }
    $run.sensitivity = [ordered]@{}
    foreach ($perturb in 'row-padding-plus-4', 'selected-fill', 'hover-fill', 'nav-selected-fill', 'segment-on-fill',
        'well-fill') {
        Step "sensitivity-$perturb" {
            $dir = Join-Path $OutputDir "native-$perturb"
            Capture-Native $dir $perturb $perturbedScenarios[$perturb]
            $out = Join-Path $OutputDir "compare-$perturb"
            Compare-Run $dir $out $perturb (Join-Path $OutputDir 'compare/report.json')
            $report = Get-Content -LiteralPath (Join-Path $out 'report.json') -Raw -Encoding utf8 | ConvertFrom-Json
            $flips = @($report.sensitivity | Where-Object { $_.id -like "*$($expectations[$perturb])*" -or
                ($perturb -ne 'row-padding-plus-4' -and $_.id -like 'parity/*wash alpha') })
            $run.sensitivity[$perturb] = [ordered]@{
                expected = $expectations[$perturb]
                flipped = $flips.Count
                harness = @($flips | Where-Object { $_.id -like 'harness-native/*' }).Count
                parity = @($flips | Where-Object { $_.id -like 'parity/*' }).Count
                examples = @($flips | Select-Object -First 4)
            }
            if (@($flips | Where-Object { $_.id -like 'harness-native/*' }).Count -eq 0) {
                throw "the $perturb perturbation flipped no '$($expectations[$perturb])' check: the comparison is not sensitive to it"
            }
        }
    }
}

if ($RealAppSmoke) {
    Step 'real-app-smoke' {
        Invoke-Checked 'cargo build pane' { cargo build -p pane --bin pane --locked -j 1 }
        & (Join-Path $workbench 'real-app-smoke.ps1') -Binary (Join-Path $repo 'target/debug/pane.exe') `
            -OutputDir (Join-Path $OutputDir 'real-app-smoke') -ApplicationRevision $revision
        if ($LASTEXITCODE -ne 0) { throw "real-app smoke exited with $LASTEXITCODE" }
    }
}

Save-Run
Write-Host "Workbench run: $OutputDir (summary: compare/summary.md)"
