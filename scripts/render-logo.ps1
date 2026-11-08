# Renders assets/logo/*.svg to PNGs and a multi-size .ico, with headless Edge.
# Small sizes (<= 32 px) use the hand-tuned mark-small.svg; larger ones mark.svg.
#   powershell -NoProfile -File scripts/render-logo.ps1
$ErrorActionPreference = "Stop"
$root = Split-Path $PSScriptRoot -Parent
$logo = Join-Path $root "assets\logo"
$out = Join-Path $logo "png"
New-Item -ItemType Directory -Force $out | Out-Null

$edge = @(
  "${env:ProgramFiles(x86)}\Microsoft\Edge\Application\msedge.exe",
  "$env:ProgramFiles\Microsoft\Edge\Application\msedge.exe"
) | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $edge) { throw "Microsoft Edge not found" }
$profile = Join-Path $env:TEMP "glaze-logo-edge"

function Render([string]$svg, [int]$size, [string]$png) {
  $html = Join-Path $env:TEMP "glaze-logo-$size.html"
  $uri = ([Uri](Join-Path $logo $svg)).AbsoluteUri
  Set-Content -Path $html -Encoding UTF8 -Value @"
<!doctype html><html><head><style>html,body{margin:0;background:transparent;overflow:hidden}
img{display:block;width:${size}px;height:${size}px}</style></head><body><img src="$uri"></body></html>
"@
  # Edge reports progress on stderr, which Windows PowerShell would turn into errors.
  $args = @("--headless=new", "--disable-gpu", "--hide-scrollbars", "--user-data-dir=`"$profile`"",
    "--default-background-color=00000000", "--force-device-scale-factor=1",
    "--window-size=$size,$size", "--screenshot=`"$png`"", ([Uri]$html).AbsoluteUri)
  Start-Process -FilePath $edge -ArgumentList $args -Wait -NoNewWindow `
    -RedirectStandardError (Join-Path $env:TEMP "glaze-logo-edge.log")
  $deadline = (Get-Date).AddSeconds(15)
  while (-not (Test-Path $png) -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 100 }
  if (-not (Test-Path $png)) { throw "render failed: $png" }
}

$sizes = 16, 20, 24, 32, 48, 64, 128, 256, 512
foreach ($s in $sizes) {
  $svg = if ($s -le 32) { "mark-small.svg" } else { "mark.svg" }
  $png = Join-Path $out "mark-$s.png"
  Remove-Item $png -ErrorAction SilentlyContinue
  Render $svg $s $png
}

# .ico with PNG-compressed entries (supported since Vista), 16..256.
$icoSizes = 16, 20, 24, 32, 48, 64, 128, 256
# The leading comma keeps each byte[] whole; without it PowerShell unrolls
# every array into one flat list of bytes (and every entry looks 1 byte long).
$blobs = @($icoSizes | ForEach-Object { , [IO.File]::ReadAllBytes((Join-Path $out "mark-$_.png")) })
$ico = New-Object IO.MemoryStream
$w = New-Object IO.BinaryWriter $ico
$w.Write([uint16]0); $w.Write([uint16]1); $w.Write([uint16]$icoSizes.Count)
$offset = 6 + 16 * $icoSizes.Count
for ($i = 0; $i -lt $icoSizes.Count; $i++) {
  $s = $icoSizes[$i]; $dim = if ($s -ge 256) { 0 } else { $s }
  $w.Write([byte]$dim); $w.Write([byte]$dim); $w.Write([byte]0); $w.Write([byte]0)
  $w.Write([uint16]1); $w.Write([uint16]32)
  $w.Write([uint32]$blobs[$i].Length); $w.Write([uint32]$offset)
  $offset += $blobs[$i].Length
}
foreach ($b in $blobs) { $w.Write([byte[]]$b) }
$w.Flush()
[IO.File]::WriteAllBytes((Join-Path $logo "glaze.ico"), $ico.ToArray())
Remove-Item $profile -Recurse -Force -ErrorAction SilentlyContinue
"rendered: $($sizes -join ', ') px + glaze.ico"
