# Watches one test Explorer: when its folder window stops answering for 4 s, saves the busiest
# threads' CPU twice (to see which one spins) and every thread's stack (cdb, non-invasive attach,
# waited for), then ends that test process.
#   watchdog.ps1 -Target <pid> [-Out file]
param([Parameter(Mandatory)][int]$Target, [string]$Out = 'C:\mypaper-targets\app-styler\round4\hang-stacks.txt')
Add-Type -Namespace R4h -Name Win -MemberDefinition @'
[DllImport("user32.dll")] public static extern System.IntPtr SendMessageTimeout(System.IntPtr h, uint m, System.IntPtr w, System.IntPtr l, uint f, uint t, out System.IntPtr r);
'@
$cdb = 'C:\Program Files (x86)\Windows Kits\10\Debuggers\x64\cdb.exe'
function Busiest { (Get-Process -Id $Target).Threads | Sort-Object TotalProcessorTime -Descending | Select-Object -First 6 Id, TotalProcessorTime | Format-Table | Out-String }
$hungFor = 0
while (Get-Process -Id $Target -ErrorAction SilentlyContinue) {
  $h = (Get-Process -Id $Target).MainWindowHandle
  $ok = $true
  if ($h -ne [IntPtr]::Zero) {
    $r = [IntPtr]::Zero
    # WM_NULL with a 2 s timeout: 0 means no answer.
    $ok = [R4h.Win]::SendMessageTimeout($h, 0, [IntPtr]::Zero, [IntPtr]::Zero, 2, 2000, [ref]$r) -ne [IntPtr]::Zero
  }
  if ($ok) { $hungFor = 0 } else { $hungFor += 2 }
  if ($hungFor -ge 4) {
    "$(Get-Date -Format HH:mm:ss) explorer $Target not answering; capturing" | Out-File $Out -Encoding utf8
    Busiest | Out-File $Out -Append -Encoding utf8
    Start-Sleep 3
    Busiest | Out-File $Out -Append -Encoding utf8
    $cdbOut = [IO.Path]::ChangeExtension($Out, '.cdb.txt')
    $dbg = Start-Process $cdb -ArgumentList "-pv -p $Target -y srv*C:\symbols*https://msdl.microsoft.com/download/symbols;C:\mypaper-targets\app-styler\release -logo $cdbOut -c `"~*k 60;q`"" -WindowStyle Hidden -PassThru
    $dbg.WaitForExit(240000) | Out-Null
    Stop-Process -Id $Target -Force -ErrorAction SilentlyContinue
    "$(Get-Date -Format HH:mm:ss) ended explorer $Target (stacks: $cdbOut)" | Out-File $Out -Append -Encoding utf8
    break
  }
  Start-Sleep -Seconds 1
}
