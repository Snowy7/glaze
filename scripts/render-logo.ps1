# Renders assets/logo/*.svg to PNGs and multi-size .ico files, with headless Edge.
# Each mark has a large source and a hand-tuned small one used at <= 32 px:
#   Glaze (app)  mark.svg  + mark-small.svg  -> png/mark-<px>.png  + glaze.ico
#   Files        files.svg + files-small.svg -> png/files-<px>.png + glaze-files.ico
#   powershell -NoProfile -File scripts/render-logo.ps1 [-Only mark|files]
param([ValidateSet("mark", "files")][string]$Only)
$ErrorActionPreference = "Stop"
$root = Split-Path $PSScriptRoot -Parent
$logo = Join-Path $root "assets\logo"
$out = Join-Path $logo "png"
New-Item -ItemType Directory -Force $out | Out-Null

$marks = @(
  @{ Name = "mark"; Large = "mark.svg"; Small = "mark-small.svg"; Ico = "glaze.ico" },
  @{ Name = "files"; Large = "files.svg"; Small = "files-small.svg"; Ico = "glaze-files.ico" }
) | Where-Object { -not $Only -or $_.Name -eq $Only }

$edge = @(
  "${env:ProgramFiles(x86)}\Microsoft\Edge\Application\msedge.exe",
  "$env:ProgramFiles\Microsoft\Edge\Application\msedge.exe"
) | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $edge) { throw "Microsoft Edge not found" }
# A profile of our own, so concurrent runs (or a user's Edge) never share one.
$profile = Join-Path $env:TEMP "glaze-logo-edge-$PID"

function Render([string]$svg, [int]$size, [string]$png) {
  $html = Join-Path $env:TEMP "glaze-logo-$PID-$size.html"
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
    -RedirectStandardError (Join-Path $env:TEMP "glaze-logo-edge-$PID.log")
  $deadline = (Get-Date).AddSeconds(15)
  while (-not (Test-Path $png) -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 100 }
  Remove-Item $html -ErrorAction SilentlyContinue
  if (-not (Test-Path $png)) { throw "render failed: $png" }
}

# .ico with PNG-compressed entries (supported since Vista).
function WriteIco([string]$name, [int[]]$sizes, [string]$ico) {
  # The leading comma keeps each byte[] whole; without it PowerShell unrolls
  # every array into one flat list of bytes (and every entry looks 1 byte long).
  $blobs = @($sizes | ForEach-Object { , [IO.File]::ReadAllBytes((Join-Path $out "$name-$_.png")) })
  $stream = New-Object IO.MemoryStream
  $w = New-Object IO.BinaryWriter $stream
  $w.Write([uint16]0); $w.Write([uint16]1); $w.Write([uint16]$sizes.Count)
  $offset = 6 + 16 * $sizes.Count
  for ($i = 0; $i -lt $sizes.Count; $i++) {
    $s = $sizes[$i]; $dim = if ($s -ge 256) { 0 } else { $s }
    $w.Write([byte]$dim); $w.Write([byte]$dim); $w.Write([byte]0); $w.Write([byte]0)
    $w.Write([uint16]1); $w.Write([uint16]32)
    $w.Write([uint32]$blobs[$i].Length); $w.Write([uint32]$offset)
    $offset += $blobs[$i].Length
  }
  foreach ($b in $blobs) { $w.Write([byte[]]$b) }
  $w.Flush()
  [IO.File]::WriteAllBytes((Join-Path $logo $ico), $stream.ToArray())
}

$sizes = 16, 20, 24, 32, 48, 64, 128, 256, 512
try {
  foreach ($mark in $marks) {
    foreach ($s in $sizes) {
      $svg = if ($s -le 32) { $mark.Small } else { $mark.Large }
      $png = Join-Path $out "$($mark.Name)-$s.png"
      Remove-Item $png -ErrorAction SilentlyContinue
      Render $svg $s $png
    }
    WriteIco $mark.Name @(16, 20, 24, 32, 48, 64, 128, 256) $mark.Ico
    "rendered $($mark.Name): $($sizes -join ', ') px + $($mark.Ico)"
  }
} finally {
  # Headless Edge can leave helper processes behind; end the ones on our profile.
  Get-CimInstance Win32_Process -Filter "Name = 'msedge.exe'" |
    Where-Object { $_.CommandLine -like "*$profile*" } |
    ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
  Remove-Item $profile -Recurse -Force -ErrorAction SilentlyContinue
  Remove-Item (Join-Path $env:TEMP "glaze-logo-edge-$PID.log") -ErrorAction SilentlyContinue
}
