# Read-only measurement of the deployed service; does not hold the desktop/input lock.
param([string]$Exe = 'D:\Coding\projects\desktop\wallpaper\target\release\mypaper-index.exe', [int]$Seconds = 900)
$ErrorActionPreference = 'Stop'
$clock = [Diagnostics.Stopwatch]::StartNew()
$rows = @()
$previousPid = 0
$doneAt = $null
$interrupted = $false
while ($clock.Elapsed.TotalSeconds -lt $Seconds) {
  $service = Get-Process mypaper-index -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $Exe } | Select-Object -First 1
  if ($service) {
    if ($previousPid -and $previousPid -ne $service.Id) { $interrupted = $true; break }
    $service.Refresh()
    $rows += [pscustomobject]@{seconds=[Math]::Round($clock.Elapsed.TotalSeconds,2); pid=$service.Id; private_bytes=$service.PrivateMemorySize64; peak_private_bytes=$service.PeakPagedMemorySize64; working_set_bytes=$service.WorkingSet64; cpu_ms=$service.TotalProcessorTime.TotalMilliseconds}
    $previousPid = $service.Id
    if ($rows.Count % 10 -eq 0) {
      $status = (& $Exe --status) | ConvertFrom-Json
      if ($LASTEXITCODE -ne 0) { throw 'Status failed' }
      if (-not $status.scanning -and -not $doneAt) { $doneAt = $clock.Elapsed.TotalSeconds }
      if ($doneAt -and $clock.Elapsed.TotalSeconds - $doneAt -gt 30) { break }
    }
  }
  Start-Sleep -Seconds 1
}
if (-not $rows.Count) { throw 'No deployed index process observed' }
$last = $rows[-1]
[pscustomobject]@{
  completed = [bool]$doneAt
  interrupted = $interrupted
  reconcile_seconds = $doneAt
  observed_seconds = [Math]::Round($clock.Elapsed.TotalSeconds,2)
  entries = $status.entries
  peak_private_bytes = ($rows | Measure-Object peak_private_bytes -Maximum).Maximum
  max_sampled_private_bytes = ($rows | Measure-Object private_bytes -Maximum).Maximum
  peak_working_set_bytes = ($rows | Measure-Object working_set_bytes -Maximum).Maximum
  final_private_bytes = $last.private_bytes
  final_working_set_bytes = $last.working_set_bytes
  rows = $rows
} | ConvertTo-Json -Depth 4
