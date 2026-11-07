# Round 5, Explorer (SeparateProcess on): bursts of two folder windows opened, each navigated 6
# times (folders, Home, Gallery), then closed. Each burst waits for the folder process to exit and
# samples the folder process (while open) and the shell's Explorer (after). With the helper
# running, also notes which pids it styled and refused (mypaper.log).
#   explorer-bursts.ps1 -Log file [-Bursts 5] [-Tag name]
param([Parameter(Mandatory)][string]$Log, [int]$Bursts = 5, [string]$Tag = '')
. "$PSScriptRoot\lib.ps1"
$script:Log = $Log
$helperLog = Join-Path $env:LOCALAPPDATA 'mypaper\mypaper.log'
function Styler { $s = Get-Process mypaper-styler -ErrorAction SilentlyContinue | Select-Object -First 1; if ($s) { 'styler {0}: {1} MB priv, {2:N2} s cpu' -f $s.Id, (Mb $s), $s.TotalProcessorTime.TotalSeconds } else { 'no styler' } }
$shellPid = Shell-Pid
Note "explorer bursts start $Tag ($Bursts bursts); shell explorer $shellPid; $(Styler)"
Sample-Explorers 'start'
foreach ($n in 1..$Bursts) {
  Take-Lock
  $opened = @(); $folderPids = @(); $peak = @(); $kept = @()
  try {
    foreach ($k in 0..1) {
      $w = Open-Folder $folders[$rand.Next($folders.Count)] $k
      if ($w) { $opened += $w; $kept += $script:FocusKept } else { Note '  nothing opened' }
    }
    $owners = @($opened | ForEach-Object { Owner (Key $_) } | Select-Object -Unique)
    # Each launch may get its own folder process: follow all of them.
    $folderPids = @($owners | Where-Object { $_ -ne $shellPid })
    if ($owners -contains $shellPid) { Note "  a window opened in the SHELL explorer $shellPid" }
    foreach ($k in 1..6) {
      foreach ($w in $opened) { try { $w.Navigate2($folders[$rand.Next($folders.Count)]) } catch { } }
      Start-Sleep -Milliseconds (500 + $rand.Next(700))
    }
    Start-Sleep -Seconds 2
    foreach ($id in $folderPids) {
      $p = Get-Process -Id $id -ErrorAction SilentlyContinue
      if ($p) { $peak += '{0}: {1} MB priv, {2} handles' -f $id, (Mb $p), $p.HandleCount }
    }
  } finally {
    foreach ($w in $opened) { try { $w.Quit() } catch { } }
    Drop-Lock
  }
  $closed = Get-Date
  $exits = @{}
  for ($i = 0; $i -lt 300 -and $exits.Count -lt $folderPids.Count; $i++) {
    foreach ($id in $folderPids) {
      if (-not $exits.ContainsKey($id) -and -not (Get-Process -Id $id -ErrorAction SilentlyContinue)) { $exits[$id] = ((Get-Date) - $closed).TotalSeconds }
    }
    Start-Sleep -Milliseconds 500
  }
  $exitText = ($folderPids | ForEach-Object {
    $styled = @(Select-String -LiteralPath $helperLog -Pattern "styling explorer \($_," -ErrorAction SilentlyContinue).Count -gt 0
    if ($exits.ContainsKey($_)) { '{0} exited {1:N1} s after closing (styled {2})' -f $_, $exits[$_], $styled } else { '{0} STILL RUNNING 150 s after closing (styled {1})' -f $_, $styled }
  }) -join '; '
  Note ("burst {0}: folder explorer(s) with both windows open: {1}; {2}; focus kept {3}" -f $n, ($peak -join ', '), $exitText, ($kept -join '/'))
  Sample-Explorers "after burst $n"
  Note "  $(Styler)"
  Start-Sleep -Seconds 10
}
Note "explorer bursts done $Tag"
