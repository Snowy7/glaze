# Round 5, Settings: open Settings on a page, move to a second page, close it, and see whether
# SystemSettings.exe exits and how big it got; the shell Explorer and the helper are sampled too.
#   settings.ps1 -Log file [-Rounds 10] [-Pause 20]
param([Parameter(Mandatory)][string]$Log, [int]$Rounds = 10, [int]$Pause = 20, [string[]]$Pages)
. "$PSScriptRoot\lib.ps1"
$script:Log = $Log
Add-Type -Namespace R5s -Name Win -MemberDefinition @'
[DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern System.IntPtr FindWindowEx(System.IntPtr p, System.IntPtr a, string c, string n);
'@ -ErrorAction SilentlyContinue
$pages = if ($Pages) { $Pages } else {
  'ms-settings:display', 'ms-settings:personalization-colors', 'ms-settings:about', 'ms-settings:network',
  'ms-settings:bluetooth', 'ms-settings:apps-volume', 'ms-settings:sound', 'ms-settings:personalization-background',
  'ms-settings:taskbar', 'ms-settings:privacy' }
# Settings' frame: the ApplicationFrameWindow titled "Settings".
function Frame { [R5s.Win]::FindWindowEx([IntPtr]::Zero, [IntPtr]::Zero, 'ApplicationFrameWindow', 'Settings') }
# Opens a Settings page without taking the focus from whoever has it (see Open-Folder).
function Open-Page([string]$uri) {
  $fg = [R5.Win]::GetForegroundWindow()
  $shell.ShellExecute($uri, '', '', 'open', 7)
  for ($i = 0; $i -lt 80; $i++) {
    Start-Sleep -Milliseconds 100
    $h = Frame
    if ($h -ne [IntPtr]::Zero -and (Get-Process SystemSettings -ErrorAction SilentlyContinue)) {
      if ([R5.Win]::GetForegroundWindow() -ne $fg) { [void][R5.Win]::ShowWindow($h, 6) }
      [void][R5.Win]::ShowWindow($h, 4)
      [void][R5.Win]::SetWindowPos($h, [IntPtr]::Zero, -2300, 120, 1200, 860, 0x0010 -bor 0x0004)
      return [R5.Win]::GetForegroundWindow() -eq $fg
    }
  }
  return $null
}
function Styler { $s = Get-Process mypaper-styler -ErrorAction SilentlyContinue | Select-Object -First 1; if ($s) { 'styler {0}: {1} MB priv' -f $s.Id, (Mb $s) } else { 'no styler' } }
Note "settings rounds start ($Rounds); $(Styler)"
Sample-Explorers 'start'
foreach ($n in 1..$Rounds) {
  Take-Lock
  $proc = $null
  try {
    $kept1 = Open-Page $pages[($n - 1) % $pages.Count]
    Start-Sleep -Seconds 5
    $kept2 = Open-Page $pages[$n % $pages.Count]
    Start-Sleep -Seconds 5
    $proc = Get-Process SystemSettings -ErrorAction SilentlyContinue | Select-Object -First 1
    $size = if ($proc) { Mb $proc } else { '?' }
    $h = Frame
    if ($h -ne [IntPtr]::Zero) { [void][R5.Win]::PostMessage($h, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) }  # WM_CLOSE
    $closed = Get-Date
  } finally { Drop-Lock }
  if (-not $proc) { Note "round ${n}: no Settings process"; continue }
  $exit = $null
  for ($i = 0; $i -lt 240; $i++) {
    if (-not (Get-Process -Id $proc.Id -ErrorAction SilentlyContinue)) { $exit = ((Get-Date) - $closed).TotalSeconds; break }
    Start-Sleep -Milliseconds 500
  }
  $after = if ($exit -ne $null) { 'exited {0:N1} s after closing' -f $exit } else { 'STILL RUNNING 120 s after closing ({0} MB)' -f (Mb (Get-Process -Id $proc.Id)) }
  Note ("round {0}: settings {1} at {2} MB priv with two pages open; {3}; focus kept {4}/{5}; {6}" -f $n, $proc.Id, $size, $after, $kept1, $kept2, (Styler))
  Sample-Explorers "after round $n"
  if ($n -lt $Rounds) { Start-Sleep -Seconds $Pause }
}
Note 'settings rounds done'
