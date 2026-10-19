# Original geometric UM artwork. Licensed under the project's MIT License.
# Run on Windows: powershell -NoProfile -File .\GENERATE-ICONS.ps1
# Uses System.Drawing on the CPU; no downloaded tools or GPU rendering.
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing

$umIcons = Join-Path $PSScriptRoot 'src-tauri/icons'
$umAssets = Join-Path $PSScriptRoot 'src/assets'
New-Item -ItemType Directory -Path $umIcons, $umAssets -Force | Out-Null
$umUtf8 = New-Object System.Text.UTF8Encoding($false)
$umLogoSvg = @'
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 512 512" fill="none">
  <rect x="8" y="8" width="496" height="496" rx="112" fill="#0d1726"/>
  <rect x="9" y="9" width="494" height="494" rx="111" stroke="#59d4e8" stroke-opacity=".28" stroke-width="2"/>
  <path d="M116 170V286C116 320 134 342 166 342S216 320 216 286V170" stroke="#59d4e8" stroke-width="30" stroke-linecap="round"/>
  <path d="M276 342V170L336 248L396 170V342" stroke="#eaf4ff" stroke-width="30" stroke-linecap="round" stroke-linejoin="round"/>
</svg>
'@
$umEngineSvg = @'
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100" fill="none">
  <path d="M18 27V61C18 71 23 76 32 76S46 71 46 61V27M65 27V76M65 27H84M65 51H80M65 76H84" stroke="#eaf4ff" stroke-width="6" stroke-linecap="square" stroke-linejoin="miter"/>
</svg>
'@
[System.IO.File]::WriteAllText((Join-Path $umAssets 'um-logo.svg'), $umLogoSvg + "`n", $umUtf8)
[System.IO.File]::WriteAllText((Join-Path $umAssets 'unreal-mark.svg'), $umEngineSvg + "`n", $umUtf8)

function New-UmRoundedPath {
    $umPath = New-Object System.Drawing.Drawing2D.GraphicsPath
    $umPath.AddArc(8, 8, 224, 224, 180, 90)
    $umPath.AddArc(280, 8, 224, 224, 270, 90)
    $umPath.AddArc(280, 280, 224, 224, 0, 90)
    $umPath.AddArc(8, 280, 224, 224, 90, 90)
    $umPath.CloseFigure()
    return $umPath
}

