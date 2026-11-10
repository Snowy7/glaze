$ErrorActionPreference='Stop'
$lock=(Join-Path $env:LOCALAPPDATA 'Temp\glaze-tools\desktop.lock')
$until=(Get-Date).AddMinutes(2)
while(-not (New-Item -ItemType Directory -Path $lock -ErrorAction SilentlyContinue)) {
  if((Get-Date) -ge $until){throw 'Desktop lock is busy'}
  Start-Sleep 2
}
try {
  Add-Type @'
using System; using System.Runtime.InteropServices;
public static class CoveredQuit {
  [DllImport("user32.dll")] public static extern bool PostThreadMessage(uint thread,uint message,IntPtr w,IntPtr l);
}
'@
  foreach($studio in Get-Process glaze-studio -ErrorAction SilentlyContinue) {
    foreach($thread in $studio.Threads) {[CoveredQuit]::PostThreadMessage([uint32]$thread.Id,0x12,[IntPtr]::Zero,[IntPtr]::Zero) | Out-Null}
  }
  $until=(Get-Date).AddSeconds(10)
  while((Get-Process glaze-studio -ErrorAction SilentlyContinue) -and (Get-Date) -lt $until){Start-Sleep -Milliseconds 200}
  if(Get-Process glaze-studio -ErrorAction SilentlyContinue){throw 'Studio did not quit gracefully'}
  $release=Join-Path (Resolve-Path "$PSScriptRoot\..\..\..\..").Path 'target\release'
  Start-Process -FilePath (Join-Path $release 'glaze-studio.exe') -WorkingDirectory $release -WindowStyle Hidden
  Start-Sleep 3
  Get-Process glaze-studio | Select-Object Id,Path
} finally {[IO.Directory]::Delete($lock)}
