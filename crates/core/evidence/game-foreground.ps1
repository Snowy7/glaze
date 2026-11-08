# Real cross-process foreground delivery: a plain topmost WS_POPUP on the primary monitor.
# Owns only the copied index fixture and the empty desktop lock directory.
param([ValidatePattern('^[a-z0-9-]+$')][string]$Label = 'fixed', [switch]$WakeProbe)
$ErrorActionPreference = 'Stop'
$probeDir = 'C:\glaze-targets\game-quiet-live'
$fixtureExe = Join-Path $probeDir 'index_game.exe'
$desktopLock = 'C:\Users\you\AppData\Local\Temp\glaze-tools\desktop.lock'
$signalPath = Join-Path $env:LOCALAPPDATA 'glaze/game-activity.json'
$output = Join-Path $PSScriptRoot "game-foreground-$Label.json"
Add-Type @'
using System; using System.Runtime.InteropServices;
public static class GameForegroundProbe {
 [StructLayout(LayoutKind.Sequential)] public struct Point {public int X,Y;}
 [StructLayout(LayoutKind.Sequential)] public struct Rect {public int Left,Top,Right,Bottom;}
 [DllImport("user32.dll")] static extern bool SetProcessDpiAwarenessContext(IntPtr c);
 [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr h,out Rect r);
 [DllImport("user32.dll")] static extern IntPtr WindowFromPoint(Point p);
 [DllImport("user32.dll")] static extern bool GetCursorPos(out Point p);
 [DllImport("user32.dll")] static extern bool SetCursorPos(int x,int y);
 [DllImport("user32.dll")] static extern void mouse_event(uint f,uint x,uint y,uint d,UIntPtr e);
 [DllImport("user32.dll")] static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h,out uint pid);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern IntPtr FindWindow(string c,string n);
 [DllImport("user32.dll")] static extern bool PostMessage(IntPtr h,uint m,IntPtr w,IntPtr l);
 public static uint ForegroundPid() {uint p;GetWindowThreadProcessId(GetForegroundWindow(),out p);return p;}
 public static bool WakePublisher() {return PostMessage(FindWindow("glaze-game-activity",null),0,IntPtr.Zero,IntPtr.Zero);}
 public static bool RefreshPublisher() {return PostMessage(FindWindow("glaze-game-activity",null),0x113,new IntPtr(1),IntPtr.Zero);}
 public static void ActivateOwn(IntPtr h,int pid) {
  SetProcessDpiAwarenessContext(new IntPtr(-4));Rect r;Point before;uint owner;
  if(!GetWindowRect(h,out r)||!GetCursorPos(out before))throw new Exception("fixture/cursor unavailable");
  var p=new Point{X=r.Left+(r.Right-r.Left)/2,Y=r.Top+(r.Bottom-r.Top)/2};
  GetWindowThreadProcessId(WindowFromPoint(p),out owner);
  if(owner!=pid)throw new Exception("activation point is not fixture-owned");
  try {SetCursorPos(p.X,p.Y);GetWindowThreadProcessId(WindowFromPoint(p),out owner);
   if(owner!=pid)throw new Exception("activation target changed");
   mouse_event(2,0,0,0,UIntPtr.Zero);mouse_event(4,0,0,0,UIntPtr.Zero);
  }finally{SetCursorPos(before.X,before.Y);}
 }
}
'@
Copy-Item -LiteralPath 'C:\glaze-targets\index\release\examples\index_game.exe' -Destination $fixtureExe -Force
New-Item -ItemType Directory -Path $desktopLock -ErrorAction Stop | Out-Null
$fixture = $null
try {
  $before = Get-Content -LiteralPath $signalPath -Raw | ConvertFrom-Json
  if ($before.activity.pause_background) { throw 'Start from Idle so the paused-only timer cannot mask missed entry' }
  $fixture = Start-Process -FilePath $fixtureExe -ArgumentList '45' -WindowStyle Normal -PassThru
  Start-Sleep -Seconds 1
  $fixture.Refresh()
  if ([GameForegroundProbe]::ForegroundPid() -ne $fixture.Id) { [GameForegroundProbe]::ActivateOwn($fixture.MainWindowHandle,$fixture.Id) }
  $observations = @()
  $clock = [Diagnostics.Stopwatch]::StartNew()
  for ($i=0; $i -lt 10; $i++) {
    Start-Sleep -Seconds 1
    $signal = Get-Content -LiteralPath $signalPath -Raw | ConvertFrom-Json
    $observations += [pscustomobject]@{second=$clock.Elapsed.TotalSeconds;foreground_pid=[GameForegroundProbe]::ForegroundPid();signal=$signal}
  }
  $activated = $observations[-1].foreground_pid -eq $fixture.Id
  $engaged = $activated -and $observations[-1].signal.activity.pause_background -and $observations[-1].signal.activity.foreground_pid -eq $fixture.Id
  $samePublisher = @($observations | Where-Object { $_.signal.publisher_pid -ne $before.publisher_pid }).Count -eq 0
  & 'C:\Users\you\AppData\Local\Temp\glaze-tools\shotall.ps1' -Out (Join-Path $probeDir "foreground-$Label.png") | Out-Null
  $wake = $null
  if ($WakeProbe) {
    $posted = [GameForegroundProbe]::WakePublisher()
    Start-Sleep -Seconds 1
    $wake = @{posted=$posted;foreground_pid=[GameForegroundProbe]::ForegroundPid();signal=(Get-Content -LiteralPath $signalPath -Raw | ConvertFrom-Json)}
    $wake.refresh_posted = [GameForegroundProbe]::RefreshPublisher()
    Start-Sleep -Seconds 1
    $wake.after_refresh = Get-Content -LiteralPath $signalPath -Raw | ConvertFrom-Json
  }
  [pscustomobject]@{fixture_pid=$fixture.Id;before=$before;activated=$activated;same_publisher=$samePublisher;engaged_without_wake=$engaged;observations=$observations;wake_probe=$wake} | ConvertTo-Json -Depth 7 | Set-Content -LiteralPath $output -Encoding utf8
  if (-not $samePublisher) { throw 'Publisher restarted during test' }
  if (-not $activated) { throw 'Fixture did not become foreground; delivery not tested' }
  if (-not $engaged) { throw 'Real foreground changed but publisher did not engage' }
} finally {
  try {
    if ($fixture -and -not $fixture.HasExited) {
      if ($fixture.Path -ne $fixtureExe) { throw 'Fixture identity changed' }
      Stop-Process -InputObject $fixture
    }
    Start-Sleep -Seconds 2
    $restored = Get-Content -LiteralPath $signalPath -Raw | ConvertFrom-Json
    $restored | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $PSScriptRoot "game-foreground-$Label-restored.json") -Encoding utf8
    & 'C:\Users\you\AppData\Local\Temp\glaze-tools\shotall.ps1' -Out (Join-Path $probeDir "foreground-$Label-restored.png") | Out-Null
    if ($restored.activity.pause_background) { throw 'Pause did not clear after fixture exit' }
  } finally { [IO.Directory]::Delete($desktopLock) }
}
