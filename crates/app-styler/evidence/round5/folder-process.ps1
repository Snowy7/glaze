# Round 5, Explorer: with SeparateProcess on, where does a new folder window live, and how long
# after its last window closes does that folder process exit? Then: does a new window start a
# new process? Nothing attached here (run before the helper, or with it, to see it attach).
#   folder-process.ps1 -Log file [-Rounds 2]
param([Parameter(Mandatory)][string]$Log, [int]$Rounds = 2)
. "$PSScriptRoot\lib.ps1"
$script:Log = $Log
$shellPid = Shell-Pid
Note "shell explorer $shellPid; SeparateProcess=$((Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced').SeparateProcess)"
foreach ($round in 1..$Rounds) {
  $fg = Fg-Pid
  Take-Lock
  try {
    $w = Open-Folder 'C:\Windows'
    if (-not $w) { Note 'no window opened'; continue }
    $owner = Owner (Key $w)
    Note ("round {0}: window {1} in explorer {2} ({3}); foreground pid before {4}, after {5}" -f $round, (Key $w), $owner,
      $(if ($owner -eq $shellPid) { 'the SHELL' } else { 'a separate process' }), $fg, (Fg-Pid))
    foreach ($k in 1..3) { $w.Navigate2($folders[$rand.Next($folders.Count)]); Start-Sleep -Milliseconds 800 }
    $w.Quit()
    $closed = Get-Date
  } finally { Drop-Lock }
  if ($owner -eq $shellPid) { continue }
  $exited = $null
  for ($i = 0; $i -lt 600; $i++) {
    if (-not (Get-Process -Id $owner -ErrorAction SilentlyContinue)) { $exited = (Get-Date) - $closed; break }
    Start-Sleep -Milliseconds 500
  }
  if ($exited) { Note ("  explorer {0} exited {1:N1} s after its last window closed" -f $owner, $exited.TotalSeconds) }
  else { Note "  explorer $owner still running 300 s after its last window closed" }
  Start-Sleep -Seconds 30
}
