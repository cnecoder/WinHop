# Crop the chosen AI icon (A1.png) to a centered square containing the dark tile,
# excluding the gray backdrop and the bottom-right watermark, then round the
# corners (area outside the arc -> transparent). Output: tools/icon-source.png
Add-Type -AssemblyName System.Drawing

$srcPath = "$PSScriptRoot\A1.png"
$outPath = "$PSScriptRoot\icon-source.png"

$src = [System.Drawing.Bitmap]::FromFile($srcPath)

# Fixed crop: centered square inside the tile, above/left of the watermark.
$cx = 272; $cy = 252; $cw = 1500; $ch = 1500

function RoundedRect($x, $y, $w, $h, $r) {
    $p = New-Object System.Drawing.Drawing2D.GraphicsPath
    $d = $r * 2
    $p.AddArc($x, $y, $d, $d, 180, 90)
    $p.AddArc($x + $w - $d, $y, $d, $d, 270, 90)
    $p.AddArc($x + $w - $d, $y + $h - $d, $d, $d, 0, 90)
    $p.AddArc($x, $y + $h - $d, $d, $d, 90, 90)
    $p.CloseFigure()
    return $p
}

$out = New-Object System.Drawing.Bitmap($cw, $ch)
$g = [System.Drawing.Graphics]::FromImage($out)
$g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
$g.Clear([System.Drawing.Color]::Transparent)
$path = RoundedRect 0 0 $cw $ch 132
$g.SetClip($path)
$g.DrawImage($src, (New-Object System.Drawing.Rectangle(0, 0, $cw, $ch)),
    $cx, $cy, $cw, $ch, [System.Drawing.GraphicsUnit]::Pixel)

$out.Save($outPath, [System.Drawing.Imaging.ImageFormat]::Png)
$g.Dispose(); $out.Dispose(); $src.Dispose()
Write-Output "Saved $outPath ($cw x $ch)"
