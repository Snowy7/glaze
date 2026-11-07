# Round 5: does a folder window opened on Gallery hang its (styled) process? Opens N windows
# straight at a page, 10 s each, checks the window answers (WM_NULL, 2 s), closes it. A hung
# one is a test window's process: it is ended.
#   gallery-trials.ps1 -Log file -Tag name [-N 5] [-Page shell:::...]
param([Parameter(Mandatory)][string]$Log, [Parameter(Mandatory)][string]$Tag, [int]$N = 5,
      [string]$Page = 'shell:::{e88865ea-0e1c-4e20-9aa6-edcd0212c87c}')
. "$PSScriptRoot\lib.ps1"
$script:Log = $Log
Add-Type -Namespace R5g -Name Win -MemberDefinition @'
[DllImport("user32.dll")] public static extern System.IntPtr SendMessageTimeout(System.IntPtr h, uint m, System.IntPtr w, System.IntPtr l, uint f, uint t, out System.IntPtr r);
'@ -ErrorAction SilentlyContinue
$hung = 0
foreach ($n in 1..$N) {
  Take-Lock
  $w = $null; $h = [IntPtr]::Zero; $owner = 0
  try {
    $w = Open-Folder $Page
    if ($w) { $h = [IntPtr](Key $w); $owner = Owner (Key $w) }
  } finally { Drop-Lock }
  if (-not $w) { Note "$Tag ${n}: nothing opened"; continue }
  Start-Sleep -Seconds 10
  $r = [IntPtr]::Zero
  $ok = [R5g.Win]::SendMessageTimeout($h, 0, [IntPtr]::Zero, [IntPtr]::Zero, 2, 2000, [ref]$r) -ne [IntPtr]::Zero
  if ($ok) {
    [void][R5.Win]::PostMessage($h, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero)
    Note "$Tag ${n}: explorer $owner answering; closed"
  } else {
    $hung++
    $p = Get-Process -Id $owner -ErrorAction SilentlyContinue
    Note ("$Tag ${n}: explorer $owner HUNG ({0:N1} s cpu); ended" -f $p.TotalProcessorTime.TotalSeconds)
    Stop-Process -Id $owner -Force -ErrorAction SilentlyContinue
  }
  Start-Sleep -Seconds 5
}
Note "$Tag done: $hung of $N hung"
