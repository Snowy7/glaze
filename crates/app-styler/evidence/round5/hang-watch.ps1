# Round 5: logs every non-shell Explorer that stops answering for 6 s, with its busiest thread
# and that thread's stack (cdb -pv, non-invasive), once per pid. Ends nothing.
#   hang-watch.ps1 -Log file [-Minutes 60]
param([Parameter(Mandatory)][string]$Log, [int]$Minutes = 60)
. "$PSScriptRoot\lib.ps1"
$script:Log = $Log
$cdb = 'C:\Program Files (x86)\Windows Kits\10\Debuggers\x64\cdb.exe'
$sym = 'srv*C:\symbols*https://msdl.microsoft.com/download/symbols'
$strikes = @{}; $done = @{}
$end = (Get-Date).AddMinutes($Minutes)
Note "hang watch start ($Minutes min)"
while ((Get-Date) -lt $end) {
  $shellPid = Shell-Pid
  foreach ($p in (Get-Process explorer -ErrorAction SilentlyContinue | Where-Object { $_.Id -ne $shellPid -and -not $done[$_.Id] })) {
    if ($p.MainWindowHandle -eq 0 -or $p.Responding) { $strikes[$p.Id] = 0; continue }
    $strikes[$p.Id] = 1 + [int]$strikes[$p.Id]
    if ($strikes[$p.Id] -lt 3) { continue }
    $done[$p.Id] = $true
    $a = @{}; $p.Threads | ForEach-Object { $a[$_.Id] = $_.TotalProcessorTime.TotalMilliseconds }
    Start-Sleep -Seconds 2; $p.Refresh()
    $busy = $p.Threads | Sort-Object { $_.TotalProcessorTime.TotalMilliseconds - $a[$_.Id] } -Descending | Select-Object -First 1
    $ms = [math]::Round($busy.TotalProcessorTime.TotalMilliseconds - $a[$busy.Id])
    $out = Join-Path $PSScriptRoot "hang-$($p.Id).txt"
    & $cdb -pv -p $p.Id -y $sym -logo $out -c ("~~[0x{0:x}]s; k 40; q" -f $busy.Id) *> $null
    Note ("explorer {0} NOT RESPONDING (started {1:HH:mm:ss}, {2} MB): thread {3} used {4} ms of 2000; stack in {5}" -f $p.Id, $p.StartTime, (Mb $p), $busy.Id, $ms, $out)
  }
  Start-Sleep -Seconds 2
}
Note 'hang watch done'
