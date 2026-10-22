param(
    [Parameter(Mandatory=$true)][string]$LightCapture,
    [Parameter(Mandatory=$true)][string]$DarkCapture,
    [double]$MinimumDelta=2
)
# Measures only the empty list area in the standard 760x460 logical/96 DPI capture.
# Detects missing transparency. Softened pattern edges require visual inspection
# separately: a brightness difference alone does not establish blur.
Add-Type -AssemblyName System.Drawing
$a=[Drawing.Bitmap]::FromFile((Resolve-Path -LiteralPath $LightCapture).Path)
$b=[Drawing.Bitmap]::FromFile((Resolve-Path -LiteralPath $DarkCapture).Path)
try {
    if($a.Width -ne $b.Width -or $a.Height -ne $b.Height -or $a.Width -lt 740 -or $a.Height -lt 400) { throw 'Expected equal standard-size captures' }
    $delta=0.0
    for($y=340;$y -lt 390;$y++) {
        for($x=30;$x -lt 730;$x++) {
            $p=$a.GetPixel($x,$y); $q=$b.GetPixel($x,$y)
            $delta += [Math]::Abs($p.R-$q.R)+[Math]::Abs($p.G-$q.G)+[Math]::Abs($p.B-$q.B)
        }
    }
    $delta /= 50*700*3
    Write-Output ('Backdrop mean RGB response: {0:F3}; required > {1}' -f $delta,$MinimumDelta)
    if($delta -le $MinimumDelta) { throw 'FAIL: no meaningful backdrop response' }
    Write-Output 'PASS: backdrop affects the rendered panel'
} finally { $a.Dispose(); $b.Dispose() }
