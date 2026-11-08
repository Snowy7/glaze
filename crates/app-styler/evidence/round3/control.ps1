# Control for the soak's Explorer memory growth: the same bursts with experimental.app_styler OFF
# (no TAP session active), then the flag back on. Each burst takes the desktop lock only while it
# is free (soak.ps1 waits), 35 s apart.
param([int]$Bursts = 3, [string]$Out = "C:\glaze-targets\app-styler\round3\control.log")
$ErrorActionPreference = 'Stop'
$config = Join-Path $env:APPDATA 'glaze\config.json'
$utf8 = New-Object Text.UTF8Encoding($false)
function Note([string]$m) { $l = '{0} {1}' -f (Get-Date -Format 'HH:mm:ss'), $m; Add-Content -LiteralPath $Out -Value $l; Write-Host $l }
function Set-Flag([bool]$on) {
  $text = [IO.File]::ReadAllText($config)
  $new = if ($on) { $text -replace '"app_styler":\s*false', '"app_styler": true' } else { $text -replace '"app_styler":\s*true', '"app_styler": false' }
  if ($new -ne $text) { $t = "$config.ctl.tmp"; [IO.File]::WriteAllText($t, $new, $utf8); Move-Item -LiteralPath $t -Destination $config -Force }
}
try {
  Set-Flag $false
  Start-Sleep -Seconds 4
  Note "flag off; styler running: $([bool](Get-Process glaze-styler -ErrorAction SilentlyContinue))"
  foreach ($n in 1..$Bursts) {
    powershell -NoProfile -ExecutionPolicy Bypass -File C:\glaze-targets\app-styler\round3\soak.ps1 -Minutes 0 -Pause 0 -Log $Out | Out-Null
    if ($n -lt $Bursts) { Start-Sleep -Seconds 35 }
  }
} finally {
  Set-Flag $true
  Start-Sleep -Seconds 4
  Note "flag on; styler running: $([bool](Get-Process glaze-styler -ErrorAction SilentlyContinue))"
}
