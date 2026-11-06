# Round 5: does navigating a folder window between Gallery, Home and folders hang its (styled)
# process? Opens N windows on C:\Windows, navigates each 8 times (Gallery, Home, a folder...),
# checking after every step that the window answers (WM_NULL, 2 s). A hung one is a test
# window's process: noted and ended.
#   nav-trials.ps1 -Log file -Tag name [-N 6]
param([Parameter(Mandatory)][string]$Log, [Parameter(Mandatory)][string]$Tag, [int]$N = 6)
. "$PSScriptRoot\lib.ps1"
$script:Log = $Log
Add-Type -Namespace R5n -Name Win -MemberDefinition @'
[DllImport("user32.dll")] public static extern System.IntPtr SendMessageTimeout(System.IntPtr h, uint m, System.IntPtr w, System.IntPtr l, uint f, uint t, out System.IntPtr r);
'@ -ErrorAction SilentlyContinue
$steps = 'shell:::{e88865ea-0e1c-4e20-9aa6-edcd0212c87c}', 'shell:::{f874310e-b6b7-47dc-bc84-b9e6b38f5903}', 'C:\Windows\System32',
  'shell:::{e88865ea-0e1c-4e20-9aa6-edcd0212c87c}', 'C:\Users\Public', 'shell:::{f874310e-b6b7-47dc-bc84-b9e6b38f5903}',
  'shell:::{e88865ea-0e1c-4e20-9aa6-edcd0212c87c}', 'C:\Windows'
function Answers([IntPtr]$h) { $r = [IntPtr]::Zero; [R5n.Win]::SendMessageTimeout($h, 0, [IntPtr]::Zero, [IntPtr]::Zero, 2, 2000, [ref]$r) -ne [IntPtr]::Zero }
$hung = 0
foreach ($n in 1..$N) {
  Take-Lock
  $w = $null; $h = [IntPtr]::Zero; $owner = 0; $at = ''
  try {
    $w = Open-Folder 'C:\Windows'
    if ($w) {
      $h = [IntPtr](Key $w); $owner = Owner (Key $w)
      Start-Sleep -Seconds 2
      foreach ($k in 0..($steps.Count - 1)) {
        if (-not (Answers $h)) { $at = "before step $k"; break }
        # Navigate2 into a hung window would block: it was checked just above.
        try { $w.Navigate2($steps[$k]) } catch { }
        Start-Sleep -Milliseconds 1500
      }
      if (-not $at -and -not (Answers $h)) { $at = 'after the last step' }
    }
  } finally { Drop-Lock }
  if (-not $w) { Note "$Tag ${n}: nothing opened"; continue }
  if ($at) {
    $hung++
    $p = Get-Process -Id $owner -ErrorAction SilentlyContinue
    Note ("$Tag ${n}: explorer $owner HUNG $at ({0:N1} s cpu); ended" -f $p.TotalProcessorTime.TotalSeconds)
    Stop-Process -Id $owner -Force -ErrorAction SilentlyContinue
  } else {
    [void][R5.Win]::PostMessage($h, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero)
    Note "$Tag ${n}: explorer $owner answered through 8 steps; closed"
  }
  Start-Sleep -Seconds 30  # LIVE.md: 30 s between lock takes
}
Note "$Tag done: $hung of $N hung"