function New-UmBitmap {
    param([int]$Width, [int]$Height, [bool]$Opaque = $false,
          [bool]$Round = $false, [bool]$Foreground = $false)
    $umBitmap = New-Object System.Drawing.Bitmap($Width, $Height, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $umGraphics = [System.Drawing.Graphics]::FromImage($umBitmap)
    $umBackground = New-Object System.Drawing.SolidBrush([System.Drawing.ColorTranslator]::FromHtml('#0d1726'))
    $umBorder = New-Object System.Drawing.Pen([System.Drawing.Color]::FromArgb(71, 89, 212, 232), 2)
    $umUPen = New-Object System.Drawing.Pen([System.Drawing.ColorTranslator]::FromHtml('#59d4e8'), 30)
    $umMPen = New-Object System.Drawing.Pen([System.Drawing.ColorTranslator]::FromHtml('#eaf4ff'), 30)
    $umShape = New-UmRoundedPath
    $umU = New-Object System.Drawing.Drawing2D.GraphicsPath
    $umM = New-Object System.Drawing.Drawing2D.GraphicsPath
    try {
        $umGraphics.Clear($(if ($Opaque) { $umBackground.Color } else { [System.Drawing.Color]::Transparent }))
        $umGraphics.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
        $umGraphics.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
        $umGraphics.CompositingQuality = [System.Drawing.Drawing2D.CompositingQuality]::HighQuality
        $umGraphics.ScaleTransform([single]($Width / 512.0), [single]($Height / 512.0))
        if (-not $Foreground) {
            if ($Round) {
                $umGraphics.FillEllipse($umBackground, 8, 8, 496, 496)
                $umGraphics.DrawEllipse($umBorder, 9, 9, 494, 494)
            } else {
                $umGraphics.FillPath($umBackground, $umShape)
                $umGraphics.DrawPath($umBorder, $umShape)
            }
        }
        foreach ($umPen in @($umUPen, $umMPen)) {
            $umPen.StartCap = [System.Drawing.Drawing2D.LineCap]::Round
            $umPen.EndCap = [System.Drawing.Drawing2D.LineCap]::Round
            $umPen.LineJoin = [System.Drawing.Drawing2D.LineJoin]::Round
        }
        $umU.AddLine(116, 170, 116, 286)
        $umU.AddBezier(116, 286, 116, 320, 134, 342, 166, 342)
        $umU.AddBezier(166, 342, 198, 342, 216, 320, 216, 286)
        $umU.AddLine(216, 286, 216, 170)
        $umM.AddLine(276, 342, 276, 170)
        $umM.AddLine(276, 170, 336, 248)
        $umM.AddLine(336, 248, 396, 170)
        $umM.AddLine(396, 170, 396, 342)
        $umGraphics.DrawPath($umUPen, $umU)
        $umGraphics.DrawPath($umMPen, $umM)
    } catch {
        $umBitmap.Dispose()
        throw
    } finally {
        $umGraphics.Dispose()
        $umBackground.Dispose(); $umBorder.Dispose(); $umUPen.Dispose(); $umMPen.Dispose()
        $umShape.Dispose(); $umU.Dispose(); $umM.Dispose()
    }
    return $umBitmap
}

# Explicit dimensions make a fresh checkout reproducible, including unused targets.
$umSizes = [ordered]@{
    '32x32.png' = 32; '64x64.png' = 64; '128x128.png' = 128
    '128x128@2x.png' = 256; '256x256.png' = 256; '512x512.png' = 512; 'icon.png' = 512
    'Square107x107Logo.png' = 107; 'Square142x142Logo.png' = 142; 'Square150x150Logo.png' = 150
    'Square284x284Logo.png' = 284; 'Square30x30Logo.png' = 30; 'Square310x310Logo.png' = 310
    'Square44x44Logo.png' = 44; 'Square71x71Logo.png' = 71; 'Square89x89Logo.png' = 89; 'StoreLogo.png' = 50
    'android/mipmap-hdpi/ic_launcher_foreground.png' = 162
    'android/mipmap-hdpi/ic_launcher_round.png' = 49; 'android/mipmap-hdpi/ic_launcher.png' = 49
    'android/mipmap-mdpi/ic_launcher_foreground.png' = 108
    'android/mipmap-mdpi/ic_launcher_round.png' = 48; 'android/mipmap-mdpi/ic_launcher.png' = 48
    'android/mipmap-xhdpi/ic_launcher_foreground.png' = 216
    'android/mipmap-xhdpi/ic_launcher_round.png' = 96; 'android/mipmap-xhdpi/ic_launcher.png' = 96
    'android/mipmap-xxhdpi/ic_launcher_foreground.png' = 324
    'android/mipmap-xxhdpi/ic_launcher_round.png' = 144; 'android/mipmap-xxhdpi/ic_launcher.png' = 144
    'android/mipmap-xxxhdpi/ic_launcher_foreground.png' = 432
    'android/mipmap-xxxhdpi/ic_launcher_round.png' = 192; 'android/mipmap-xxxhdpi/ic_launcher.png' = 192
    'ios/AppIcon-20x20@1x.png' = 20; 'ios/AppIcon-20x20@2x-1.png' = 40
    'ios/AppIcon-20x20@2x.png' = 40; 'ios/AppIcon-20x20@3x.png' = 60
    'ios/AppIcon-29x29@1x.png' = 29; 'ios/AppIcon-29x29@2x-1.png' = 58
    'ios/AppIcon-29x29@2x.png' = 58; 'ios/AppIcon-29x29@3x.png' = 87
    'ios/AppIcon-40x40@1x.png' = 40; 'ios/AppIcon-40x40@2x-1.png' = 80
    'ios/AppIcon-40x40@2x.png' = 80; 'ios/AppIcon-40x40@3x.png' = 120
    'ios/AppIcon-512@2x.png' = 1024; 'ios/AppIcon-60x60@2x.png' = 120
    'ios/AppIcon-60x60@3x.png' = 180; 'ios/AppIcon-76x76@1x.png' = 76
    'ios/AppIcon-76x76@2x.png' = 152; 'ios/AppIcon-83.5x83.5@2x.png' = 167
}
foreach ($umEntry in $umSizes.GetEnumerator()) {
    $umRelative = [string]$umEntry.Key
    $umPath = Join-Path $umIcons $umRelative
    New-Item -ItemType Directory -Path (Split-Path $umPath -Parent) -Force | Out-Null
    $umBitmap = New-UmBitmap -Width $umEntry.Value -Height $umEntry.Value `
        -Opaque ($umRelative.StartsWith('ios/')) `
        -Round ($umRelative.EndsWith('_round.png')) `
        -Foreground ($umRelative.EndsWith('_foreground.png'))
    try { $umBitmap.Save($umPath, [System.Drawing.Imaging.ImageFormat]::Png) }
    finally { $umBitmap.Dispose() }
}

function Get-UmPngBytes {
    param([int]$Size)
    $umBitmap = New-UmBitmap -Width $Size -Height $Size
    $umStream = New-Object System.IO.MemoryStream
    try {
        $umBitmap.Save($umStream, [System.Drawing.Imaging.ImageFormat]::Png)
        return ,$umStream.ToArray()
    } finally { $umBitmap.Dispose(); $umStream.Dispose() }
}

# Modern Windows ICO entries hold PNG images; 256 is encoded as zero in the byte fields.
$umIcoSizes = @(16, 24, 32, 48, 64, 128, 256)
$umIcoImages = @($umIcoSizes | ForEach-Object { Get-UmPngBytes -Size $_ })
foreach ($umIcoName in @('icon.ico', 'unreal-manager.ico')) {
    $umStream = [System.IO.File]::Create((Join-Path $umIcons $umIcoName))
    $umWriter = New-Object System.IO.BinaryWriter($umStream)
    try {
        $umWriter.Write([uint16]0); $umWriter.Write([uint16]1); $umWriter.Write([uint16]$umIcoSizes.Count)
        $umOffset = 6 + 16 * $umIcoSizes.Count
        for ($umIndex = 0; $umIndex -lt $umIcoSizes.Count; $umIndex++) {
            $umSizeByte = [byte]$(if ($umIcoSizes[$umIndex] -eq 256) { 0 } else { $umIcoSizes[$umIndex] })
            $umWriter.Write($umSizeByte); $umWriter.Write($umSizeByte)
            $umWriter.Write([byte]0); $umWriter.Write([byte]0)
            $umWriter.Write([uint16]1); $umWriter.Write([uint16]32)
            $umWriter.Write([uint32]$umIcoImages[$umIndex].Length); $umWriter.Write([uint32]$umOffset)
            $umOffset += $umIcoImages[$umIndex].Length
        }
        foreach ($umImage in $umIcoImages) { $umWriter.Write([byte[]]$umImage) }
    } finally { $umWriter.Dispose() }
}

function Write-UmBigEndianUInt32 {
    param([System.IO.BinaryWriter]$Writer, [uint32]$Value)
    $umBytes = [System.BitConverter]::GetBytes($Value)
    if ([System.BitConverter]::IsLittleEndian) { [Array]::Reverse($umBytes) }
    $Writer.Write($umBytes)
}

# ICNS PNG chunks cover standard and Retina sizes without an external encoder.
$umIcnsTypes = [ordered]@{ 'icp4' = 16; 'icp5' = 32; 'icp6' = 64; 'ic07' = 128; 'ic08' = 256; 'ic09' = 512; 'ic10' = 1024 }
$umIcnsChunks = @($umIcnsTypes.GetEnumerator() | ForEach-Object {
    [PSCustomObject]@{ Type = $_.Key; Data = (Get-UmPngBytes -Size $_.Value) }
})
$umIcnsLength = 8
foreach ($umChunk in $umIcnsChunks) { $umIcnsLength += 8 + $umChunk.Data.Length }
$umStream = [System.IO.File]::Create((Join-Path $umIcons 'icon.icns'))
$umWriter = New-Object System.IO.BinaryWriter($umStream)
try {
    $umWriter.Write([System.Text.Encoding]::ASCII.GetBytes('icns'))
    Write-UmBigEndianUInt32 -Writer $umWriter -Value $umIcnsLength
    foreach ($umChunk in $umIcnsChunks) {
        $umWriter.Write([System.Text.Encoding]::ASCII.GetBytes($umChunk.Type))
        Write-UmBigEndianUInt32 -Writer $umWriter -Value (8 + $umChunk.Data.Length)
        $umWriter.Write([byte[]]$umChunk.Data)
    }
} finally { $umWriter.Dispose() }

$umAndroidBackground = Join-Path $umIcons 'android/values/ic_launcher_background.xml'
New-Item -ItemType Directory -Path (Split-Path $umAndroidBackground -Parent) -Force | Out-Null
[System.IO.File]::WriteAllText($umAndroidBackground, @'
<?xml version="1.0" encoding="utf-8"?>
<resources>
  <color name="ic_launcher_background">#0d1726</color>
</resources>
'@ + "`n", $umUtf8)
Write-Host "Generated $($umSizes.Count) PNGs, both ICO files, ICNS and original SVG assets."
