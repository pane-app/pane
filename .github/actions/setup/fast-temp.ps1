# Puts TMP and TEMP, where the tests make their temporary folders, on a
# faster drive than the runner's C:, as astral-sh/uv's
# .github/workflows/setup-dev-drive.ps1 does: the D: drive a runner image
# may have, else a Dev Drive (ReFS, no antivirus filter) made for this job
# in a virtual disk; either is given a change journal, as C: has. Anything
# going wrong leaves TMP and TEMP on C:, as they were, with a warning: the
# drive is a speed-up, never a requirement.
$ErrorActionPreference = 'Stop'
try {
    if (Test-Path 'D:\') {
        $drive = 'D:'
        Write-Output 'Using the runner''s D: drive'
    } else {
        $disk = 'C:\pane-dev-drive.vhdx'
        # 20 GB, allocated as it fills: room for the tests' temporary files.
        $volume = New-VHD -Path $disk -SizeBytes 20GB -Dynamic |
            Mount-VHD -Passthru |
            Initialize-Disk -Passthru |
            New-Partition -AssignDriveLetter -UseMaximumSize |
            Format-Volume -DevDrive -Confirm:$false -Force
        $drive = "$($volume.DriveLetter):"
        fsutil devdrv trust $drive
        fsutil devdrv enable /disallowAv
        # Mounted again so the antivirus setting applies; the letter is
        # read again, since remounting may give the volume another one.
        Dismount-VHD -Path $disk
        Mount-VHD -Path $disk
        $letter = Get-VHD -Path $disk | Get-Disk | Get-Partition |
            Where-Object DriveLetter | Select-Object -First 1 -ExpandProperty DriveLetter
        $drive = "${letter}:"
        fsutil devdrv query $drive
        Write-Output "Using a Dev Drive at $drive"
    }
    # Windows keeps a change journal on C: by default, and Pane catches up
    # from it after a restart (#174, #175); the runner's D: keeps none, so
    # the tests that catch up from the journal would reconcile instead.
    # The journal is made here as on C: (32 MB, grown by 8 MB), and read
    # back. Making one that is there already changes nothing.
    fsutil usn createjournal m=0x2000000 a=0x800000 $drive
    if ($LASTEXITCODE -ne 0) { throw "no change journal could be made on $drive" }
    fsutil usn queryjournal $drive | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "$drive keeps no change journal" }
    $temp = "$drive\tmp"
    New-Item -ItemType Directory -Force -Path $temp | Out-Null
    # Written to and read back, so a drive that is not usable is never used.
    $probe = Join-Path $temp 'probe'
    Set-Content -Path $probe -Value 'probe'
    Remove-Item -Path $probe
    Add-Content -Path $env:GITHUB_ENV -Value "TMP=$temp"
    Add-Content -Path $env:GITHUB_ENV -Value "TEMP=$temp"
    Write-Output "TMP and TEMP are $temp"
} catch {
    Write-Output "::warning::Temporary files stay on C: ($_)"
}
