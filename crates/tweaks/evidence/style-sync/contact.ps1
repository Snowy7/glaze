# Compact visual evidence. Original full-window/desktop captures stay in the local tools
# folder; the committed crops show the tested chrome and omit account/device details.
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
$cases = @('liquid-dark','oneui-dark','fluent-dark','solid-dark','mono-dark','liquid-light','explicit')
$sheet = New-Object Drawing.Bitmap 660, (50 + $cases.Count * 78)
$graphics = [Drawing.Graphics]::FromImage($sheet)
$font = New-Object Drawing.Font 'Segoe UI', 10
try {
  $graphics.Clear([Drawing.Color]::FromArgb(238,238,240))
  $graphics.DrawString('Resolved styles on temporary native DWM frames', $font, [Drawing.Brushes]::Black, 12, 12)
  $y = 44
  foreach ($case in $cases) {
    $path = Join-Path $PSScriptRoot "$case-frame.png"
    $image = [Drawing.Image]::FromFile($path)
    try {
      $graphics.DrawString($case, $font, [Drawing.Brushes]::Black, 12, $y)
      $graphics.DrawImageUnscaled($image, 12, ($y + 20))
    } finally { $image.Dispose() }
    $y += 78
  }
  $sheet.Save((Join-Path $PSScriptRoot 'native-frames.png'))
} finally { $graphics.Dispose(); $sheet.Dispose(); $font.Dispose() }

foreach ($case in @('liquid-dark','liquid-light','explicit')) {
  $image = [Drawing.Bitmap]::FromFile((Join-Path $PSScriptRoot "$case-ApplicationFrameHost.png"))
  try {
    $rect = [Drawing.Rectangle]::new(330, 0, [Math]::Min(1050, $image.Width - 330), [Math]::Min(280, $image.Height))
    $crop = $image.Clone($rect, $image.PixelFormat)
    try { $crop.Save((Join-Path $PSScriptRoot "$case-settings.png")) } finally { $crop.Dispose() }
  } finally { $image.Dispose() }
  $image = [Drawing.Bitmap]::FromFile((Join-Path $PSScriptRoot "$case-explorer.png"))
  try {
    $crop = $image.Clone([Drawing.Rectangle]::new(0, 0, $image.Width, [Math]::Min(140, $image.Height)), $image.PixelFormat)
    try { $crop.Save((Join-Path $PSScriptRoot "$case-explorer-header.png")) } finally { $crop.Dispose() }
  } finally { $image.Dispose() }
}

$tools = (Join-Path $env:LOCALAPPDATA 'Temp\glaze-tools\')
$archive = [IO.Path]::GetFullPath((Join-Path $tools 'style-sync-full'))
if (-not $archive.StartsWith($tools, [StringComparison]::OrdinalIgnoreCase)) { throw 'Invalid capture archive' }
[IO.Directory]::CreateDirectory($archive) | Out-Null
$manifest = foreach ($file in Get-ChildItem -LiteralPath $PSScriptRoot -File | Where-Object { $_.Name -match '-(explorer|ApplicationFrameHost|desktop)\.png$' }) {
  $destination = Join-Path $archive $file.Name
  $hash = (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash
  Move-Item -LiteralPath $file.FullName -Destination $destination -Force
  [pscustomobject]@{file=$file.Name;sha256=$hash}
}
$manifest | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $PSScriptRoot 'full-capture-hashes.json') -Encoding utf8
