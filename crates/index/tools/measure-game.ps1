# Owns only index_game.exe and the desktop lock; never moves another application's window.
param([int]$Seconds = 60, [string]$FixtureExe = 'C:\mypaper-targets\index\release\examples\index_game.exe')
$ErrorActionPreference = 'Stop'
$lockDir = 'C:\Users\you\AppData\Local\Temp\mypaper-tools\desktop.lock'
$signalPath = Join-Path $env:LOCALAPPDATA 'mypaper\game-activity.json'
$evidence = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\evidence'))
$deployed = 'D:\Coding\projects\desktop\wallpaper\target\release\mypaper-index.exe'
$service = @(Get-CimInstance Win32_Process -Filter "Name='mypaper-index.exe'" | Where-Object { $_.ExecutablePath -eq $deployed -and $_.CommandLine -match '--parent' })
if ($service.Count -ne 1) { throw 'Expected one core-owned deployed index' }
$index = Get-Process -Id $service[0].ProcessId
if ([IntPtr]::Size -ne 8) { throw '64-bit PowerShell required for native thread counters' }
Add-Type @'
using System; using System.Collections.Generic; using System.Runtime.InteropServices;
public static class IndexGameThreads {
 [StructLayout(LayoutKind.Sequential)] public struct Point { public int X,Y; }
 [StructLayout(LayoutKind.Sequential)] public struct Rect { public int Left,Top,Right,Bottom; }
 [DllImport("user32.dll")] static extern bool SetProcessDpiAwarenessContext(IntPtr c);
 [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr h,out Rect r);
 [DllImport("user32.dll")] static extern IntPtr WindowFromPoint(Point p);
 [DllImport("user32.dll")] static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h,out uint pid);
 [DllImport("user32.dll")] static extern bool GetCursorPos(out Point p);
 [DllImport("user32.dll")] static extern bool SetCursorPos(int x,int y);
 [DllImport("user32.dll")] static extern void mouse_event(uint flags,uint x,uint y,uint data,UIntPtr extra);
 public static uint ForegroundPid() {uint pid;GetWindowThreadProcessId(GetForegroundWindow(),out pid);return pid;}
 public static void ActivateOwn(IntPtr hwnd,int pid) {
  SetProcessDpiAwarenessContext(new IntPtr(-4)); Rect r; Point before; uint owner;
  if(!GetWindowRect(hwnd,out r))throw new Exception("fixture has no window");
  var p=new Point{X=r.Left+(r.Right-r.Left)/2,Y=r.Top+(r.Bottom-r.Top)/2};
  GetWindowThreadProcessId(WindowFromPoint(p),out owner);
  if(owner!=pid)throw new Exception("fixture activation point is not owned; click skipped");
  if(!GetCursorPos(out before))throw new Exception("cursor unavailable");
  try {
   SetCursorPos(p.X,p.Y);GetWindowThreadProcessId(WindowFromPoint(p),out owner);
   if(owner!=pid)throw new Exception("fixture activation target changed; click skipped");
   mouse_event(2,0,0,0,UIntPtr.Zero);mouse_event(4,0,0,0,UIntPtr.Zero);
  }finally{SetCursorPos(before.X,before.Y);}
 }
 [DllImport("ntdll.dll")] static extern int NtQuerySystemInformation(int c,IntPtr b,int n,out int r);
 public static Dictionary<long,long> Read(int pid) {
  var result=new Dictionary<long,long>(); int n=16<<20,r; IntPtr b=Marshal.AllocHGlobal(n);
  try {
   if(NtQuerySystemInformation(5,b,n,out r)!=0)throw new Exception("thread snapshot failed");
   long off=0;
   while(true) {
    IntPtr p=new IntPtr(b.ToInt64()+off); int next=Marshal.ReadInt32(p,0),count=Marshal.ReadInt32(p,4);
    if(Marshal.ReadIntPtr(p,0x50).ToInt64()==pid) {
     for(int i=0;i<count;i++) {IntPtr t=new IntPtr(p.ToInt64()+0x100+i*0x50);result[Marshal.ReadIntPtr(t,0x30).ToInt64()]=(uint)Marshal.ReadInt32(t,0x40);}
     return result;
    }
    if(next==0)break; off+=next;
   }
  }finally{Marshal.FreeHGlobal(b);}return result;
 }
}
'@
$until = [DateTime]::UtcNow.AddMinutes(2)
while (-not (New-Item -ItemType Directory -Path $lockDir -ErrorAction SilentlyContinue)) {
  if ([DateTime]::UtcNow -ge $until) { throw 'Desktop lock remained busy' }
  Start-Sleep -Seconds 2
}
try {
  $fixture = Start-Process -FilePath $FixtureExe -ArgumentList '180' -WindowStyle Normal -PassThru
  $null = $fixture.Handle
  Start-Sleep -Seconds 1
  $fixture.Refresh()
  [IndexGameThreads]::ActivateOwn($fixture.MainWindowHandle,$fixture.Id)
  Start-Sleep -Seconds 10
  & 'C:\Users\you\AppData\Local\Temp\mypaper-tools\shotall.ps1' -Out (Join-Path $evidence 'round2-game-fullscreen.png') | Out-Null
  $signal = Get-Content -LiteralPath $signalPath -Raw | ConvertFrom-Json
  [pscustomobject]@{fixture_pid=$fixture.Id;foreground_pid=[IndexGameThreads]::ForegroundPid();signal=$signal} | ConvertTo-Json -Depth 5 | Set-Content -Encoding utf8 (Join-Path $evidence 'round2-game-activation.json')
  if (-not $signal.activity.pause_background -or $signal.activity.foreground_pid -ne $fixture.Id) { throw 'Owned fullscreen fixture did not engage the real game hint' }
  Start-Sleep -Seconds 2
  $index.Refresh()
  $started = $index.StartTime
  $cpuBefore = $index.TotalProcessorTime.TotalMilliseconds
  $threadsBefore = [IndexGameThreads]::Read($index.Id)
  $clock = [Diagnostics.Stopwatch]::StartNew()
  $samples = @()
  for ($i=0; $i -lt $Seconds; $i++) {
    Start-Sleep -Seconds 1
    $signal = Get-Content -LiteralPath $signalPath -Raw | ConvertFrom-Json
    if (-not $signal.activity.pause_background -or $signal.activity.foreground_pid -ne $fixture.Id) { throw 'Game hint changed during measurement' }
    $index.Refresh()
    if ($index.HasExited -or $index.StartTime -ne $started) { throw 'Index restarted during game measurement' }
    $samples += [pscustomobject]@{second=$i+1;private_bytes=$index.PrivateMemorySize64;working_set_bytes=$index.WorkingSet64;pause_background=$signal.activity.pause_background;foreground_pid=$signal.activity.foreground_pid}
  }
  $elapsed = $clock.Elapsed.TotalSeconds
  $index.Refresh()
  $threadsAfter = [IndexGameThreads]::Read($index.Id)
  $switches = 0L
  foreach ($tid in $threadsAfter.Keys) { $old = if ($threadsBefore.ContainsKey($tid)) { $threadsBefore[$tid] } else { 0 }; $switches += $threadsAfter[$tid]-$old }
  [pscustomobject]@{
    index_pid=$index.Id;fixture_pid=$fixture.Id;seconds=$elapsed
    cpu_core_pct=($index.TotalProcessorTime.TotalMilliseconds-$cpuBefore)/($elapsed*10)
    context_switches_per_second=$switches/$elapsed
    max_private_bytes=($samples|Measure-Object private_bytes -Maximum).Maximum
    max_working_set_bytes=($samples|Measure-Object working_set_bytes -Maximum).Maximum
    final_private_bytes=$index.PrivateMemorySize64;final_working_set_bytes=$index.WorkingSet64
    samples=$samples
  } | ConvertTo-Json -Depth 5 | Set-Content -Encoding utf8 (Join-Path $evidence 'round2-game.json')
} finally {
  try {
    if ($fixture -and -not $fixture.HasExited) {
      if ($fixture.Path -ne $FixtureExe) { throw 'Fixture process identity changed' }
      Stop-Process -InputObject $fixture
    }
    Start-Sleep -Seconds 2
    & 'C:\Users\you\AppData\Local\Temp\mypaper-tools\shotall.ps1' -Out (Join-Path $evidence 'round2-game-restored.png') | Out-Null
  } finally { [IO.Directory]::Delete($lockDir) }
}
