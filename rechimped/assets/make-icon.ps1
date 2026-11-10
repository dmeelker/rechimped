Add-Type -AssemblyName System.Drawing

$accent = [System.Drawing.ColorTranslator]::FromHtml('#f36f21')
$fur = [System.Drawing.ColorTranslator]::FromHtml('#5d3a24')
$skin = [System.Drawing.ColorTranslator]::FromHtml('#f6d2ab')
$innerEar = [System.Drawing.ColorTranslator]::FromHtml('#e9b088')
$ink = [System.Drawing.ColorTranslator]::FromHtml('#2b1a10')
$sizes = 16, 20, 24, 32, 40, 48, 64, 128, 256

function New-RoundedRect([single]$x, [single]$y, [single]$w, [single]$h, [single]$r) {
    $p = New-Object System.Drawing.Drawing2D.GraphicsPath
    $d = 2 * $r
    $p.AddArc($x, $y, $d, $d, 180, 90)
    $p.AddArc($x + $w - $d, $y, $d, $d, 270, 90)
    $p.AddArc($x + $w - $d, $y + $h - $d, $d, $d, 0, 90)
    $p.AddArc($x, $y + $h - $d, $d, $d, 90, 90)
    $p.CloseFigure()
    $p
}

function Fill-Oval($g, $brush, [double]$s, [double]$cx, [double]$cy, [double]$rx, [double]$ry) {
    $g.FillEllipse($brush, [single](($cx - $rx) * $s), [single](($cy - $ry) * $s), [single](2 * $rx * $s), [single](2 * $ry * $s))
}

function Draw-Icon([int]$s) {
    $bmp = New-Object System.Drawing.Bitmap $s, $s, ([System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.SmoothingMode = 'AntiAlias'
    $g.PixelOffsetMode = 'HighQuality'
    $g.Clear([System.Drawing.Color]::Transparent)
    $small = $s -le 24

    # Edge to edge on tiny sizes so it stays legible.
    $m = if ($small) { 0 } else { $s * 0.04 }
    $bg = New-RoundedRect $m $m ($s - 2 * $m) ($s - 2 * $m) ($s * 0.22)
    $g.FillPath((New-Object System.Drawing.SolidBrush $accent), $bg)

    $furB = New-Object System.Drawing.SolidBrush $fur
    $skinB = New-Object System.Drawing.SolidBrush $skin
    $inkB = New-Object System.Drawing.SolidBrush $ink

    Fill-Oval $g $furB $s 0.20 0.50 0.15 0.15
    Fill-Oval $g $furB $s 0.80 0.50 0.15 0.15
    if (-not $small) {
        $earB = New-Object System.Drawing.SolidBrush $innerEar
        Fill-Oval $g $earB $s 0.20 0.50 0.085 0.085
        Fill-Oval $g $earB $s 0.80 0.50 0.085 0.085
    }
    Fill-Oval $g $furB $s 0.50 0.50 0.33 0.33

    Fill-Oval $g $skinB $s 0.405 0.45 0.125 0.14
    Fill-Oval $g $skinB $s 0.595 0.45 0.125 0.14
    Fill-Oval $g $skinB $s 0.50 0.63 0.215 0.16

    $eye = if ($small) { 0.055 } else { 0.045 }
    Fill-Oval $g $inkB $s 0.42 0.46 $eye ($eye * 1.15)
    Fill-Oval $g $inkB $s 0.58 0.46 $eye ($eye * 1.15)
    if ($s -ge 48) {
        Fill-Oval $g ([System.Drawing.Brushes]::White) $s 0.432 0.445 0.014 0.014
        Fill-Oval $g ([System.Drawing.Brushes]::White) $s 0.592 0.445 0.014 0.014
    }

    if (-not $small) {
        Fill-Oval $g $inkB $s 0.475 0.585 0.016 0.012
        Fill-Oval $g $inkB $s 0.525 0.585 0.016 0.012
    }
    $pen = New-Object System.Drawing.Pen $ink, ([Math]::Max(1.2, $s * 0.03))
    $pen.StartCap = 'Round'; $pen.EndCap = 'Round'
    $w = 0.19 * $s
    $g.DrawArc($pen, [single](0.5 * $s - $w / 2), [single](0.56 * $s), [single]$w, [single](0.12 * $s), 20, 140)

    $g.Dispose()
    $ms = New-Object System.IO.MemoryStream
    $bmp.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()
    , $ms.ToArray()
}

$frames = foreach ($s in $sizes) { , (Draw-Icon $s) }
$out = New-Object System.IO.MemoryStream
$bw = New-Object System.IO.BinaryWriter $out
$bw.Write([uint16]0); $bw.Write([uint16]1); $bw.Write([uint16]$sizes.Count)
$offset = 6 + 16 * $sizes.Count
for ($i = 0; $i -lt $sizes.Count; $i++) {
    $s = $sizes[$i]; $len = $frames[$i].Length
    $dim = if ($s -ge 256) { 0 } else { $s }
    $bw.Write([byte]$dim); $bw.Write([byte]$dim); $bw.Write([byte]0); $bw.Write([byte]0)
    $bw.Write([uint16]1); $bw.Write([uint16]32); $bw.Write([uint32]$len); $bw.Write([uint32]$offset)
    $offset += $len
}
foreach ($f in $frames) { $bw.Write($f) }
$bw.Flush()
[System.IO.File]::WriteAllBytes((Join-Path $PSScriptRoot 'rechimped.ico'), $out.ToArray())
"Wrote rechimped.ico ($($sizes -join ', ') px)"

# Linux desktop icon (hicolor 256x256), installed by linux/install.sh.
[System.IO.File]::WriteAllBytes((Join-Path $PSScriptRoot 'rechimped.png'), (Draw-Icon 256))
"Wrote rechimped.png (256 px)"
