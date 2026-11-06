# Bounded headless performance observation; no lifecycle changes or test filesystem writes.
param([int]$Seconds = 1200)
$ErrorActionPreference = 'Stop'
$exe = Join-Path (Resolve-Path "$PSScriptRoot/../../..").Path 'target/release/mypaper-index.exe'
$index = Get-Process mypaper-index -ErrorAction Stop | Where-Object Path -eq $exe | Select-Object -First 1
if (-not $index) { throw 'Deployed index not running' }
$null = $index.Handle
$logPath = Join-Path $env:LOCALAPPDATA 'mypaper/index/service.log'
$rows = [Collections.Generic.List[object]]::new()
$clock = [Diagnostics.Stopwatch]::StartNew()
$doneAt = $null
do {
  $index.Refresh()
  if ($index.HasExited) { throw 'Index restarted during observation' }
  $rows.Add([pscustomobject]@{seconds_from_start=([DateTime]::UtcNow-$index.StartTime.ToUniversalTime()).TotalSeconds;cpu_seconds=$index.TotalProcessorTime.TotalSeconds;private_bytes=$index.PrivateMemorySize64;working_set_bytes=$index.WorkingSet64})
  $log = @(Get-Content -LiteralPath $logPath | Where-Object { $_ -match "^\d+ $($index.Id) " } | ForEach-Object { [string]$_ })
  if (-not $doneAt -and ($log | Where-Object { $_ -match 'initial reconciliation and notification replay complete' })) { $doneAt = $clock.Elapsed.TotalSeconds }
  if ($null -ne $doneAt -and $clock.Elapsed.TotalSeconds-$doneAt -ge 5) { break }
  Start-Sleep -Seconds 1
} while ($clock.Elapsed.TotalSeconds -lt $Seconds)
[pscustomobject]@{pid=$index.Id;completed=($null -ne $doneAt);exe_sha256=(Get-FileHash $exe).Hash;log=$log;rows=$rows} |
  ConvertTo-Json -Depth 5 | Set-Content -Encoding utf8 (Join-Path $PSScriptRoot '../evidence/startup-catchup.json')
