# App styler round 3 soak: our own Explorer windows opened, navigated and closed, Settings pages
# switched, and the rules files flipped (styled -> changed -> none -> styled) in quick bursts, with
# Explorer/Settings/helper memory and handles sampled between bursts. No synthetic input: Shell
# COM for Explorer, ms-settings: URIs for Settings. The desktop lock is held per burst only.
#
#   soak.ps1 -Minutes 30 [-Log file] [-Pause 45]
#   soak.ps1 -Minutes 0          one burst
param([int]$Minutes = 15, [string]$Log = "C:\glaze-targets\app-styler\round3\soak.log",
      [int]$Pause = 45, [switch]$Bare)
$ErrorActionPreference = 'Stop'
$dir = Join-Path $env:LOCALAPPDATA 'glaze\app-styler'
$lock = (Join-Path $env:LOCALAPPDATA 'Temp\glaze-tools\desktop.lock')
$utf8 = New-Object Text.UTF8Encoding($false)

function Note([string]$m) {
  $line = '{0} {1}' -f (Get-Date -Format 'HH:mm:ss'), $m
  Add-Content -LiteralPath $Log -Value $line -Encoding UTF8; Write-Host $line
}
function Write-Rules([string]$app, [string]$text) {
  $path = Join-Path $dir "$app.json"; $tmp = "$path.soak.tmp"
  [IO.File]::WriteAllText($tmp, $text, $utf8); Move-Item -LiteralPath $tmp -Destination $path -Force
}
function Sample([string]$tag) {
  $parts = foreach ($p in Get-Process explorer, SystemSettings, glaze-styler -ErrorAction SilentlyContinue) {
    '{0} {1}: {2:N1} MB priv, {3} handles, {4:N2} s cpu' -f $p.Name, $p.Id, ($p.PrivateMemorySize64 / 1MB), $p.HandleCount, $p.TotalProcessorTime.TotalSeconds
  }
  Note ("[$tag] " + ($parts -join ' | '))
}
function Take-Lock {
  for ($i = 0; $i -lt 240; $i++) {
    if (New-Item -ItemType Directory $lock -ErrorAction SilentlyContinue) { return }
    Start-Sleep -Milliseconds 500
  }
  throw 'desktop lock busy for 2 minutes'
}
function Drop-Lock { if (Test-Path -LiteralPath $lock) { Remove-Item -LiteralPath $lock -Recurse -Force } }

# The rules as the helper wrote them, and two variants: other values (new brushes, radii), none.
$original = @{}
foreach ($app in 'explorer', 'settings') { $original[$app] = [IO.File]::ReadAllText((Join-Path $dir "$app.json")) }
# -Bare: the TAP stays attached with no rules (no styling, no property queries) throughout.
if ($Bare) { foreach ($app in 'explorer', 'settings') { $original[$app] = '{}' } }
function Variant([string]$text) { $text.Replace('"16"', '"6"').Replace('"10"', '"7"').Replace('Transparent', '#08808080').Replace('#66FFFFFF', '#55FFFFFF') }
$cycle = @(
  @{ name = 'changed'; f = { param($a) if ($Bare) { '{}' } else { Variant $original[$a] } } },
  @{ name = 'none'; f = { param($a) '{}' } },
  @{ name = 'styled'; f = { param($a) $original[$a] } }
)
$flip = 0
function Flip {
  $step = $cycle[$script:flip % $cycle.Count]; $script:flip++
  foreach ($app in 'explorer', 'settings') { Write-Rules $app (& $step.f $app) }
  return $step.name
}

$shell = New-Object -ComObject Shell.Application
function Hwnds { @($shell.Windows() | ForEach-Object { try { [int64]$_.HWND } catch { } }) }
function Open-Folder([string]$path) {
  $before = Hwnds; $shell.Open($path)
  for ($i = 0; $i -lt 40; $i++) {
    Start-Sleep -Milliseconds 150
    $new = $shell.Windows() | Where-Object { try { $before -notcontains [int64]$_.HWND } catch { $false } } | Select-Object -First 1
    if ($new) { return $new }
  }
  return $null
}
$folders = 'C:\Windows', 'C:\Windows\System32', 'C:\Program Files', 'C:\Users\Public',
  'shell:::{f874310e-b6b7-47dc-bc84-b9e6b38f5903}', 'shell:::{e88865ea-0e1c-4e20-9aa6-edcd0212c87c}',
  'C:\Windows\Fonts', 'C:\glaze-targets\app-styler\round3'
$pages = 'display', 'personalization-colors', 'apps-volume', 'privacy', 'windowsupdate', 'about',
  'bluetooth', 'network-status', 'sound', 'taskbar'
$r = New-Object Random 7

function Burst([int]$n) {
  Take-Lock
  $opened = @()
  try {
    $t0 = Get-Date
    foreach ($k in 1..2) {
      $w = Open-Folder $folders[$r.Next($folders.Count)]
      if ($w) { $opened += $w; Note "  opened window $([int64]$w.HWND)" }
      Note "  rules -> $(Flip)"
    }
    foreach ($k in 1..6) {
      foreach ($w in $opened) { try { $w.Navigate2($folders[$r.Next($folders.Count)]) } catch { } }
      Start-Process "ms-settings:$($pages[$r.Next($pages.Count)])"
      Start-Sleep -Milliseconds (300 + $r.Next(700))
      Note "  rules -> $(Flip) (navigated)"
    }
    # Close one while a rules change is landing, the other after.
    if ($opened.Count -gt 0) { Note "  rules -> $(Flip)"; $opened[0].Quit(); Note '  closed one during a change' }
    Start-Sleep -Milliseconds 400
    foreach ($w in ($opened | Select-Object -Skip 1)) { try { $w.Quit() } catch { } }
    foreach ($app in 'explorer', 'settings') { Write-Rules $app $original[$app] }
    $script:flip = 0
    Note ("  burst {0}: {1:N0} s with the lock; rules back to styled" -f $n, ((Get-Date) - $t0).TotalSeconds)
  } finally {
    foreach ($w in $opened) { try { $w.Quit() } catch { } }
    foreach ($app in 'explorer', 'settings') { Write-Rules $app $original[$app] }
    Drop-Lock
  }
}

Note "soak start ($Minutes min)"
Sample 'start'
$end = (Get-Date).AddMinutes($Minutes)
$n = 0
$hold = 'C:\glaze-targets\app-styler\round3\pause'
do {
  # Another agent's measurement window: no lock, no windows; keep sampling.
  if (Test-Path -LiteralPath $hold) {
    Sample 'held (no burst)'
  } else {
    $n++
    Burst $n
    Start-Sleep -Seconds 3
    Sample "after burst $n"
  }
  if ((Get-Date) -lt $end) { Start-Sleep -Seconds $Pause }
} while ((Get-Date) -lt $end)
Start-Sleep -Seconds 10
Sample 'end'
Note 'soak done'
