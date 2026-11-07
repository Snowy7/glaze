# Round 5 soak: for -Minutes, alternate an Explorer burst (explorer-bursts.ps1, one burst) and a
# Settings session (settings.ps1, one round), then sample every Explorer, Settings and the helper.
# Notes any Settings or folder Explorer process still running after its windows closed.
#   soak.ps1 -Log file [-Minutes 30]
param([Parameter(Mandatory)][string]$Log, [int]$Minutes = 30)
. "$PSScriptRoot\lib.ps1"
$script:Log = $Log
$end = (Get-Date).AddMinutes($Minutes)
$cycle = 0
Note "soak start ($Minutes min)"
while ((Get-Date) -lt $end) {
  $cycle++
  & "$PSScriptRoot\explorer-bursts.ps1" -Log $Log -Bursts 1 -Tag "cycle $cycle"
  & "$PSScriptRoot\settings.ps1" -Log $Log -Rounds 1 -Pause 0
  $left = @(Get-Process SystemSettings -ErrorAction SilentlyContinue) + @(Get-Process explorer -ErrorAction SilentlyContinue | Where-Object { $_.Id -ne (Shell-Pid) })
  if ($left) { Note ("  still running after cycle {0}: {1}" -f $cycle, (($left | ForEach-Object { "$($_.ProcessName) $($_.Id) $(Mb $_) MB" }) -join ', ')) }
  Start-Sleep -Seconds 20
}
Note "soak done after $cycle cycles"
