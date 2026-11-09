param([Parameter(Mandatory)][int]$ProbeProcess, [Parameter(Mandatory)][string]$OutputDirectory)
# Captures only our separately launched host; never enumerates or writes to user apps.
$ErrorActionPreference = 'Stop'
$process = Get-Process -Id $ProbeProcess
if ($process.ProcessName -ne 'corner-probe-host') { throw 'Expected the owned corner-probe-host process.' }
$hwnd = $process.MainWindowHandle
if ($hwnd -eq [IntPtr]::Zero) { throw 'The test window is not ready.' }
Add-Type @'
using System; using System.Runtime.InteropServices;
public static class CornerNative {
 [DllImport("dwmapi.dll")] public static extern int DwmGetWindowAttribute(IntPtr h,int a,out uint v,int n);
 [DllImport("dwmapi.dll")] public static extern int DwmSetWindowAttribute(IntPtr h,int a,ref uint v,int n);
 [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
 [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
}
'@
$capture = Join-Path $env:CARGO_TARGET_DIR 'release\examples\corner_capture.exe'
[uint32]$original = 0
if ([CornerNative]::DwmGetWindowAttribute($hwnd,33,[ref]$original,4)) { throw 'Cannot read test preference.' }
$foreground = [CornerNative]::GetForegroundWindow().ToInt64()
$results = @()
try {
 foreach ($mode in @(@{name='default';value=0},@{name='round';value=2},@{name='small';value=3},@{name='square';value=1})) {
  [uint32]$value=$mode.value
  $set=[CornerNative]::DwmSetWindowAttribute($hwnd,33,[ref]$value,4)
  [uint32]$actual=0
  $get=[CornerNative]::DwmGetWindowAttribute($hwnd,33,[ref]$actual,4)
  Start-Sleep -Milliseconds 150
  & $capture $hwnd.ToInt64() $ProbeProcess (Join-Path $OutputDirectory ($mode.name+'.bmp'))
  if($LASTEXITCODE){throw 'Owned window capture failed.'}
  $results += @{mode=$mode.name;set_hresult=$set;get_hresult=$get;actual=$actual;dpi=[CornerNative]::GetDpiForWindow($hwnd)}
 }
} finally { [void][CornerNative]::DwmSetWindowAttribute($hwnd,33,[ref]$original,4) }
@{modes=$results;foreground_before=$foreground;foreground_after=[CornerNative]::GetForegroundWindow().ToInt64();pid=$ProbeProcess;hwnd=$hwnd.ToInt64()} | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $OutputDirectory 'modes.json')
